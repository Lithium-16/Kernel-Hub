use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, bail};
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub node_id: String,
    pub node_name: String,
    #[serde(default = "default_listen")]
    pub listen: SocketAddr,
    /// Shared secret clients send in `hello`. Replaced by pairing tokens later in phase 0.
    pub token: String,
    pub modules_dir: PathBuf,
    pub data_dir: PathBuf,
    /// Python interpreter used for `runtime = "python"` modules (must have kernel_sdk installed).
    #[serde(default = "default_python")]
    pub python: String,
    /// Node.js binary used for `runtime = "node"` modules.
    #[serde(default = "default_node")]
    pub node: String,
    /// Only start these modules (all discovered modules when empty).
    #[serde(default)]
    pub enabled_modules: Vec<String>,
    #[serde(default)]
    pub supervisor: SupervisorConfig,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SupervisorConfig {
    pub ping_interval_ms: u64,
    pub ping_misses: u32,
    pub status_interval_ms: u64,
    pub start_timeout_ms: u64,
    pub backoff_initial_ms: u64,
    pub backoff_max_ms: u64,
    pub max_crashes: usize,
    pub crash_window_s: u64,
}

impl Default for SupervisorConfig {
    fn default() -> Self {
        Self {
            ping_interval_ms: 10_000,
            ping_misses: 3,
            status_interval_ms: 2_000,
            start_timeout_ms: 15_000,
            backoff_initial_ms: 1_000,
            backoff_max_ms: 60_000,
            max_crashes: 5,
            crash_window_s: 300,
        }
    }
}

impl SupervisorConfig {
    pub fn ping_interval(&self) -> Duration {
        Duration::from_millis(self.ping_interval_ms)
    }
    pub fn status_interval(&self) -> Duration {
        Duration::from_millis(self.status_interval_ms)
    }
    pub fn start_timeout(&self) -> Duration {
        Duration::from_millis(self.start_timeout_ms)
    }
    pub fn crash_window(&self) -> Duration {
        Duration::from_secs(self.crash_window_s)
    }
}

fn default_listen() -> SocketAddr {
    "127.0.0.1:47800".parse().expect("valid default address")
}

fn default_python() -> String {
    if cfg!(windows) {
        "python".into()
    } else {
        "python3".into()
    }
}

fn default_node() -> String {
    "node".into()
}

impl Config {
    pub fn load(path: &Path) -> anyhow::Result<Self> {
        let text =
            std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        let mut cfg: Self =
            toml::from_str(&text).with_context(|| format!("parsing {}", path.display()))?;
        let base = path.parent().unwrap_or(Path::new("."));
        cfg.modules_dir = absolutize(base, &cfg.modules_dir);
        cfg.data_dir = absolutize(base, &cfg.data_dir);
        cfg.validate()?;
        Ok(cfg)
    }

    pub fn validate(&self) -> anyhow::Result<()> {
        if self.node_id.is_empty() || self.node_name.is_empty() {
            bail!("node_id and node_name must not be empty");
        }
        if self.token.len() < 8 {
            bail!("token must be at least 8 characters");
        }
        Ok(())
    }

    pub fn logs_dir(&self) -> PathBuf {
        self.data_dir.join("logs")
    }

    pub fn db_path(&self) -> PathBuf {
        self.data_dir.join("node.db")
    }
}

fn absolutize(base: &Path, p: &Path) -> PathBuf {
    if p.is_absolute() {
        p.to_path_buf()
    } else {
        base.join(p)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loads_and_resolves_relative_paths() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("node.toml");
        std::fs::write(
            &path,
            r#"
node_id = "pluto"
node_name = "Pluto"
token = "a-long-dev-token"
modules_dir = "modules"
data_dir = "data"
[supervisor]
ping_interval_ms = 500
"#,
        )
        .unwrap();
        let cfg = Config::load(&path).unwrap();
        assert_eq!(cfg.modules_dir, dir.path().join("modules"));
        assert_eq!(cfg.listen.port(), 47800);
        assert_eq!(cfg.supervisor.ping_interval_ms, 500);
        assert_eq!(cfg.supervisor.max_crashes, 5);
    }

    #[test]
    fn rejects_short_token_and_unknown_keys() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("node.toml");
        std::fs::write(
            &path,
            "node_id='a'\nnode_name='A'\ntoken='short'\nmodules_dir='m'\ndata_dir='d'\n",
        )
        .unwrap();
        assert!(Config::load(&path).is_err());
        std::fs::write(&path, "node_id='a'\nnode_name='A'\ntoken='long-enough'\nmodules_dir='m'\ndata_dir='d'\nbogus=1\n").unwrap();
        assert!(Config::load(&path).is_err());
    }

    #[test]
    fn example_config_parses() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("node.example.toml");
        let cfg = Config::load(&path).unwrap();
        assert_eq!(cfg.node_id, "pluto");
        assert!(cfg.enabled_modules.is_empty());
    }
}
