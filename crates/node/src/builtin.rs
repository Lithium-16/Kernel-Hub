//! The node's own module: version, updates and restarts, shown and permissioned like any
//! other module so the desktop app and the palette get buttons for it.

use kernel_protocol::{ActionSpec, AiTier};
use serde_json::json;

pub const ID: &str = "node";
pub const NAME: &str = "Node";
pub const ICON: &str = "server";

fn spec(id: &str, label: &str, icon: &str, description: &str, ai: AiTier) -> ActionSpec {
    ActionSpec {
        id: id.into(),
        label: label.into(),
        icon: Some(icon.into()),
        description: Some(description.into()),
        ai,
        params: Default::default(),
        quiet: false,
    }
}

pub fn actions() -> Vec<ActionSpec> {
    vec![
        spec(
            "update.check",
            "Check for updates",
            "refresh-cw",
            "Asks GitHub for the newest build of the node.",
            AiTier::Safe,
        ),
        spec(
            "update.install",
            "Install update",
            "download",
            "Downloads the newest build and restarts the node on it. Rolls back if it fails.",
            AiTier::Confirm,
        ),
        ActionSpec {
            params: json!({
                "enabled": {"type": "bool", "description": "Install new builds as soon as they're found", "default": true},
            })
            .as_object()
            .cloned()
            .unwrap_or_default(),
            ..spec(
                "update.auto",
                "Auto-update",
                "refresh-ccw-dot",
                "Turns installing new builds by themselves on or off (found every few hours).",
                AiTier::Never,
            )
        },
        ActionSpec {
            quiet: true,
            ..spec(
                "modules.list",
                "List modules",
                "blocks",
                "Every module this build has, and whether it's switched on.",
                AiTier::Safe,
            )
        },
        ActionSpec {
            params: json!({
                "module": {"type": "string", "description": "Module id"},
                "enabled": {"type": "bool", "default": true},
            })
            .as_object()
            .cloned()
            .unwrap_or_default(),
            ..spec(
                "modules.enable",
                "Switch module on or off",
                "toggle-right",
                "Turns a module on or off on this PC and restarts Kernel to apply it.",
                AiTier::Never,
            )
        },
        spec(
            "node.restart",
            "Restart node",
            "rotate-cw",
            "Restarts kerneld and every module it runs.",
            AiTier::Confirm,
        ),
        ActionSpec {
            params: json!({
                "module": {"type": "string", "description": "Module id, or node for kerneld itself", "default": "node"},
                "lines": {"type": "int", "default": 100, "min": 1, "max": 1000},
            })
            .as_object()
            .cloned()
            .unwrap_or_default(),
            quiet: true,
            ..spec(
                "logs.tail",
                "Show log",
                "scroll-text",
                "The last lines of a module's log (or the node's own).",
                AiTier::Safe,
            )
        },
        ActionSpec {
            params: json!({
                "module": {"type": "string", "description": "Module id"},
            })
            .as_object()
            .cloned()
            .unwrap_or_default(),
            quiet: true,
            ..spec(
                "settings.get",
                "Module settings",
                "sliders-horizontal",
                "A module's settings on this PC, with their defaults. Secrets are hidden.",
                AiTier::Safe,
            )
        },
        ActionSpec {
            params: json!({
                "module": {"type": "string", "description": "Module id"},
                "values": {"type": "string", "description": "JSON object of setting: new value"},
            })
            .as_object()
            .cloned()
            .unwrap_or_default(),
            ..spec(
                "settings.set",
                "Change module settings",
                "save",
                "Saves settings for a module on this PC and restarts it.",
                AiTier::Never,
            )
        },
        spec(
            "diag.bundle",
            "Save diagnostics",
            "package",
            "Saves a zip with recent logs, the config (secrets hidden), module states and recent events.",
            AiTier::Safe,
        ),
        spec(
            "backup.now",
            "Back up Kernel's data",
            "database-backup",
            "Copies the activity and event log to the backups folder (also done every night; a week is kept).",
            AiTier::Safe,
        ),
    ]
}
