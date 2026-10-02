//! The activity log: every action, who ran it, and how it went.

use std::path::Path;
use std::sync::Mutex;

use kernel_protocol::{ActivityEntry, ActivityResult, Actor, ActorKind, ErrorCode};
use rusqlite::{Connection, params};

pub struct ActivityStore {
    conn: Mutex<Connection>,
}

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS activity (
    id          TEXT PRIMARY KEY,
    ts          TEXT NOT NULL,
    actor_kind  TEXT NOT NULL,
    actor_ref   TEXT,
    node_id     TEXT NOT NULL,
    module      TEXT NOT NULL,
    action      TEXT NOT NULL,
    params_json TEXT NOT NULL,
    result      TEXT NOT NULL,
    error_code  TEXT,
    duration_ms INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS activity_ts ON activity (ts DESC, id DESC);
CREATE INDEX IF NOT EXISTS activity_module ON activity (module, ts DESC);
CREATE TABLE IF NOT EXISTS assistant_usage (
    month         TEXT PRIMARY KEY,
    usd           REAL NOT NULL,
    input_tokens  INTEGER NOT NULL,
    output_tokens INTEGER NOT NULL
);
";

fn result_str(r: ActivityResult) -> &'static str {
    match r {
        ActivityResult::Ok => "ok",
        ActivityResult::Error => "error",
        ActivityResult::Denied => "denied",
    }
}

fn parse_result(s: &str) -> ActivityResult {
    match s {
        "ok" => ActivityResult::Ok,
        "denied" => ActivityResult::Denied,
        _ => ActivityResult::Error,
    }
}

fn parse_actor(s: &str) -> ActorKind {
    match s {
        "assistant" => ActorKind::Assistant,
        "automation" => ActorKind::Automation,
        "phone" => ActorKind::Phone,
        _ => ActorKind::User,
    }
}

impl ActivityStore {
    pub fn open(path: &Path) -> rusqlite::Result<Self> {
        Self::init(Connection::open(path)?)
    }

    pub fn in_memory() -> rusqlite::Result<Self> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> rusqlite::Result<Self> {
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.execute_batch(SCHEMA)?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    pub fn insert(&self, e: &ActivityEntry) -> rusqlite::Result<()> {
        let conn = self.conn.lock().expect("activity db lock");
        conn.execute(
            "INSERT INTO activity (id, ts, actor_kind, actor_ref, node_id, module, action, params_json, result, error_code, duration_ms)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![
                e.id,
                e.ts,
                e.actor.kind.as_str(),
                e.actor.reference,
                e.node_id,
                e.module,
                e.action,
                serde_json::to_string(&e.params).expect("params serialize"),
                result_str(e.result),
                e.error_code.map(ErrorCode::as_str),
                e.duration_ms as i64,
            ],
        )?;
        Ok(())
    }

    /// Add Claude API spend to a month (`2026-10`).
    pub fn add_usage(
        &self,
        month: &str,
        usd: f64,
        input: u64,
        output: u64,
    ) -> rusqlite::Result<()> {
        let conn = self.conn.lock().expect("activity db lock");
        conn.execute(
            "INSERT INTO assistant_usage (month, usd, input_tokens, output_tokens) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(month) DO UPDATE SET usd = usd + ?2, input_tokens = input_tokens + ?3,
             output_tokens = output_tokens + ?4",
            params![month, usd, input as i64, output as i64],
        )?;
        Ok(())
    }

    /// Claude API spend in a month, in USD.
    pub fn usage(&self, month: &str) -> f64 {
        let conn = self.conn.lock().expect("activity db lock");
        conn.query_row(
            "SELECT usd FROM assistant_usage WHERE month = ?1",
            params![month],
            |r| r.get(0),
        )
        .unwrap_or(0.0)
    }

    /// A consistent copy of the whole database (activity and events) at `path`.
    pub fn backup_to(&self, path: &Path) -> rusqlite::Result<()> {
        let conn = self.conn.lock().expect("activity db lock");
        conn.execute("VACUUM INTO ?1", params![path.to_string_lossy()])?;
        Ok(())
    }

