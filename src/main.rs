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
mod building;
mod camera;
mod combat;
mod components;
mod data;
mod ecs;
mod economy;
mod groups;
mod hud;
mod map;
mod minimap;
mod movement;
mod nav;
mod render;
mod selection;
mod sim;
mod spatial;
mod stance;
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

/// The player's faction; only these units are selectable/commandable.
const PLAYER_FACTION: &str = "vanguard";
/// The enemy faction.
const ENEMY_FACTION: &str = "crimson";

/// Spawn `count` units in a loose block centered on `center`, cycling unit types, all
/// assigned to `faction` and drawn with `tint`. Armed types also get a `Weapon`.
fn spawn_army(
    world: &mut hecs::World,
    defs: &Definitions,
    sprites: &Sprites,
    faction: &str,
    center: Vec2,
    tint: Color,
    count: usize,
) {
    let cols = (count as f32).sqrt().ceil().max(1.0) as usize;
    for i in 0..count {
        let unit = &defs.units[i % defs.units.len()];
        let sprite = sprites.unit_index(&unit.sprite);
        let gx = (i % cols) as f32 - cols as f32 * 0.5;
        let gy = (i / cols) as f32 - cols as f32 * 0.5;
        let pos = center + vec2(gx * 30.0, gy * 30.0);
        let e = world.spawn((
            Position(pos),
            Velocity(Vec2::ZERO),
            Heading(-std::f32::consts::FRAC_PI_2),
            MoveState { last: pos, stall: 0 },
            Mobility { speed: unit.speed, turn_rate: unit.turn_rate },
            Renderable { sprite, tint, size: unit.radius * 2.6 },
            Faction(faction.to_string()),
            components::UnitKind { id: unit.id.clone(), name: unit.name.clone() },
            stance::Stance::Aggressive,
            components::Health { cur: unit.hp, max: unit.hp },
        ));
        if unit.dps > 0.0 {
            let _ = world.insert_one(e, components::Weapon { range: unit.range, dps: unit.dps });
        }
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
        // Remember the formation offset (so queued legs keep the shape) and drop any
        // pending waypoints — a fresh move order replaces the whole plan.
        let _ = world.insert_one(e, components::Formation { offset: goal - click });
        let _ = world.remove_one::<components::OrderQueue>(e);
    }
}

/// Issue a single move leg for one unit toward `anchor`, holding its formation `offset`
/// so the group's shape translates along the path (goal = anchor + offset). Used to
/// advance queued waypoints.
fn issue_leg(world: &mut hecs::World, nav: &NavGrid, cache: &mut FlowCache, e: Entity, anchor: Vec2, offset: Vec2) {
    let (tx, ty) = ((anchor.x / map::TILE_SIZE) as i32, (anchor.y / map::TILE_SIZE) as i32);
    if tx < 0 || ty < 0 || tx as usize >= nav.w || ty as usize >= nav.h || !nav.passable(tx as usize, ty as usize) {
        return;
    }
    let flow = cache.get_or_build(nav, (tx as usize, ty as usize));
    let spacing = movement::UNIT_RADIUS * 2.2;
    let seek = offset.length() + spacing * 2.0;
    let arrive = spacing * 2.0;
    let _ = world.insert_one(e, MoveOrder { flow, goal: anchor + offset, anchor, seek, arrive });
}

