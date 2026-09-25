//! The node's own module: version, updates and restarts, shown and permissioned like any
//! other module so the desktop app and the palette get buttons for it.

use kernel_protocol::{ActionSpec, AiTier};

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
        spec(
            "node.restart",
            "Restart node",
            "rotate-cw",
            "Restarts kerneld and every module it runs.",
            AiTier::Confirm,
        ),
    ]
}
