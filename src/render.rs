//! Rendering layer (macroquad). Phase 1: tilemap (view-culled) + entities + overlay,
//! rendered through the game camera. `present` optionally targets an offscreen
//! render target for autonomous screenshot capture.

use macroquad::prelude::*;

use crate::camera::GameCamera;
use crate::components::{Position, Renderable};
use crate::map::{self, TileMap};
use crate::sim::Sim;

const BG: Color = Color::new(0.07, 0.085, 0.11, 1.0);

/// Render one frame. `target = None` draws to the screen; `Some(rt)` draws into an
/// offscreen render target (used by capture mode).
pub fn present(
    world: &hecs::World,
    map: &TileMap,
    camera: &GameCamera,
    sim: &Sim,
    target: Option<RenderTarget>,
) {
    let sw = screen_width();
    let sh = screen_height();
    let view = camera.view_rect(sw, sh);

    // World pass (world-space camera).
    let mut world_cam = Camera2D::from_display_rect(view);
    world_cam.render_target = target.clone();
    set_camera(&world_cam);
    clear_background(BG);
    draw_tiles(map, view);
    draw_entities(world);

    // Overlay pass (screen-space).
    match &target {
        None => set_default_camera(),
        Some(_) => {
            let mut ui = Camera2D::from_display_rect(Rect::new(0.0, 0.0, sw, sh));
            ui.render_target = target.clone();
            set_camera(&ui);
        }
    }
    draw_overlay(world, map, camera, sim, sh);
    set_default_camera();
}

fn draw_tiles(map: &TileMap, view: Rect) {
    let ts = map::TILE_SIZE;
    // Cull to the visible tile range so 256x256 stays cheap.
    let min_tx = ((view.x / ts).floor() as i32).max(0) as usize;
    let min_ty = ((view.y / ts).floor() as i32).max(0) as usize;
    let max_tx = (((view.x + view.w) / ts).ceil() as i32).clamp(0, map.width as i32) as usize;
    let max_ty = (((view.y + view.h) / ts).ceil() as i32).clamp(0, map.height as i32) as usize;

    for ty in min_ty..max_ty {
        for tx in min_tx..max_tx {
            let c = map::tile_color(map.get(tx, ty));
            draw_rectangle(tx as f32 * ts, ty as f32 * ts, ts, ts, c);
        }
    }
}

fn draw_entities(world: &hecs::World) {
    let outline = Color::new(0.0, 0.0, 0.0, 0.5);
    for (_e, (pos, r)) in world.query::<(&Position, &Renderable)>().iter() {
        draw_circle(pos.0.x, pos.0.y, r.radius, r.color);
        draw_circle_lines(pos.0.x, pos.0.y, r.radius, 1.5, outline);
    }
}

fn draw_overlay(world: &hecs::World, map: &TileMap, camera: &GameCamera, sim: &Sim, sh: f32) {
    draw_text(
        "Phase 1 - tilemap + camera + ECS  (WASD/arrows pan, mouse wheel zoom)",
        16.0,
        28.0,
        24.0,
        WHITE,
    );
    let info = format!(
        "fps {} | tick {} | cam ({:.0},{:.0}) zoom {:.2} | entities {} | map {}x{}",
        get_fps(),
        sim.tick_count,
        camera.center.x,
        camera.center.y,
        camera.scale,
        world.len(),
        map.width,
        map.height,
    );
    draw_text(&info, 16.0, sh - 16.0, 22.0, Color::new(0.78, 0.78, 0.78, 1.0));
}
