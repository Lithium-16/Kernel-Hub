//! Sends events to Discord through a channel webhook, filtered by `[notify]`.

use std::time::Duration;

use kernel_protocol::{EventLevel, NodeEvent};
use serde_json::json;
use tokio::sync::{broadcast, watch};

use crate::config::NotifyConfig;

/// Discord allows about 30 webhook messages a minute; stay well under it.
const SPACING: Duration = Duration::from_millis(2_500);

fn rank(level: EventLevel) -> u8 {
    match level {
        EventLevel::Info => 0,
        EventLevel::Warn => 1,
        EventLevel::Error => 2,
    }
}

/// `pattern` names a kind (`player.joined`) or a module and kind (`minecraft.player.joined`);
/// a trailing `*` matches anything after it (`minecraft.*`).
fn matches(pattern: &str, e: &NodeEvent) -> bool {
    let full = format!("{}.{}", e.module, e.kind);
    match pattern.strip_suffix('*') {
        Some(prefix) => e.kind.starts_with(prefix) || full.starts_with(prefix),
        None => pattern == e.kind || pattern == full,
    }
}

pub fn wanted(cfg: &NotifyConfig, e: &NodeEvent) -> bool {
    if cfg.mute.iter().any(|p| matches(p, e)) {
        return false;
    }
    let min = EventLevel::parse(&cfg.min_level).unwrap_or(EventLevel::Warn);
    rank(e.level) >= rank(min) || cfg.include.iter().any(|p| matches(p, e))
}

pub fn discord_text(node_name: &str, module_name: &str, e: &NodeEvent) -> String {
    let icon = match e.level {
        EventLevel::Info => "🟢",
        EventLevel::Warn => "🟡",
        EventLevel::Error => "🔴",
    };
    // No pings from whatever a module or player name contains.
    let message = e.message.replace('@', "@\u{200b}");
    format!("{icon} **{node_name} · {module_name}** {message}")
}

pub struct Notifier {
    pub cfg: NotifyConfig,
    pub node_name: String,
    /// Module id to display name.
    pub names: std::collections::HashMap<String, String>,
    pub http: reqwest::Client,
}

impl Notifier {
    pub async fn run(
        self,
        mut rx: broadcast::Receiver<NodeEvent>,
        mut stop: watch::Receiver<bool>,
    ) {
        loop {
            let event = tokio::select! {
                e = rx.recv() => e,
                _ = stop.changed() => return,
            };
            let event = match event {
                Ok(e) => e,
                Err(broadcast::error::RecvError::Lagged(n)) => {
                    tracing::warn!(skipped = n, "too many events; some alerts were skipped");
                    continue;
                }
                Err(broadcast::error::RecvError::Closed) => return,
            };
            if !wanted(&self.cfg, &event) {
                continue;
            }
            let module = self
                .names
                .get(&event.module)
                .map_or(event.module.as_str(), String::as_str);
            let text = discord_text(&self.node_name, module, &event);
            self.send(&text).await;
            tokio::time::sleep(SPACING).await;
        }
    }

    async fn send(&self, text: &str) {
        let body = json!({"content": text, "allowed_mentions": {"parse": []}});
        for _ in 0..3 {
            let resp = self
                .http
                .post(&self.cfg.discord_webhook)
                .json(&body)
                .send()
                .await;
            match resp {
                Ok(r) if r.status().is_success() => return,
                Ok(r) if r.status().as_u16() == 429 => {
                    let wait = r
                        .json::<serde_json::Value>()
                        .await
                        .ok()
                        .and_then(|v| v["retry_after"].as_f64())
                        .unwrap_or(5.0)
                        .clamp(0.5, 60.0);
                    tokio::time::sleep(Duration::from_secs_f64(wait)).await;
                }
                Ok(r) => {
                    tracing::warn!(status = %r.status(), "Discord refused the alert");
                    return;
                }
                Err(e) => {
                    tracing::warn!(error = %e, "could not reach Discord");
                    tokio::time::sleep(Duration::from_secs(5)).await;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(module: &str, kind: &str, level: EventLevel) -> NodeEvent {
        NodeEvent {
            id: "1".into(),
            ts: "2026-09-30T20:00:00Z".into(),
            node_id: "pluto".into(),
            module: module.into(),
            kind: kind.into(),
            level,
            message: "Steve joined @everyone".into(),
            data: Default::default(),
        }
    }

    #[test]
    fn filters_by_level_include_and_mute() {
        let mut cfg = NotifyConfig::default();
        let joined = event("minecraft", "player.joined", EventLevel::Info);
        let crashed = event("minecraft", "server.crashed", EventLevel::Error);
        let kicked = event("roblox", "game.disconnected", EventLevel::Warn);
        assert!(!wanted(&cfg, &joined));
        assert!(wanted(&cfg, &crashed) && wanted(&cfg, &kicked));

        cfg.include = vec!["player.joined".into()];
        assert!(wanted(&cfg, &joined));
        cfg.include = vec!["minecraft.*".into()];
        assert!(wanted(&cfg, &joined));
        cfg.mute = vec!["roblox.*".into()];
        assert!(!wanted(&cfg, &kicked));
        cfg.min_level = "error".into();
        cfg.mute.clear();
        assert!(!wanted(&cfg, &kicked) && wanted(&cfg, &crashed));
    }

    #[test]
    fn formats_without_pinging() {
        let text = discord_text(
            "Pluto",
            "Minecraft",
            &event("minecraft", "player.joined", EventLevel::Info),
        );
        assert_eq!(
            text,
            "🟢 **Pluto · Minecraft** Steve joined @\u{200b}everyone"
        );
    }
}
