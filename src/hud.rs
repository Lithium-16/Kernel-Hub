//! HUD: every screen-space panel in one place, so `main.rs` stays orchestration.
//! Each panel is laid out anchored to the window (reflows on resize), drawn through
//! the `Ui` toolkit, and its rect feeds input capture so clicks never leak to the
//! world. New panels are added here (command card, build menu) without touching the
//! main loop's input/render wiring.

use hecs::World;
use macroquad::prelude::*;

use crate::components::{Selected, UnitKind};
use crate::data::BuildingDef;
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
    /// Player clicked a group card — select all its members.
    SelectGroup(u32),
    /// Player clicked a building in the build panel — start placing it.
    PlaceBuilding(usize),
    /// Player closed the build panel without selecting.
    CloseBuildPanel,
}

/// Anchored rects for every HUD panel this frame, computed before world input so the
/// loop can ask which clicks belong to the UI (`contains`).
pub struct HudLayout {
    pub top: Rect,
    pub bottom: Rect,
    pub selection: Option<Rect>,
    pub command: Option<Rect>,
    pub minimap: Rect,
    pub build_panel: Option<Rect>,
    pub route_panel: Option<Rect>,
    pub depot_panel: Option<Rect>,
}

/// Minimap panel size (square), anchored top-right under the resource bar.
pub const MINIMAP_SIZE: f32 = 180.0;

// Build panel button size and columns
const BUILD_BTN: f32 = 110.0;
const BUILD_COLS: usize = 4;
const BUILD_BTN_H: f32 = 52.0;

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
            build_panel: None,
            route_panel: None,
            depot_panel: None,
        }
    }

    /// Add route panel rect when routes exist or route mode is active.
    pub fn with_depot_panel(mut self, sw: f32, sh: f32) -> Self {
        // Depot panel: 200×160, centre of screen just above the bottom bar
        self.depot_panel = Some(Rect::new(sw * 0.5 - 100.0, sh - BAR_H - 8.0 - 168.0, 200.0, 168.0));
        self
    }

    pub fn with_route_panel(mut self, num_routes: usize, route_mode: bool, sh: f32) -> Self {
        if num_routes > 0 || route_mode {
            let panel_w = 220.0;
            let row_h   = 36.0;
            let n_rows  = num_routes.max(1) as f32;
            let panel_h = 28.0 + n_rows * row_h + if route_mode { 44.0 } else { 0.0 };
            self.route_panel = Some(Rect::new(8.0, sh - BAR_H - 8.0 - panel_h, panel_w, panel_h));
        }
        self
    }

    /// Recompute including the build panel (call when build_open is true).
    pub fn with_build_panel(mut self, num_buildings: usize, sw: f32, sh: f32) -> Self {
        let rows = ((num_buildings as f32) / BUILD_COLS as f32).ceil().max(1.0) as usize;
        let pw = BUILD_COLS as f32 * (BUILD_BTN + 4.0) + 8.0;
        let ph = rows as f32 * (BUILD_BTN_H + 4.0) + 30.0;
        self.build_panel = Some(Rect::new(
            (sw - pw) * 0.5,
            sh - BAR_H - 8.0 - ph,
            pw, ph,
        ));
        self
    }

    /// Does a screen point land on any HUD panel? World input is skipped when true.
    pub fn contains(&self, p: Vec2) -> bool {
        self.top.contains(p)
            || self.bottom.contains(p)
            || self.minimap.contains(p)
            || self.selection.is_some_and(|r| r.contains(p))
            || self.command.is_some_and(|r| r.contains(p))
            || self.build_panel.is_some_and(|r| r.contains(p))
            || self.route_panel.is_some_and(|r| r.contains(p))
            || self.depot_panel.is_some_and(|r| r.contains(p))
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
pub struct BuildState<'a> {
    pub buildings: &'a [BuildingDef],
    pub placing: Option<usize>,
    pub panel_open: bool,
}

