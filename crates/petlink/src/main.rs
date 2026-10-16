#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]
// On other systems only the portable logic builds (CI tests it); the overlay is Windows-only.
#![cfg_attr(not(windows), allow(dead_code))]
#[cfg(windows)]
mod app;
mod config;
mod files;
mod fx;
#[cfg(windows)]
mod mailui;
mod net;
mod pet;
mod protocol;
mod social;
mod sprites;
#[cfg(windows)]
mod tray;
mod update;
#[cfg(windows)]
mod win;
mod world;

#[cfg(not(windows))]
fn main() {
    println!("PetLink runs on Windows");
}

#[cfg(windows)]
fn init_logging(dir: &std::path::Path) -> tracing_appender::non_blocking::WorkerGuard {
    let (w, guard) =
        tracing_appender::non_blocking(tracing_appender::rolling::daily(dir, "petlink.log"));
    tracing_subscriber::fmt()
        .with_writer(w)
        .with_ansi(false)
        .init();
    std::panic::set_hook(Box::new(|info| tracing::error!("panic: {info}")));
    guard
}

#[cfg(windows)]
fn main() -> anyhow::Result<()> {
    let dir = config::Config::dir();
    std::fs::create_dir_all(dir.join("logs"))?;
    let _guard = init_logging(&dir.join("logs"));
    update::cleanup_old();
    win::set_dpi_aware();
    // After an update the old copy may still be exiting: give it up to 5 s.
    let name = instance_name(&dir);
    let mut tries = 0;
    while !win::single_instance(&name) {
        tries += 1;
        if tries >= 10 {
            tracing::info!("another PetLink is already running");
            return Ok(());
        }
        std::thread::sleep(std::time::Duration::from_millis(500));
    }
    let config = match config::Config::load(&dir)? {
        Some(c) => c,
        None => match mailui::ask_for_invite()? {
            Some(c) => {
                c.save(&dir)?;
                // winit allows one event loop per process: start over as a fresh process.
                std::process::Command::new(std::env::current_exe()?).spawn()?;
                return Ok(());
            }
            None => return Ok(()),
        },
    };
    update::spawn_checker();
    tracing::info!("PetLink {} starting", env!("CARGO_PKG_VERSION"));
    app::run(config, dir).map_err(|e| anyhow::anyhow!("{e}"))
}

/// One copy per config folder (so two test clients can run side by side).
#[cfg(windows)]
fn instance_name(dir: &std::path::Path) -> String {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    dir.hash(&mut h);
    format!("PetLink-{:x}", h.finish())
}
