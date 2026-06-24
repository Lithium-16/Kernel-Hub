//! Rendering layer (macroquad). Everything sprite-based draws from one atlas texture
//! so draws batch into a few GPU calls. Order: tiles -> selection rings -> units ->
//! hitboxes (shapes) -> screen overlay.

use macroquad::prelude::*;

use crate::assets::Sprites;
use crate::camera::GameCamera;
use crate::components::{Heading, MoveOrder, Position, Renderable, Selected};
use crate::map::{self, TileMap};
use crate::sim::Sim;

const BG: Color = Color::new(0.07, 0.085, 0.11, 1.0);

/// Render one frame. `target = None` draws to the screen; `Some(rt)` draws offscreen.
#[allow(clippy::too_many_arguments)]
pub fn present(
    world: &hecs::World,
    map: &TileMap,
    fog: &crate::fog::FogGrid,
    camera: &GameCamera,
    sim: &Sim,
    sprites: &Sprites,
    drag: Option<(Vec2, Vec2)>,
    ghost: Option<(Rect, bool)>,
    tick_ms: f32,
    target: Option<RenderTarget>,
) {
    let sw = screen_width();
    let sh = screen_height();
    let view = camera.view_rect(sw, sh);

    let mut world_cam = Camera2D::from_display_rect(view);
    world_cam.render_target = target.clone();
    set_camera(&world_cam);
    clear_background(BG);
    draw_tiles(map, view, sprites);
    draw_buildings(world);
    draw_depots(world);
    draw_move_orders(world);
    draw_selection_rings(world, sprites);
    // Units: hull+turret sprites when loaded, atlas fallback otherwise.
    draw_entities_fogged(world, fog, sprites, camera.scale);
    // Turret barrel lines only shown when using atlas fallback (no turret sprite yet).
    if !sprites.has_hull_turrets() {
        draw_turret_barrels_fogged(world, fog, camera.scale);
    }
    draw_building_turrets(world, fog);
    draw_engineer_beams(world);
    draw_tracers(world);
    draw_health_bars_fogged(world, fog);
    draw_hitboxes(world);
    draw_range_circles(world);
    draw_blueprints(world);
    draw_fog_overlay(fog, view);

    // Placement ghost (world space): green = valid, red = blocked.
    if let Some((r, valid)) = ghost {
        let (fill, edge) = if valid {
            (Color::new(0.45, 1.0, 0.55, 0.25), Color::new(0.45, 1.0, 0.55, 0.95))
        } else {
            (Color::new(1.0, 0.35, 0.35, 0.25), Color::new(1.0, 0.35, 0.35, 0.95))
        };
        draw_rectangle(r.x, r.y, r.w, r.h, fill);
        draw_rectangle_lines(r.x, r.y, r.w, r.h, 2.0, edge);
    }

    match &target {
        None => set_default_camera(),
        Some(_) => {
            let mut ui = Camera2D::from_display_rect(Rect::new(0.0, 0.0, sw, sh));
            ui.render_target = target.clone();
            set_camera(&ui);
        }
    }
    if let Some((a, b)) = drag {
        let r = Rect::new(a.x.min(b.x), a.y.min(b.y), (b.x - a.x).abs(), (b.y - a.y).abs());
        draw_rectangle_lines(r.x, r.y, r.w, r.h, 1.5, Color::new(0.5, 1.0, 0.6, 0.9));
    }
    // Building labels drawn in screen space (world-space text appears mirrored).
    draw_building_labels(world, view, sw, sh, camera.scale);
    draw_overlay(world, map, camera, sim, tick_ms, sh);
    set_default_camera();
}

