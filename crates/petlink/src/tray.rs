//! The notification-area icon and its menu.

use crate::sprites::Sprites;
use crate::win;
use anyhow::Context;
use std::collections::HashMap;
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

pub struct Tray {
    icon: TrayIcon,
    plain: Icon,
    dotted: Icon,
    peek: CheckMenuItem,
    visit: MenuItem,
    autostart: CheckMenuItem,
    actions: HashMap<MenuId, TrayAction>,
}

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

impl Tray {
    pub fn new(peek_on: bool) -> anyhow::Result<Tray> {
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
        let quit = MenuItem::new("Quit", true, None);
        add(letter.id(), TrayAction::WriteLetter);
        add(peek.id(), TrayAction::TogglePeek);
        add(visit.id(), TrayAction::Visit);
        add(autostart.id(), TrayAction::Autostart);
        add(update.id(), TrayAction::CheckUpdates);
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
        Ok(Tray {
            icon,
            plain,
            dotted,
            peek,
            visit,
            autostart,
            actions,
        })
    }

    pub fn poll(&self) -> Option<TrayAction> {
        let ev = MenuEvent::receiver().try_recv().ok()?;
        let action = *self.actions.get(&ev.id)?;
        if action == TrayAction::Autostart
            && let Err(e) = win::set_autostart(self.autostart.is_checked())
        {
            tracing::warn!("autostart: {e:#}");
            self.autostart.set_checked(win::autostart_enabled());
        }
        Some(action)
    }

    pub fn set_peek(&self, on: bool) {
        self.peek.set_checked(on);
        let _ = self.icon.set_icon(Some(if on {
            self.dotted.clone()
        } else {
            self.plain.clone()
        }));
    }

    pub fn set_friend(&self, name: &str) {
        self.peek.set_text(format!("Let {name} peek"));
    }

    pub fn set_visiting(&self, away: bool) {
        self.visit
            .set_text(if away { "Call home" } else { "Send to visit" });
    }
}
