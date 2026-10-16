//! The notification-area icon and its menu.

use crate::sprites::Sprites;
use crate::win;
use anyhow::Context;
use std::collections::HashMap;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::Duration;
use tray_icon::menu::{
    CheckMenuItem, Menu, MenuEvent, MenuId, MenuItem, PredefinedMenuItem, Submenu,
};
use tray_icon::{Icon, TrayIcon, TrayIconBuilder};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayAction {
    WriteLetter,
    TogglePeek,
    Visit,
    /// A hue; only changes how my own pet looks (friends see the hue the relay has).
    BowColour(u16),
    Scale(u8),
    Autostart,
    CheckUpdates,
    Uninstall,
    Quit,
}

const BOWS: &[(&str, u16)] = &[
    ("Red", 0),
    ("Pink", 330),
    ("Orange", 30),
    ("Gold", 45),
    ("Green", 120),
    ("Teal", 170),
    ("Blue", 210),
    ("Purple", 270),
];

fn icons() -> anyhow::Result<(Icon, Icon)> {
    let head = image::imageops::crop_imm(&Sprites::load(0)?.idle, 0, 0, 32, 32).to_image();
    let mut dot = head.clone();
    for y in 26..32 {
        for x in 26..32 {
            dot.put_pixel(x, y, image::Rgba([40, 200, 70, 255]));
        }
    }
    let mk = |i: image::RgbaImage| Icon::from_rgba(i.into_raw(), 32, 32).context("tray icon");
    Ok((mk(head)?, mk(dot)?))
}

impl Parts {
    fn build(peek_on: bool) -> anyhow::Result<Parts> {
        let (plain, dotted) = icons()?;
        let mut actions = HashMap::new();
        let mut add = |id: &MenuId, a: TrayAction| {
            actions.insert(id.clone(), a);
        };
        let letter = MenuItem::new("Write letter…", true, None);
        let peek = CheckMenuItem::new("Let friend peek", true, peek_on, None);
        let visit = MenuItem::new("Send to visit", true, None);
        let bows = Submenu::new("Bow colour", true);
        let mut bow_items = Vec::new();
        for (name, hue) in BOWS {
            let item = MenuItem::new(*name, true, None);
            add(item.id(), TrayAction::BowColour(*hue));
            bow_items.push(item);
        }
        for i in &bow_items {
            bows.append(i)?;
        }
        let sizes = Submenu::new("Pet size", true);
        let mut size_items = Vec::new();
        for n in 1..=4u8 {
            let item = MenuItem::new(format!("×{n}"), true, None);
            add(item.id(), TrayAction::Scale(n));
            size_items.push(item);
        }
        for i in &size_items {
            sizes.append(i)?;
        }
        let autostart =
            CheckMenuItem::new("Start with Windows", true, win::autostart_enabled(), None);
        let update = MenuItem::new("Check for updates", true, None);
        let uninstall = MenuItem::new("Uninstall PetLink…", true, None);
        let quit = MenuItem::new("Quit", true, None);
        add(letter.id(), TrayAction::WriteLetter);
        add(peek.id(), TrayAction::TogglePeek);
        add(visit.id(), TrayAction::Visit);
        add(autostart.id(), TrayAction::Autostart);
        add(update.id(), TrayAction::CheckUpdates);
        add(uninstall.id(), TrayAction::Uninstall);
        add(quit.id(), TrayAction::Quit);
        let menu = Menu::new();
        menu.append_items(&[
            &letter,
            &peek,
            &visit,
            &PredefinedMenuItem::separator(),
            &bows,
            &sizes,
            &autostart,
            &update,
            &uninstall,
            &PredefinedMenuItem::separator(),
            &quit,
        ])?;
        let icon = TrayIconBuilder::new()
            .with_menu(Box::new(menu))
            .with_tooltip("PetLink")
            .with_icon(if peek_on {
                dotted.clone()
            } else {
                plain.clone()
            })
            .build()
            .context("creating the tray icon")?;
        Ok(Parts {
            icon,
            plain,
            dotted,
            peek,
            visit,
            autostart,
            actions,
        })
    }
}

enum Cmd {
    Peek(bool),
    Friend(String),
    Visiting(bool),
    /// Drop the icon (so no ghost stays behind), then ack.
    Remove(Sender<()>),
    #[cfg(debug_assertions)]
    ShowMenu,
}

/// Lives on the tray thread: `TrayIcon` and the menu items are `!Send`.
struct Parts {
    icon: TrayIcon,
    plain: Icon,
    dotted: Icon,
    peek: CheckMenuItem,
    visit: MenuItem,
    autostart: CheckMenuItem,
    actions: HashMap<MenuId, TrayAction>,
}

