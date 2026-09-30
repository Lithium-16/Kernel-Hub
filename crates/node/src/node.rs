//! The node's core: runs actions with permission checks and records them in the activity log.

use std::sync::Arc;
use std::time::{Duration, Instant};

use kernel_protocol::{
    ActionInvoke, ActionResult, ActionSpec, ActivityEntry, ActivityResult, Actor, ActorKind,
    AiTier, ErrorCode, ErrorInfo, ModuleInfo, ModuleState, NodeInfo, new_id, now_ts,
};
use serde_json::json;
use tokio::sync::watch;

use crate::activity::ActivityStore;
use crate::builtin;
use crate::config::Config;
use crate::events::EventHub;
use crate::mcp::McpError;
use crate::supervisor::Supervisor;
use crate::update::{self, Updater};

/// Extra time allowed on top of an action's own timeout (which the module enforces).
const TIMEOUT_GRACE: Duration = Duration::from_secs(5);

pub struct Node {
    pub cfg: Config,
    pub supervisor: Arc<Supervisor>,
    pub activity: ActivityStore,
    pub events: Arc<EventHub>,
    pub updater: Updater,
    /// Set to true to ask the process to exit (after handing off to the update helper).
    pub exit: watch::Sender<bool>,
    pub started: Instant,
}

fn fail(code: ErrorCode, message: impl Into<String>) -> ActionResult {
    ActionResult::failure(ErrorInfo::new(code, message))
}

/// The assistant and automations get `safe` actions only, until approvals arrive in phase 2.
fn permission(actor: &Actor, spec: &ActionSpec) -> Option<ActionResult> {
    if actor.kind.is_human() {
        return None;
    }
    match spec.ai {
        AiTier::Safe => None,
        AiTier::Never => Some(fail(
            ErrorCode::NotPermitted,
            format!("'{}' can only be run with a button", spec.id),
        )),
        AiTier::Confirm => Some(fail(
            ErrorCode::NeedsApproval,
            format!("'{}' needs your approval", spec.id),
        )),
    }
}

impl Node {
    pub fn info(&self) -> NodeInfo {
        NodeInfo {
            id: self.cfg.node_id.clone(),
            name: self.cfg.node_name.clone(),
            version: env!("CARGO_PKG_VERSION").into(),
        }
    }

    /// Every module, with the node's own module last.
    pub fn catalog(&self) -> Vec<ModuleInfo> {
        let mut modules = self.supervisor.catalog();
        let mut status = self.updater.status();
        status.insert("uptime_s".into(), json!(self.started.elapsed().as_secs()));
        modules.push(ModuleInfo {
            id: builtin::ID.into(),
            name: builtin::NAME.into(),
            icon: builtin::ICON.into(),
            version: env!("CARGO_PKG_VERSION").into(),
            state: ModuleState::Running,
            actions: builtin::actions(),
            status: Some(status),
        });
        modules
    }