fn draw_tiles(map: &TileMap, view: Rect, sprites: &Sprites) {
    let ts = map::TILE_SIZE;
    let min_tx = ((view.x / ts).floor() as i32).max(0) as usize;
    let min_ty = ((view.y / ts).floor() as i32).max(0) as usize;
    let max_tx = (((view.x + view.w) / ts).ceil() as i32).clamp(0, map.width as i32) as usize;
    let max_ty = (((view.y + view.h) / ts).ceil() as i32).clamp(0, map.height as i32) as usize;

    for ty in min_ty..max_ty {
        for tx in min_tx..max_tx {
            let src = sprites.tile_rect(map.get(tx, ty));
            // Per-tile brightness jitter so terrain isn't a flat slab of one color.
            let j = map::tile_jitter(tx, ty) * 0.09;
            let s = (1.0 + j).clamp(0.0, 1.4);
            let shade = Color::new(s, s, s, 1.0);
            draw_texture_ex(
                &sprites.atlas,
                tx as f32 * ts,
                ty as f32 * ts,
                shade,
                DrawTextureParams {
                    dest_size: Some(vec2(ts, ts)),
                    source: Some(src),
                    ..Default::default()
                },
            );
        }
    }
}

/// Flying bullets: a bright projectile travels shooter→target over its lifetime, with a
/// short colored trail (cyan = friendly fire, orange = enemy). Purely visual.
/// Bullet tracers: an invisible bullet travels from muzzle to target; the only visual
/// is the glowing wake it leaves — a short fixed-length trail that fades as it arrives.
/// No dot, no sphere — just the tracer streak.
fn draw_tracers(world: &hecs::World) {
    use crate::components::{Tracer, TRACER_TTL};
    const TRAIL_PX: f32 = 18.0; // world-px length of the streak behind the bullet
    for (_e, t) in world.query::<&Tracer>().iter() {
        let prog = (1.0 - t.ttl / TRACER_TTL).clamp(0.0, 1.0);
        let total = t.from.distance(t.to).max(0.001);
        let dir = (t.to - t.from) / total;
        // Bullet position along the path
        let head_dist = prog * total;
        let tail_dist = (head_dist - TRAIL_PX).max(0.0);
        let head = t.from + dir * head_dist;
        let tail = t.from + dir * tail_dist;
        // Fade out in the last 30% of travel so it disappears on impact
        let alpha = if prog > 0.7 { ((1.0 - prog) / 0.3).clamp(0.0, 1.0) } else { 1.0 };
        let c = t.color;
        draw_line(tail.x, tail.y, head.x, head.y, 1.5, Color::new(c.r, c.g, c.b, alpha * 0.95));
    }
}

fn draw_health_bars_fogged(world: &hecs::World, fog: &crate::fog::FogGrid) {
    use crate::components::{Faction, Health};
    for (_e, (pos, r, h, fac)) in world.query::<(&Position, &Renderable, &Health, &Faction)>().iter() {
        if fac.0 != crate::PLAYER_FACTION && !fog.visible_world(pos.0) { continue; }
        if h.cur >= h.max || h.max <= 0.0 {
            continue;
        }
        let frac = (h.cur / h.max).clamp(0.0, 1.0);
        let w = r.size * 0.8;
        let x = pos.0.x - w * 0.5;
        let y = pos.0.y - r.size * 0.62;
        let fill = if frac > 0.5 {
            Color::new(0.35, 0.9, 0.4, 0.95)
        } else if frac > 0.25 {
            Color::new(0.95, 0.85, 0.3, 0.95)
        } else {
            Color::new(0.95, 0.35, 0.3, 0.95)
        };
        draw_rectangle(x, y, w, 3.5, Color::new(0.0, 0.0, 0.0, 0.7));
        draw_rectangle(x, y, w * frac, 3.5, fill);
    }
}

