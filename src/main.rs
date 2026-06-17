//! Cold War RTS (working title) — entry point.
//!
//! Phase 1: fixed-timestep sim loop, a large tilemap, a pannable/zoomable camera,
//! an ECS world, and placeholder units spawned from data-driven definitions.

// Some scaffolding is intentionally unused while systems are wired up phase by phase.
#![allow(dead_code, unused_imports)]

use macroquad::prelude::*;

mod camera;
mod components;
mod data;
mod ecs;
mod map;
mod render;
mod sim;

use components::{Faction, Position, Renderable};
use data::Definitions;
use map::TileMap;

fn window_conf() -> Conf {
    Conf {
        window_title: "Cold War RTS (working title) - Phase 1".to_owned(),
        window_width: 1280,
        window_height: 720,
        high_dpi: true,
        ..Default::default()
    }
}

/// Spawn a few of each defined unit type near the map center (placeholder content).
fn spawn_initial_units(world: &mut hecs::World, defs: &Definitions, map: &TileMap) {
    let center = map.size_px() * 0.5;
    for (i, unit) in defs.units.iter().enumerate() {
        let color = Color::from_rgba(unit.color.0, unit.color.1, unit.color.2, 255);
        for n in 0..6 {
            let pos = center
                + vec2(i as f32 * 44.0 - defs.units.len() as f32 * 22.0, n as f32 * 40.0 - 120.0);
            world.spawn((
                Position(pos),
                Renderable { color, radius: unit.radius },
                Faction(unit.faction.clone()),
            ));
        }
    }
}

#[macroquad::main(window_conf)]
async fn main() {
    // Optional capture mode for autonomous verification (set COLDWAR_CAPTURE=path).
    let capture_path = std::env::var("COLDWAR_CAPTURE").ok();
    let mut frame: u32 = 0;

    let defs = data::load_definitions();
    let map = TileMap::generate_test(256, 256);
    let map_px = map.size_px();

    let mut world = ecs::new_world();
    spawn_initial_units(&mut world, &defs, &map);

    let mut cam = camera::GameCamera::centered(map_px);

    let mut sim = sim::Sim::new();
    let tick_dt = 1.0 / sim::TICK_RATE as f32;
    let mut accumulator = 0.0f32;

    loop {
        cam.update(map_px);

        // Fixed-timestep simulation, decoupled from the render framerate.
        accumulator += get_frame_time();
        while accumulator >= tick_dt {
            sim.tick();
            accumulator -= tick_dt;
        }

        render::present(&world, &map, &cam, &sim, None);

        next_frame().await;

        if let Some(path) = &capture_path {
            frame += 1;
            if frame >= 5 {
                let rt = render_target(screen_width() as u32, screen_height() as u32);
                render::present(&world, &map, &cam, &sim, Some(rt.clone()));
                rt.texture.get_texture_data().export_png(path);
                break;
            }
        }
    }
}
