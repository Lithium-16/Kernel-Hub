//! Windows for writing and reading letters, the peek lens, and the first-run invite prompt.

use crate::config::Config;
use crate::files;
use crate::net::{self, Net};
use crate::protocol::{Envelope, FileRef, Msg};
use egui::{TextureHandle, TextureOptions, ViewportBuilder, ViewportId};
use std::cell::RefCell;
use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::mpsc::{Receiver, Sender, channel};

pub const MAX_FILE: u64 = 100 * 1024 * 1024;
/// Longest side of a picture we turn into a texture (GPUs refuse much more than this).
const MAX_SIDE: u32 = 4096;

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
pub struct Compose {
    pub text: String,
    pub files: Vec<PathBuf>,
    pub error: String,
}

impl Compose {
    pub fn add_files(&mut self, paths: impl IntoIterator<Item = PathBuf>) {
        for p in paths {
            match std::fs::metadata(&p) {
                Ok(m) if m.is_file() && m.len() > MAX_FILE => {
                    self.error = format!("{}: too big: max 100 MB", file_name(&p));
                }
                Ok(m) if m.is_file() => {
                    self.error.clear();
                    if !self.files.contains(&p) {
                        self.files.push(p);
                    }
                }
                _ => self.error = format!("{}: can't be attached", file_name(&p)),
            }
        }
    }
}

fn file_name(p: &std::path::Path) -> String {
    p.file_name()
        .map_or_else(|| "file".into(), |n| n.to_string_lossy().into_owned())
}

pub struct Reading {
    pub mail: Envelope,
    /// Where this letter's downloads are kept until the letter is closed.
    pub dir: PathBuf,
    pub from: String,
    pub images: HashMap<String, TextureHandle>,
    pub bytes: HashMap<String, Vec<u8>>,
    /// Per file: the last thing to tell the reader (errors).
    pub status: HashMap<String, String>,
    pub saved_dir: HashMap<String, PathBuf>,
    pub confirm: Option<String>,
    pub loading: Vec<String>,
}

pub enum MailAction {
    Send { text: String, files: Vec<PathBuf> },
}

pub struct Done {
    pub file_id: String,
    pub result: Result<Vec<u8>, String>,
    /// The picture, if it is one, already decoded (and shrunk) off the UI thread.
    pub image: Option<egui::ColorImage>,
}

pub struct Windows {
    pub compose: Option<Compose>,
    pub reading: Vec<Reading>,
    pub lens: Option<Lens>,
    tx: Sender<Done>,
    rx: Receiver<Done>,
    /// A peek picture on its way: (file id, who from).
    pub peek_wait: Option<(String, String)>,
    /// Received downloads live here (inbox/<mail_id>/<file_id>) until the letter is closed.
    pub inbox: PathBuf,
    /// Letters whose window was just closed: the app can forget their parcels.
    pub closed: Vec<String>,
    /// Debug test hook: save every file as soon as it is down.
    pub autosave: bool,
}

pub struct Lens {
    pub texture: TextureHandle,
    pub bytes: Vec<u8>,
    pub zoom: f32,
    pub offset: egui::Vec2,
    pub title: String,
    pub status: String,
}

impl Default for Windows {
    fn default() -> Self {
        let (tx, rx) = channel();
        Windows {
            compose: None,
            reading: Vec::new(),
            lens: None,
            tx,
            rx,
            peek_wait: None,
            inbox: PathBuf::new(),
            closed: Vec::new(),
            autosave: false,
        }
    }
}

/// A name made of letters, digits, `-` and `_` only: ids from the friend become path parts.
fn path_part(s: &str) -> String {
    let p: String = s
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .collect();
    if p.is_empty() { "_".into() } else { p }
}

/// Fetches a file on a background thread and reports on the channel. With `cache`, a file
/// already on disk is used instead of downloading, and a fresh download is kept there.
fn spawn_download(
    config: &Config,
    ctx: &egui::Context,
    file_id: String,
    tx: Sender<Done>,
    cache: Option<PathBuf>,
) {
    let (config, ctx) = (config.clone(), ctx.clone());
    std::thread::spawn(move || {
        let cached = cache.as_ref().and_then(|p| std::fs::read(p).ok());
        let result = match cached {
            Some(bytes) => Ok(bytes),
            None => tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|e| e.to_string())
                .and_then(|rt| {
                    rt.block_on(net::download(&config, &file_id))
                        .map_err(|e| format!("{e:#}"))
                }),
        };
        if let (Ok(bytes), Some(p)) = (&result, &cache)
            && !p.exists()
            && let Err(e) = crate::files::atomic_write(p, bytes)
        {
            tracing::warn!("couldn't keep {}: {e}", p.display());
        }
        let image = result.as_ref().ok().and_then(|b| decode_image(b));
        let _ = tx.send(Done {
            file_id,
            result,
            image,
        });
        ctx.request_repaint();
    });
}