/// Fog overlay: black for Hidden tiles, dark tint for LastSeen tiles.
fn draw_fog_overlay(fog: &crate::fog::FogGrid, view: Rect) {
    use crate::fog::FogState;
    let ts = crate::map::TILE_SIZE;
    let min_tx = ((view.x / ts).floor() as i32).max(0) as usize;
    let min_ty = ((view.y / ts).floor() as i32).max(0) as usize;
    let max_tx = (((view.x + view.w) / ts).ceil() as i32).clamp(0, fog.w as i32) as usize;
    let max_ty = (((view.y + view.h) / ts).ceil() as i32).clamp(0, fog.h as i32) as usize;
    for ty in min_ty..max_ty {
        for tx in min_tx..max_tx {
            let c = match fog.state(tx, ty) {
                FogState::Hidden   => Color::new(0.0, 0.0, 0.0, 1.0),
                FogState::LastSeen => Color::new(0.0, 0.0, 0.0, 0.55),
                FogState::Visible  => continue,
            };
            draw_rectangle(tx as f32 * ts, ty as f32 * ts, ts, ts, c);
        }
    }
}

/// Draw all placed buildings, with health bars for any that have taken damage.
///
/// Buildings with a Health component show a bar above the footprint when cur < max.
/// Bar width = footprint width * 0.8, positioned 6px above the building top edge.
fn draw_buildings(world: &hecs::World) {
    use crate::components::{BuildingKind, Health, LoadingZone};
    let ts = map::TILE_SIZE;
    let edge = Color::new(0.05, 0.06, 0.08, 1.0);
    for (_e, (b, health_opt, _kind_opt, lz_opt)) in
        world.query::<(&crate::components::Building, Option<&Health>, Option<&BuildingKind>, Option<&LoadingZone>)>().iter()
    {
        let (x, y) = (b.tx as f32 * ts, b.ty as f32 * ts);
        let (w, h) = (b.w as f32 * ts, b.h as f32 * ts);

        // Loading zone tile: amber stripe below the building footprint.
        if let Some(lz) = lz_opt {
            let lz_x = lz.world_pos.x - ts * 0.5;
            let lz_y = lz.world_pos.y - ts * 0.5;
            draw_rectangle(lz_x, lz_y, ts, ts, Color::new(0.70, 0.55, 0.20, 0.55));
            draw_rectangle_lines(lz_x, lz_y, ts, ts, 1.0, Color::new(1.0, 0.80, 0.35, 0.70));
        }

        draw_rectangle(x, y, w, h, b.color);
        draw_rectangle_lines(x, y, w, h, 2.0, edge);

        // Health bar: only shown when damaged.
        if let Some(hp) = health_opt {
            if hp.cur < hp.max && hp.max > 0.0 {
                let frac = (hp.cur / hp.max).clamp(0.0, 1.0);
                let bar_w = w * 0.8;
                let bar_x = x + (w - bar_w) * 0.5;
                let bar_y = y - 6.0;
                let fill = if frac > 0.5 {
                    Color::new(0.35, 0.9, 0.4, 0.95)
                } else if frac > 0.25 {
                    Color::new(0.95, 0.85, 0.3, 0.95)
                } else {
                    Color::new(0.95, 0.35, 0.3, 0.95)
                };
                draw_rectangle(bar_x, bar_y, bar_w, 4.0, Color::new(0.0, 0.0, 0.0, 0.7));
                draw_rectangle(bar_x, bar_y, bar_w * frac, 4.0, fill);
            }
        }
    }
}

/// Draw all Blueprint entities: translucent faction-tinted footprint + bright outline +
/// green progress bar below. Called BEFORE draw_fog_overlay so fog can mask them.
fn draw_blueprints(world: &hecs::World) {
    use crate::components::{Blueprint, Building, Faction};
    let ts = map::TILE_SIZE;
    for (_e, (bp, b, fac)) in
        world.query::<(&Blueprint, &Building, &Faction)>().iter()
    {
        let (x, y) = (b.tx as f32 * ts, b.ty as f32 * ts);
        let (w, h) = (b.w as f32 * ts, b.h as f32 * ts);

        // Faction tint: blue for player, red for enemy.
        let (fill_tint, edge_tint) = if fac.0 == crate::PLAYER_FACTION {
            (
                Color::new(0.3, 0.55, 1.0, 0.35),
                Color::new(0.4, 0.7, 1.0, 0.90),
            )
        } else {
            (
                Color::new(1.0, 0.3, 0.3, 0.35),
                Color::new(1.0, 0.45, 0.45, 0.90),
            )
        };

        // Translucent footprint fill.
        draw_rectangle(x, y, w, h, fill_tint);
        // Bright outline.
        draw_rectangle_lines(x, y, w, h, 2.0, edge_tint);

        // Progress bar below the footprint.
        let bar_w = w * 0.8;
        let bar_h = 5.0;
        let bar_x = x + (w - bar_w) * 0.5;
        let bar_y = (b.ty + b.h) as f32 * ts + 4.0;
        let frac = bp.progress.clamp(0.0, 1.0);
        draw_rectangle(bar_x, bar_y, bar_w, bar_h, Color::new(0.1, 0.1, 0.1, 0.75));
        draw_rectangle(bar_x, bar_y, bar_w * frac, bar_h, Color::new(0.25, 0.95, 0.35, 0.92));
    }
}

