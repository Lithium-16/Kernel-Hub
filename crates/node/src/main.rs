use std::path::PathBuf;

use anyhow::Context;
use kernel_node::config::Config;
use tracing_subscriber::EnvFilter;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

fn config_path() -> anyhow::Result<PathBuf> {
    let mut args = std::env::args().skip(1);
    match args.next().as_deref() {
        Some("--config" | "-c") => args
            .next()
            .map(PathBuf::from)
            .context("--config needs a path"),
        Some("--version" | "-V") => {
            println!("kerneld {}", env!("CARGO_PKG_VERSION"));
            std::process::exit(0);
        }
        Some("--help" | "-h") => {
            println!("usage: kerneld --config <node.toml>   (or set KERNEL_CONFIG)");
            std::process::exit(0);
        }
        Some(other) => anyhow::bail!("unknown argument '{other}'"),
        None => std::env::var_os("KERNEL_CONFIG")
            .map(PathBuf::from)
            .context("no config: pass --config or set KERNEL_CONFIG"),
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cfg = Config::load(&config_path()?)?;

    std::fs::create_dir_all(cfg.logs_dir())?;
    let file = tracing_appender::rolling::daily(cfg.logs_dir(), "kerneld.log");
    let (file, _guard) = tracing_appender::non_blocking(file);
    tracing_subscriber::registry()
        .with(EnvFilter::try_from_env("KERNEL_LOG").unwrap_or_else(|_| EnvFilter::new("info")))
        .with(tracing_subscriber::fmt::layer().with_writer(std::io::stderr))
        .with(tracing_subscriber::fmt::layer().json().with_writer(file))
        .init();

    let running = kernel_node::start(cfg).await?;
    tokio::signal::ctrl_c()
        .await
        .context("waiting for ctrl-c")?;
    tracing::info!("shutting down");
    running.shutdown().await;
    Ok(())
}
