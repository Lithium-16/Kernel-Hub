//! Starts each module as a child process, keeps it healthy, and restarts it with backoff.

use std::collections::{BTreeMap, VecDeque};
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

use kernel_protocol::{ModuleInfo, ModuleState};
use serde_json::{Map, Value};
use tokio::process::Command;
use tokio::sync::{Notify, watch};
use tokio::task::JoinHandle;

use crate::config::{Config, SupervisorConfig};
use crate::manifest::{Manifest, Runtime};
use crate::mcp::McpClient;

/// Environment variables passed through to modules. Everything else is dropped.
const ENV_PASSTHROUGH: &[&str] = &[
    "PATH",
    "SYSTEMROOT",
    "SystemRoot",
    "WINDIR",
    "TEMP",
    "TMP",
    "HOME",
    "USERPROFILE",
    "LOCALAPPDATA",
    "APPDATA",
    "LANG",
    "LC_ALL",
];

#[derive(Default)]
struct SlotState {
    state: Option<ModuleState>,
    client: Option<Arc<McpClient>>,
    status: Option<Map<String, Value>>,
    last_error: Option<String>,
}

pub struct Slot {
    pub manifest: Manifest,
    inner: RwLock<SlotState>,
    restart: Notify,
}

impl Slot {
    fn new(manifest: Manifest) -> Self {
        Self {
            manifest,
            inner: RwLock::default(),
            restart: Notify::new(),
        }
    }

    pub fn state(&self) -> ModuleState {
        self.inner
            .read()
            .expect("slot lock")
            .state
            .unwrap_or(ModuleState::Starting)
    }

    pub fn last_error(&self) -> Option<String> {
        self.inner.read().expect("slot lock").last_error.clone()
    }

    /// The live client, only while the module is running.
    pub fn client(&self) -> Option<Arc<McpClient>> {
        let s = self.inner.read().expect("slot lock");
        match s.state {
            Some(ModuleState::Running) => s.client.clone(),
            _ => None,
        }
    }

    pub fn info(&self) -> ModuleInfo {
        let s = self.inner.read().expect("slot lock");
        let m = &self.manifest;
        ModuleInfo {
            id: m.id.clone(),
            name: m.name.clone(),
            icon: m.icon.clone(),
            version: m.version.clone(),
            state: s.state.unwrap_or(ModuleState::Starting),
            actions: m.actions.iter().map(|a| a.spec.clone()).collect(),
            status: s.status.clone(),
        }
    }

    fn set_state(&self, state: ModuleState) {
        self.inner.write().expect("slot lock").state = Some(state);
    }

    fn set_running(&self, client: Arc<McpClient>) {
        let mut s = self.inner.write().expect("slot lock");
        s.state = Some(ModuleState::Running);
        s.client = Some(client);
        s.last_error = None;
    }

    fn set_down(&self, state: ModuleState, error: Option<String>) {
        let mut s = self.inner.write().expect("slot lock");
        s.state = Some(state);
        s.client = None;
        s.status = None;
        if error.is_some() {
            s.last_error = error;
        }
    }

    fn set_status(&self, status: Map<String, Value>) {
        self.inner.write().expect("slot lock").status = Some(status);
    }
}

#[derive(Clone)]
struct RunCtx {
    cfg: SupervisorConfig,
    python: String,
    node: String,
    logs_dir: PathBuf,
    settings_dir: PathBuf,
}

pub struct Supervisor {
    slots: BTreeMap<String, Arc<Slot>>,
}

impl Supervisor {
    pub fn start(
        manifests: Vec<Manifest>,
        cfg: &Config,
        shutdown: watch::Receiver<bool>,
    ) -> (Arc<Self>, Vec<JoinHandle<()>>) {
        let ctx = RunCtx {
            cfg: cfg.supervisor.clone(),
            python: cfg.python.clone(),
            node: cfg.node.clone(),
            logs_dir: cfg.logs_dir().join("modules"),
            settings_dir: cfg.module_settings_dir(),
        };
        let mut slots = BTreeMap::new();
        let mut handles = Vec::new();
        for manifest in manifests {
            let slot = Arc::new(Slot::new(manifest));
            slots.insert(slot.manifest.id.clone(), slot.clone());
            handles.push(tokio::spawn(run_slot(slot, ctx.clone(), shutdown.clone())));
        }
        (Arc::new(Self { slots }), handles)
    }

    pub fn get(&self, id: &str) -> Option<Arc<Slot>> {
        self.slots.get(id).cloned()
    }

    pub fn catalog(&self) -> Vec<ModuleInfo> {
        self.slots.values().map(|s| s.info()).collect()
    }

    /// Ask a failed module to start again. Returns false for unknown modules.
    pub fn restart(&self, id: &str) -> bool {
        self.slots.get(id).map(|s| s.restart.notify_one()).is_some()
    }
}

enum Exit {
    Shutdown,
    Crashed(String),
}

