//! Kernel desktop shell: the main window, and a palette window toggled with Alt+Space.
//! Everything else lives in the web frontend, which talks to the node directly.

// No console window behind the app in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod update;

use tauri::{AppHandle, Manager, WindowEvent};
use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Modifiers, Shortcut, ShortcutState};

fn toggle_palette(app: &AppHandle) {
    let Some(palette) = app.get_webview_window("palette") else {
        return;
    };
    if palette.is_visible().unwrap_or(false) {
        let _ = palette.hide();
    } else {
        let _ = palette.center();
        let _ = palette.show();
        let _ = palette.set_focus();
    }
}

/// This app's build number (0 for builds made on a PC).
#[tauri::command]
fn app_build() -> u64 {
    update::build()
}

/// A newer build of the desktop app on GitHub, if there is one.
#[tauri::command]
async fn update_check() -> Result<Option<update::Available>, String> {
    if update::build() == 0 {
        return Err("this is a development build, so it doesn't update itself".into());
    }
    update::check().await
}

/// Download, verify and install the newest build, then restart into it.
#[tauri::command]
async fn update_install(app: AppHandle) -> Result<u64, String> {
    let Some(available) = update::check().await? else {
        return Err("already up to date".into());
    };
    let installer = update::download(&available).await?;
    update::run_installer(&installer)?;
    // Exit in a moment, so the reply reaches the window first; the installer waits for it.
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(600));
        app.exit(0);
    });
    Ok(available.build)
}

fn show_main(app: &AppHandle) {
    if let Some(main) = app.get_webview_window("main") {
        let _ = main.unminimize();
        let _ = main.show();
        let _ = main.set_focus();
    }
}

fn main() {
    let palette_key = Shortcut::new(Some(Modifiers::ALT), Code::Space);

    tauri::Builder::default()
        // A second launch focuses the running app instead of opening another one.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            show_main(app)
        }))
        // Windows notifications for node events (crashes, disconnects, finished jobs).
        .plugin(tauri_plugin_notification::init())
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(move |app, shortcut, event| {
                    if shortcut == &palette_key && event.state() == ShortcutState::Pressed {
                        toggle_palette(app);
                    }
                })
                .build(),
        )
        .setup(move |app| {
            // Another program may already own Alt+Space. The app still works without it.
            if let Err(e) = app.global_shortcut().register(palette_key) {
                eprintln!("could not register Alt+Space: {e}");
            }
            Ok(())
        })
        .on_window_event(|window, event| match (window.label(), event) {
            // The palette gets out of the way as soon as you click elsewhere.
            ("palette", WindowEvent::Focused(false)) => {
                let _ = window.hide();
            }
            // Closing the main window quits, instead of leaving the hidden palette running.
            ("main", WindowEvent::CloseRequested { .. }) => window.app_handle().exit(0),
            _ => {}
        })
        .invoke_handler(tauri::generate_handler![
            app_build,
            update_check,
            update_install
        ])
        .run(tauri::generate_context!())
        .expect("error while running Kernel");
}
