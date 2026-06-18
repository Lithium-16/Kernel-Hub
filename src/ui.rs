//! Immediate-mode UI toolkit (Phase 2.5). Screen-space widgets with **input layering**:
//! the UI marks the pointer "captured" when it's over a panel/button, so the world
//! skips clicks the UI handled. Everything later (command card, build menu, economy
//! bar, minimap) builds on this. Theme-driven so the whole look changes in one place.

use macroquad::prelude::*;

pub struct Theme {
    pub panel_bg: Color,
    pub panel_border: Color,
    pub button: Color,
    pub button_hover: Color,
    pub button_press: Color,
    pub text: Color,
    pub font_size: f32,
}

impl Default for Theme {
    fn default() -> Self {
        Self {
            panel_bg: Color::new(0.10, 0.12, 0.15, 0.92),
            panel_border: Color::new(0.30, 0.36, 0.42, 1.0),
            button: Color::new(0.18, 0.22, 0.28, 1.0),
            button_hover: Color::new(0.26, 0.32, 0.40, 1.0),
            button_press: Color::new(0.36, 0.46, 0.56, 1.0),
            text: Color::new(0.92, 0.94, 0.96, 1.0),
            font_size: 20.0,
        }
    }
}

pub struct Ui {
    pub theme: Theme,
    mouse: Vec2,
    down: bool,
    released: bool,
    captured: bool,
}

impl Default for Ui {
    fn default() -> Self {
        Self::new()
    }
}

impl Ui {
    pub fn new() -> Self {
        Self { theme: Theme::default(), mouse: Vec2::ZERO, down: false, released: false, captured: false }
    }

    /// Call once at the start of each frame, before any widgets.
    pub fn begin(&mut self) {
        let (mx, my) = mouse_position();
        self.mouse = vec2(mx, my);
        self.down = is_mouse_button_down(MouseButton::Left);
        self.released = is_mouse_button_released(MouseButton::Left);
        self.captured = false;
    }

    /// Did the UI consume the pointer this frame? World input should be skipped if so.
    pub fn captured(&self) -> bool {
        self.captured
    }

    fn over(&mut self, r: Rect) -> bool {
        let inside = r.contains(self.mouse);
        if inside {
            self.captured = true;
        }
        inside
    }

    pub fn panel(&mut self, r: Rect) {
        self.over(r);
        draw_rectangle(r.x, r.y, r.w, r.h, self.theme.panel_bg);
        draw_rectangle_lines(r.x, r.y, r.w, r.h, 2.0, self.theme.panel_border);
    }

    /// Draw a button; returns true on click (mouse released inside it).
    pub fn button(&mut self, r: Rect, label: &str) -> bool {
        let inside = self.over(r);
        let col = if inside && self.down {
            self.theme.button_press
        } else if inside {
            self.theme.button_hover
        } else {
            self.theme.button
        };
        draw_rectangle(r.x, r.y, r.w, r.h, col);
        draw_rectangle_lines(r.x, r.y, r.w, r.h, 1.5, self.theme.panel_border);
        let fs = self.theme.font_size;
        let d = measure_text(label, None, fs as u16, 1.0);
        draw_text(label, r.x + (r.w - d.width) * 0.5, r.y + (r.h + d.height) * 0.5, fs, self.theme.text);
        inside && self.released
    }

    pub fn label(&self, pos: Vec2, text: &str) {
        draw_text(text, pos.x, pos.y, self.theme.font_size, self.theme.text);
    }

    /// A filled bar (frac 0..1) — for resource/health readouts later.
    pub fn bar(&self, r: Rect, frac: f32, fill: Color) {
        draw_rectangle(r.x, r.y, r.w, r.h, self.theme.button);
        draw_rectangle(r.x, r.y, r.w * frac.clamp(0.0, 1.0), r.h, fill);
        draw_rectangle_lines(r.x, r.y, r.w, r.h, 1.5, self.theme.panel_border);
    }
}
