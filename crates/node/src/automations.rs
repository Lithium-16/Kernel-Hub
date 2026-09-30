//! Automations with triggers, from node.toml:
//!
//! - `[[on_event]]`: when an event happens (`comfyui.queue.finished`), run an action.
//! - `[[when]]`: when a module's status meets a condition (`players_online == 0`) for
//!   `for_min` minutes, run an action, once, until the condition stops holding.
//!
//! Runs go through the same permission check and activity log as everything else, as an
//! automation; `confirm` actions need `approved = true`.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use kernel_protocol::{ActionInvoke, Actor, ActorKind, EventLevel};
use serde::Deserialize;
use serde_json::{Map, Value, json};
use tokio::sync::{broadcast, watch};

use crate::node::Node;

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OnEventConfig {
    pub name: String,
    /// An event kind, or module and kind, with an optional trailing `*`: `comfyui.queue.finished`.
    pub event: String,
    pub module: String,
    pub action: String,
    #[serde(default)]
    pub params: Map<String, Value>,
    #[serde(default)]
    pub approved: bool,
    /// Don't run again within this many minutes.
    #[serde(default = "one")]
    pub cooldown_min: f64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WhenConfig {
    pub name: String,
    /// The module whose status is checked.
    pub status: String,
    /// `<key> <op> <value>`: `players_online == 0`, `vram_free_mb < 1024`, `state != running`.
    pub condition: String,
    /// How long the condition must hold.
    #[serde(default)]
    pub for_min: f64,
    pub module: String,
    pub action: String,
    #[serde(default)]
    pub params: Map<String, Value>,
    #[serde(default)]
    pub approved: bool,
}

fn one() -> f64 {
    1.0
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Op {
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Condition {
    pub key: String,
    pub op: Op,
    pub value: Value,
}

pub fn parse_condition(text: &str) -> Result<Condition, String> {
    let bad = || format!("condition '{text}' should look like \"players_online == 0\"");
    let parts: Vec<&str> = text.split_whitespace().collect();
    let [key, op, rest @ ..] = parts.as_slice() else {
        return Err(bad());
    };
    if rest.is_empty() {
        return Err(bad());
    }
    let op = match *op {
        "==" | "=" => Op::Eq,
        "!=" => Op::Ne,
        "<" => Op::Lt,
        "<=" => Op::Le,
        ">" => Op::Gt,
        ">=" => Op::Ge,
        _ => return Err(bad()),
    };
    let raw = rest.join(" ");
    let value = serde_json::from_str(&raw).unwrap_or(Value::String(raw.trim_matches('\'').into()));
    if matches!(op, Op::Lt | Op::Le | Op::Gt | Op::Ge) && !value.is_number() {
        return Err(format!("condition '{text}': <, <=, > and >= need a number"));
    }
    Ok(Condition {
        key: (*key).into(),
        op,
        value,
    })
}

impl Condition {
    /// Against a module's status. A missing key or a null value is never true.
    pub fn holds(&self, status: &Map<String, Value>) -> bool {
        let mut current = status.get(self.key.split('.').next().unwrap_or(""));
        for part in self.key.split('.').skip(1) {
            current = current.and_then(|v| v.get(part));
        }
        let Some(actual) = current.filter(|v| !v.is_null()) else {
            return false;
        };
        match (actual.as_f64(), self.value.as_f64()) {
            (Some(a), Some(b)) => match self.op {
                Op::Eq => a == b,
                Op::Ne => a != b,
                Op::Lt => a < b,
                Op::Le => a <= b,
                Op::Gt => a > b,
                Op::Ge => a >= b,
            },
            _ => match self.op {
                Op::Eq => actual == &self.value,
                Op::Ne => actual != &self.value,
                _ => false,
            },
        }
    }
}

#[derive(Debug, Clone, Default)]
struct RunState {
    last: Option<String>,
    last_result: Option<String>,
}

pub struct Automations {
    pub on_event: Vec<OnEventConfig>,
    pub when: Vec<(WhenConfig, Condition)>,
    state: Mutex<Vec<RunState>>,
}

impl Automations {
    pub fn new(on_event: &[OnEventConfig], when: &[WhenConfig]) -> Result<Self, String> {
        let when = when
            .iter()
            .map(|w| {
                parse_condition(&w.condition)
                    .map(|c| (w.clone(), c))
                    .map_err(|e| format!("when \"{}\": {e}", w.name))
            })
            .collect::<Result<Vec<_>, _>>()?;
        let n = on_event.len() + when.len();
        Ok(Self {
            on_event: on_event.to_vec(),
            when,
            state: Mutex::new(vec![RunState::default(); n]),
        })
    }

    pub fn is_empty(&self) -> bool {
        self.on_event.is_empty() && self.when.is_empty()
    }

    /// For the Node module's status.
    pub fn status(&self) -> Value {
        let state = self.state.lock().expect("automation state");
        let triggers = self
            .on_event
            .iter()
            .map(|a| {
                (
                    a.name.clone(),
                    format!("on {}", a.event),
                    a.module.clone(),
                    a.action.clone(),
                )
            })
            .chain(self.when.iter().map(|(w, _)| {
                let lasting = if w.for_min > 0.0 {
                    format!(" for {}m", w.for_min)
                } else {
                    String::new()
                };
                (
                    w.name.clone(),
                    format!("when {} {}{lasting}", w.status, w.condition),
                    w.module.clone(),
                    w.action.clone(),
                )
            }));
        Value::Array(
            triggers
                .zip(state.iter())
                .map(|((name, trigger, module, action), s)| {
                    json!({
                        "name": name,
                        "trigger": trigger,
                        "action": format!("{module}/{action}"),
                        "last_run": s.last,
                        "last_result": s.last_result,
                    })
                })
                .collect(),
        )
    }
}

async fn run(
    node: &Arc<Node>,
    index: usize,
    name: &str,
    module: &str,
    action: &str,
    params: &Map<String, Value>,
    approved: bool,
) {
    let req = ActionInvoke {
        module: module.into(),
        action: action.into(),
        params: params.clone(),
        actor: Actor {
            kind: ActorKind::Automation,
            reference: Some(format!("automation: {name}")),
        },
        approval_id: None,
    };
    tracing::info!(automation = name, module, action, "running automation");
    let result = node.invoke_scheduled(req, approved).await;
    let outcome = match &result.error {
        None => "ok".to_string(),
        Some(e) => {
            node.events.emit(
                crate::builtin::ID,
                "automation.failed",
                EventLevel::Warn,
                format!("Automation \"{name}\" failed: {}", e.message),
                json!({"automation": name})
                    .as_object()
                    .cloned()
                    .unwrap_or_default(),
            );
            format!("{}: {}", e.code.as_str(), e.message)
        }
    };
    let mut state = node.automations.state.lock().expect("automation state");
    state[index].last = Some(kernel_protocol::now_ts());
    state[index].last_result = Some(outcome);
}

/// Event triggers, until shutdown.
pub async fn on_events(node: Arc<Node>, mut stop: watch::Receiver<bool>) {
    if node.automations.on_event.is_empty() {
        return;
    }
    let mut rx = node.events.subscribe();
    let mut last_run: Vec<Option<Instant>> = vec![None; node.automations.on_event.len()];
    loop {
        let event = tokio::select! {
            e = rx.recv() => e,
            _ = stop.changed() => return,
        };
        let event = match event {
            Ok(e) => e,
            Err(broadcast::error::RecvError::Lagged(_)) => continue,
            Err(broadcast::error::RecvError::Closed) => return,
        };
        // An automation's own failures never trigger automations (no loops).
        if event.kind.starts_with("automation.") {
            continue;
        }
        for (i, a) in node.automations.on_event.iter().enumerate() {
            if !crate::notify::matches(&a.event, &event) {
                continue;
            }
            let cooldown = Duration::from_secs_f64(a.cooldown_min.max(0.0) * 60.0);
            if last_run[i].is_some_and(|t| t.elapsed() < cooldown) {
                continue;
            }
            last_run[i] = Some(Instant::now());
            let (node, a) = (node.clone(), a.clone());
            tokio::spawn(async move {
                run(
                    &node, i, &a.name, &a.module, &a.action, &a.params, a.approved,
                )
                .await;
            });
        }
    }
}

/// Status triggers, checked every `every`, until shutdown.
pub async fn on_status(node: Arc<Node>, every: Duration, mut stop: watch::Receiver<bool>) {
    let offset = node.automations.on_event.len();
    let count = node.automations.when.len();
    if count == 0 {
        return;
    }
    let mut since: Vec<Option<Instant>> = vec![None; count];
    let mut fired = vec![false; count];
    loop {
        tokio::select! {
            _ = tokio::time::sleep(every) => {}
            _ = stop.changed() => return,
        }
        let catalog = node.catalog();
        for (i, (w, cond)) in node.automations.when.iter().enumerate() {
            let holds = catalog
                .iter()
                .find(|m| m.id == w.status)
                .and_then(|m| m.status.as_ref())
                .is_some_and(|s| cond.holds(s));
            if !holds {
                since[i] = None;
                fired[i] = false;
                continue;
            }
            let start = *since[i].get_or_insert_with(Instant::now);
            if !fired[i] && start.elapsed() >= Duration::from_secs_f64(w.for_min.max(0.0) * 60.0) {
                fired[i] = true;
                let (node, w) = (node.clone(), w.clone());
                tokio::spawn(async move {
                    run(
                        &node,
                        offset + i,
                        &w.name,
                        &w.module,
                        &w.action,
                        &w.params,
                        w.approved,
                    )
                    .await;
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn status(v: Value) -> Map<String, Value> {
        v.as_object().unwrap().clone()
    }

    #[test]
    fn parses_and_checks_conditions() {
        let empty = parse_condition("players_online == 0").unwrap();
        assert!(empty.holds(&status(json!({"players_online": 0}))));
        assert!(!empty.holds(&status(json!({"players_online": 2}))));
        assert!(
            !empty.holds(&status(json!({"players_online": null}))),
            "a stopped server isn't empty"
        );
        assert!(!empty.holds(&status(json!({}))));

        let low = parse_condition("vram_free_mb < 1024").unwrap();
        assert!(low.holds(&status(json!({"vram_free_mb": 900}))));
        assert!(!low.holds(&status(json!({"vram_free_mb": "lots"}))));

        let state = parse_condition("state != running").unwrap();
        assert_eq!(state.value, json!("running"));
        assert!(state.holds(&status(json!({"state": "crashed"}))));
        assert!(!state.holds(&status(json!({"state": "running"}))));

        let flag = parse_condition("keepalive == false").unwrap();
        assert!(flag.holds(&status(json!({"keepalive": false}))));

        let nested = parse_condition("services.playit != \"active\"").unwrap();
        assert!(nested.holds(&status(json!({"services": {"playit": "failed"}}))));

        for bad in ["players_online", "a ~ 1", "state > running", "x =="] {
            assert!(parse_condition(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn rows_for_the_node_page() {
        let on: OnEventConfig = toml::from_str(
            "name='Free VRAM'\nevent='comfyui.queue.finished'\nmodule='vram'\naction='vram.free_idle'",
        )
        .unwrap();
        let when: WhenConfig = toml::from_str(
            "name='Stop empty'\nstatus='minecraft'\ncondition='players_online == 0'\nfor_min=30\nmodule='minecraft'\naction='server.stop'\napproved=true",
        )
        .unwrap();
        let a = Automations::new(&[on], &[when]).unwrap();
        let rows = a.status();
        assert_eq!(rows[0]["trigger"], "on comfyui.queue.finished");
        assert_eq!(
            rows[1]["trigger"],
            "when minecraft players_online == 0 for 30m"
        );
        assert!(
            Automations::new(
                &[],
                &[WhenConfig {
                    condition: "nonsense".into(),
                    ..toml::from_str(
                        "name='x'\nstatus='a'\ncondition='a == 1'\nmodule='a'\naction='b.c'"
                    )
                    .unwrap()
                }]
            )
            .is_err()
        );
    }
}