fn draw_selection_rings(world: &hecs::World, sprites: &Sprites) {
    let tint = Color::new(0.45, 1.0, 0.55, 0.9);
    let src = sprites.selection_rect();
    for (_e, (pos, r, _)) in world.query::<(&Position, &Renderable, &Selected)>().iter() {
        let s = r.size * 1.9;
        draw_texture_ex(
            &sprites.atlas,
            pos.0.x - s * 0.5,
            pos.0.y - s * 0.5,
            tint,
            DrawTextureParams {
                dest_size: Some(vec2(s, s)),
                source: Some(src),
                ..Default::default()
            },
        );
    }
}

/// Below this zoom level units render as colored dots for legibility.
const OVERVIEW_ZOOM: f32 = 0.22;

/// Draw all units visible through fog.
///
/// When hull/turret textures are loaded (frame 1+):
///   1. Hull   — 256×256, rotates with Heading (body direction).
///   2. Turret — 256×256, same canvas pivot, rotates with Turret.angle independently.
///
/// Before frame 1, falls back to the placeholder atlas sprite (atlas-based rendering).
fn draw_entities_fogged(world: &hecs::World, fog: &crate::fog::FogGrid, sprites: &Sprites, cam_scale: f32) {
    use crate::components::{Faction, Turret};
    let overview = cam_scale < OVERVIEW_ZOOM;
    let real_sprites = sprites.has_hull_turrets();

    for (_e, (pos, r, head, fac, turret_opt)) in
        world.query::<(&Position, &Renderable, &Heading, &Faction, Option<&Turret>)>().iter()
    {
        let is_enemy = fac.0 != crate::PLAYER_FACTION;
        let vis = !is_enemy || fog.visible_world(pos.0);
        if !vis { continue; }

        if overview {
            let dot_r = 3.0 / cam_scale;
            draw_circle(pos.0.x, pos.0.y, dot_r, r.tint);
            continue;
        }

        let x    = pos.0.x - r.size * 0.5;
        let y    = pos.0.y - r.size * 0.5;
        let size = vec2(r.size, r.size);
        let hull_rot = head.0 + std::f32::consts::FRAC_PI_2;

        if real_sprites && !r.hull_sprite.is_empty() {
            // Hull sprite — rotates with body heading.
            if let Some(hull_tex) = sprites.hull(&r.hull_sprite, is_enemy) {
                draw_texture_ex(hull_tex, x, y, WHITE,
                    DrawTextureParams { dest_size: Some(size), rotation: hull_rot, ..Default::default() });
            }
            // Turret sprite — rotates with Turret.angle independently.
            if let Some(turret) = turret_opt {
                if !r.turret_sprite.is_empty() {
                    if let Some(turret_tex) = sprites.turret(&r.turret_sprite, is_enemy) {
                        let turret_rot = turret.angle + std::f32::consts::FRAC_PI_2;
                        draw_texture_ex(turret_tex, x, y, WHITE,
                            DrawTextureParams { dest_size: Some(size), rotation: turret_rot, ..Default::default() });
                    }
                }
            }
        } else {
            // Fallback: placeholder atlas sprite with faction tint.
            let src = sprites.unit_rect(r.sprite);
            draw_texture_ex(&sprites.atlas, x, y, r.tint,
                DrawTextureParams { dest_size: Some(size), source: Some(src), rotation: hull_rot, ..Default::default() });
        }
    }
}

