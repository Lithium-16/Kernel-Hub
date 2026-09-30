//! Events: things that happened without anyone asking (a crash, a player joining, a new
//! build). Kept in the node database, pushed to connected clients and sent as alerts.

use std::path::Path;
use std::sync::Mutex;

use kernel_protocol::{EventLevel, NodeEvent, new_id, now_ts};
use rusqlite::{Connection, params};
use serde_json::{Map, Value};
use tokio::sync::broadcast;

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS events (
    id        TEXT PRIMARY KEY,
    ts        TEXT NOT NULL,
    node_id   TEXT NOT NULL,
    module    TEXT NOT NULL,
    kind      TEXT NOT NULL,
    level     TEXT NOT NULL,
    message   TEXT NOT NULL,
    data_json TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS events_ts ON events (ts DESC, id DESC);
CREATE INDEX IF NOT EXISTS events_module ON events (module, ts DESC);
";

/// Old events are dropped past this many, so the table stays small.
const KEEP: i64 = 5_000;

pub struct EventHub {
    node_id: String,
    conn: Mutex<Connection>,
    live: broadcast::Sender<NodeEvent>,
}

impl EventHub {
    pub fn open(path: &Path, node_id: &str) -> rusqlite::Result<Self> {
        Self::init(Connection::open(path)?, node_id)
    }

    pub fn in_memory(node_id: &str) -> rusqlite::Result<Self> {
        Self::init(Connection::open_in_memory()?, node_id)
    }

    fn init(conn: Connection, node_id: &str) -> rusqlite::Result<Self> {
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.execute_batch(SCHEMA)?;
        Ok(Self {
            node_id: node_id.into(),
            conn: Mutex::new(conn),
            live: broadcast::channel(256).0,
        })
    }

    /// Every event from now on. Slow receivers skip ahead rather than hold anyone up.
    pub fn subscribe(&self) -> broadcast::Receiver<NodeEvent> {
        self.live.subscribe()
    }

    pub fn emit(
        &self,
        module: &str,
        kind: &str,
        level: EventLevel,
        message: impl Into<String>,
        data: Map<String, Value>,
    ) -> NodeEvent {
        let event = NodeEvent {
            id: new_id(),
            ts: now_ts(),
            node_id: self.node_id.clone(),
            module: module.into(),
            kind: kind.into(),
            level,
            message: message.into(),
            data,
        };
        tracing::info!(module, kind, level = level.as_str(), message = %event.message, "event");
        if let Err(e) = self.insert(&event) {
            tracing::error!(error = %e, "failed to store event");
        }
        let _ = self.live.send(event.clone());
        event
    }

    fn insert(&self, e: &NodeEvent) -> rusqlite::Result<()> {
        let conn = self.conn.lock().expect("events db lock");
        conn.execute(
            "INSERT INTO events (id, ts, node_id, module, kind, level, message, data_json)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                e.id,
                e.ts,
                e.node_id,
                e.module,
                e.kind,
                e.level.as_str(),
                e.message,
                serde_json::to_string(&e.data).expect("data serialize"),
            ],
        )?;
        conn.execute(
            "DELETE FROM events WHERE id NOT IN (SELECT id FROM events ORDER BY ts DESC, id DESC LIMIT ?1)",
            params![KEEP],
        )?;
        Ok(())
    }

    /// Newest first. `limit` is clamped to 1..=500.
    pub fn query(&self, limit: u32, module: Option<&str>) -> rusqlite::Result<Vec<NodeEvent>> {
        let limit = limit.clamp(1, 500);
        let conn = self.conn.lock().expect("events db lock");
        let mut stmt = conn.prepare(
            "SELECT id, ts, node_id, module, kind, level, message, data_json
             FROM events WHERE (?1 IS NULL OR module = ?1) ORDER BY ts DESC, id DESC LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![module, limit], |r| {
            let data: String = r.get(7)?;
            Ok(NodeEvent {
                id: r.get(0)?,
                ts: r.get(1)?,
                node_id: r.get(2)?,
                module: r.get(3)?,
                kind: r.get(4)?,
                level: EventLevel::parse(&r.get::<_, String>(5)?).unwrap_or(EventLevel::Info),
                message: r.get(6)?,
                data: serde_json::from_str(&data).unwrap_or_default(),
            })
        })?;
        rows.collect()
    }
}

/// Turn one entry of a module's `kernel://events` list into an event, or None if malformed.
pub fn from_module(raw: &Value) -> Option<(String, EventLevel, String, Map<String, Value>)> {
    let kind = raw.get("kind")?.as_str()?;
    if !valid_kind(kind) {
        return None;
    }
    let level = raw
        .get("level")
        .and_then(Value::as_str)
        .map_or(Some(EventLevel::Info), EventLevel::parse)?;
    let message: String = raw.get("message")?.as_str()?.chars().take(500).collect();
    let data = raw
        .get("data")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    Some((kind.into(), level, message, data))
}

/// `word(.word)*`, lower case, like `server.crashed`.
pub fn valid_kind(kind: &str) -> bool {
    !kind.is_empty()
        && kind.len() <= 64
        && kind.split('.').all(|part| {
            part.chars().next().is_some_and(|c| c.is_ascii_lowercase())
                && part
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn stores_queries_and_broadcasts() {
        let hub = EventHub::in_memory("pluto").unwrap();
        let mut rx = hub.subscribe();
        hub.emit(
            "minecraft",
            "player.joined",
            EventLevel::Info,
            "Steve joined",
            Map::new(),
        );
        let e = hub.emit(
            "roblox",
            "game.disconnected",
            EventLevel::Warn,
            "Kicked",
            Map::new(),
        );
        assert_eq!(rx.try_recv().unwrap().kind, "player.joined");
        assert_eq!(rx.try_recv().unwrap(), e);
        let all = hub.query(10, None).unwrap();
        assert_eq!(all.len(), 2);
        assert_eq!(hub.query(10, Some("roblox")).unwrap(), vec![e]);
    }

    #[test]
    fn parses_module_events() {
        let ok = json!({"kind": "server.crashed", "level": "error", "message": "boom", "data": {"code": 1}});
        let (kind, level, message, data) = from_module(&ok).unwrap();
        assert_eq!(
            (kind.as_str(), level, message.as_str()),
            ("server.crashed", EventLevel::Error, "boom")
        );
        assert_eq!(data["code"], 1);
        assert_eq!(
            from_module(&json!({"kind": "a", "message": "m"}))
                .unwrap()
                .1,
            EventLevel::Info
        );
        assert!(from_module(&json!({"kind": "Bad Kind", "message": "m"})).is_none());
        assert!(from_module(&json!({"kind": "a", "level": "loud", "message": "m"})).is_none());
        assert!(from_module(&json!({"kind": "a"})).is_none());
        assert!(
            valid_kind("player.joined")
                && valid_kind("update")
                && !valid_kind("a..b")
                && !valid_kind("1a")
        );
    }
}
