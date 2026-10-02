//! Choices made from the app rather than in node.toml: install updates automatically, and which
//! modules run. Kept in `<data_dir>/node-prefs.json`; anything set here wins over node.toml.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::config::Config;

const FILE: &str = "node-prefs.json";

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Prefs {
    /// Overrides `[update] auto_install`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auto_install: Option<bool>,
    /// Module id -> on/off; overrides `enabled_modules` for the modules listed.
    pub modules: BTreeMap<String, bool>,
}

impl Prefs {
    pub fn path(cfg: &Config) -> PathBuf {
        cfg.data_dir.join(FILE)
    }

    /// A missing or unreadable file is no prefs (node.toml decides).
    pub fn load(path: &Path) -> Self {
        std::fs::read(path)
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_vec_pretty(self)?)?;
        std::fs::rename(tmp, path)
    }

    pub fn auto_install(&self, cfg: &Config) -> bool {
        self.auto_install.unwrap_or(cfg.update.auto_install)
    }

    /// Whether module `id` should run: the app's switch if it was used, else node.toml
    /// (an empty `enabled_modules` runs everything).
    pub fn module_enabled(&self, cfg: &Config, id: &str) -> bool {
        match self.modules.get(id) {
            Some(on) => *on,
            None => cfg.enabled_modules.is_empty() || cfg.enabled_modules.iter().any(|m| m == id),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn switches_win_over_node_toml() {
        let dir = tempfile::tempdir().unwrap();
        let mut cfg: Config = toml::from_str(
            r#"
            node_id = "pluto"
            node_name = "Pluto"
            listen = "127.0.0.1:0"
            token = "t"
            modules_dir = "m"
            data_dir = "d"
            enabled_modules = ["minecraft"]
            "#,
        )
        .unwrap();
        cfg.data_dir = dir.path().into();
        let path = Prefs::path(&cfg);
        let mut p = Prefs::load(&path);
        assert_eq!(p, Prefs::default());
        assert!(p.module_enabled(&cfg, "minecraft") && !p.module_enabled(&cfg, "flowrace"));
        assert!(!p.auto_install(&cfg));

        p.modules.insert("flowrace".into(), true);
        p.modules.insert("minecraft".into(), false);
        p.auto_install = Some(true);
        p.save(&path).unwrap();
        let p = Prefs::load(&path);
        assert!(p.module_enabled(&cfg, "flowrace") && !p.module_enabled(&cfg, "minecraft"));
        assert!(p.auto_install(&cfg));

        std::fs::write(&path, "not json").unwrap();
        assert_eq!(Prefs::load(&path), Prefs::default());
    }
}
