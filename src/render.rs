//! Rendering layer (macroquad): view-culled tilemap, sprite entities, selection
//! rings, and a screen-space overlay, drawn through the game camera. `present`
//! optionally targets an offscreen render target for autonomous screenshot capture.

use macroquad::prelude::*;

use crate::assets::Sprites;
use crate::camera::GameCamera;
use crate::components::{Heading, Position, Renderable, Selected};
use crate::map::{self, TileMap};
use crate::sim::Sim;

const BG: Color = Color::new(0.07, 0.085, 0.11, 1.0);

/// Render one frame. `target = None` draws to the screen; `Some(rt)` draws offscreen.
/// `drag` is an optional screen-space selection box (start, end).
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

    // World pass.
    let mut world_cam = Camera2D::from_display_rect(view);
    world_cam.render_target = target.clone();
    set_camera(&world_cam);
    clear_background(BG);
    draw_tiles(map, view, sprites);
    draw_selection_rings(world, sprites);
    draw_entities(world, sprites);

    // Overlay pass (screen-space).
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
            let tex = sprites.tile(map.get(tx, ty));
            draw_texture_ex(
                tex,
                tx as f32 * ts,
                ty as f32 * ts,
                WHITE,
                DrawTextureParams { dest_size: Some(vec2(ts, ts)), ..Default::default() },
            );
        }
    }
}

fn draw_selection_rings(world: &hecs::World, sprites: &Sprites) {
    let tint = Color::new(0.45, 1.0, 0.55, 0.9);
    for (_e, (pos, r, _)) in world.query::<(&Position, &Renderable, &Selected)>().iter() {
        let s = r.size * 1.9;
        draw_texture_ex(
            &sprites.selection,
            pos.0.x - s * 0.5,
            pos.0.y - s * 0.5,
            tint,
            DrawTextureParams { dest_size: Some(vec2(s, s)), ..Default::default() },
        );
    }
}

fn draw_entities(world: &hecs::World, sprites: &Sprites) {
    for (_e, (pos, r, head)) in world.query::<(&Position, &Renderable, &Heading)>().iter() {
        let tex = sprites.unit_texture(r.sprite);
        draw_texture_ex(
            tex,
            pos.0.x - r.size * 0.5,
            pos.0.y - r.size * 0.5,
            r.tint,
            DrawTextureParams {
                dest_size: Some(vec2(r.size, r.size)),
                // Sprites are authored pointing "up"; rotate to face travel direction.
                rotation: head.0 + std::f32::consts::FRAC_PI_2,
                ..Default::default()
            },
        );
    }
}

fn draw_overlay(world: &hecs::World, map: &TileMap, camera: &GameCamera, sim: &Sim, tick_ms: f32, sh: f32) {
    let selected = world.query::<&crate::components::Selected>().iter().count();
    let moving = world.query::<&crate::components::MoveOrder>().iter().count();
    draw_text(
        "Drag-select units, right-click to move  (WASD/arrows pan, mouse wheel zoom)",
        16.0,
        28.0,
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
    let _ = sim;
    draw_text(&info, 16.0, sh - 16.0, 22.0, Color::new(0.80, 0.80, 0.80, 1.0));
}