    /// Ask the process to exit in a moment, so the reply to the current action goes out first.
    pub fn exit_soon(self: &Arc<Self>) {
        let node = self.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_secs(1)).await;
            let _ = node.exit.send(true);
        });
    }

    /// Install a new build without a button press (`[update] auto_install`).
    pub async fn auto_install(self: &Arc<Self>) {
        let req = ActionInvoke {
            module: builtin::ID.into(),
            action: "update.install".into(),
            params: Default::default(),
            actor: Actor {
                kind: ActorKind::Automation,
                reference: Some("auto-update".into()),
            },
            approval_id: None,
        };
        let started = Instant::now();
        let result = self.install_update().await;
        self.record(&req, &result, started);
    }

    pub fn check_token(&self, given: &str) -> bool {
        constant_time_eq(given.as_bytes(), self.cfg.token.as_bytes())
    }

    /// Run an action on behalf of `req.actor`. Every call, allowed or not, is logged
    /// (except quiet ones).
    pub async fn invoke(self: &Arc<Self>, req: ActionInvoke) -> ActionResult {
        let started = Instant::now();
        let result = if req.module == builtin::ID {
            self.run_builtin(&req).await
        } else {
            self.run(&req).await
        };
        if !self.is_quiet(&req) {
            self.record(&req, &result, started);
        }
        result
    }

    fn record(&self, req: &ActionInvoke, result: &ActionResult, started: Instant) {
        let (outcome, code) = match &result.error {
            None => (ActivityResult::Ok, None),
            Some(e) if matches!(e.code, ErrorCode::NeedsApproval | ErrorCode::NotPermitted) => {
                (ActivityResult::Denied, Some(e.code))
            }
            Some(e) => (ActivityResult::Error, Some(e.code)),
        };
        let entry = ActivityEntry {
            id: new_id(),
            ts: now_ts(),
            actor: req.actor.clone(),
            node_id: self.cfg.node_id.clone(),
            module: req.module.clone(),
            action: req.action.clone(),
            params: req.params.clone(),
            result: outcome,
            error_code: code,
            duration_ms: started.elapsed().as_millis() as u64,
        };
        if let Err(e) = self.activity.insert(&entry) {
            tracing::error!(error = %e, "failed to write activity log");
        }
    }

    async fn install_update(self: &Arc<Self>) -> ActionResult {
        match self.updater.install().await {
            Ok(Some(build)) => {
                tracing::info!(build, "installing update; restarting");
                self.exit_soon();
                ActionResult::success(json!({
                    "installing": build,
                    "message": "restarting on the new build, back in under a minute"
                }))
            }
            Ok(None) => {
                ActionResult::success(json!({ "update": "current", "build": update::build() }))
            }
            Err(e) => fail(ErrorCode::ModuleFailed, e),
        }
    }

    async fn run_builtin(self: &Arc<Self>, req: &ActionInvoke) -> ActionResult {
        let Some(spec) = builtin::actions().into_iter().find(|a| a.id == req.action) else {
            return fail(
                ErrorCode::InvalidParams,
                format!("module '{}' has no action '{}'", req.module, req.action),
            );
        };
        if let Some(denied) = permission(&req.actor, &spec) {
            return denied;
        }
        match req.action.as_str() {
            "update.check" => match self.updater.check().await {
                Ok(Some(r)) => ActionResult::success(json!({
                    "update": "available", "latest_build": r.build, "build": update::build()
                })),
                Ok(None) => {
                    ActionResult::success(json!({ "update": "current", "build": update::build() }))
                }
                Err(e) => fail(ErrorCode::ModuleFailed, e),
            },
            "update.install" => self.install_update().await,
            "node.restart" => match self.updater.restart() {
                Ok(()) => {
                    self.exit_soon();
                    ActionResult::success(json!({ "message": "restarting, back in a few seconds" }))
                }
                Err(e) => fail(ErrorCode::ModuleFailed, e),
            },
            _ => fail(ErrorCode::Internal, "unhandled built-in action"),
        }
    }

    /// Quiet actions (safe, read-only, polled) stay out of the activity log.
    fn is_quiet(&self, req: &ActionInvoke) -> bool {
        self.supervisor
            .get(&req.module)
            .and_then(|slot| slot.manifest.action(&req.action).map(|a| a.spec.quiet))
            .unwrap_or(false)
    }

    async fn run(&self, req: &ActionInvoke) -> ActionResult {
        let Some(slot) = self.supervisor.get(&req.module) else {
            return fail(
                ErrorCode::InvalidParams,
                format!("unknown module '{}'", req.module),
            );
        };
        let Some(action) = slot.manifest.action(&req.action) else {
            return fail(
                ErrorCode::InvalidParams,
                format!("module '{}' has no action '{}'", req.module, req.action),
            );
        };

        if let Some(denied) = permission(&req.actor, &action.spec) {
            return denied;
        }

        let Some(client) = slot.client() else {
            let why = match slot.state() {
                ModuleState::Failed => format!(
                    "module '{}' failed and needs a restart ({})",
                    req.module,
                    slot.last_error().unwrap_or_default()
                ),
                _ => format!("module '{}' is not running", req.module),
            };
            return fail(ErrorCode::Offline, why);
        };

        match client
            .call_tool(
                &action.tool_name(),
                &req.params,
                action.timeout + TIMEOUT_GRACE,
            )
            .await
        {
            Ok(Ok(value)) => ActionResult::success(value),
            Ok(Err(info)) => ActionResult::failure(info),
            Err(McpError::Timeout) => fail(
                ErrorCode::Timeout,
                format!("'{}' did not answer in time", req.action),
            ),
            Err(McpError::Closed) => fail(
                ErrorCode::ModuleFailed,
                format!("module '{}' exited during the action", req.module),
            ),
            Err(e) => fail(ErrorCode::ModuleFailed, e.to_string()),
        }
    }
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

#[cfg(test)]
mod tests {
    use super::constant_time_eq;

    #[test]
    fn compares_tokens() {
        assert!(constant_time_eq(b"secret-token", b"secret-token"));
        assert!(!constant_time_eq(b"secret-token", b"secret-tokeN"));
        assert!(!constant_time_eq(b"short", b"longer"));
    }
}