impl Parts {
    fn apply(&self, cmd: Cmd) {
        match cmd {
            Cmd::Peek(on) => {
                self.peek.set_checked(on);
                let _ = self.icon.set_icon(Some(if on {
                    self.dotted.clone()
                } else {
                    self.plain.clone()
                }));
            }
            Cmd::Friend(name) => self.peek.set_text(format!("Let {name} peek")),
            Cmd::Visiting(away) => {
                self.visit
                    .set_text(if away { "Call home" } else { "Send to visit" })
            }
            Cmd::Remove(_) => {}
            #[cfg(debug_assertions)]
            Cmd::ShowMenu => {
                // The menu is modal on this thread; close it from outside after a moment.
                let hwnd = self.icon.window_handle() as isize;
                std::thread::spawn(move || {
                    std::thread::sleep(Duration::from_secs(4));
                    win::post_cancel_mode(hwnd);
                });
                self.icon.show_menu();
            }
        }
    }

    fn action(&self, ev: &MenuEvent) -> Option<TrayAction> {
        let action = *self.actions.get(&ev.id)?;
        if action == TrayAction::Autostart
            && let Err(e) = win::set_autostart(self.autostart.is_checked())
        {
            tracing::warn!("autostart: {e:#}");
            self.autostart.set_checked(win::autostart_enabled());
        }
        Some(action)
    }
}

/// The tray runs on its own thread with its own message loop: its popup menu is modal, and on
/// winit's thread that wedged the whole frame loop.
fn run(
    peek_on: bool,
    ctx: egui::Context,
    cmds: Receiver<Cmd>,
    out: Sender<TrayAction>,
    init: Sender<anyhow::Result<u32>>,
) {
    let parts = match Parts::build(peek_on) {
        Ok(p) => p,
        Err(e) => {
            let _ = init.send(Err(e));
            return;
        }
    };
    let _ = init.send(Ok(win::thread_id()));
    while win::pump_one() {
        while let Ok(cmd) = cmds.try_recv() {
            if let Cmd::Remove(ack) = cmd {
                drop(parts);
                let _ = ack.send(());
                return;
            }
            parts.apply(cmd);
        }
        while let Ok(ev) = MenuEvent::receiver().try_recv() {
            if let Some(a) = parts.action(&ev) {
                let _ = out.send(a);
                ctx.request_repaint();
            }
        }
    }
}

/// The app-side handle; cheap to clone for helper threads.
#[derive(Clone)]
pub struct TrayHandle {
    cmds: Sender<Cmd>,
    tid: u32,
}

impl TrayHandle {
    fn send(&self, c: Cmd) {
        let _ = self.cmds.send(c);
        win::post_wake(self.tid);
    }

    /// Takes the icon off the notification area (waits briefly for the tray thread).
    pub fn remove(&self) {
        let (ack, done) = channel();
        self.send(Cmd::Remove(ack));
        let _ = done.recv_timeout(Duration::from_millis(500));
    }
}

pub struct Tray {
    pub handle: TrayHandle,
    actions: Receiver<TrayAction>,
    #[cfg(debug_assertions)]
    inject: Sender<TrayAction>,
}

impl Tray {
    pub fn new(peek_on: bool, ctx: egui::Context) -> anyhow::Result<Tray> {
        let (cmds, cmd_rx) = channel();
        let (out, actions) = channel();
        let (init, init_rx) = channel();
        #[cfg(debug_assertions)]
        let inject = out.clone();
        std::thread::Builder::new()
            .name("tray".into())
            .spawn(move || run(peek_on, ctx, cmd_rx, out, init))?;
        let tid = init_rx.recv().context("tray thread died")??;
        Ok(Tray {
            handle: TrayHandle { cmds, tid },
            actions,
            #[cfg(debug_assertions)]
            inject,
        })
    }

    pub fn poll(&self) -> Option<TrayAction> {
        self.actions.try_recv().ok()
    }

    pub fn set_peek(&self, on: bool) {
        self.handle.send(Cmd::Peek(on));
    }

    pub fn set_friend(&self, name: &str) {
        self.handle.send(Cmd::Friend(name.to_owned()));
    }

    pub fn set_visiting(&self, away: bool) {
        self.handle.send(Cmd::Visiting(away));
    }

    /// Debug autotest: open the context menu (closes itself after 4 s).
    #[cfg(debug_assertions)]
    pub fn show_menu(&self) {
        self.handle.send(Cmd::ShowMenu);
    }

    /// Debug autotest: as if the user had clicked this item.
    #[cfg(debug_assertions)]
    pub fn inject(&self, a: TrayAction) {
        let _ = self.inject.send(a);
    }
}
