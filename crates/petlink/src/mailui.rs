//! Windows for writing and reading letters, and the first-run invite prompt.

use crate::config::Config;
use std::cell::RefCell;
use std::rc::Rc;

/// A tiny window asking for the invite; `None` if it was closed without one.
pub fn ask_for_invite() -> anyhow::Result<Option<Config>> {
    let result: Rc<RefCell<Option<Config>>> = Rc::default();
    let out = result.clone();
    let (mut text, mut error) = (String::new(), String::new());
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("PetLink setup")
            .with_inner_size([420.0, 160.0])
            .with_resizable(false),
        ..Default::default()
    };
    eframe::run_ui_native("PetLink setup", options, move |ui, _| {
        let ctx = ui.ctx().clone();
        egui::CentralPanel::default().show(ui, |ui| {
            ui.label("Paste the invite you were sent");
            ui.add(egui::TextEdit::singleline(&mut text).desired_width(f32::INFINITY));
            if ui.button("Connect").clicked() || ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                match Config::from_invite(&text) {
                    Ok(c) => {
                        *out.borrow_mut() = Some(c);
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                    Err(e) => error = format!("{e:#}"),
                }
            }
            if !error.is_empty() {
                ui.colored_label(egui::Color32::LIGHT_RED, &error);
            }
        });
    })
    .map_err(|e| anyhow::anyhow!("{e}"))?;
    let c = result.borrow_mut().take();
    Ok(c)
}

#[derive(Default)]
pub struct Windows {}
