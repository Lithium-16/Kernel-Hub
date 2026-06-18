//! Cold War RTS (working title) — entry point.
//!
//! Phase 2-3: tilemap, camera, ECS, placeholder sprites, flow-field movement with
//! avoidance + collision + facing, selection, and per-unit move orders.

// Some scaffolding is intentionally unused while systems are wired up phase by phase.
#![allow(dead_code, unused_imports)]

use std::sync::Arc;

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
mod ui;

use assets::Sprites;
use components::{Faction, Heading, MoveOrder, Position, Renderable, Selected, Velocity};
use data::Definitions;
use map::TileMap;
use nav::{FlowField, NavGrid};
use spatial::SpatialGrid;

fn window_conf() -> Conf {
    // COLDWAR_VSYNC=0 disables vsync (uncaps fps past the monitor refresh).
    let swap_interval = if std::env::var("COLDWAR_VSYNC").as_deref() == Ok("0") {
        Some(0)
    } else {
        Some(1)
    };
    Conf {
        window_title: "Cold War RTS (working title) - Phase 3".to_owned(),
        window_width: 1280,
        window_height: 720,
        high_dpi: false,
        platform: miniquad::conf::Platform { swap_interval, ..Default::default() },
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
        let pos = center + vec2(gx * 30.0, gy * 30.0);
        world.spawn((
            Position(pos),
            Velocity(Vec2::ZERO),
            Heading(-std::f32::consts::FRAC_PI_2),
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

/// Build one shared flow field for `goal` and assign it to `units` as a move order.
/// Only the listed units are affected — other units keep their existing orders.
fn issue_move(world: &mut hecs::World, nav: &NavGrid, units: &[Entity], goal: Vec2) {
    let (tx, ty) = ((goal.x / map::TILE_SIZE) as i32, (goal.y / map::TILE_SIZE) as i32);
    if units.is_empty()
        || tx < 0
        || ty < 0
        || tx as usize >= nav.w
        || ty as usize >= nav.h
        || !nav.passable(tx as usize, ty as usize)
    {
        return;
    }
    let flow = Arc::new(FlowField::to_goal(nav, (tx as usize, ty as usize)));
    let arrive = (movement::UNIT_RADIUS * (units.len() as f32).sqrt() * 1.5).max(24.0);
    for &e in units {
        let _ = world.insert_one(e, MoveOrder { flow: flow.clone(), goal, arrive });
    }
}

/// Bottom HUD bar actions.
enum HudAction {
    None,
    Stop,
    ClearSel,
}

/// Draw the bottom HUD bar (immediate-mode UI) and return any button action.
fn draw_hud(ui: &mut ui::Ui, world: &hecs::World, sw: f32, sh: f32) -> HudAction {
    ui.panel(Rect::new(0.0, sh - 56.0, sw, 56.0));
    let mut action = HudAction::None;
    if ui.button(Rect::new(10.0, sh - 48.0, 96.0, 40.0), "Stop") {
        action = HudAction::Stop;
    }
    if ui.button(Rect::new(114.0, sh - 48.0, 96.0, 40.0), "Clear") {
        action = HudAction::ClearSel;
    }
    let n = world.query::<&Selected>().iter().count();
    ui.label(vec2(228.0, sh - 22.0), &format!("Selected: {n}"));
    action
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
    let mut drag_start: Option<Vec2> = None;
    let mut ui = ui::Ui::new();

    let mut sim = sim::Sim::new();
    let tick_dt = 1.0 / sim::TICK_RATE as f32;
    let mut accumulator = 0.0f32;

    // Capture mode: select all and order a move so the screenshot shows movement.
    if capture_path.is_some() {
        let all: Vec<Entity> = world.query::<&Position>().iter().map(|(e, _)| e).collect();
        for &e in &all {
            let _ = world.insert_one(e, Selected);
        }
        let goal = std::env::var("COLDWAR_GOAL")
            .ok()
            .and_then(|s| {
                let mut it = s.split(',');
                let x = it.next()?.trim().parse::<f32>().ok()?;
                let y = it.next()?.trim().parse::<f32>().ok()?;
                Some(vec2(x * map::TILE_SIZE, y * map::TILE_SIZE))
            })
            .unwrap_or(map_px * 0.5 + vec2(-700.0, -700.0));
        issue_move(&mut world, &nav, &all, goal);
    }
    let capture_frames: u32 = std::env::var("COLDWAR_FRAMES")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(200);

    // Headless sim benchmark (native): COLDWAR_BENCH=<ticks>. Prints ms/sim-tick.
    if let Ok(b) = std::env::var("COLDWAR_BENCH") {
        let ticks: u32 = b.parse().unwrap_or(300);
        let all: Vec<Entity> = world.query::<&Position>().iter().map(|(e, _)| e).collect();
        issue_move(&mut world, &nav, &all, map_px * 0.5 + vec2(-2000.0, -2000.0));
        let start = std::time::Instant::now();
        for _ in 0..ticks {
            grid.rebuild(&world);
            movement::step(&mut world, &grid, &nav, map_px, tick_dt);
            grid.rebuild(&world);
            movement::resolve_collisions(&mut world, &grid, &nav, map_px, 2);
            movement::settle_arrivals(&mut world);
        }
        let per = start.elapsed().as_secs_f64() * 1000.0 / ticks as f64;
        println!("BENCH {count} units: {per:.3} ms/sim-tick avg over {ticks} ticks");
        std::process::exit(0);
    }

    loop {
        let (mx, my) = mouse_position();
        let mp = vec2(mx, my);
        let sw = screen_width();
        let sh = screen_height();
        ui.begin();
        let over_ui = mp.y >= sh - 56.0; // bottom HUD bar region

        cam.update(map_px);
        let view = cam.view_rect(sw, sh);
        let cam2d = Camera2D::from_display_rect(view);

        // --- Selection (left mouse) ---
        if is_mouse_button_pressed(MouseButton::Left) && !over_ui {
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
                    // Click: select the unit whose sprite is under the cursor.
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

        // --- Move order (right mouse): only the currently-selected units ---
        if is_mouse_button_pressed(MouseButton::Right) && !over_ui {
            let goal = cam2d.screen_to_world(mp);
            let sel: Vec<Entity> = world.query::<&Selected>().iter().map(|(e, _)| e).collect();
            issue_move(&mut world, &nav, &sel, goal);
        }

        // --- Fixed-timestep simulation ---
        let t0 = get_time();
        accumulator += get_frame_time();
        while accumulator >= tick_dt {
            grid.rebuild(&world);
            movement::step(&mut world, &grid, &nav, map_px, tick_dt);
            grid.rebuild(&world);
            movement::resolve_collisions(&mut world, &grid, &nav, map_px, 2);
            movement::settle_arrivals(&mut world);
            sim.tick();
            accumulator -= tick_dt;
        }
        let tick_ms = ((get_time() - t0) * 1000.0) as f32;

        let drag_box = drag_start.map(|s| (s, mp));
        render::present(&world, &map, &cam, &sim, &sprites, drag_box, tick_ms, None);
        match draw_hud(&mut ui, &world, sw, sh) {
            HudAction::Stop => {
                let sel: Vec<Entity> = world.query::<&Selected>().iter().map(|(e, _)| e).collect();
                for e in sel {
                    let _ = world.remove_one::<MoveOrder>(e);
                }
            }
            HudAction::ClearSel => clear_selection(&mut world),
            HudAction::None => {}
        }

        next_frame().await;

        if let Some(path) = &capture_path {
            frame += 1;
            if frame >= capture_frames {
                let rt = render_target(screen_width() as u32, screen_height() as u32);
                render::present(&world, &map, &cam, &sim, &sprites, None, tick_ms, Some(rt.clone()));
                let mut uicam = Camera2D::from_display_rect(Rect::new(0.0, 0.0, sw, sh));
                uicam.render_target = Some(rt.clone());
                set_camera(&uicam);
                let _ = draw_hud(&mut ui, &world, sw, sh);
                set_default_camera();
                rt.texture.get_texture_data().export_png(path);
                break;
            }
        }
    }
}
