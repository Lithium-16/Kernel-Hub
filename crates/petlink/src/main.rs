#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]
// On other systems only the portable logic builds (CI tests it); the overlay is Windows-only.
#![cfg_attr(not(windows), allow(dead_code))]
#[cfg(windows)]
mod app;
mod config;
mod files;
mod fx;
mod games;
mod install;
#[cfg(windows)]
mod mailui;
mod net;
mod objects;
mod pet;
mod protocol;
mod social;
mod sounds;
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

/// Logs to `dir`; if the log file can't be created (e.g. an antivirus blocking writes), runs
/// without a log file instead of failing to start.
#[cfg(windows)]
fn init_logging(dir: &std::path::Path) -> Option<tracing_appender::non_blocking::WorkerGuard> {
    let appender = std::fs::create_dir_all(dir).ok().and_then(|()| {
        tracing_appender::rolling::Builder::new()
            .rotation(tracing_appender::rolling::Rotation::DAILY)
            .filename_prefix("petlink.log")
            .build(dir)
            .ok()
    });
    let guard = appender.map(|a| {
        let (w, guard) = tracing_appender::non_blocking(a);
        tracing_subscriber::fmt()
            .with_writer(w)
            .with_ansi(false)
            .init();
        guard
    });
    std::panic::set_hook(Box::new(|info| tracing::error!("panic: {info}")));
    guard
}

/// The release build has no console, so a startup error would vanish silently: show it.
#[cfg(windows)]
fn main() {
    if let Err(e) = run() {
        tracing::error!("startup failed: {e:#}");
        rfd::MessageDialog::new()
            .set_level(rfd::MessageLevel::Error)
            .set_title("PetLink couldn't start")
            .set_description(format!(
                "{e:#}\n\nIf this says access is denied, your antivirus is probably blocking \
                 PetLink. Add PetLink as an exception in your antivirus and start it again."
            ))
            .show();
        std::process::exit(1);
    }
}

#[cfg(windows)]
fn run() -> anyhow::Result<()> {
    let dir = config::Config::dir();
    let _guard = init_logging(&dir.join("logs"));
    let args: Vec<String> = std::env::args().collect();
    let welcome = args.iter().any(|a| a == "--installed");
    if let Some(dest) = install::install_exe()
        && install::should_install(
            &std::env::current_exe()?,
            &dest,
            std::env::var_os("PETLINK_DIR").is_some(),
            args.iter().any(|a| a == "--no-install"),
        )
    {
        match install::run(&dir, &instance_name(&dir)) {
            Ok(()) => return Ok(()),
            Err(e) => tracing::error!("install failed, running from here: {e:#}"),
        }
    }
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
        None => match install::own_invite() {
            Some(c) => {
                c.save(&dir)?;
                c
            }
            None => match mailui::ask_for_invite()? {
                Some(c) => {
                    c.save(&dir)?;
                    // winit allows one event loop per process: start over as a fresh process.
                    std::process::Command::new(std::env::current_exe()?).spawn()?;
                    return Ok(());
                }
                None => return Ok(()),
            },
        },
    };
    update::spawn_checker();
    tracing::info!("PetLink {} starting", env!("CARGO_PKG_VERSION"));
    app::run(config, dir, welcome).map_err(|e| anyhow::anyhow!("{e}"))
}

/// One copy per config folder (so two test clients can run side by side).
#[cfg(windows)]
fn instance_name(dir: &std::path::Path) -> String {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    dir.hash(&mut h);
    format!("PetLink-{:x}", h.finish())
}
