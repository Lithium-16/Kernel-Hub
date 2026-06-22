//! HUD: every screen-space panel in one place, so `main.rs` stays orchestration.
//! Each panel is laid out anchored to the window (reflows on resize), drawn through
//! the `Ui` toolkit, and its rect feeds input capture so clicks never leak to the
//! world. New panels are added here (command card, build menu) without touching the
//! main loop's input/render wiring.

use hecs::World;
use macroquad::prelude::*;

use crate::components::{Selected, UnitKind};
use crate::economy::Economy;
use crate::stance::{self, Stance};
use crate::ui::Ui;

/// Top resource-bar height.
pub const TOP_H: f32 = 34.0;
/// Bottom command-bar height.
pub const BAR_H: f32 = 56.0;
const SEL_W: f32 = 230.0;

/// Result of a HUD interaction for the main loop to act on.
pub enum HudAction {
    None,
    Stop,
    ClearSel,
    SetStance(Stance),
}

/// Anchored rects for every HUD panel this frame, computed before world input so the
/// loop can ask which clicks belong to the UI (`contains`).
pub struct HudLayout {
    pub top: Rect,
    pub bottom: Rect,
    pub selection: Option<Rect>,
    pub command: Option<Rect>,
    pub minimap: Rect,
}

/// Minimap panel size (square), anchored top-right under the resource bar.
pub const MINIMAP_SIZE: f32 = 180.0;

impl HudLayout {
    pub fn compute(world: &World, sw: f32, sh: f32) -> Self {
        let (tally, total) = selection_summary(world);
        let rows = tally.len();
        let selection = (rows > 0).then(|| {
            let h = 34.0 + rows as f32 * 24.0;
            Rect::new(sw - SEL_W - 8.0, sh - BAR_H - 8.0 - h, SEL_W, h)
        });
        // Command card: bottom-left, above the command bar, only with a selection.
        let command = (total > 0).then(|| Rect::new(8.0, sh - BAR_H - 8.0 - 104.0, 330.0, 104.0));
        let minimap = Rect::new(sw - MINIMAP_SIZE - 8.0, TOP_H + 8.0, MINIMAP_SIZE, MINIMAP_SIZE);
        Self {
            top: Rect::new(0.0, 0.0, sw, TOP_H),
            bottom: Rect::new(0.0, sh - BAR_H, sw, BAR_H),
            selection,
            command,
            minimap,
        }
    }

    /// Does a screen point land on any HUD panel? World input is skipped when true.
    pub fn contains(&self, p: Vec2) -> bool {
        self.top.contains(p)
            || self.bottom.contains(p)
            || self.minimap.contains(p)
            || self.selection.is_some_and(|r| r.contains(p))
            || self.command.is_some_and(|r| r.contains(p))
    }
}

/// Tally the current selection by unit type (display name), first-seen order, + total.
pub fn selection_summary(world: &World) -> (Vec<(String, u32)>, u32) {
    let mut tally: Vec<(String, u32)> = Vec::new();
    let mut total = 0u32;
    for (_e, (_s, k)) in world.query::<(&Selected, &UnitKind)>().iter() {
        total += 1;
        if let Some(row) = tally.iter_mut().find(|(n, _)| *n == k.name) {
            row.1 += 1;
        } else {
            tally.push((k.name.clone(), 1));
        }
    }
    (tally, total)
}

/// Draw the whole HUD and return any interaction (command card overrides bottom bar).
pub fn draw(ui: &mut Ui, world: &World, eco: &Economy, layout: &HudLayout) -> HudAction {
    draw_top_bar(ui, eco, layout.top);
    let mut action = draw_bottom_bar(ui, world, layout.bottom);
    if let Some(r) = layout.command {
        if let Some(a) = draw_command_card(ui, world, r) {
            action = a;
        }
    }
    if let Some(r) = layout.selection {
        draw_selection_panel(ui, world, r);
    }
    action
}