/// Advance waypoint queues: any unit with no active `MoveOrder` but a non-empty
/// `OrderQueue` pops its next anchor and starts that leg. Run once per frame.
fn advance_queues(world: &mut hecs::World, nav: &NavGrid, cache: &mut FlowCache) {
    let ready: Vec<(Entity, Vec2, Vec2)> = world
        .query::<(&components::OrderQueue, Option<&components::Formation>)>()
        .without::<&MoveOrder>()
        .iter()
        .filter_map(|(e, (q, f))| q.anchors.front().map(|&a| (e, a, f.map_or(Vec2::ZERO, |f| f.offset))))
        .collect();
    for (e, anchor, offset) in ready {
        if let Ok(q) = world.query_one_mut::<&mut components::OrderQueue>(e) {
            q.anchors.pop_front();
        }
        issue_leg(world, nav, cache, e, anchor, offset);
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
    let mut nav = NavGrid::from_map(&map);
    let mut flow_cache = FlowCache::new(48);
    let minimap = minimap::Minimap::build(&map);

    let mut world = ecs::new_world();
    let count: usize = std::env::var("COLDWAR_UNITS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(200);
    // Two opposing armies: player (vanguard, white) left of center, enemy (crimson, red
    // tint) right of center, so they can clash when ordered together.
    let player_tint = Color::new(0.85, 0.92, 1.0, 1.0);
    let enemy_tint = Color::new(1.0, 0.55, 0.55, 1.0);
    spawn_army(&mut world, &defs, &sprites, PLAYER_FACTION, map_px * 0.5 + vec2(-600.0, 0.0), player_tint, count);
    spawn_army(&mut world, &defs, &sprites, ENEMY_FACTION, map_px * 0.5 + vec2(600.0, 0.0), enemy_tint, count);

    let mut cam = camera::GameCamera::centered(map_px);
    if let Ok(z) = std::env::var("COLDWAR_ZOOM") {
        if let Ok(s) = z.parse::<f32>() {
            cam.scale = s;
        }
    }

    let mut grid = SpatialGrid::new(map_px, 24.0);
    let mut drag_start: Option<Vec2> = None;
    let mut last_click: (f64, Option<Entity>) = (0.0, None); // (time, entity) for double-click
    let mut placing: Option<usize> = None; // index into defs.buildings while in placement mode
    let mut ai_timer = 0.0f32; // enemy re-evaluates its advance on this cadence
    let mut ui = ui::Ui::new();
    let economy = economy::Economy::default();
    let mut control_groups = groups::ControlGroups::new();

    let mut sim = sim::Sim::new();
    let tick_dt = 1.0 / sim::TICK_RATE as f32;
    let mut accumulator = 0.0f32;

    // Capture mode: select all and order a move so the screenshot shows movement.
    if capture_path.is_some() {
        // Demo building near map center so captures show the building render + nav block.
        if let Some(def) = defs.buildings.first() {
            let (tx, ty) = (126usize, 126usize);
            if building::can_place(&nav, tx as i32, ty as i32, def.w, def.h) {
                let color = Color::from_rgba(def.color.0, def.color.1, def.color.2, 255);
                world.spawn((components::Building { tx, ty, w: def.w, h: def.h, color },));
                for dy in 0..def.h {
                    for dx in 0..def.w {
                        nav.set_blocked(tx + dx, ty + dy);
                    }
                }
            }
        }
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
        let input_layout = hud::HudLayout::compute(&world, sw, sh);
        let over_ui = input_layout.contains(mp);

        // Minimap click / drag recenters the camera (handled before cam.update clamps).
        if is_mouse_button_down(MouseButton::Left) && input_layout.minimap.contains(mp) {
            cam.center = minimap
                .world_at(input_layout.minimap, mp)
                .clamp(Vec2::ZERO, map_px);
        }

        cam.update(map_px);
        let view = cam.view_rect(sw, sh);
        let cam2d = Camera2D::from_display_rect(view);

        // --- Building placement mode ---
        // B cycles through building types (then off); Esc / right-click exits. While
        // placing, a ghost previews the snapped footprint and left-click commits it.
        if is_key_pressed(KeyCode::B) {
            placing = match placing {
                None => (!defs.buildings.is_empty()).then_some(0),
                Some(i) if i + 1 < defs.buildings.len() => Some(i + 1),
                _ => None,
            };
        }
        let mut ghost: Option<(Rect, bool)> = None;
        if let Some(idx) = placing {
            let def = &defs.buildings[idx];
            let world_mp = cam2d.screen_to_world(mp);
            let (tx, ty) = building::snap_origin(world_mp, def.w, def.h, map::TILE_SIZE);
            let foot = Rect::new(
                tx as f32 * map::TILE_SIZE,
                ty as f32 * map::TILE_SIZE,
                def.w as f32 * map::TILE_SIZE,
                def.h as f32 * map::TILE_SIZE,
            );
            let nav_ok = building::can_place(&nav, tx, ty, def.w, def.h);
            let unit_clear = !world.query::<&Position>().iter().any(|(_, p)| foot.contains(p.0));
            let valid = nav_ok && unit_clear;
            ghost = Some((foot, valid));

            if is_mouse_button_pressed(MouseButton::Left) && !over_ui && valid {
                let color = Color::from_rgba(def.color.0, def.color.1, def.color.2, 255);
                world.spawn((components::Building {
                    tx: tx as usize,
                    ty: ty as usize,
                    w: def.w,
                    h: def.h,
                    color,
                },));
                for dy in 0..def.h {
                    for dx in 0..def.w {
                        nav.set_blocked(tx as usize + dx, ty as usize + dy);
                    }
                }
                flow_cache.clear(); // nav changed: stale routes must not be reused
            }
            if is_mouse_button_pressed(MouseButton::Right) || is_key_pressed(KeyCode::Escape) {
                placing = None;
            }
        }
        let placing_active = placing.is_some();

        // --- Selection (left mouse) ---
        if is_mouse_button_pressed(MouseButton::Left) && !over_ui && !placing_active {
            drag_start = Some(mp);
        }
        if is_mouse_button_released(MouseButton::Left) && !placing_active {
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
                // Only the player's own units are selectable.
                to_sel.retain(|&e| {
                    world.get::<&Faction>(e).map(|f| f.0 == PLAYER_FACTION).unwrap_or(false)
                });
                for e in to_sel {
                    let _ = world.insert_one(e, Selected);
                }
            }
        }

        // --- Move order (right mouse): only the currently-selected units ---
        // Plain RMB = fresh formation move; Shift+RMB = queue a waypoint (if the group
        // already has a plan to append to, else it's just a fresh move).
        if is_mouse_button_pressed(MouseButton::Right) && !over_ui && !placing_active {
            let click = cam2d.screen_to_world(mp);
            let shift = is_key_down(KeyCode::LeftShift) || is_key_down(KeyCode::RightShift);
            let sel: Vec<Entity> = world.query::<&Selected>().iter().map(|(e, _)| e).collect();
            let has_active = sel.iter().any(|&e| {
                world.get::<&MoveOrder>(e).is_ok()
                    || world.get::<&components::OrderQueue>(e).map(|q| !q.anchors.is_empty()).unwrap_or(false)
            });
            if shift && has_active {
                for &e in &sel {
                    if let Ok(q) = world.query_one_mut::<&mut components::OrderQueue>(e) {
                        q.anchors.push_back(click);
                    } else {
                        let mut anchors = std::collections::VecDeque::new();
                        anchors.push_back(click);
                        let _ = world.insert_one(e, components::OrderQueue { anchors });
                    }
                }
            } else {
                issue_move(&mut world, &nav, &mut flow_cache, &sel, click);
            }
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

        // --- Restart (R): wipe the field, reset nav, respawn both armies ---
        if is_key_pressed(KeyCode::R) {
            let all: Vec<Entity> = world.iter().map(|e| e.entity()).collect();
            for e in all {
                let _ = world.despawn(e);
            }
            nav = NavGrid::from_map(&map);
            flow_cache.clear();
            placing = None;
            spawn_army(&mut world, &defs, &sprites, PLAYER_FACTION, map_px * 0.5 + vec2(-600.0, 0.0), player_tint, count);
            spawn_army(&mut world, &defs, &sprites, ENEMY_FACTION, map_px * 0.5 + vec2(600.0, 0.0), enemy_tint, count);
        }

        // --- Enemy AI: periodically order the whole enemy force to advance on the
        // player's center of mass, so it actually attacks instead of waiting. ---
        ai_timer -= get_frame_time();
        if ai_timer <= 0.0 {
            ai_timer = 1.5;
            let mut sum = Vec2::ZERO;
            let mut n = 0u32;
            for (_e, (f, p)) in world.query::<(&Faction, &Position)>().iter() {
                if f.0 == PLAYER_FACTION {
                    sum += p.0;
                    n += 1;
                }
            }
            if n > 0 {
                let target = sum / n as f32;
                let enemies: Vec<Entity> = world
                    .query::<&Faction>()
                    .iter()
                    .filter(|(_, f)| f.0 == ENEMY_FACTION)
                    .map(|(e, _)| e)
                    .collect();
                if !enemies.is_empty() {
                    issue_move(&mut world, &nav, &mut flow_cache, &enemies, target);
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
            grid.rebuild(&world);
            combat::step(&mut world, &grid, tick_dt, PLAYER_FACTION);
            sim.tick();
            accumulator -= tick_dt;
        }
        // Units that just arrived and still have queued waypoints start their next leg.
        advance_queues(&mut world, &nav, &mut flow_cache);
        let tick_ms = ((get_time() - t0) * 1000.0) as f32;

        let drag_box = drag_start.map(|s| (s, mp));
        render::present(&world, &map, &cam, &sim, &sprites, drag_box, ghost, tick_ms, None);
        // Recompute layout after this frame's input so panels reflect current selection.
        let hud_layout = hud::HudLayout::compute(&world, sw, sh);
        match hud::draw(&mut ui, &world, &economy, &hud_layout) {
            hud::HudAction::Stop => {
                let sel: Vec<Entity> = world.query::<&Selected>().iter().map(|(e, _)| e).collect();
                for e in sel {
                    let _ = world.remove_one::<MoveOrder>(e);
                    let _ = world.remove_one::<components::OrderQueue>(e);
                }
            }
            hud::HudAction::ClearSel => clear_selection(&mut world),
            hud::HudAction::SetStance(s) => {
                stance::set_selected(&mut world, s);
                // Hold-Ground also halts current movement and cancels queued waypoints.
                if s == stance::Stance::HoldGround {
                    let sel: Vec<Entity> = world.query::<&Selected>().iter().map(|(e, _)| e).collect();
                    for e in sel {
                        let _ = world.remove_one::<MoveOrder>(e);
                        let _ = world.remove_one::<components::OrderQueue>(e);
                    }
                }
            }
            hud::HudAction::None => {}
        }
        minimap.draw(&world, view, hud_layout.minimap);

        // --- Win/lose banner (only once a battle has been spawned) ---
        if count > 0 {
            let mut player_alive = 0usize;
            let mut enemy_alive = 0usize;
            for (_e, (f, _h)) in world.query::<(&Faction, &components::Health)>().iter() {
                if f.0 == PLAYER_FACTION {
                    player_alive += 1;
                } else if f.0 == ENEMY_FACTION {
                    enemy_alive += 1;
                }
            }
            let banner = if player_alive == 0 {
                Some(("DEFEAT", Color::new(1.0, 0.4, 0.4, 1.0)))
            } else if enemy_alive == 0 {
                Some(("VICTORY", Color::new(0.5, 1.0, 0.6, 1.0)))
            } else {
                None
            };
            if let Some((text, color)) = banner {
                let big = 74.0;
                let d = measure_text(text, None, big as u16, 1.0);
                draw_text(text, (sw - d.width) * 0.5, sh * 0.42, big, color);
                let hint = "Press R to restart";
                let dh = measure_text(hint, None, 28, 1.0);
                draw_text(hint, (sw - dh.width) * 0.5, sh * 0.42 + 46.0, 28.0, WHITE);
            }
        }

        next_frame().await;

        if let Some(path) = &capture_path {
            frame += 1;
            if frame >= capture_frames {
                let rt = render_target(screen_width() as u32, screen_height() as u32);
                render::present(&world, &map, &cam, &sim, &sprites, None, None, tick_ms, Some(rt.clone()));
                let mut uicam = Camera2D::from_display_rect(Rect::new(0.0, 0.0, sw, sh));
                uicam.render_target = Some(rt.clone());
                set_camera(&uicam);
                let cap_layout = hud::HudLayout::compute(&world, sw, sh);
                let _ = hud::draw(&mut ui, &world, &economy, &cap_layout);
                minimap.draw(&world, cam.view_rect(sw, sh), cap_layout.minimap);
                set_default_camera();
                rt.texture.get_texture_data().export_png(path);
                break;
            }
        }
    }
}