/// Decodes a picture and shrinks it to at most `MAX_SIDE` on its longest side.
pub fn decode_image(bytes: &[u8]) -> Option<egui::ColorImage> {
    let mut img = image::load_from_memory(bytes).ok()?;
    if img.width().max(img.height()) > MAX_SIDE {
        img = img.resize(MAX_SIDE, MAX_SIDE, image::imageops::FilterType::Triangle);
    }
    let img = img.to_rgba8();
    Some(egui::ColorImage::from_rgba_unmultiplied(
        [img.width() as usize, img.height() as usize],
        img.as_raw(),
    ))
}

pub fn open_folder(dir: &std::path::Path) {
    let _ = std::process::Command::new("explorer").arg(dir).spawn();
}

impl Windows {
    pub fn open_reading(
        &mut self,
        ctx: &egui::Context,
        config: &Config,
        mail: Envelope,
        from: String,
    ) {
        let files: Vec<FileRef> = match &mail.msg {
            Msg::Mail { files, .. } => files.clone(),
            _ => return,
        };
        let Msg::Mail { mail_id, .. } = &mail.msg else {
            return;
        };
        let dir = self.inbox.join(path_part(mail_id));
        let loading = files.iter().map(|f| f.file_id.clone()).collect();
        for f in &files {
            let cache = dir.join(path_part(&f.file_id));
            spawn_download(config, ctx, f.file_id.clone(), self.tx.clone(), Some(cache));
        }
        self.reading.push(Reading {
            dir,
            mail,
            from,
            images: HashMap::new(),
            bytes: HashMap::new(),
            status: HashMap::new(),
            saved_dir: HashMap::new(),
            confirm: None,
            loading,
        });
    }

    pub fn download_peek(
        &mut self,
        ctx: &egui::Context,
        config: &Config,
        file_id: String,
        from: String,
    ) {
        spawn_download(config, ctx, file_id.clone(), self.tx.clone(), None);
        self.peek_wait = Some((file_id, from));
    }

    fn on_done(&mut self, ctx: &egui::Context, mut d: Done) {
        if self
            .peek_wait
            .as_ref()
            .is_some_and(|(id, _)| *id == d.file_id)
        {
            let (_, from) = self.peek_wait.take().unwrap_or_default();
            match d.result {
                Ok(bytes) => match d.image {
                    Some(img) => {
                        let texture = ctx.load_texture("peek", img, TextureOptions::LINEAR);
                        tracing::info!("peek picture received ({} bytes)", bytes.len());
                        self.lens = Some(Lens {
                            texture,
                            bytes,
                            zoom: 1.0,
                            offset: egui::Vec2::ZERO,
                            title: format!("Peek at {from}"),
                            status: String::new(),
                        });
                    }
                    None => tracing::warn!("the peek picture couldn't be decoded"),
                },
                Err(e) => tracing::warn!("peek download failed: {e}"),
            }
            return;
        }
        for r in &mut self.reading {
            let Some(pos) = r.loading.iter().position(|id| *id == d.file_id) else {
                continue;
            };
            r.loading.remove(pos);
            match &d.result {
                Ok(bytes) => {
                    tracing::info!("downloaded {} ({} bytes)", d.file_id, bytes.len());
                    if let Some(img) = d.image.take() {
                        let tex = ctx.load_texture(&d.file_id, img, TextureOptions::LINEAR);
                        r.images.insert(d.file_id.clone(), tex);
                    }
                    r.bytes.insert(d.file_id.clone(), bytes.clone());
                    r.status.remove(&d.file_id);
                }
                Err(e) => {
                    tracing::warn!("download {} failed: {e}", d.file_id);
                    r.status
                        .insert(d.file_id.clone(), format!("Download failed: {e}"));
                }
            }
        }
    }

