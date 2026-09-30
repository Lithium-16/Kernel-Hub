//! kerneld: runs modules, exposes them over a WebSocket API, and logs every action.

pub mod activity;
pub mod builtin;
pub mod config;
pub mod events;
pub mod manifest;
pub mod mcp;
pub mod netfilter;
pub mod node;
pub mod notify;
pub mod server;
pub mod supervisor;
pub mod update;

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::Context;
use tokio::sync::watch;
use tokio::task::JoinHandle;

use crate::activity::ActivityStore;
use crate::config::Config;
use crate::events::EventHub;
use crate::node::Node;
use crate::supervisor::Supervisor;
use crate::update::Updater;

pub struct Running {
    pub addr: SocketAddr,
    pub node: Arc<Node>,
    shutdown: watch::Sender<bool>,
    tasks: Vec<JoinHandle<()>>,
}

impl Running {
    /// Stop the server and all modules, waiting up to 10 seconds.
    pub async fn shutdown(self) {
        let _ = self.shutdown.send(true);
        let all = futures::future::join_all(self.tasks);
        if tokio::time::timeout(Duration::from_secs(10), all)
            .await
            .is_err()
        {
            tracing::warn!("shutdown timed out");
        }
    }
}

/// Look for new builds a minute after start, then every `every`.
async fn check_for_updates(node: Arc<Node>, every: Duration, mut stop: watch::Receiver<bool>) {
    let mut wait = Duration::from_secs(60);
    let mut announced = 0;
    loop {
        tokio::select! {
            _ = tokio::time::sleep(wait) => {}
            _ = stop.changed() => return,
        }
        wait = every;
        match node.updater.check().await {
            Ok(Some(r)) if node.cfg.update.auto_install => {
                tracing::info!(build = r.build, "new build found; installing");
                node.auto_install().await;
            }
            Ok(Some(r)) => {
                tracing::info!(build = r.build, "new build available");
                if r.build != announced {
                    announced = r.build;
                    node.events.emit(
                        builtin::ID,
                        "update.available",
                        kernel_protocol::EventLevel::Info,
                        format!("Build {} is out (running {})", r.build, update::build()),
                        Default::default(),
                    );
                }
            }
            Ok(None) => tracing::debug!("up to date"),
            Err(e) => tracing::warn!(error = %e, "update check failed"),
        }
    }
}

pub async fn start(cfg: Config) -> anyhow::Result<Running> {
    std::fs::create_dir_all(cfg.logs_dir().join("modules"))
        .with_context(|| format!("creating {}", cfg.data_dir.display()))?;
    let activity = ActivityStore::open(&cfg.db_path()).context("opening activity log")?;
    let events =
        Arc::new(EventHub::open(&cfg.db_path(), &cfg.node_id).context("opening event log")?);

    let mut manifests = Vec::new();
    for found in manifest::discover(&cfg.modules_dir) {
        match found {
            Ok(m) if m.id == builtin::ID => {
                tracing::error!(dir = %m.dir.display(), "module id 'node' is reserved for the node itself")
            }
            Ok(m) if cfg.enabled_modules.is_empty() || cfg.enabled_modules.contains(&m.id) => {
                manifests.push(m)
            }
            Ok(m) => tracing::debug!(module = %m.id, "module not enabled"),
            Err(e) => tracing::error!(error = %e, "invalid module manifest"),
        }
    }
    tracing::info!(count = manifests.len(), dir = %cfg.modules_dir.display(), "modules discovered");

    let (shutdown, shutdown_rx) = watch::channel(false);
    let (supervisor, mut tasks) =
        Supervisor::start(manifests, &cfg, events.clone(), shutdown_rx.clone());

    let listener = tokio::net::TcpListener::bind(cfg.listen)
        .await
        .with_context(|| format!("binding {}", cfg.listen))?;
    let addr = listener.local_addr()?;
    let (exit, _) = watch::channel(false);
    let updater = Updater::new(&cfg);
    let node = Arc::new(Node {
        cfg,
        supervisor,
        activity,
        events,
        updater,
        exit,
        started: Instant::now(),
    });
    let interval = node.cfg.update.check_interval_h;
    if node.updater.enabled() && interval > 0 {
        tasks.push(tokio::spawn(check_for_updates(
            node.clone(),
            Duration::from_secs(interval * 3600),
            shutdown_rx.clone(),
        )));
    }

    if !node.cfg.notify.discord_webhook.is_empty() {
        let notifier = notify::Notifier {
            cfg: node.cfg.notify.clone(),
            node_name: node.cfg.node_name.clone(),
            names: node.catalog().into_iter().map(|m| (m.id, m.name)).collect(),
            http: reqwest::Client::builder()
                .user_agent(concat!("kerneld/", env!("CARGO_PKG_VERSION")))
                .timeout(Duration::from_secs(20))
                .build()
                .context("building HTTP client")?,
        };
        tasks.push(tokio::spawn(
            notifier.run(node.events.subscribe(), shutdown_rx.clone()),
        ));
    }

    let app = server::router(server::AppState {
        node: node.clone(),
        shutdown: shutdown_rx.clone(),
    });
    let mut stop = shutdown_rx;
    tasks.push(tokio::spawn(async move {
        let serve = axum::serve(
            listener,
            app.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .with_graceful_shutdown(async move {
            let _ = stop.changed().await;
        });
        if let Err(e) = serve.await {
            tracing::error!(error = %e, "server error");
        }
    }));
    tracing::info!(%addr, node = %node.cfg.node_id, "kerneld listening");

    Ok(Running {
        addr,
        node,
        shutdown,
        tasks,
    })
}
