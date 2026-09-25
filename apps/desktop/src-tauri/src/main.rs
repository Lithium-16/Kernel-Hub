//! Kernel desktop shell: the main window, and a palette window toggled with Alt+Space.
//! Everything else lives in the web frontend, which talks to the node directly.

// No console window behind the app in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

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
        .run(tauri::generate_context!())
        .expect("error while running Kernel");
}