/// Depot buildings: tan square + supply-range ring. Distinct from placeable buildings.
fn draw_depots(world: &hecs::World) {
    use crate::components::{Building, Faction};
    use crate::depot::Depot;
    let ts = map::TILE_SIZE;
    let sz = ts * 2.2;
    // Skip depots that are also Building entities (HQ, Supply Depot buildings) —
    // draw_buildings already draws their footprint; we only draw the supply-range ring.
    for (e, (pos, depot, fac)) in world.query::<(&Position, &Depot, &Faction)>().iter() {
        let is_building = world.get::<&Building>(e).is_ok();
        if is_building {
            // Just draw the supply-range ring so the player can see coverage.
            draw_circle_lines(pos.0.x, pos.0.y, depot.supply_range, 1.0,
                Color::new(1.0, 0.88, 0.55, 0.18));
            continue;
        }
        let fill = if fac.0 == crate::PLAYER_FACTION {
            Color::new(0.55, 0.48, 0.30, 0.90)
        } else {
            Color::new(0.50, 0.22, 0.22, 0.85)
        };
        let x = pos.0.x - sz * 0.5;
        let y = pos.0.y - sz * 0.5;
        draw_rectangle(x, y, sz, sz, fill);
        draw_rectangle_lines(x, y, sz, sz, 2.5, Color::new(1.0, 0.88, 0.55, 0.85));
        draw_circle_lines(pos.0.x, pos.0.y, depot.supply_range, 1.0, Color::new(1.0, 0.88, 0.55, 0.18));
        // Small inner marker square.
        let m = sz * 0.3;
        draw_rectangle(pos.0.x - m * 0.5, pos.0.y - m * 0.5, m, m, Color::new(1.0, 0.88, 0.55, 0.65));
    }
}

/// Move-order feedback for the **selected** units only (drawing a line for every moving
/// unit crisscrosses the field and reads like weapon fire). A faint line to each goal +
/// a destination marker at each distinct goal.
fn draw_move_orders(world: &hecs::World) {
    let line = Color::new(0.45, 1.0, 0.55, 0.22);
    let mark = Color::new(0.45, 1.0, 0.55, 0.95);
    let mut goals: Vec<Vec2> = Vec::new();
    for (_e, (pos, order, _sel)) in world.query::<(&Position, &MoveOrder, &Selected)>().iter() {
        draw_line(pos.0.x, pos.0.y, order.goal.x, order.goal.y, 1.0, line);
        if !goals.iter().any(|g| g.distance(order.goal) < 2.0) {
            goals.push(order.goal);
        }
    }
    for g in goals {
        draw_circle_lines(g.x, g.y, 11.0, 2.0, mark);
        draw_line(g.x - 9.0, g.y, g.x + 9.0, g.y, 1.5, mark);
        draw_line(g.x, g.y - 9.0, g.x, g.y + 9.0, 1.5, mark);
    }

    // Queued waypoints (Shift+RMB) for selected units: fainter rings at each anchor.
    let wp = Color::new(0.45, 1.0, 0.55, 0.5);
    let mut waypoints: Vec<Vec2> = Vec::new();
    for (_e, (q, _sel)) in world.query::<(&crate::components::OrderQueue, &Selected)>().iter() {
        for &a in &q.anchors {
            if !waypoints.iter().any(|p| p.distance(a) < 2.0) {
                waypoints.push(a);
            }
        }
    }
    for a in waypoints {
        draw_circle_lines(a.x, a.y, 7.0, 1.5, wp);
    }
}

