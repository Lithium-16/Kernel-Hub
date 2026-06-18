//! Cold War RTS (working title) — entry point.
//!
//! Phase 2: tilemap, camera, ECS, placeholder sprites, plus the movement stack —
//! spatial grid, flow-field pathfinding, local avoidance, selection & move orders.

// Some scaffolding is intentionally unused while systems are wired up phase by phase.
#![allow(dead_code, unused_imports)]

use hecs::Entity;
use macroquad::prelude::*;

mod assets;
mod camera;
mod components;
mod data;
mod ecs;
mod map;
mod movement;
mod nav;
mod render;
mod sim;
mod spatial;

use assets::Sprites;
use components::{Faction, Moving, Position, Renderable, Selected};
use data::Definitions;
use map::TileMap;
use nav::{FlowField, NavGrid};
use spatial::SpatialGrid;

fn window_conf() -> Conf {
    Conf {
        window_title: "Cold War RTS (working title) - Phase 2".to_owned(),
        window_width: 1280,
        window_height: 720,
        high_dpi: false,
        ..Default::default()
    }
}

/// Spawn `count` units in a loose block near the map center, cycling unit types.
fn spawn_army(world: &mut hecs::World, defs: &Definitions, sprites: &Sprites, map: &TileMap, count: usize) {
    let center = map.size_px() * 0.5;
    let cols = (count as f32).sqrt().ceil() as usize;
    for i in 0..count {
        let unit = &defs.units[i % defs.units.len()];
        let tint = Color::from_rgba(unit.color.0, unit.color.1, unit.color.2, 255);
        let sprite = sprites.unit_index(&unit.sprite);
        let gx = (i % cols) as f32 - cols as f32 * 0.5;
        let gy = (i / cols) as f32 - cols as f32 * 0.5;
        let pos = center + vec2(gx * 16.0, gy * 16.0);
        world.spawn((
            Position(pos),
            Renderable { sprite, tint, size: unit.radius * 2.6 },
            Faction(unit.faction.clone()),
        ));
    }
}

fn clear_selection(world: &mut hecs::World) {
    let sel: Vec<Entity> = world.query::<&Selected>().iter().map(|(e, _)| e).collect();
    for e in sel {
        let _ = world.remove_one::<Selected>(e);
    }
}

