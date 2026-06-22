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
mod economy;
mod groups;
mod hud;
mod map;
mod movement;
mod nav;
mod render;
mod selection;
mod sim;
mod spatial;
mod ui;

use assets::Sprites;
use components::{Faction, Heading, Mobility, MoveOrder, MoveState, Position, Renderable, Selected, Velocity};
use data::Definitions;
use map::TileMap;
use nav::{FlowCache, FlowField, NavGrid};
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
            MoveState { last: pos, stall: 0 },
            Mobility { speed: unit.speed, turn_rate: unit.turn_rate },
            Renderable { sprite, tint, size: unit.radius * 2.6 },
            Faction(unit.faction.clone()),
            components::UnitKind { id: unit.id.clone(), name: unit.name.clone() },
        ));
    }
}

fn clear_selection(world: &mut hecs::World) {
    let sel: Vec<Entity> = world.query::<&Selected>().iter().map(|(e, _)| e).collect();
    for e in sel {
        let _ = world.remove_one::<Selected>(e);
    }
}

/// Issue a group move to `click`. One shared flow field routes everyone to the area,
/// but each unit is assigned its OWN destination slot in a packed formation around the
/// click — so the group fans into a block instead of all crushing the same point (the
/// real cause of packed-group jitter). Each unit then seeks its slot (see movement::step).
/// Only the listed units are affected — others keep their existing orders.
fn issue_move(world: &mut hecs::World, nav: &NavGrid, cache: &mut FlowCache, units: &[Entity], click: Vec2) {
    let (tx, ty) = ((click.x / map::TILE_SIZE) as i32, (click.y / map::TILE_SIZE) as i32);
    if units.is_empty()
        || tx < 0
        || ty < 0
        || tx as usize >= nav.w
        || ty as usize >= nav.h
        || !nav.passable(tx as usize, ty as usize)
    {
        return;
    }
    let flow = cache.get_or_build(nav, (tx as usize, ty as usize));

    // Packed formation slots centered on the click (spacing > collision diameter so no
    // two slots fight for the same space).
    let n = units.len();
    let cols = (n as f32).sqrt().ceil().max(1.0) as i32;
    let rows = ((n as i32 + cols - 1) / cols).max(1);
    let spacing = movement::UNIT_RADIUS * 2.2;
    let mut slots: Vec<Vec2> = Vec::with_capacity(n);
    for i in 0..n as i32 {
        let cx = (i % cols) as f32 - (cols - 1) as f32 * 0.5;
        let cy = (i / cols) as f32 - (rows - 1) as f32 * 0.5;
        slots.push(click + vec2(cx * spacing, cy * spacing));
    }
    // Units flip from flow-following to slot-seeking once within `seek` of the anchor.
    // Size it to the formation's half-diagonal (+ margin) so even the outermost slot is
    // reachable — otherwise far units pile at the anchor and never reach their slot.
    let half = vec2(cols as f32, rows as f32) * spacing * 0.5;
    let seek = half.length() + spacing * 2.0;

    // Greedy nearest-slot assignment (each unit takes its closest free slot — keeps the
    // formation from criss-crossing). Falls back to the click point if slots run out.
    let mut positions: Vec<(Entity, Vec2)> = Vec::with_capacity(n);
    for &e in units {
        if let Ok(p) = world.query_one_mut::<&Position>(e) {
            positions.push((e, p.0));
        }
    }
    // Stall-window radius: a unit that gets within this of its slot but then stops making
    // progress (blocked by the packed crowd) counts as arrived. Sized to ~2 slot pitches
    // so units whose exact slot is occupied still settle cleanly instead of nudging forever.
    let arrive = spacing * 2.0;
    let mut taken = vec![false; slots.len()];
    for (e, p) in positions {
        let mut best: Option<usize> = None;
        let mut best_d = f32::MAX;
        for (si, s) in slots.iter().enumerate() {
            if !taken[si] {
                let d = p.distance(*s);
                if d < best_d {
                    best_d = d;
                    best = Some(si);
                }
            }
        }
        let goal = best.map(|si| {
            taken[si] = true;
            slots[si]
        }).unwrap_or(click);
        let _ = world.insert_one(e, MoveOrder { flow: flow.clone(), goal, anchor: click, seek, arrive });
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
    let mut flow_cache = FlowCache::new(48);

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
    let mut last_click: (f64, Option<Entity>) = (0.0, None); // (time, entity) for double-click
    let mut ui = ui::Ui::new();
    let economy = economy::Economy::default();
    let mut control_groups = groups::ControlGroups::new();

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
        issue_move(&mut world, &nav, &mut flow_cache, &all, goal);
    }
    let capture_frames: u32 = std::env::var("COLDWAR_FRAMES")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(200);

    // Headless sim benchmark (native): COLDWAR_BENCH=<ticks>. Prints ms/sim-tick.
    if let Ok(b) = std::env::var("COLDWAR_BENCH") {
        let ticks: u32 = b.parse().unwrap_or(300);
        let all: Vec<Entity> = world.query::<&Position>().iter().map(|(e, _)| e).collect();
        issue_move(&mut world, &nav, &mut flow_cache, &all, map_px * 0.5 + vec2(-2000.0, -2000.0));
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

    // Headless jitter diagnostic (native): COLDWAR_SETTLE=<ticks>. Orders all units to a
    // nearby point, runs the sim, and over the final 40 ticks reports how many units are
    // still moving and the mean per-tick displacement — a direct measure of resting jitter.
    if let Ok(s) = std::env::var("COLDWAR_SETTLE") {
        let ticks: u32 = s.parse().unwrap_or(900);
        let all: Vec<Entity> = world.query::<&Position>().iter().map(|(e, _)| e).collect();
        issue_move(&mut world, &nav, &mut flow_cache, &all, map_px * 0.5 + vec2(140.0, 140.0));
        let tail = 40u32;
        let mut prev: std::collections::HashMap<Entity, Vec2> = std::collections::HashMap::new();
        let mut samples = 0u32;
        let mut total_disp = 0.0f64;
        let mut max_disp = 0.0f32;
        for t in 0..ticks {
            grid.rebuild(&world);
            movement::step(&mut world, &grid, &nav, map_px, tick_dt);
            grid.rebuild(&world);
            movement::resolve_collisions(&mut world, &grid, &nav, map_px, 2);
            movement::settle_arrivals(&mut world);
            if t >= ticks - tail {
                for (e, pos) in world.query::<&Position>().iter() {
                    if let Some(p) = prev.get(&e) {
                        let d = pos.0.distance(*p);
                        total_disp += d as f64;
                        max_disp = max_disp.max(d);
                        samples += 1;
                    }
                    prev.insert(e, pos.0);
                }
            }
        }
        let moving = world.query::<&MoveOrder>().iter().count();
        let mean = if samples > 0 { total_disp / samples as f64 } else { 0.0 };
        println!(
            "SETTLE {count} units after {ticks} ticks: still_moving={moving}  mean_disp={mean:.4} px/tick  max_disp={max_disp:.3} px/tick (last {tail} ticks)"
        );
        std::process::exit(0);
    }

    loop {
        let (mx, my) = mouse_position();
        let mp = vec2(mx, my);
        let sw = screen_width();
        let sh = screen_height();
        ui.begin();
        // Input layering: compute HUD panel rects up front (anchored to window size) and
        // gate world input on them, so clicks on any panel never fall through to the world.
        let over_ui = hud::HudLayout::compute(&world, sw, sh).contains(mp);

        cam.update(map_px);
        let view = cam.view_rect(sw, sh);
        let cam2d = Camera2D::from_display_rect(view);

        // --- Selection (left mouse) ---
        if is_mouse_button_pressed(MouseButton::Left) && !over_ui {
            drag_start = Some(mp);
        }
        if is_mouse_button_released(MouseButton::Left) {
            if let Some(start) = drag_start.take() {
                // Shift adds to the current selection instead of replacing it.
                let shift = is_key_down(KeyCode::LeftShift) || is_key_down(KeyCode::RightShift);
                if !shift {
                    clear_selection(&mut world);
                }
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
                    let now = get_time();
                    if let Some(e) = best {
                        // Double-click the same unit → select all of its type on screen.
                        let dbl = last_click.1 == Some(e) && now - last_click.0 < 0.35;
                        if dbl {
                            if let Ok(k) = world.get::<&components::UnitKind>(e) {
                                let kind = k.id.clone();
                                drop(k);
                                to_sel = selection::same_kind_in_rect(&world, &kind, view);
                            }
                        } else {
                            to_sel.push(e);
                        }
                        last_click = (now, Some(e));
                    } else {
                        last_click = (now, None);
                    }
                } else {
                    to_sel = selection::in_rect(&world, Rect::new(minx, miny, maxx - minx, maxy - miny));
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
            issue_move(&mut world, &nav, &mut flow_cache, &sel, goal);
        }

        // --- Control groups (1-9): Ctrl+N assigns the selection, N recalls it ---
        let ctrl = is_key_down(KeyCode::LeftControl) || is_key_down(KeyCode::RightControl);
        for (key, n) in [
            (KeyCode::Key1, 1usize),
            (KeyCode::Key2, 2),
            (KeyCode::Key3, 3),
            (KeyCode::Key4, 4),
            (KeyCode::Key5, 5),
            (KeyCode::Key6, 6),
            (KeyCode::Key7, 7),
            (KeyCode::Key8, 8),
            (KeyCode::Key9, 9),
        ] {
            if is_key_pressed(key) {
                if ctrl {
                    control_groups.assign(n, &world);
                } else {
                    control_groups.select(n, &mut world);
                }
            }
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
        // Recompute layout after this frame's input so panels reflect current selection.
        let hud_layout = hud::HudLayout::compute(&world, sw, sh);
        match hud::draw(&mut ui, &world, &economy, &hud_layout) {
            hud::HudAction::Stop => {
                let sel: Vec<Entity> = world.query::<&Selected>().iter().map(|(e, _)| e).collect();
                for e in sel {
                    let _ = world.remove_one::<MoveOrder>(e);
                }
            }
            hud::HudAction::ClearSel => clear_selection(&mut world),
            hud::HudAction::None => {}
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
                let cap_layout = hud::HudLayout::compute(&world, sw, sh);
                let _ = hud::draw(&mut ui, &world, &economy, &cap_layout);
                set_default_camera();
                rt.texture.get_texture_data().export_png(path);
                break;
            }
        }
    }
}