fn draw_turret_barrels_fogged(world: &hecs::World, fog: &crate::fog::FogGrid, cam_scale: f32) {
    use crate::components::{Faction, Turret};
    // Skip turret barrels entirely in overview mode — dots don't have barrels.
    if cam_scale < OVERVIEW_ZOOM { return; }
    for (_e, (pos, r, turret, fac)) in world.query::<(&Position, &Renderable, &Turret, &Faction)>().iter() {
        if fac.0 != crate::PLAYER_FACTION && !fog.visible_world(pos.0) { continue; }
        let barrel_len = r.size * 0.52;
        let barrel_w = r.size * 0.14;
        let cos = turret.angle.cos();
        let sin = turret.angle.sin();
        let tip = pos.0 + vec2(cos, sin) * barrel_len;
        let base = pos.0 + vec2(cos, sin) * r.size * 0.10;
        draw_line(base.x, base.y, tip.x, tip.y, barrel_w, Color::new(0.18, 0.20, 0.22, 1.0));
        draw_circle(pos.0.x, pos.0.y, r.size * 0.22, Color::new(0.22, 0.24, 0.27, 1.0));
    }
}

/// Weapon range circles on selected units — helps the player understand engagement ranges.
fn draw_range_circles(world: &hecs::World) {
    use crate::components::Weapon;
    // Outline only — no filled circle so overlapping ranges don't compound opacity.
    let edge = Color::new(1.0, 0.85, 0.3, 0.45);
    for (_e, (pos, wpn, _sel)) in world.query::<(&Position, &Weapon, &Selected)>().iter() {
        if wpn.range > 0.0 {
            draw_circle_lines(pos.0.x, pos.0.y, wpn.range, 1.0, edge);
        }
    }
}

/// Faint neon-green collision circle for selected units (drawn on top).
fn draw_hitboxes(world: &hecs::World) {
    let c = Color::new(0.2, 1.0, 0.3, 0.55);
    for (_e, (pos, _)) in world.query::<(&Position, &Selected)>().iter() {
        draw_circle_lines(pos.0.x, pos.0.y, crate::movement::UNIT_RADIUS, 1.0, c);
    }
}

/// Convert world position to screen pixels, accounting for the Y-flip in the world camera.
fn world_to_screen(world_pos: Vec2, view: Rect, sw: f32, sh: f32) -> Vec2 {
    let nx = (world_pos.x - view.x) / view.w;
    let ny = 1.0 - (world_pos.y - view.y) / view.h;
    vec2(nx * sw, ny * sh)
}

/// Draw building kind labels in screen space. Only visible when zoomed in enough.
fn draw_building_labels(world: &hecs::World, view: Rect, sw: f32, sh: f32, cam_scale: f32) {
    use crate::components::{BuildingKind, Position};
    // Labels too small to read when fully zoomed out
    if cam_scale < 0.3 { return; }
    let font_sz = (10.0_f32 * cam_scale).clamp(9.0, 14.0);

    for (_e, (pos, kind)) in world.query::<(&Position, &BuildingKind)>().iter() {
        let sp = world_to_screen(pos.0, view, sw, sh);
        if sp.x < 0.0 || sp.x > sw || sp.y < 0.0 || sp.y > sh { continue; }
        let name = kind.0.replace('_', " ").to_uppercase();
        let d = measure_text(&name, None, font_sz as u16, 1.0);
        // Dark background chip
        draw_rectangle(sp.x - d.width * 0.5 - 2.0, sp.y - font_sz - 2.0,
                       d.width + 4.0, font_sz + 4.0,
                       Color::new(0.0, 0.0, 0.0, 0.65));
        draw_text(&name, sp.x - d.width * 0.5, sp.y, font_sz,
                  Color::new(1.0, 0.95, 0.80, 0.95));
    }
}