async fn run_slot(slot: Arc<Slot>, ctx: RunCtx, mut shutdown: watch::Receiver<bool>) {
    let id = slot.manifest.id.clone();
    let initial = Duration::from_millis(ctx.cfg.backoff_initial_ms);
    let max = Duration::from_millis(ctx.cfg.backoff_max_ms);
    let mut backoff = initial;
    let mut crashes: VecDeque<Instant> = VecDeque::new();

    loop {
        if *shutdown.borrow() {
            slot.set_down(ModuleState::Stopped, None);
            return;
        }
        slot.set_state(ModuleState::Starting);
        let started = Instant::now();
        match run_once(&slot, &ctx, &mut shutdown).await {
            Exit::Shutdown => {
                slot.set_down(ModuleState::Stopped, None);
                tracing::info!(module = %id, "module stopped");
                return;
            }
            Exit::Crashed(reason) => {
                tracing::warn!(module = %id, %reason, "module exited");
                let now = Instant::now();
                crashes.push_back(now);
                while crashes
                    .front()
                    .is_some_and(|t| now.duration_since(*t) > ctx.cfg.crash_window())
                {
                    crashes.pop_front();
                }
                if started.elapsed() > Duration::from_secs(60) {
                    backoff = initial;
                }
                if crashes.len() > ctx.cfg.max_crashes {
                    slot.set_down(
                        ModuleState::Failed,
                        Some(format!("crashed {} times: {reason}", crashes.len())),
                    );
                    tracing::error!(module = %id, "module failed; waiting for a manual restart");
                    tokio::select! {
                        _ = slot.restart.notified() => {}
                        _ = shutdown.changed() => {}
                    }
                    crashes.clear();
                    backoff = initial;
                    continue;
                }
                slot.set_down(ModuleState::Starting, Some(reason));
                tokio::select! {
                    _ = tokio::time::sleep(backoff) => {}
                    _ = shutdown.changed() => {}
                }
                backoff = (backoff * 2).min(max);
            }
        }
    }
}

fn command_for(m: &Manifest, ctx: &RunCtx) -> Command {
    let program = match m.runtime {
        Runtime::Python => &ctx.python,
        Runtime::Node => &ctx.node,
    };
    let mut cmd = Command::new(program);
    cmd.arg(&m.entry).current_dir(&m.dir).env_clear();
    for key in ENV_PASSTHROUGH {
        if let Ok(v) = std::env::var(key) {
            cmd.env(key, v);
        }
    }
    cmd.env("KERNEL_MODULE_ID", &m.id)
        .env("KERNEL_MODULE_DIR", &m.dir)
        // Per-machine settings live in the data folder so updates don't replace them.
        .env(
            "KERNEL_SETTINGS_FILE",
            ctx.settings_dir.join(format!("{}.toml", m.id)),
        )
        .env("KERNEL_LOG_LEVEL", "INFO")
        .env("PYTHONUNBUFFERED", "1")
        .env("PYTHONIOENCODING", "utf-8")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .kill_on_drop(true);
    // kerneld has no console in release builds; without this every module would get a window.
    #[cfg(windows)]
    cmd.creation_flags(crate::update::CREATE_NO_WINDOW);
    let log = std::fs::create_dir_all(&ctx.logs_dir).and_then(|_| {
        std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(ctx.logs_dir.join(format!("{}.log", m.id)))
    });
    cmd.stderr(match log {
        Ok(f) => Stdio::from(f),
        Err(_) => Stdio::null(),
    });
    cmd
}

async fn run_once(slot: &Slot, ctx: &RunCtx, shutdown: &mut watch::Receiver<bool>) -> Exit {
    let m = &slot.manifest;
    let mut child = match command_for(m, ctx).spawn() {
        Ok(c) => c,
        Err(e) => return Exit::Crashed(format!("failed to start: {e}")),
    };
    let (Some(stdin), Some(stdout)) = (child.stdin.take(), child.stdout.take()) else {
        return Exit::Crashed("no stdio pipes".into());
    };
    let client = McpClient::start(stdout, stdin);

    tokio::select! {
        r = client.initialize(ctx.cfg.start_timeout()) => {
            if let Err(e) = r {
                let _ = child.kill().await;
                return Exit::Crashed(format!("handshake failed: {e}"));
            }
        }
        status = child.wait() => {
            return Exit::Crashed(format!("exited during start-up ({})", describe(status)));
        }
        _ = shutdown.changed() => {
            let _ = child.kill().await;
            return Exit::Shutdown;
        }
    }

    slot.set_running(client.clone());
    tracing::info!(module = %m.id, pid = child.id(), "module running");
    let rpc_timeout = ctx.cfg.ping_interval().min(Duration::from_secs(5));
    if let Ok(status) = client.read_status(rpc_timeout).await {
        slot.set_status(status);
    }

    let mut ping = tokio::time::interval_at(
        tokio::time::Instant::now() + ctx.cfg.ping_interval(),
        ctx.cfg.ping_interval(),
    );
    let mut poll = tokio::time::interval_at(
        tokio::time::Instant::now() + ctx.cfg.status_interval(),
        ctx.cfg.status_interval(),
    );
    let mut misses = 0;

    loop {
        tokio::select! {
            status = child.wait() => return Exit::Crashed(format!("exited ({})", describe(status))),
            _ = ping.tick() => {
                if client.ping(rpc_timeout).await.is_ok() {
                    misses = 0;
                } else {
                    misses += 1;
                    if misses >= ctx.cfg.ping_misses {
                        let _ = child.kill().await;
                        return Exit::Crashed(format!("stopped responding ({misses} missed pings)"));
                    }
                }
            }
            _ = poll.tick() => {
                if let Ok(status) = client.read_status(rpc_timeout).await {
                    slot.set_status(status);
                }
            }
            _ = shutdown.changed() => {
                client.close();
                if tokio::time::timeout(Duration::from_secs(5), child.wait()).await.is_err() {
                    let _ = child.kill().await;
                }
                return Exit::Shutdown;
            }
        }
    }
}

fn describe(status: std::io::Result<std::process::ExitStatus>) -> String {
    match status {
        Ok(s) => s.to_string(),
        Err(e) => e.to_string(),
    }
}