/// Command card: standard commands + stance buttons; the active stance is outlined.
fn draw_command_card(ui: &mut Ui, world: &World, r: Rect) -> Option<HudAction> {
    ui.panel(r);
    ui.label(vec2(r.x + 10.0, r.y + 22.0), "Commands");
    let mut action = None;

    // Stop (clears orders).
    if ui.button(Rect::new(r.x + r.w - 84.0, r.y + 6.0, 76.0, 24.0), "Stop") {
        action = Some(HudAction::Stop);
    }

    // Stance row, active stance outlined in accent green.
    let current = stance::dominant(world);
    let accent = Color::new(0.45, 1.0, 0.55, 0.95);
    let (bw, bh) = (100.0, 32.0);
    let mut x = r.x + 10.0;
    for s in Stance::ALL {
        let rect = Rect::new(x, r.y + 36.0, bw, bh);
        if ui.button(rect, s.label()) {
            action = Some(HudAction::SetStance(s));
        }
        if current == Some(s) {
            draw_rectangle_lines(rect.x, rect.y, rect.w, rect.h, 2.5, accent);
        }
        x += bw + 6.0;
    }
    action
}

/// Top bar: resource readouts with hover tooltips. Power turns amber when overdrawn.
fn draw_top_bar(ui: &mut Ui, eco: &Economy, r: Rect) {
    ui.panel(r);
    let mp: Vec2 = mouse_position().into();
    let fs = ui.theme.font_size as u16;
    let amber = Color::new(1.0, 0.72, 0.25, 1.0);

    let segments = [
        ("Metal", eco.metal.to_string(), false, "Metal — refined stock for structures and vehicles."),
        ("Fuel", eco.fuel.to_string(), false, "Fuel — drawn by vehicles and some production."),
        ("Comp", eco.components.to_string(), false, "Components — advanced manufacturing input."),
        (
            "Power",
            format!("{}/{}", eco.power_used, eco.power_cap),
            eco.overdrawn(),
            "Power draw vs capacity. Over capacity throttles production.",
        ),
    ];

    let mut x = r.x + 14.0;
    let mut tip: Option<(&str, Vec2)> = None;
    for (name, val, warn, help) in &segments {
        let text = format!("{name} {val}");
        let w = measure_text(&text, None, fs, 1.0).width;
        let seg = Rect::new(x - 6.0, r.y + 3.0, w + 12.0, r.h - 6.0);
        let pos = vec2(x, r.y + 23.0);
        if *warn {
            ui.label_colored(pos, &text, amber);
        } else {
            ui.label(pos, &text);
        }
        if seg.contains(mp) {
            tip = Some((help, vec2(mp.x, r.y + r.h + 6.0)));
        }
        x += w + 26.0;
    }
    if let Some((t, at)) = tip {
        ui.tooltip(t, at);
    }
}

/// Bottom command bar: Stop / Clear + a selection count.
fn draw_bottom_bar(ui: &mut Ui, world: &World, r: Rect) -> HudAction {
    ui.panel(r);
    let mut action = HudAction::None;
    if ui.button(Rect::new(r.x + 10.0, r.y + 8.0, 96.0, 40.0), "Stop") {
        action = HudAction::Stop;
    }
    if ui.button(Rect::new(r.x + 114.0, r.y + 8.0, 96.0, 40.0), "Clear") {
        action = HudAction::ClearSel;
    }
    let n = world.query::<&Selected>().iter().count();
    ui.label(vec2(r.x + 228.0, r.y + 34.0), &format!("Selected: {n}"));
    action
}

/// Selection panel: header with the total + one row per unit type with its count.
fn draw_selection_panel(ui: &mut Ui, world: &World, r: Rect) {
    let (tally, total) = selection_summary(world);
    ui.panel(r);
    ui.label(vec2(r.x + 10.0, r.y + 22.0), &format!("Selection ({total})"));
    let fs = ui.theme.font_size as u16;
    for (i, (name, n)) in tally.iter().enumerate() {
        let y = r.y + 46.0 + i as f32 * 24.0;
        ui.label(vec2(r.x + 14.0, y), name);
        let count = format!("x{n}");
        let d = measure_text(&count, None, fs, 1.0);
        ui.label(vec2(r.x + r.w - 14.0 - d.width, y), &count);
    }
}
