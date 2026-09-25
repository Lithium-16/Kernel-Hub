//! The node's core: runs actions with permission checks and records them in the activity log.

use std::sync::Arc;
use std::time::{Duration, Instant};

use kernel_protocol::{
    ActionInvoke, ActionResult, ActivityEntry, ActivityResult, AiTier, ErrorCode, ErrorInfo,
    ModuleState, NodeInfo, new_id, now_ts,
};

use crate::activity::ActivityStore;
use crate::config::Config;
use crate::mcp::McpError;
use crate::supervisor::Supervisor;

/// Extra time allowed on top of an action's own timeout (which the module enforces).
const TIMEOUT_GRACE: Duration = Duration::from_secs(5);

pub struct Node {
    pub cfg: Config,
    pub supervisor: Arc<Supervisor>,
    pub activity: ActivityStore,
}

impl Node {
    pub fn info(&self) -> NodeInfo {
        NodeInfo {
            id: self.cfg.node_id.clone(),
            name: self.cfg.node_name.clone(),
            version: env!("CARGO_PKG_VERSION").into(),
        }
    }

    pub fn check_token(&self, given: &str) -> bool {
        constant_time_eq(given.as_bytes(), self.cfg.token.as_bytes())
    }

    /// Run an action on behalf of `req.actor`. Every call, allowed or not, is logged.
    pub async fn invoke(&self, req: ActionInvoke) -> ActionResult {
        let started = Instant::now();
        let result = self.run(&req).await;
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
        result
    }

    async fn run(&self, req: &ActionInvoke) -> ActionResult {
        let fail = |code, msg: String| ActionResult::failure(ErrorInfo::new(code, msg));

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

        if !req.actor.kind.is_human() {
            match action.spec.ai {
                AiTier::Safe => {}
                AiTier::Never => {
                    return fail(
                        ErrorCode::NotPermitted,
                        format!("'{}' can only be run with a button", req.action),
                    );
                }
                // Approvals arrive in phase 2. Until then, `confirm` actions are human-only.
                AiTier::Confirm => {
                    return fail(
                        ErrorCode::NeedsApproval,
                        format!("'{}' needs your approval", req.action),
                    );
                }
            }
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
