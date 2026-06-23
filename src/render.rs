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
    draw_move_orders(world);
    draw_selection_rings(world, sprites);
    // Only draw entities visible through fog.
    draw_entities_fogged(world, fog, sprites, camera.scale);
    draw_turret_barrels_fogged(world, fog, camera.scale);
    draw_tracers(world);
    draw_health_bars_fogged(world, fog);
    draw_hitboxes(world);
    draw_range_circles(world);
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

fn draw_buildings(world: &hecs::World) {
    let ts = map::TILE_SIZE;
    let edge = Color::new(0.05, 0.06, 0.08, 1.0);
    for (_e, b) in world.query::<&crate::components::Building>().iter() {
        let (x, y) = (b.tx as f32 * ts, b.ty as f32 * ts);
        let (w, h) = (b.w as f32 * ts, b.h as f32 * ts);
        draw_rectangle(x, y, w, h, b.color);
        draw_rectangle_lines(x, y, w, h, 2.0, edge);
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

fn draw_entities_fogged(world: &hecs::World, fog: &crate::fog::FogGrid, sprites: &Sprites, cam_scale: f32) {
    use crate::components::Faction;
    let overview = cam_scale < OVERVIEW_ZOOM;
    for (_e, (pos, r, head, fac)) in world.query::<(&Position, &Renderable, &Heading, &Faction)>().iter() {
        let vis = fac.0 == crate::PLAYER_FACTION || fog.visible_world(pos.0);
        if !vis { continue; }
        if overview {
            // Strategic overview: draw a screen-size-stable dot (radius = 3 screen px).
            let dot_r = 3.0 / cam_scale;
            draw_circle(pos.0.x, pos.0.y, dot_r, r.tint);
        } else {
            let src = sprites.unit_rect(r.sprite);
            draw_texture_ex(
                &sprites.atlas,
                pos.0.x - r.size * 0.5,
                pos.0.y - r.size * 0.5,
                r.tint,
                DrawTextureParams {
                    dest_size: Some(vec2(r.size, r.size)),
                    source: Some(src),
                    rotation: head.0 + std::f32::consts::FRAC_PI_2,
                    ..Default::default()
                },
            );
        }
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

fn draw_overlay(world: &hecs::World, map: &TileMap, camera: &GameCamera, sim: &Sim, tick_ms: f32, sh: f32) {
    let selected = world.query::<&Selected>().iter().count();
    let moving = world.query::<&crate::components::MoveOrder>().iter().count();
    let _ = sim;
    // Below the top resource bar so the two don't overlap.
    draw_text(
        "Drag-select, RMB move (Shift=queue), dbl-click=type, Ctrl+1-9 group, B=build, R=restart  (WASD pan)",
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
