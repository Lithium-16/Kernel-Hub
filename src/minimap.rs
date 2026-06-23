//! Minimap (Phase 3 UI). Terrain is baked once into a texture at startup; each frame
//! we draw that texture plus live unit dots and the camera viewport rectangle. Clicking
//! (or dragging on) the minimap recenters the camera.
//!
//! Y convention: the world is rendered through a Y-flipping camera (world Y up), so the
//! minimap flips its texture and inverts the dot/viewport Y to match what's on screen.

use hecs::World;
use macroquad::prelude::*;

use crate::components::{Faction, Position, Selected};
use crate::fog::FogGrid;
use crate::map::{self, TileMap};

pub struct Minimap {
    tex: Texture2D,
    map_px: Vec2,
}

impl Minimap {
    /// Bake the terrain into a texture (one texel per tile).
    pub fn build(map: &TileMap) -> Self {
        let mut img = Image::gen_image_color(map.width as u16, map.height as u16, BLANK);
        for y in 0..map.height {
            for x in 0..map.width {
                img.set_pixel(x as u32, y as u32, map::tile_color(map.get(x, y)));
            }
        }
        let tex = Texture2D::from_image(&img);
        tex.set_filter(FilterMode::Nearest);
        Self { tex, map_px: map.size_px() }
    }

    /// World position under a screen point inside `panel` (inverse of the dot mapping).
    pub fn world_at(&self, panel: Rect, p: Vec2) -> Vec2 {
        let nx = ((p.x - panel.x) / panel.w).clamp(0.0, 1.0);
        let ny = ((p.y - panel.y) / panel.h).clamp(0.0, 1.0);
        vec2(nx * self.map_px.x, (1.0 - ny) * self.map_px.y)
    }

    fn to_panel(&self, panel: Rect, world: Vec2) -> Vec2 {
        let nx = world.x / self.map_px.x;
        let ny = world.y / self.map_px.y;
        vec2(panel.x + nx * panel.w, panel.y + (1.0 - ny) * panel.h)
    }

    /// Draw the minimap into `panel` (screen space; call with the default camera active).
    /// `view` is the camera's visible world rect, drawn as the viewport box.
    /// Enemy units are only shown when their tile is currently Visible through fog.
    pub fn draw(&self, world: &World, fog: &FogGrid, view: Rect, panel: Rect) {
        draw_rectangle(panel.x - 3.0, panel.y - 3.0, panel.w + 6.0, panel.h + 6.0, Color::new(0.06, 0.08, 0.10, 0.95));
        draw_texture_ex(
            &self.tex,
            panel.x,
            panel.y,
            WHITE,
            DrawTextureParams {
                dest_size: Some(vec2(panel.w, panel.h)),
                flip_y: true, // match the Y-up world camera
                ..Default::default()
            },
        );

        // Unit dots: blue for player, red for enemy (only when visible through fog).
        // Selected units get a bright green override.
        let player_dot = Color::new(0.55, 0.75, 1.0, 1.0);
        let enemy_dot  = Color::new(1.0, 0.45, 0.45, 1.0);
        let sel_dot    = Color::new(0.45, 1.0, 0.55, 1.0);
        for (e, (pos, fac)) in world.query::<(&Position, &Faction)>().iter() {
            // Skip non-unit entities (depots, buildings) — they have no Renderable.
            if world.get::<&crate::components::Renderable>(e).is_err() { continue; }
            let is_player = fac.0 == crate::PLAYER_FACTION;
            // Enemy dots hidden in fog.
            if !is_player && !fog.visible_world(pos.0) { continue; }
            let selected = world.get::<&Selected>(e).is_ok();
            let c = if selected { sel_dot } else if is_player { player_dot } else { enemy_dot };
            let s = if selected { 2.5 } else { 1.6 };
            let p = self.to_panel(panel, pos.0);
            draw_rectangle(p.x - s * 0.5, p.y - s * 0.5, s, s, c);
        }

        // Camera viewport rectangle.
        let a = self.to_panel(panel, vec2(view.x, view.y));
        let b = self.to_panel(panel, vec2(view.x + view.w, view.y + view.h));
        let (x0, x1) = (a.x.min(b.x), a.x.max(b.x));
        let (y0, y1) = (a.y.min(b.y), a.y.max(b.y));
        draw_rectangle_lines(
            x0.max(panel.x),
            y0.max(panel.y),
            (x1 - x0).min(panel.w),
            (y1 - y0).min(panel.h),
            1.5,
            Color::new(0.9, 0.9, 0.95, 0.9),
        );
        draw_rectangle_lines(panel.x, panel.y, panel.w, panel.h, 2.0, Color::new(0.30, 0.36, 0.42, 1.0));
    }
}
