//! Pannable / zoomable game camera over the world, clamped to map bounds.

use macroquad::prelude::*;

pub struct GameCamera {
    /// Center of view, in world pixels.
    pub center: Vec2,
    /// Zoom factor (screen pixels per world pixel). >1 zooms in.
    pub scale: f32,
}

impl GameCamera {
    pub fn centered(map_px: Vec2) -> Self {
        Self { center: map_px * 0.5, scale: 1.0 }
    }

    /// Visible world rectangle for the current screen size.
    pub fn view_rect(&self, sw: f32, sh: f32) -> Rect {
        let w = sw / self.scale;
        let h = sh / self.scale;
        Rect::new(self.center.x - w * 0.5, self.center.y - h * 0.5, w, h)
    }

    /// Handle pan (WASD/arrows) + zoom (mouse wheel); clamp to the map.
    pub fn update(&mut self, map_px: Vec2) {
        let dt = get_frame_time();
        let pan = 700.0 / self.scale * dt;

        // Y convention: the world camera (Camera2D::from_display_rect) flips Y, so
        // "screen up" = larger world Y. Pan must match that (same convention the mouse
        // uses via Camera2D::screen_to_world). W/up = +Y, S/down = -Y. Never hand-assume
        // screen-down == +Y here; derive screen-relative directions from this convention.
        if is_key_down(KeyCode::W) || is_key_down(KeyCode::Up) {
            self.center.y += pan; // up
        }
        if is_key_down(KeyCode::S) || is_key_down(KeyCode::Down) {
            self.center.y -= pan; // down
        }
        if is_key_down(KeyCode::A) || is_key_down(KeyCode::Left) {
            self.center.x -= pan; // left (X is not flipped)
        }
        if is_key_down(KeyCode::D) || is_key_down(KeyCode::Right) {
            self.center.x += pan; // right
        }

        let (_, wheel_y) = mouse_wheel();
        if wheel_y != 0.0 {
            let factor = if wheel_y > 0.0 { 1.1 } else { 1.0 / 1.1 };
            self.scale = (self.scale * factor).clamp(0.25, 5.0);
        }

        self.center.x = self.center.x.clamp(0.0, map_px.x);
        self.center.y = self.center.y.clamp(0.0, map_px.y);
    }
}