fn draw_overlay(world: &hecs::World, map: &TileMap, camera: &GameCamera, sim: &Sim, tick_ms: f32, sh: f32) {
    let selected = world.query::<&Selected>().iter().count();
    let moving = world.query::<&crate::components::MoveOrder>().iter().count();
    let _ = sim;
    // Below the top resource bar so the two don't overlap.
    draw_text(
        "Drag-select · RMB move · Ctrl+1-9 groups · G=cycle · B=build · T=route (Tab=resource) · R=restart",
        16.0,
        crate::hud::TOP_H + 24.0,
        24.0,
        WHITE,
    );
    let info = format!(
        "fps {} | tick {:.2}ms | entities {} | selected {} | moving {} | zoom {:.2} | map {}x{}",
        get_fps(),
        tick_ms,
        world.len(),
        selected,
        moving,
        camera.scale,
        map.width,
        map.height,
    );
    draw_text(&info, 16.0, sh - 66.0, 22.0, Color::new(0.80, 0.80, 0.80, 1.0));
}

/// Turret barrels for buildings — buildings have Turret+Position but not Renderable,
/// so they're skipped by draw_turret_barrels_fogged. Draw their barrels here.
fn draw_building_turrets(world: &hecs::World, fog: &crate::fog::FogGrid) {
    use crate::components::{Building, Faction, Turret};
    for (_e, (pos, b, turret, fac)) in
        world.query::<(&Position, &Building, &Turret, &Faction)>().iter()
    {
        // Hide enemy building turrets in fog
        if fac.0 != crate::PLAYER_FACTION && !fog.visible_world(pos.0) { continue; }

        let ts = map::TILE_SIZE;
        let building_w = b.w as f32 * ts;
        let building_h = b.h as f32 * ts;

        // Turret base: dark circle in the building centre
        let base_r = (building_w.min(building_h) * 0.28).clamp(6.0, 20.0);
        draw_circle(pos.0.x, pos.0.y, base_r, Color::new(0.18, 0.20, 0.22, 1.0));
        draw_circle_lines(pos.0.x, pos.0.y, base_r, 1.5, Color::new(0.40, 0.44, 0.50, 1.0));

        // Barrel: line from centre outward along turret angle
        let barrel_len = base_r + (building_w.min(building_h) * 0.30).clamp(8.0, 24.0);
        let barrel_w   = (base_r * 0.40).clamp(2.5, 6.0);
        let cos = turret.angle.cos();
        let sin = turret.angle.sin();
        let tip  = pos.0 + vec2(cos, sin) * barrel_len;
        let base = pos.0 + vec2(cos, sin) * base_r * 0.4;
        draw_line(base.x, base.y, tip.x, tip.y, barrel_w, Color::new(0.22, 0.25, 0.28, 1.0));
        // Muzzle cap
        draw_circle(tip.x, tip.y, barrel_w * 0.7, Color::new(0.30, 0.34, 0.38, 1.0));
    }
}

/// Animated construction beams from Engineers to their claimed Blueprint.
/// A pulsing cyan line gives clear visual feedback that building is in progress.
fn draw_engineer_beams(world: &hecs::World) {
    use crate::components::{IsBuilding, UnitKind};
    let t = get_time() as f32;

    for (_e, (pos, ib, kind)) in world.query::<(&Position, &IsBuilding, &UnitKind)>().iter() {
        if kind.id != "engineer" { continue; }

        // Get blueprint position
        let bp_pos = match world.get::<&Position>(ib.blueprint) {
            Ok(p) => p.0,
            Err(_) => continue,
        };

        let dist = pos.0.distance(bp_pos);
        // Only show beam when engineer is within build range
        if dist > crate::construction::ARRIVE_RANGE { continue; }

        // Pulse: alpha oscillates between 0.4 and 1.0
        let pulse = ((t * 4.0).sin() * 0.5 + 0.5) * 0.6 + 0.4;
        let beam  = Color::new(0.35, 1.0, 0.80, pulse);
        let glow  = Color::new(0.35, 1.0, 0.80, pulse * 0.25);

        // Outer glow (wide, faint)
        draw_line(pos.0.x, pos.0.y, bp_pos.x, bp_pos.y, 5.0, glow);
        // Core beam (narrow, bright)
        draw_line(pos.0.x, pos.0.y, bp_pos.x, bp_pos.y, 1.5, beam);

        // Spark at the blueprint end — small circle that pulses
        let spark_r = 3.0 + ((t * 6.0).sin() * 1.5).abs();
        draw_circle(bp_pos.x, bp_pos.y, spark_r, Color::new(0.5, 1.0, 0.9, pulse));
    }
}

