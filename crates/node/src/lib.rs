//! kerneld: runs modules, exposes them over a WebSocket API, and logs every action.

pub mod activity;
pub mod config;
pub mod manifest;
pub mod mcp;
pub mod netfilter;
pub mod node;
pub mod server;
pub mod supervisor;

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;
use tokio::sync::watch;
use tokio::task::JoinHandle;

use crate::activity::ActivityStore;
use crate::config::Config;
use crate::node::Node;
use crate::supervisor::Supervisor;

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

pub async fn start(cfg: Config) -> anyhow::Result<Running> {
    std::fs::create_dir_all(cfg.logs_dir().join("modules"))
        .with_context(|| format!("creating {}", cfg.data_dir.display()))?;
    let activity = ActivityStore::open(&cfg.db_path()).context("opening activity log")?;

    let mut manifests = Vec::new();
    for found in manifest::discover(&cfg.modules_dir) {
        match found {
            Ok(m) if cfg.enabled_modules.is_empty() || cfg.enabled_modules.contains(&m.id) => {
                manifests.push(m)
            }
            Ok(m) => tracing::debug!(module = %m.id, "module not enabled"),
            Err(e) => tracing::error!(error = %e, "invalid module manifest"),
        }
    }
    tracing::info!(count = manifests.len(), dir = %cfg.modules_dir.display(), "modules discovered");

    let (shutdown, shutdown_rx) = watch::channel(false);
    let (supervisor, mut tasks) = Supervisor::start(manifests, &cfg, shutdown_rx.clone());

    let listener = tokio::net::TcpListener::bind(cfg.listen)
        .await
        .with_context(|| format!("binding {}", cfg.listen))?;
    let addr = listener.local_addr()?;
    let node = Arc::new(Node {
        cfg,
        supervisor,
        activity,
    });

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