    pub fn show(&mut self, ctx: &egui::Context, _net: &Net, config: &Config) -> Option<MailAction> {
        while let Ok(d) = self.rx.try_recv() {
            self.on_done(ctx, d);
        }
        let mut action = None;
        if self.autosave {
            for r in &mut self.reading {
                let files = match &r.mail.msg {
                    Msg::Mail { files, .. } => files.clone(),
                    _ => continue,
                };
                for f in files {
                    if r.bytes.contains_key(&f.file_id) && !r.saved_dir.contains_key(&f.file_id) {
                        save_file(r, &f, config);
                    }
                }
            }
        }
        if let Some(c) = &mut self.compose {
            let mut close = false;
            ctx.show_viewport_immediate(
                ViewportId::from_hash_of("compose"),
                ViewportBuilder::default()
                    .with_title("Write a letter")
                    .with_inner_size([380.0, 320.0])
                    .with_always_on_top(),
                |ui, _| {
                    if ui.ctx().input(|i| i.viewport().close_requested()) {
                        close = true;
                        return;
                    }
                    egui::CentralPanel::default().show(ui, |ui| {
                        ui.add(
                            egui::TextEdit::multiline(&mut c.text)
                                .desired_rows(8)
                                .desired_width(f32::INFINITY)
                                .char_limit(4000)
                                .hint_text("Write something…"),
                        );
                        let mut remove = None;
                        for (i, f) in c.files.iter().enumerate() {
                            ui.horizontal(|ui| {
                                if ui.small_button("✕").clicked() {
                                    remove = Some(i);
                                }
                                ui.label(file_name(f));
                            });
                        }
                        if let Some(i) = remove {
                            c.files.remove(i);
                        }
                        ui.horizontal(|ui| {
                            if ui.button("Attach…").clicked()
                                && let Some(p) = rfd::FileDialog::new().pick_files()
                            {
                                c.add_files(p);
                            }
                            let empty = c.text.trim().is_empty() && c.files.is_empty();
                            if ui.add_enabled(!empty, egui::Button::new("Send")).clicked() {
                                action = Some(MailAction::Send {
                                    text: c.text.trim().to_owned(),
                                    files: std::mem::take(&mut c.files),
                                });
                                close = true;
                            }
                        });
                        if !c.error.is_empty() {
                            ui.colored_label(egui::Color32::LIGHT_RED, &c.error);
                        }
                    });
                },
            );
            if close {
                self.compose = None;
            }
        }
        let tx = self.tx.clone();
        let closed = &mut self.closed;
        self.reading.retain_mut(|r| {
            let Msg::Mail {
                mail_id,
                text,
                files,
            } = r.mail.msg.clone()
            else {
                return false;
            };
            let mut keep = true;
            ctx.show_viewport_immediate(
                ViewportId::from_hash_of(("reading", &mail_id)),
                ViewportBuilder::default()
                    .with_title(format!("Letter from {}", r.from))
                    .with_inner_size([400.0, 420.0]),
                |ui, _| {
                    if ui.ctx().input(|i| i.viewport().close_requested()) {
                        keep = false;
                        return;
                    }
                    egui::CentralPanel::default().show(ui, |ui| {
                        egui::ScrollArea::vertical().show(ui, |ui| {
                            if !text.is_empty() {
                                ui.label(&text);
                                ui.separator();
                            }
                            for f in &files {
                                show_file(ui, r, f, config, &tx);
                            }
                        });
                    });
                },
            );
            if !keep {
                // Closing the letter is the user letting go of it: forget its downloads too.
                let _ = std::fs::remove_dir_all(&r.dir);
                closed.push(mail_id);
            }
            keep
        });
        self.show_lens(ctx, config);
        action
    }
}

fn show_file(ui: &mut egui::Ui, r: &mut Reading, f: &FileRef, config: &Config, tx: &Sender<Done>) {
    ui.group(|ui| {
        ui.label(format!("{} ({} KB)", f.name, f.size.div_ceil(1024)));
        if let Some(tex) = r.images.get(&f.file_id) {
            let size = tex.size_vec2();
            let scale = (360.0 / size.x).min(1.0);
            ui.add(egui::Image::new((tex.id(), size * scale)));
        }
        if r.loading.contains(&f.file_id) {
            ui.label("Downloading…");
        } else if r.bytes.contains_key(&f.file_id) {
            if let Some(dir) = r.saved_dir.get(&f.file_id).cloned() {
                ui.horizontal(|ui| {
                    ui.label("Saved to Downloads\\PetLink");
                    if ui.button("Open folder").clicked() {
                        open_folder(&dir);
                    }
                });
            } else if r.confirm.as_deref() == Some(&f.file_id) {
                ui.colored_label(
                    egui::Color32::from_rgb(240, 180, 60),
                    "This is a program or script. Only save it if you trust it.",
                );
                ui.horizontal(|ui| {
                    if ui.button("Save anyway").clicked() {
                        save_file(r, f, config);
                    }
                    if ui.button("Cancel").clicked() {
                        r.confirm = None;
                    }
                });
            } else if ui.button("Save").clicked() {
                if files::is_dangerous(&f.name) {
                    r.confirm = Some(f.file_id.clone());
                } else {
                    save_file(r, f, config);
                }
            }
            if let Some(s) = r.status.get(&f.file_id) {
                ui.colored_label(egui::Color32::LIGHT_RED, s);
            }
        } else if let Some(err) = r.status.get(&f.file_id).cloned() {
            ui.colored_label(egui::Color32::LIGHT_RED, err);
            if ui.button("Retry").clicked() {
                r.status.remove(&f.file_id);
                r.loading.push(f.file_id.clone());
                let cache = r.dir.join(path_part(&f.file_id));
                spawn_download(config, ui.ctx(), f.file_id.clone(), tx.clone(), Some(cache));
            }
        }
    });
}