    /// Newest first. `limit` is clamped to 1..=500.
    pub fn query(&self, limit: u32, module: Option<&str>) -> rusqlite::Result<Vec<ActivityEntry>> {
        let limit = limit.clamp(1, 500);
        let conn = self.conn.lock().expect("activity db lock");
        let mut stmt = conn.prepare(
            "SELECT id, ts, actor_kind, actor_ref, node_id, module, action, params_json, result, error_code, duration_ms
             FROM activity WHERE (?1 IS NULL OR module = ?1) ORDER BY ts DESC, id DESC LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![module, limit], |r| {
            let params_json: String = r.get(7)?;
            let error_code: Option<String> = r.get(9)?;
            let duration: i64 = r.get(10)?;
            Ok(ActivityEntry {
                id: r.get(0)?,
                ts: r.get(1)?,
                actor: Actor {
                    kind: parse_actor(&r.get::<_, String>(2)?),
                    reference: r.get(3)?,
                },
                node_id: r.get(4)?,
                module: r.get(5)?,
                action: r.get(6)?,
                params: serde_json::from_str(&params_json).unwrap_or_default(),
                result: parse_result(&r.get::<_, String>(8)?),
                error_code: error_code.as_deref().and_then(ErrorCode::parse),
                duration_ms: duration.max(0) as u64,
            })
        })?;
        rows.collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn entry(
        id: &str,
        ts: &str,
        module: &str,
        result: ActivityResult,
        code: Option<ErrorCode>,
    ) -> ActivityEntry {
        ActivityEntry {
            id: id.into(),
            ts: ts.into(),
            actor: Actor {
                kind: ActorKind::Assistant,
                reference: Some("chat-1".into()),
            },
            node_id: "pluto".into(),
            module: module.into(),
            action: "greet.say".into(),
            params: json!({"name": "Pluto"}).as_object().unwrap().clone(),
            result,
            error_code: code,
            duration_ms: 12,
        }
    }

    #[test]
    fn round_trips_and_orders_newest_first() {
        let store = ActivityStore::in_memory().unwrap();
        let a = entry(
            "A",
            "2026-09-24T20:00:00Z",
            "hello",
            ActivityResult::Ok,
            None,
        );
        let b = entry(
            "B",
            "2026-09-24T21:00:00Z",
            "hello",
            ActivityResult::Denied,
            Some(ErrorCode::NeedsApproval),
        );
        let c = entry(
            "C",
            "2026-09-24T22:00:00Z",
            "minecraft",
            ActivityResult::Error,
            Some(ErrorCode::Offline),
        );
        for e in [&a, &b, &c] {
            store.insert(e).unwrap();
        }
        assert_eq!(
            store.query(10, None).unwrap(),
            vec![c.clone(), b.clone(), a.clone()]
        );
        assert_eq!(store.query(10, Some("hello")).unwrap(), vec![b, a]);
        assert_eq!(store.query(1, None).unwrap().len(), 1);
        assert_eq!(
            store.query(0, None).unwrap().len(),
            1,
            "limit is clamped to at least 1"
        );
    }
}

#[cfg(test)]
mod backup_tests {
    use super::*;

    #[test]
    fn backs_up_to_a_file() {
        let dir = tempfile::tempdir().unwrap();
        let store = ActivityStore::open(&dir.path().join("node.db")).unwrap();
        let copy = dir.path().join("copy.db");
        store.backup_to(&copy).unwrap();
        let again = ActivityStore::open(&copy).unwrap();
        assert!(again.query(10, None).unwrap().is_empty());
    }

    #[test]
    fn adds_up_assistant_spend() {
        let store = ActivityStore::in_memory().unwrap();
        assert_eq!(store.usage("2026-10"), 0.0);
        store.add_usage("2026-10", 0.25, 1000, 200).unwrap();
        store.add_usage("2026-10", 0.5, 10, 20).unwrap();
        assert!((store.usage("2026-10") - 0.75).abs() < 1e-9);
        assert_eq!(store.usage("2026-11"), 0.0);
    }
}
