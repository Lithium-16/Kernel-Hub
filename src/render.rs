//! Rendering layer (macroquad). Milestone 0: a placeholder scene + debug overlay.

use macroquad::prelude::*;

use crate::sim::Sim;

/// Draw the placeholder scene — an animated marker driven by sim time, which proves
/// the sim → render data flow works.
pub fn draw_scene(sim: &Sim) {
    let t = sim.elapsed_secs();
    let cx = screen_width() * 0.5 + t.sin() * 140.0;
    let cy = screen_height() * 0.5;

    draw_circle(cx, cy, 26.0, Color::from_rgba(120, 200, 160, 255));
    draw_text(
        "Cold War RTS (working title) - Milestone 0 scaffold",
        24.0,
        48.0,
        30.0,
        WHITE,
    );
}

/// Bottom-left debug readout.
pub fn draw_debug_overlay(sim: &Sim) {
    let text = format!("fps {}  |  sim tick {}", get_fps(), sim.tick_count);
    draw_text(
        &text,
        24.0,
        screen_height() - 24.0,
        22.0,
        Color::from_rgba(190, 190, 190, 255),
    );
}
