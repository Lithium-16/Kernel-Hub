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
    camera: &GameCamera,
    sim: &Sim,
    sprites: &Sprites,
    drag: Option<(Vec2, Vec2)>,
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
    draw_move_orders(world);
    draw_selection_rings(world, sprites);
    draw_entities(world, sprites);
    draw_hitboxes(world);

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
            draw_texture_ex(
                &sprites.atlas,
                tx as f32 * ts,
                ty as f32 * ts,
                WHITE,
                DrawTextureParams {
                    dest_size: Some(vec2(ts, ts)),
                    source: Some(src),
                    ..Default::default()
                },
            );
        }
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

fn draw_entities(world: &hecs::World, sprites: &Sprites) {
    for (_e, (pos, r, head)) in world.query::<(&Position, &Renderable, &Heading)>().iter() {
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

/// Move-order feedback: a faint line from each moving unit to its goal, and a
/// destination marker (ring + cross) at each distinct goal.
fn draw_move_orders(world: &hecs::World) {
    let line = Color::new(0.45, 1.0, 0.55, 0.18);
    let mark = Color::new(0.45, 1.0, 0.55, 0.95);
    let mut goals: Vec<Vec2> = Vec::new();
    for (_e, (pos, order)) in world.query::<(&Position, &MoveOrder)>().iter() {
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
        "Drag-select, right-click move, Ctrl+1-9 set group, 1-9 recall  (WASD pan, wheel zoom)",
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
