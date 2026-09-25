// Release builds on Windows have no console: kerneld runs in the background at logon and must
// never pop a window (Pluto is also the Roblox AFK machine). Logs go to files.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

use std::path::PathBuf;

use anyhow::{Context, bail};
use kernel_node::config::Config;
use kernel_node::update;
use tracing_subscriber::EnvFilter;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

const USAGE: &str = "usage: kerneld --config <node.toml>   (or set KERNEL_CONFIG)
       kerneld apply-update --config <node.toml> --start <kerneld> [--staging <dir>]";

enum Cmd {
    Run(PathBuf),
    Apply(update::ApplyArgs),
}

fn parse() -> anyhow::Result<Cmd> {
    let mut args = std::env::args().skip(1);
    match args.next().as_deref() {
        Some("--config" | "-c") => Ok(Cmd::Run(
            args.next()
                .map(PathBuf::from)
                .context("--config needs a path")?,
        )),
        Some("apply-update") => {
            let (mut config, mut start, mut staging) = (None, None, None);
            while let Some(flag) = args.next() {
                let value = args
                    .next()
                    .map(PathBuf::from)
                    .with_context(|| format!("{flag} needs a value"))?;
                match flag.as_str() {
                    "--config" => config = Some(value),
                    "--start" => start = Some(value),
                    "--staging" => staging = Some(value),
                    other => bail!("unknown argument '{other}'"),
                }
            }
            Ok(Cmd::Apply(update::ApplyArgs {
                config: config.context("apply-update needs --config")?,
                start: start.context("apply-update needs --start")?,
                staging,
            }))
        }
        Some("--version" | "-V") => {
            println!(
                "kerneld {} (build {})",
                env!("CARGO_PKG_VERSION"),
                update::build()
            );
            std::process::exit(0);
        }
        Some("--help" | "-h") => {
            println!("{USAGE}");
            std::process::exit(0);
        }
        Some(other) => bail!("unknown argument '{other}'\n{USAGE}"),
        None => Ok(Cmd::Run(
            std::env::var_os("KERNEL_CONFIG")
                .map(PathBuf::from)
                .context("no config: pass --config or set KERNEL_CONFIG")?,
        )),
    }
}

fn logging(
    cfg: &Config,
    file_name: &str,
) -> anyhow::Result<tracing_appender::non_blocking::WorkerGuard> {
    std::fs::create_dir_all(cfg.logs_dir())?;
    let file = tracing_appender::rolling::daily(cfg.logs_dir(), file_name);
    let (file, guard) = tracing_appender::non_blocking(file);
    tracing_subscriber::registry()
        .with(EnvFilter::try_from_env("KERNEL_LOG").unwrap_or_else(|_| EnvFilter::new("info")))
        .with(tracing_subscriber::fmt::layer().with_writer(std::io::stderr))
        .with(tracing_subscriber::fmt::layer().json().with_writer(file))
        .init();
    Ok(guard)
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let path = match parse()? {
        Cmd::Run(path) => path,
        Cmd::Apply(args) => {
            let cfg = Config::load(&args.config)?;
            let _guard = logging(&cfg, "update.log")?;
            tracing::info!(staging = ?args.staging, "applying update");
            return update::apply(&args)
                .inspect_err(|e| tracing::error!(error = %e, "update failed"));
        }
    };
    let cfg = Config::load(&path)?;
    let _guard = logging(&cfg, "kerneld.log")?;
    let Some(_lock) = update::acquire_lock(&cfg.lock_path())? else {
        bail!(
            "another kerneld is already running for {}",
            cfg.data_dir.display()
        );
    };
    tracing::info!(
        version = env!("CARGO_PKG_VERSION"),
        build = update::build(),
        "starting"
    );

    let running = kernel_node::start(cfg).await?;
    let mut exit = running.node.exit.subscribe();
    tokio::select! {
        r = tokio::signal::ctrl_c() => r.context("waiting for ctrl-c")?,
        _ = exit.changed() => tracing::info!("exit requested"),
    }
    tracing::info!("shutting down");
    running.shutdown().await;
    Ok(())
}