/// Saves into Downloads\PetLink (never opens the file).
pub fn save_file(r: &mut Reading, f: &FileRef, config: &Config) {
    let Some(bytes) = r.bytes.get(&f.file_id) else {
        return;
    };
    let dir = files::downloads_dir();
    match files::save(&dir, &f.name, bytes, &config.files_url()) {
        Ok(p) => {
            tracing::info!("saved {}", p.display());
            r.saved_dir.insert(f.file_id.clone(), dir);
        }
        Err(e) => {
            tracing::warn!("saving {} failed: {e}", f.name);
            r.status
                .insert(f.file_id.clone(), format!("Couldn't save: {e}"));
        }
    }
    r.confirm = None;
}

impl Windows {
    fn show_lens(&mut self, ctx: &egui::Context, config: &Config) {
        let Some(l) = &mut self.lens else { return };
        let mut close = false;
        ctx.show_viewport_immediate(
            ViewportId::from_hash_of("lens"),
            ViewportBuilder::default()
                .with_title(l.title.clone())
                .with_inner_size([900.0, 560.0]),
            |ui, _| {
                if ui.ctx().input(|i| i.viewport().close_requested()) {
                    close = true;
                    return;
                }
                egui::CentralPanel::default().show(ui, |ui| {
                    ui.horizontal(|ui| {
                        if ui.button("Save").clicked() {
                            let (y, mo, d, h, mi) = crate::win::local_time();
                            let name = format!("peek-{y:04}-{mo:02}-{d:02}-{h:02}{mi:02}.jpg");
                            l.status = match files::save(
                                &files::downloads_dir(),
                                &name,
                                &l.bytes,
                                &config.files_url(),
                            ) {
                                Ok(p) => {
                                    tracing::info!("saved {}", p.display());
                                    "Saved to Downloads\\PetLink".to_owned()
                                }
                                Err(e) => format!("Could not save: {e}"),
                            };
                        }
                        ui.label(&l.status);
                        ui.weak("wheel to zoom, drag to move");
                    });
                    let area = ui.available_rect_before_wrap();
                    let resp = ui.allocate_rect(area, egui::Sense::click_and_drag());
                    let size = l.texture.size_vec2();
                    let fit = (area.width() / size.x).min(area.height() / size.y);
                    let center = area.center() + l.offset;
                    if resp.hovered()
                        && let Some(p) = ui.input(|i| i.pointer.hover_pos())
                    {
                        let scroll = ui.input(|i| i.smooth_scroll_delta.y);
                        let zoom = (l.zoom * (1.0 + scroll * 0.004)).clamp(1.0, 8.0);
                        // Keep the point under the cursor where it is.
                        let new_center = p - (p - center) * (zoom / l.zoom);
                        l.zoom = zoom;
                        l.offset = new_center - area.center();
                    }
                    l.offset += resp.drag_delta();
                    if l.zoom <= 1.0 {
                        l.offset = egui::Vec2::ZERO;
                    }
                    let shown = size * fit * l.zoom;
                    let slack = ((shown - area.size()) / 2.0).max(egui::Vec2::ZERO);
                    l.offset = l.offset.clamp(-slack, slack);
                    let rect = egui::Rect::from_center_size(area.center() + l.offset, shown);
                    ui.painter_at(area).image(
                        l.texture.id(),
                        rect,
                        egui::Rect::from_min_max(egui::Pos2::ZERO, egui::Pos2::new(1.0, 1.0)),
                        egui::Color32::WHITE,
                    );
                });
            },
        );
        if close {
            self.lens = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png(w: u32, h: u32) -> Vec<u8> {
        let mut out = std::io::Cursor::new(Vec::new());
        image::RgbaImage::new(w, h)
            .write_to(&mut out, image::ImageFormat::Png)
            .unwrap();
        out.into_inner()
    }

    #[test]
    fn big_pictures_are_shrunk_and_small_ones_are_not() {
        let big = decode_image(&png(9000, 300)).unwrap();
        assert_eq!(big.size[0], MAX_SIDE as usize);
        assert!(big.size[1] < 300);
        assert_eq!(decode_image(&png(40, 30)).unwrap().size, [40, 30]);
        assert!(decode_image(b"not a picture").is_none());
    }

    #[test]
    fn ids_from_a_friend_cannot_climb_out_of_the_inbox() {
        assert_eq!(path_part("..\\..\\x/y"), "xy");
        assert_eq!(path_part(".."), "_");
        assert_eq!(path_part("f_01J-9"), "f_01J-9");
    }
}
