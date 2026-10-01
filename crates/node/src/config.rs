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
    /// Also accept clients on the home network. Off: only this machine and Tailscale.
    #[serde(default)]
    pub allow_lan: bool,
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
    #[serde(default)]
    pub update: UpdateConfig,
    #[serde(default)]
    pub notify: NotifyConfig,
    #[serde(default)]
    pub assistant: AssistantConfig,
    /// Actions to run on a timer (`[[schedule]]` tables).
    #[serde(default)]
    pub schedule: Vec<crate::schedule::ScheduleConfig>,
    /// Actions run when an event happens (`[[on_event]]` tables).
    #[serde(default)]
    pub on_event: Vec<crate::automations::OnEventConfig>,
    /// Actions run when a module's status meets a condition for a while (`[[when]]` tables).
    #[serde(default)]
    pub when: Vec<crate::automations::WhenConfig>,
    /// The file this config was loaded from; the update helper restarts kerneld with it.
    #[serde(skip)]
    pub path: Option<PathBuf>,
}

/// Self-update from GitHub Releases (see `update.rs`).
#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct UpdateConfig {
    /// `owner/repo` to take `node-build-*` releases from. Empty turns updates off.
    pub repo: String,
    /// Read-only GitHub token, needed when the repo is private.
    pub token: String,
    /// GitHub API base URL (changed only by tests).
    pub api: String,
    /// How often to look for a new build. 0 = only when asked.
    pub check_interval_h: u64,
    /// Install new builds as soon as they're found, instead of waiting for the button.
    pub auto_install: bool,
    /// `uv`, used to reinstall the Python SDK that ships with each build.
    pub uv: String,
    /// Windows scheduled task that runs kerneld. When set, restarts go through it so its
    /// watchdog keeps covering the new process.
    pub scheduled_task: String,
}

impl Default for UpdateConfig {
    fn default() -> Self {
        Self {
            repo: String::new(),
            token: String::new(),
            api: "https://api.github.com".into(),
            check_interval_h: 6,
            auto_install: false,
            uv: "uv".into(),
            scheduled_task: String::new(),
        }
    }
}

/// The assistant (see `assistant.rs`): a local model first, Claude as the fallback.
#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct AssistantConfig {
    pub enabled: bool,
    /// Tried in order. The first is the VRAM module's turn-taking proxy, the second Ollama itself.
    pub ollama_urls: Vec<String>,
    pub model: String,
    /// Optional. Without it there is no fallback.
    pub anthropic_api_key: String,
    pub claude_model: String,
    /// Claude API spend per calendar month; at the cap, only the local model answers.
    pub monthly_budget_usd: f64,
    /// Tool calls per message, at most.
    pub max_steps: usize,
    /// Kernel's tsundere catgirl voice (D11). Off: plain and neutral.
    pub persona: bool,
    /// Messages API base URL (changed only by tests).
    pub anthropic_api: String,
}

impl Default for AssistantConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            ollama_urls: vec![
                "http://127.0.0.1:11435".into(),
                "http://127.0.0.1:11434".into(),
            ],
            model: "qwen3:8b".into(),
            anthropic_api_key: String::new(),
            claude_model: "claude-haiku-4-5".into(),
            monthly_budget_usd: 3.0,
            max_steps: 8,
            persona: true,
            anthropic_api: "https://api.anthropic.com".into(),
        }
    }
}

/// Alerts for events (see `notify.rs`).
#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct NotifyConfig {
    /// Discord webhook URL (channel settings, Integrations, Webhooks). Empty turns alerts off.
    pub discord_webhook: String,
    /// Send events at this level and above: "info", "warn" or "error".
    pub min_level: String,
    /// Also send these events whatever their level, like "player.joined" or "minecraft.*".
    pub include: Vec<String>,
    /// Never send these, even when the level says so.
    pub mute: Vec<String>,
}

impl Default for NotifyConfig {
    fn default() -> Self {
        Self {
            discord_webhook: String::new(),
            min_level: "warn".into(),
            include: Vec::new(),
            mute: Vec::new(),
        }
    }
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
        // Absolute first: relative paths inside resolve against the file's folder, and
        // modules run with their own working directory.
        let path =
            &std::path::absolute(path).with_context(|| format!("resolving {}", path.display()))?;
        let text =
            std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        let mut cfg: Self =
            toml::from_str(&text).with_context(|| format!("parsing {}", path.display()))?;
        let base = path.parent().unwrap_or(Path::new("."));
        cfg.modules_dir = absolutize(base, &cfg.modules_dir);
        cfg.data_dir = absolutize(base, &cfg.data_dir);
        cfg.path = Some(path.clone());
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
        let repo = &self.update.repo;
        if !repo.is_empty() && repo.split('/').filter(|p| !p.is_empty()).count() != 2 {
            bail!("update.repo must look like owner/repo");
        }
        crate::automations::Automations::new(&self.on_event, &self.when)
            .map_err(|e| anyhow::anyhow!(e))?;
        for s in &self.schedule {
            crate::schedule::parse(&s.at)
                .map_err(|e| anyhow::anyhow!("schedule \"{}\": {e}", s.name))?;
        }
        if kernel_protocol::EventLevel::parse(&self.notify.min_level).is_none() {
            bail!("notify.min_level must be info, warn or error");
        }
        let hook = &self.notify.discord_webhook;
        if !hook.is_empty()
            && !hook.starts_with("https://")
            && !hook.starts_with("http://127.0.0.1")
        {
            bail!("notify.discord_webhook must be an https:// URL");
        }
        Ok(())
    }

    pub fn logs_dir(&self) -> PathBuf {
        self.data_dir.join("logs")
    }

    pub fn db_path(&self) -> PathBuf {
        self.data_dir.join("node.db")
    }

    /// Held for as long as kerneld runs: one node per data folder, and the update helper
    /// waits on it to know the old process is gone.
    pub fn lock_path(&self) -> PathBuf {
        self.data_dir.join("kerneld.lock")
    }

    /// Per-machine settings for each module, kept outside the app folder so updates keep them.
    pub fn module_settings_dir(&self) -> PathBuf {
        self.data_dir.join("settings")
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
    fn relative_config_paths_become_absolute() {
        // A folder in the current directory, so the config can be named by a relative path
        // (changing the working directory would race with other tests).
        let dir = tempfile::tempdir_in(".").unwrap();
        std::fs::write(
            dir.path().join("node.toml"),
            "node_id='a'\nnode_name='A'\ntoken='long-enough'\nmodules_dir='m'\ndata_dir='d'\n",
        )
        .unwrap();
        let relative = Path::new(dir.path().file_name().unwrap()).join("node.toml");
        assert!(relative.is_relative());
        let cfg = Config::load(&relative).unwrap();
        assert!(cfg.data_dir.is_absolute(), "{}", cfg.data_dir.display());
        assert!(cfg.modules_dir.is_absolute());
        assert!(cfg.path.unwrap().is_absolute());
    }

    #[test]
    fn example_config_parses() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("node.example.toml");
        let cfg = Config::load(&path).unwrap();
        assert_eq!(cfg.node_id, "pluto");
        assert!(cfg.enabled_modules.is_empty());
    }
}