#[macroquad::main(window_conf)]
async fn main() {
    let capture_path = std::env::var("COLDWAR_CAPTURE").ok();
    let mut frame: u32 = 0;

    let sprites = Sprites::load();
    let defs = data::load_definitions();
    let map = TileMap::generate_test(256, 256);
    let map_px = map.size_px();
    let nav = NavGrid::from_map(&map);

    let mut world = ecs::new_world();
    let count: usize = std::env::var("COLDWAR_UNITS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(200);
    spawn_army(&mut world, &defs, &sprites, &map, count);

    let mut cam = camera::GameCamera::centered(map_px);
    if let Ok(z) = std::env::var("COLDWAR_ZOOM") {
        if let Ok(s) = z.parse::<f32>() {
            cam.scale = s;
        }
    }

    let mut grid = SpatialGrid::new(map_px, 24.0);
    let mut active_flow: Option<FlowField> = None;
    let mut goal = Vec2::ZERO;
    let mut drag_start: Option<Vec2> = None;

    let mut sim = sim::Sim::new();
    let tick_dt = 1.0 / sim::TICK_RATE as f32;
    let mut accumulator = 0.0f32;

    // Capture mode: auto-select everything and issue a move so the screenshot shows
    // flow-field movement + avoidance, then grab a frame and exit.
    if capture_path.is_some() {
        let all: Vec<Entity> = world.query::<&Position>().iter().map(|(e, _)| e).collect();
        for e in all {
            let _ = world.insert_one(e, Selected);
            let _ = world.insert_one(e, Moving);
        }
        goal = map_px * 0.5 + vec2(-700.0, -700.0);
        let (tx, ty) = ((goal.x / map::TILE_SIZE) as usize, (goal.y / map::TILE_SIZE) as usize);
        active_flow = Some(FlowField::to_goal(&nav, (tx, ty)));
    }

    loop {
        let (mx, my) = mouse_position();
        let mp = vec2(mx, my);
        let sw = screen_width();
        let sh = screen_height();

        cam.update(map_px);
        let view = cam.view_rect(sw, sh);
        let cam2d = Camera2D::from_display_rect(view);

        // --- Selection (left mouse) ---
        if is_mouse_button_pressed(MouseButton::Left) {
            drag_start = Some(mp);
        }
        if is_mouse_button_released(MouseButton::Left) {
            if let Some(start) = drag_start.take() {
                clear_selection(&mut world);
                let a = cam2d.screen_to_world(start);
                let b = cam2d.screen_to_world(mp);
                let (minx, maxx) = (a.x.min(b.x), a.x.max(b.x));
                let (miny, maxy) = (a.y.min(b.y), a.y.max(b.y));
                let mut to_sel: Vec<Entity> = Vec::new();
                if (maxx - minx) * (maxy - miny) < 64.0 {
                    // Click: select the unit whose sprite is actually under the cursor
                    // (nearest center among those hit), so overlapping units don't mis-pick.
                    let click = b;
                    let mut best = None;
                    let mut bestd = f32::MAX;
                    for (e, (pos, r)) in world.query::<(&Position, &Renderable)>().iter() {
                        let d = pos.0.distance(click);
                        if d <= r.size * 0.5 && d < bestd {
                            bestd = d;
                            best = Some(e);
                        }
                    }
                    if let Some(e) = best {
                        to_sel.push(e);
                    }
                } else {
                    for (e, pos) in world.query::<&Position>().iter() {
                        if pos.0.x >= minx && pos.0.x <= maxx && pos.0.y >= miny && pos.0.y <= maxy {
                            to_sel.push(e);
                        }
                    }
                }
                for e in to_sel {
                    let _ = world.insert_one(e, Selected);
                }
            }
        }

        // --- Move order (right mouse) ---
        if is_mouse_button_pressed(MouseButton::Right) {
            let w = cam2d.screen_to_world(mp);
            let (tx, ty) = ((w.x / map::TILE_SIZE) as i32, (w.y / map::TILE_SIZE) as i32);
            if tx >= 0
                && ty >= 0
                && (tx as usize) < nav.w
                && (ty as usize) < nav.h
                && nav.passable(tx as usize, ty as usize)
            {
                active_flow = Some(FlowField::to_goal(&nav, (tx as usize, ty as usize)));
                goal = w;
                let sel: Vec<Entity> = world.query::<&Selected>().iter().map(|(e, _)| e).collect();
                for e in sel {
                    let _ = world.insert_one(e, Moving);
                }
            }
        }

        // --- Fixed-timestep simulation ---
        let t0 = get_time();
        accumulator += get_frame_time();
        while accumulator >= tick_dt {
            grid.rebuild(&world);
            if let Some(flow) = &active_flow {
                movement::step(&mut world, &grid, flow, goal, map_px, tick_dt);
            }
            sim.tick();
            accumulator -= tick_dt;
        }
        let tick_ms = ((get_time() - t0) * 1000.0) as f32;

        let drag_box = drag_start.map(|s| (s, mp));
        render::present(&world, &map, &cam, &sim, &sprites, drag_box, tick_ms, None);

        next_frame().await;

        if let Some(path) = &capture_path {
            frame += 1;
            if frame >= 200 {
                let rt = render_target(screen_width() as u32, screen_height() as u32);
                render::present(&world, &map, &cam, &sim, &sprites, None, tick_ms, Some(rt.clone()));
                rt.texture.get_texture_data().export_png(path);
                break;
            }
        }
    }
}