/// Draw active supply routes as coloured lines in world space, plus route-drawing
/// mode overlay. Called after present() so it runs on the world camera.
///
/// Route colours by resource:
///   Ammo             → yellow-orange
///   Fuel             → cyan-blue
///   BuildingSupplies → tan
///   WeaponParts      → purple
pub fn draw_route_overlay(
    world: &hecs::World,
    routes: &crate::supply_route::RouteRegistry,
    route_origin: Option<Option<hecs::Entity>>,
    camera: &GameCamera,
    sw: f32,
    sh: f32,
) {
    use crate::components::Position;
    use crate::depot::ResourceType;

    // Switch to world camera for line drawing.
    let view = camera.view_rect(sw, sh);
    let world_cam = Camera2D::from_display_rect(view);
    set_camera(&world_cam);

    fn resource_color(r: ResourceType) -> Color {
        match r {
            ResourceType::Ammo             => Color::new(1.00, 0.80, 0.20, 0.85),
            ResourceType::Fuel             => Color::new(0.30, 0.80, 1.00, 0.85),
            ResourceType::BuildingSupplies => Color::new(0.80, 0.65, 0.35, 0.85),
            ResourceType::WeaponParts      => Color::new(0.75, 0.40, 1.00, 0.85),
        }
    }

    // Draw each route as a line with an arrowhead at the midpoint.
    for route in routes.all() {
        let origin_pos = match world.get::<&Position>(route.origin) {
            Ok(p) => p.0, Err(_) => continue,
        };
        let dest_pos = match world.get::<&Position>(route.destination) {
            Ok(p) => p.0, Err(_) => continue,
        };
        let col = resource_color(route.resource);

        // Main line
        draw_line(origin_pos.x, origin_pos.y, dest_pos.x, dest_pos.y, 2.5, col);

        // Arrowhead at 60% along the line
        let mid = origin_pos + (dest_pos - origin_pos) * 0.6;
        let dir = (dest_pos - origin_pos).normalize_or_zero();
        let perp = vec2(-dir.y, dir.x);
        let arrow_len = 18.0;
        let arrow_w = 8.0;
        draw_triangle(
            mid + dir * arrow_len,
            mid - dir * (arrow_len * 0.3) + perp * arrow_w,
            mid - dir * (arrow_len * 0.3) - perp * arrow_w,
            col,
        );

        // Small dot at origin and destination
        draw_circle(origin_pos.x, origin_pos.y, 5.0, col);
        draw_circle_lines(dest_pos.x, dest_pos.y, 7.0, 2.0, col);
    }

    // Route-drawing mode: highlight depots + show pending origin.
    if let Some(origin_opt) = route_origin {
        let pending   = Color::new(1.0, 0.9, 0.2, 0.9);

        // Pulse all player depots to show they're clickable.
        let t = get_time() as f32;
        let pulse = ((t * 3.0).sin() * 0.3 + 0.7) as f32;
        for (_e, pos) in world.query::<(&Position, &crate::depot::Depot)>().iter().map(|(_, (p, _))| ((), p)) {
            draw_circle_lines(pos.0.x, pos.0.y, 40.0 + pulse * 8.0, 2.0,
                Color::new(0.45, 1.0, 0.55, pulse * 0.6));
        }

        // Highlight selected origin in bright yellow.
        if let Some(origin_e) = origin_opt {
            if let Ok(p) = world.get::<&Position>(origin_e) {
                draw_circle_lines(p.0.x, p.0.y, 50.0, 3.0, pending);
                draw_circle(p.0.x, p.0.y, 8.0, pending);
            }
        }
    }

    // Back to screen camera.
    set_default_camera();
}