#[allow(clippy::too_many_arguments)]
pub fn draw(
    ui: &mut Ui,
    world: &World,
    eco: &Economy,
    ai: &crate::ai_brain::AiBrain,
    groups: &crate::combat_group::GroupRegistry,
    layout: &HudLayout,
    build: &BuildState<'_>,
) -> HudAction {
    draw_top_bar(ui, eco, ai, layout.top);
    let mut action = draw_bottom_bar(ui, world, layout.bottom, build.panel_open);
    if let Some(r) = layout.command {
        if let Some(a) = draw_command_card(ui, world, r) {
            action = a;
        }
    }
    if let Some(a) = draw_group_panel(ui, groups, world, layout.bottom) {
        action = a;
    }
    if let Some(r) = layout.build_panel {
        if let Some(a) = draw_build_panel(ui, build.buildings, r, build.placing) {
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

/// Group summary panel: clickable cards showing strength, avg ammo %, avg fuel %.
fn draw_group_panel(ui: &mut Ui, groups: &crate::combat_group::GroupRegistry, world: &World, bottom: Rect) -> Option<HudAction> {
    use crate::components::{AmmoStorage, FuelTank};

    let mut x = bottom.x + 262.0;
    let player_faction = crate::PLAYER_FACTION;
    let mut action = None;
    let accent = Color::new(0.45, 1.0, 0.55, 0.95);

    for g in groups.all() {
        if g.faction != player_faction { continue; }
        let card_w = 148.0;
        let card_h = 50.0;
        let r = Rect::new(x, bottom.y + 3.0, card_w, card_h);

        let selected = g.any_selected(world);
        if selected {
            draw_rectangle(r.x - 1.0, r.y - 1.0, r.w + 2.0, r.h + 2.0,
                Color::new(0.45, 1.0, 0.55, 0.18));
        }
        ui.panel(r);
        if selected {
            draw_rectangle_lines(r.x, r.y, r.w, r.h, 2.0, accent);
        }
        if ui.button(r, "") {
            action = Some(HudAction::SelectGroup(g.id));
        }

        // ── Name ─────────────────────────────────────────────────────────
        ui.label(vec2(r.x + 6.0, r.y + 14.0), &g.name);

        // ── Strength bar ─────────────────────────────────────────────────
        let strength = g.strength();
        let orig = g.original_strength.max(1);
        let frac = strength as f32 / orig as f32;
        let bar_w = r.w - 12.0;
        let str_bar = Rect::new(r.x + 6.0, r.y + 19.0, bar_w, 5.0);
        let str_fill = if frac > 0.6 { Color::new(0.4, 0.9, 0.4, 1.0) }
                       else if frac > 0.3 { Color::new(0.9, 0.8, 0.3, 1.0) }
                       else { Color::new(0.9, 0.3, 0.3, 1.0) };
        ui.bar(str_bar, frac, str_fill);

        // ── Ammo + fuel averages (only for members that have storage) ────
        let (mut ammo_sum, mut ammo_n) = (0.0f32, 0u32);
        let (mut fuel_sum, mut fuel_n) = (0.0f32, 0u32);
        for &e in &g.members {
            if let Ok(a) = world.get::<&AmmoStorage>(e) {
                ammo_sum += a.shots as f32 / a.capacity.max(1) as f32;
                ammo_n += 1;
            }
            if let Ok(f) = world.get::<&FuelTank>(e) {
                fuel_sum += f.fuel / f.capacity.max(0.001);
                fuel_n += 1;
            }
        }

        let mut bar_y = r.y + 27.0;

        if ammo_n > 0 {
            let avg = ammo_sum / ammo_n as f32;
            let ammo_bar = Rect::new(r.x + 6.0, bar_y, bar_w, 4.0);
            let ammo_col = if avg > 0.4 { Color::new(1.0, 0.85, 0.3, 1.0) }
                           else if avg > 0.1 { Color::new(1.0, 0.55, 0.2, 1.0) }
                           else { Color::new(1.0, 0.25, 0.25, 1.0) };
            ui.bar(ammo_bar, avg, ammo_col);
            bar_y += 6.0;
        }

        if fuel_n > 0 {
            let avg = fuel_sum / fuel_n as f32;
            let fuel_bar = Rect::new(r.x + 6.0, bar_y, bar_w, 4.0);
            let fuel_col = if avg > 0.4 { Color::new(0.35, 0.75, 1.0, 1.0) }
                           else if avg > 0.1 { Color::new(0.5, 0.5, 1.0, 1.0) }
                           else { Color::new(1.0, 0.25, 0.25, 1.0) };
            ui.bar(fuel_bar, avg, fuel_col);
        }

        // Strength count bottom-right
        let fs = ui.theme.font_size as u16;
        let label = format!("{}/{}", strength, orig);
        let d = measure_text(&label, None, fs, 1.0);
        ui.label(vec2(r.x + r.w - 6.0 - d.width, r.y + card_h - 4.0), &label);

        x += card_w + 5.0;
    }
    action
}

/// Top bar: resource readouts with colour-coded values + AI prep timer.
///
/// Colour rules per value:
/// - `> 200` → white (healthy)
/// - `1..=200` → amber (low)
/// - `== 0` → red (empty / critical)
fn draw_top_bar(ui: &mut Ui, eco: &Economy, ai: &crate::ai_brain::AiBrain, r: Rect) {
    ui.panel(r);
    let mp: Vec2 = mouse_position().into();
    let fs = ui.theme.font_size as u16;
    let white  = WHITE;
    let amber  = Color::new(1.0, 0.72, 0.25, 1.0);
    let red    = Color::new(1.0, 0.25, 0.25, 1.0);

    // AI prep countdown — shown right-aligned in the top bar.
    if ai.is_preparing() {
        let secs = ai.prep_seconds_left();
        let mins = (secs / 60.0) as u32;
        let s = (secs as u32) % 60;
        let text = format!("ENEMY IN  {:02}:{:02}", mins, s);
        let warn = Color::new(1.0, 0.55, 0.3, 1.0);
        let d = measure_text(&text, None, fs, 1.0);
        ui.label_colored(vec2(r.x + r.w - d.width - 14.0, r.y + 23.0), &text, warn);
    } else {
        let d = measure_text("ENEMY ADVANCING", None, fs, 1.0);
        ui.label_colored(vec2(r.x + r.w - d.width - 14.0, r.y + 23.0), "ENEMY ADVANCING", Color::new(1.0, 0.35, 0.35, 1.0));
    }

    /// Pick the display colour for a resource quantity.
    fn res_color(v: u32, white: Color, amber: Color, red: Color) -> Color {
        if v == 0          { red }
        else if v <= 200   { amber }
        else               { white }
    }

    let segments: [(&str, u32, &str); 4] = [
        ("Ammo",     eco.ammo,     "Ammo — consumed by combat. Flows from depots."),
        ("Fuel",     eco.fuel,     "Fuel — consumed by vehicles. Flows from refineries."),
        ("Supplies", eco.supplies, "Building Supplies — used for construction and repairs."),
        ("Parts",    eco.parts,    "Weapon Parts — enables production and reinforcement."),
    ];

    let mut x = r.x + 14.0;
    let mut tip: Option<(&str, Vec2)> = None;
    for (name, val, help) in &segments {
        let text = format!("{name} {val}");
        let color = res_color(*val, white, amber, red);
        let w = measure_text(&text, None, fs, 1.0).width;
        let seg = Rect::new(x - 6.0, r.y + 3.0, w + 12.0, r.h - 6.0);
        let pos = vec2(x, r.y + 23.0);
        ui.label_colored(pos, &text, color);
        if seg.contains(mp) {
            tip = Some((help, vec2(mp.x, r.y + r.h + 6.0)));
        }
        x += w + 26.0;
    }
    if let Some((t, at)) = tip {
        ui.tooltip(t, at);
    }
}

/// Bottom command bar: Stop / Clear / Build + selection count.
fn draw_bottom_bar(ui: &mut Ui, world: &World, r: Rect, build_open: bool) -> HudAction {
    ui.panel(r);
    let mut action = HudAction::None;
    if ui.button(Rect::new(r.x + 10.0, r.y + 8.0, 76.0, 40.0), "Stop") {
        action = HudAction::Stop;
    }
    if ui.button(Rect::new(r.x + 94.0, r.y + 8.0, 76.0, 40.0), "Clear") {
        action = HudAction::ClearSel;
    }
    // Build toggle button — highlighted when panel is open
    let build_rect = Rect::new(r.x + 178.0, r.y + 8.0, 76.0, 40.0);
    if build_open {
        draw_rectangle(build_rect.x - 1.0, build_rect.y - 1.0, build_rect.w + 2.0, build_rect.h + 2.0,
            Color::new(0.45, 1.0, 0.55, 0.22));
    }
    if ui.button(build_rect, "Build") {
        action = if build_open { HudAction::CloseBuildPanel } else { HudAction::PlaceBuilding(0) };
    }
    if build_open {
        draw_rectangle_lines(build_rect.x, build_rect.y, build_rect.w, build_rect.h, 2.0,
            Color::new(0.45, 1.0, 0.55, 0.9));
    }
    let n = world.query::<&Selected>().iter().count();
    ui.label(vec2(r.x + 262.0, r.y + 34.0), &format!("Selected: {n}"));
    action
}

/// Build panel: grid of building type buttons, shown above the bottom bar.
/// Returns PlaceBuilding(idx) when a button is clicked, CloseBuildPanel on X.
fn draw_build_panel(
    ui: &mut Ui,
    buildings: &[BuildingDef],
    r: Rect,
    placing: Option<usize>,
) -> Option<HudAction> {
    ui.panel(r);

    // Title + close button
    ui.label(vec2(r.x + 10.0, r.y + 20.0), "Place Blueprint");
    if ui.button(Rect::new(r.x + r.w - 30.0, r.y + 6.0, 24.0, 20.0), "X") {
        return Some(HudAction::CloseBuildPanel);
    }

    let pad = 4.0;
    let mut action = None;
    let accent = Color::new(0.45, 1.0, 0.55, 0.95);

    // Filter out HQ — players don't place HQs
    let placeable: Vec<(usize, &BuildingDef)> = buildings.iter().enumerate()
        .filter(|(_, b)| b.id != "hq")
        .collect();

    for (slot, (orig_idx, def)) in placeable.iter().enumerate() {
        let col = slot % BUILD_COLS;
        let row = slot / BUILD_COLS;
        let bx = r.x + pad + col as f32 * (BUILD_BTN + pad);
        let by = r.y + 30.0 + row as f32 * (BUILD_BTN_H + pad);
        let btn = Rect::new(bx, by, BUILD_BTN, BUILD_BTN_H);

        let is_selected = placing == Some(*orig_idx);
        if is_selected {
            draw_rectangle(btn.x - 1.0, btn.y - 1.0, btn.w + 2.0, btn.h + 2.0,
                Color::new(0.45, 1.0, 0.55, 0.22));
        }
        if ui.button(btn, "") {
            action = Some(HudAction::PlaceBuilding(*orig_idx));
        }
        if is_selected {
            draw_rectangle_lines(btn.x, btn.y, btn.w, btn.h, 2.0, accent);
        }

        // Building colour swatch
        let swatch = Rect::new(btn.x + 6.0, btn.y + 6.0, 18.0, 18.0);
        let c = Color::from_rgba(def.color.0, def.color.1, def.color.2, 220);
        draw_rectangle(swatch.x, swatch.y, swatch.w, swatch.h, c);
        draw_rectangle_lines(swatch.x, swatch.y, swatch.w, swatch.h, 1.0,
            Color::new(1.0, 1.0, 1.0, 0.4));

        // Name (truncated to fit)
        let name = if def.name.len() > 14 { &def.name[..14] } else { &def.name };
        ui.label(vec2(btn.x + 6.0, btn.y + 30.0), name);

        // Supply cost
        if def.required_supplies > 0 {
            let cost = format!("{}sup", def.required_supplies);
            let fs = ui.theme.font_size as u16;
            let d = measure_text(&cost, None, fs, 1.0);
            let amber = Color::new(1.0, 0.72, 0.25, 1.0);
            ui.label_colored(vec2(btn.x + btn.w - d.width - 4.0, btn.y + 46.0), &cost, amber);
        }

        // Size label
        let size = format!("{}×{}", def.w, def.h);
        ui.label(vec2(btn.x + 6.0, btn.y + 46.0), &size);
    }

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

// ─── Building Context Panel ──────────────────────────────────────────────────

/// Unified right-click context panel for any building entity.
/// Shows type, health, depot stocks, extractor/processor status, blueprint progress.
/// Returns true when the close button is pressed.
pub fn draw_building_context(
    ui: &mut Ui,
    building_e: hecs::Entity,
    world: &World,
    sw: f32,
    sh: f32,
) -> bool {
    use crate::components::{Blueprint, BuildingKind, Health, IsBuilding, UnitKind};
    use crate::depot::{Depot, ResourceType};

    // Panel: right-side, 220×240, anchored above bottom bar
    let pw = 220.0_f32;
    let ph = 240.0_f32;
    let r = Rect::new(sw - pw - 8.0, sh - BAR_H - 8.0 - ph, pw, ph);
    ui.panel(r);

    let mut closed = false;
    if ui.button(Rect::new(r.x + r.w - 26.0, r.y + 6.0, 20.0, 16.0), "X") {
        closed = true;
    }

    let mut y = r.y + 20.0;

    // ── Title ────────────────────────────────────────────────────────────────
    // Check if this is actually a unit (engineer with IsBuilding) or a building
    if let Ok(uk) = world.get::<&UnitKind>(building_e) {
        ui.label(vec2(r.x + 8.0, y), &uk.name.to_uppercase());
        y += 14.0;
        if let Ok(ib) = world.get::<&IsBuilding>(building_e) {
            if let Ok(bp) = world.get::<&Blueprint>(ib.blueprint) {
                let pct = (bp.progress * 100.0) as u32;
                ui.label(vec2(r.x + 8.0, y + 12.0), &format!("Building: {}%", pct));
                let bw = r.w - 16.0;
                draw_rectangle(r.x + 8.0, y + 16.0, bw, 6.0, Color::new(0.15,0.17,0.20,1.0));
                draw_rectangle(r.x + 8.0, y + 16.0, bw * bp.progress, 6.0, Color::new(0.4,0.8,0.4,1.0));
                y += 28.0;
                ui.label(vec2(r.x + 8.0, y + 12.0), &format!("Supplies used: {}/{}", bp.supplies_consumed, bp.required_supplies));
            }
        }
        return closed;
    }

    let kind = world.get::<&BuildingKind>(building_e)
        .map(|k| k.0.replace('_', " ").to_uppercase())
        .unwrap_or_else(|_| "BUILDING".into());
    ui.label(vec2(r.x + 8.0, y), &kind);
    y += 14.0;

    // ── Health ───────────────────────────────────────────────────────────────
    if let Ok(h) = world.get::<&Health>(building_e) {
        let frac = (h.cur / h.max).clamp(0.0, 1.0);
        let hcol = if frac > 0.5 { Color::new(0.35,0.9,0.4,1.0) }
                   else if frac > 0.25 { Color::new(0.95,0.85,0.3,1.0) }
                   else { Color::new(0.95,0.35,0.3,1.0) };
        let bw = r.w - 16.0;
        draw_rectangle(r.x + 8.0, y, bw, 6.0, Color::new(0.15,0.17,0.20,1.0));
        draw_rectangle(r.x + 8.0, y, bw * frac, 6.0, hcol);
        ui.label(vec2(r.x + 8.0, y + 14.0), &format!("HP: {:.0}/{:.0}", h.cur, h.max));
        y += 22.0;
    }

    // ── Depot stocks ─────────────────────────────────────────────────────────
    if let Ok(depot) = world.get::<&Depot>(building_e) {
        let res_list = [
            (ResourceType::Ammo,             "Ammo",  Color::new(1.0,0.80,0.20,1.0)),
            (ResourceType::Fuel,             "Fuel",  Color::new(0.3,0.80,1.00,1.0)),
            (ResourceType::BuildingSupplies, "Supp",  Color::new(0.8,0.65,0.35,1.0)),
            (ResourceType::WeaponParts,      "Parts", Color::new(0.75,0.40,1.00,1.0)),
        ];
        for (res, name, col) in &res_list {
            let val = depot.get(*res);
            draw_rectangle(r.x + 8.0, y, 5.0, 12.0, *col);
            let tcol = if val == 0 { Color::new(1.0,0.3,0.3,1.0) }
                       else if val < 200 { Color::new(1.0,0.72,0.25,1.0) }
                       else { ui.theme.text };
            ui.label_colored(vec2(r.x + 18.0, y + 11.0), &format!("{name}: {val}"), tcol);
            let bw = r.w - 26.0;
            draw_rectangle(r.x + 18.0, y + 13.0, bw, 3.0, Color::new(0.15,0.17,0.20,1.0));
            let fw = (val as f32 / 2000.0).clamp(0.0,1.0) * bw;
            draw_rectangle(r.x + 18.0, y + 13.0, fw, 3.0, *col);
            y += 20.0;
        }
    }

    // ── Extractor status ─────────────────────────────────────────────────────
    if let Ok(ext) = world.get::<&crate::extraction::Extractor>(building_e) {
        let res = ext.kind.output_resource();
        let amt = ext.kind.output_amount();
        let cycle = ext.kind.cycle_secs();
        let next = (cycle - ext.cooldown.min(cycle)).max(0.0);
        ui.label(vec2(r.x + 8.0, y + 12.0),
            &format!("Produces {:?}×{} every {:.0}s", res, amt, cycle));
        ui.label(vec2(r.x + 8.0, y + 26.0), &format!("Next: {:.1}s", next));
        y += 36.0;
    }

    // ── Processor status ─────────────────────────────────────────────────────
    if let Ok(proc) = world.get::<&crate::processing::Processor>(building_e) {
        let (out_res, out_amt) = proc.kind.output();
        let cycle = proc.kind.cycle_secs();
        let next = (cycle - proc.cooldown.min(cycle)).max(0.0);
        if let Some((in_res, in_amt)) = proc.kind.input() {
            ui.label(vec2(r.x + 8.0, y + 12.0),
                &format!("{:?}({})->  {:?}({})", in_res, in_amt, out_res, out_amt));
        } else {
            ui.label(vec2(r.x + 8.0, y + 12.0), &format!("Produces {:?}×{}", out_res, out_amt));
        }
        ui.label(vec2(r.x + 8.0, y + 26.0), &format!("Next: {:.1}s  / {:.0}s", next, cycle));
    }

    closed
}

// ─── Depot Inspection Panel ──────────────────────────────────────────────────

/// Floating info panel showing a depot's current stock levels.
/// Call when `layout.depot_panel` is Some (depot is selected).
/// Returns true if the close button was clicked.
pub fn draw_depot_panel(
    ui: &mut Ui,
    depot_e: hecs::Entity,
    world: &World,
    layout: &HudLayout,
) -> bool {
    use crate::depot::{Depot, ResourceType};

    let r = match layout.depot_panel { Some(r) => r, None => return false };
    ui.panel(r);

    // Title: depot kind if available
    let kind = world.get::<&crate::components::BuildingKind>(depot_e)
        .map(|k| k.0.replace('_', " ").to_uppercase())
        .unwrap_or_else(|_| "DEPOT".into());
    ui.label(vec2(r.x + 8.0, r.y + 18.0), &kind);

    let mut closed = false;
    if ui.button(Rect::new(r.x + r.w - 26.0, r.y + 6.0, 20.0, 16.0), "X") {
        closed = true;
    }

    let depot = match world.get::<&Depot>(depot_e) {
        Ok(d) => d,
        Err(_) => return closed,
    };

    let res_list = [
        (ResourceType::Ammo,             "Ammo",     Color::new(1.0, 0.80, 0.20, 1.0)),
        (ResourceType::Fuel,             "Fuel",     Color::new(0.3, 0.80, 1.00, 1.0)),
        (ResourceType::BuildingSupplies, "Supplies", Color::new(0.8, 0.65, 0.35, 1.0)),
        (ResourceType::WeaponParts,      "Parts",    Color::new(0.75, 0.40, 1.00, 1.0)),
    ];

    let mut y = r.y + 32.0;
    for (res, name, col) in &res_list {
        let val = depot.get(*res);
        draw_rectangle(r.x + 8.0, y, 6.0, 14.0, *col);
        let label = format!("{name}: {val}");
        let text_col = if val == 0 { Color::new(1.0, 0.3, 0.3, 1.0) }
                       else if val < 200 { Color::new(1.0, 0.72, 0.25, 1.0) }
                       else { ui.theme.text };
        ui.label_colored(vec2(r.x + 20.0, y + 12.0), &label, text_col);
        // Mini bar
        let bw = r.w - 24.0;
        let cap = 2000u32;
        draw_rectangle(r.x + 8.0, y + 16.0, bw, 4.0, Color::new(0.15, 0.17, 0.20, 1.0));
        let fill_w = (val as f32 / cap as f32).clamp(0.0, 1.0) * bw;
        draw_rectangle(r.x + 8.0, y + 16.0, fill_w, 4.0, *col);
        y += 34.0;
    }
    closed
}

// ─── Supply Route Panel ───────────────────────────────────────────────────────

/// Draw the supply route management panel on the left side when routes exist or
/// route mode is active. Returns the id of any route whose Delete button was clicked.
pub fn draw_route_panel(
    ui: &mut Ui,
    routes: &crate::supply_route::RouteRegistry,
    world: &World,
    route_mode_active: bool,
    route_resource_idx: usize,
    layout: &HudLayout,
) -> Option<u32> {
    use crate::depot::ResourceType;

    let panel = layout.route_panel?;
    ui.panel(panel);

    let (px, row_h) = (panel.x, 36.0_f32);
    let panel_w = panel.w;
    let mut y = panel.y + 18.0;
    ui.label(vec2(px + 8.0, y), "Supply Routes");
    y += 12.0;

    let res_names = ["Ammo", "Fuel", "Supplies", "Parts"];
    let res_colors = [
        Color::new(1.0, 0.80, 0.20, 1.0),
        Color::new(0.3, 0.80, 1.00, 1.0),
        Color::new(0.8, 0.65, 0.35, 1.0),
        Color::new(0.75, 0.40, 1.00, 1.0),
    ];

    fn res_idx(r: ResourceType) -> usize {
        match r {
            ResourceType::Ammo             => 0,
            ResourceType::Fuel             => 1,
            ResourceType::BuildingSupplies => 2,
            ResourceType::WeaponParts      => 3,
        }
    }

    let mut delete_id = None;
    for route in routes.all() {
        let ri = res_idx(route.resource);
        let col = res_colors[ri];

        // Health dot: green = healthy, amber = low (<40%), red = empty
        let dest_stock = world.get::<&crate::depot::Depot>(route.destination)
            .map(|d| d.get(route.resource)).unwrap_or(0);
        let fill_frac = if route.desired_stock > 0 { dest_stock as f32 / route.desired_stock as f32 } else { 1.0 };
        let health_col = if fill_frac >= 0.4 { Color::new(0.3, 0.9, 0.3, 1.0) }
                         else if fill_frac > 0.0 { Color::new(1.0, 0.72, 0.25, 1.0) }
                         else { Color::new(1.0, 0.3, 0.3, 1.0) };
        draw_circle(px + 10.0, y + 12.0, 5.0, health_col);
        draw_rectangle(px + 20.0, y, 4.0, 24.0, col);

        let label = format!("{} {}/{} ({}🚛)", res_names[ri], dest_stock, route.desired_stock, route.active_trucks);
        ui.label(vec2(px + 28.0, y + 16.0), &label);

        if ui.button(Rect::new(px + panel_w - 30.0, y + 6.0, 22.0, 20.0), "X") {
            delete_id = Some(route.id);
        }
        y += row_h;
    }

    if routes.all().is_empty() {
        ui.label(vec2(px + 8.0, y + 16.0), "No routes. T=draw route");
        y += row_h;
    }

    if route_mode_active {
        let ri = route_resource_idx % 4;
        let mode_label = format!("Drawing: {} (Tab to change)", res_names[ri]);
        draw_rectangle(px + 6.0, y + 4.0, panel_w - 12.0, 32.0, Color::new(0.45, 1.0, 0.55, 0.12));
        draw_rectangle_lines(px + 6.0, y + 4.0, panel_w - 12.0, 32.0, 1.5, Color::new(0.45, 1.0, 0.55, 0.8));
        ui.label_colored(vec2(px + 12.0, y + 22.0), &mode_label, Color::new(0.45, 1.0, 0.55, 1.0));
    }

    delete_id
}
