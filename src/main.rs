//! Cold War RTS (working title) — entry point.
//!
//! Phase 2-3: tilemap, camera, ECS, placeholder sprites, flow-field movement with
//! avoidance + collision + facing, selection, and per-unit move orders.

// Some scaffolding is intentionally unused while systems are wired up phase by phase.
#![allow(dead_code, unused_imports)]

use std::sync::Arc;

use hecs::Entity;
use macroquad::prelude::*;

mod ai_brain;
mod assets;
mod building;
mod camera;
mod combat;
mod combat_group;
mod construction;
mod debug;
mod components;
mod data;
mod depot;
mod depot_spawn;
mod ecs;
mod economy;
mod extraction;
mod fog;
mod groups;
mod hud;
mod map;
mod minimap;
mod movement;
mod nav;
mod render;
mod resupply;
mod selection;
mod sim;
mod spatial;
mod processing;
mod spawn_building;
mod stance;
mod supply_route;
mod truck;
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
pub const PLAYER_FACTION: &str = "vanguard";
/// The enemy faction.
pub const ENEMY_FACTION: &str = "crimson";

/// Spawn `count` units in a block centered on `center` for the given faction.
/// Returns the list of spawned entity IDs so the caller can form a CombatGroup.
#[allow(clippy::too_many_arguments)]
fn spawn_army(
    world: &mut hecs::World,
    defs: &Definitions,
    sprites: &Sprites,
    faction: &str,
    center: Vec2,
    tint: Color,
    unit_id_filter: Option<&str>, // None = cycle all types
    count: usize,
) -> Vec<hecs::Entity> {
    let cols = (count as f32).sqrt().ceil().max(1.0) as usize;
    let filtered: Vec<_> = defs.units.iter()
        .filter(|u| unit_id_filter.is_none() || unit_id_filter == Some(u.id.as_str()))
        .collect();
    let unit_pool: Vec<_> = if filtered.is_empty() { defs.units.iter().collect() } else { filtered };
    let mut entities = Vec::with_capacity(count);
    for i in 0..count {
        let unit = unit_pool[i % unit_pool.len()];
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
            Renderable {
                sprite,
                tint,
                size: unit.radius * 2.6,
                hull_sprite: unit.hull_sprite.clone(),
                turret_sprite: unit.turret_sprite.clone(),
            },
            Faction(faction.to_string()),
            components::UnitKind { id: unit.id.clone(), name: unit.name.clone() },
            stance::Stance::Aggressive,
            components::Health { cur: unit.hp, max: unit.hp },
            components::VisionRange(if unit.vision_range > 0.0 { unit.vision_range } else { fog::VISION_RADIUS_PX }),
        ));
        if unit.fire_rate > 0.0 {
            let _ = world.insert_one(e, components::Weapon {
                range: unit.range,
                damage: unit.damage,
                fire_rate: unit.fire_rate,
                cooldown: 0.0,
            });
            // Onboard ammo storage: unit definition drives capacity (0 = unarmed, skipped).
            if unit.ammo_capacity > 0 {
                let _ = world.insert_one(e, components::AmmoStorage::new(unit.ammo_capacity));
            }
        }
        if unit.turret_turn_rate > 0.0 {
            let _ = world.insert_one(e, components::Turret {
                angle: -std::f32::consts::FRAC_PI_2,
                turn_rate: unit.turret_turn_rate,
            });
        }
        // Onboard fuel tank: vehicles only (fuel_capacity > 0 in defs).
        if unit.fuel_capacity > 0.0 {
            let burn = if unit.fuel_burn_rate > 0.0 { unit.fuel_burn_rate } else { 0.05 };
            let _ = world.insert_one(e, components::FuelTank {
                fuel: unit.fuel_capacity,
                capacity: unit.fuel_capacity,
                burn_rate: burn,
            });
        }
        entities.push(e);
    }
    entities
}

/// Set up both sides symmetrically. Player spawns bottom-left, enemy top-right.
/// Each side gets one Armored Group and one Engineer Group registered in the GroupRegistry.
/// The enemy waits PREP_TICKS before advancing.
#[allow(clippy::too_many_arguments)]
fn spawn_scenario(
    world: &mut hecs::World,
    nav: &mut NavGrid,
    groups: &mut combat_group::GroupRegistry,
    defs: &Definitions,
    sprites: &Sprites,
    map: &map::TileMap,
    count: usize,
    player_tint: Color,
    enemy_tint: Color,
) {
    let p_spawn = map.player_spawn();
    let e_spawn = map.enemy_spawn();

    // Player side
    let p_armor = spawn_army(world, defs, sprites, PLAYER_FACTION, p_spawn, player_tint, Some("tank"), count);
    let p_eng   = spawn_army(world, defs, sprites, PLAYER_FACTION, p_spawn + vec2(100.0, 0.0), player_tint, Some("engineer"), 5);
    let p_recon = spawn_army(world, defs, sprites, PLAYER_FACTION, p_spawn + vec2(0.0, -120.0), player_tint, Some("scout"), 4);
    groups.add("1st Armored Group", PLAYER_FACTION, p_armor);
    groups.add("1st Engineer Group", PLAYER_FACTION, p_eng);
    groups.add("1st Recon Group", PLAYER_FACTION, p_recon);

    // Enemy side (identical capability — symmetric AI)
    let e_armor = spawn_army(world, defs, sprites, ENEMY_FACTION, e_spawn, enemy_tint, Some("tank"), count);
    let e_eng   = spawn_army(world, defs, sprites, ENEMY_FACTION, e_spawn + vec2(-100.0, 0.0), enemy_tint, Some("engineer"), 5);
    let e_recon = spawn_army(world, defs, sprites, ENEMY_FACTION, e_spawn + vec2(0.0, 120.0), enemy_tint, Some("scout"), 4);
    groups.add("1st Enemy Armored Group", ENEMY_FACTION, e_armor);
    groups.add("1st Enemy Engineer Group", ENEMY_FACTION, e_eng);
    groups.add("1st Enemy Recon Group", ENEMY_FACTION, e_recon);

    // Spawn HQ buildings for both sides. HQ includes a Depot pre-stocked with starting resources.
    if let Some(hq_def) = defs.buildings.iter().find(|b| b.id == "hq") {
        // Player HQ: offset from spawn so it doesn't block units.
        let phq_pos = p_spawn + vec2(0.0, 200.0);
        let ptx = ((phq_pos.x / map::TILE_SIZE) as usize).saturating_sub(hq_def.w / 2);
        let pty = ((phq_pos.y / map::TILE_SIZE) as usize).saturating_sub(hq_def.h / 2);
        spawn_building::spawn_hq(world, hq_def, phq_pos, PLAYER_FACTION, 2000, 1500, 800, 400);
        for dy in 0..hq_def.h { for dx in 0..hq_def.w { nav.set_blocked(ptx + dx, pty + dy); } }

        // Enemy HQ
        let ehq_pos = e_spawn + vec2(0.0, -200.0);
        let etx = ((ehq_pos.x / map::TILE_SIZE) as usize).saturating_sub(hq_def.w / 2);
        let ety = ((ehq_pos.y / map::TILE_SIZE) as usize).saturating_sub(hq_def.h / 2);
        spawn_building::spawn_hq(world, hq_def, ehq_pos, ENEMY_FACTION, 2000, 1500, 800, 400);
        for dy in 0..hq_def.h { for dx in 0..hq_def.w { nav.set_blocked(etx + dx, ety + dy); } }
    }

    // HQ buildings (spawned above) already contain pre-stocked Depot components.
    // No additional depot_spawn needed — HQ is the starting depot.
}

/// Find the nearest passable world-position to `pos`. Used to spawn trucks
/// and issue move orders outside building footprints.
fn nearest_passable(pos: Vec2, nav: &NavGrid) -> Vec2 {
    let tx = (pos.x / map::TILE_SIZE) as i32;
    let ty = (pos.y / map::TILE_SIZE) as i32;
    for r in 0i32..30 {
        for dy in -r..=r {
            for dx in -r..=r {
                if dx.abs() != r && dy.abs() != r { continue; } // outer ring only
                let nx = tx + dx;
                let ny = ty + dy;
                if nx >= 0 && ny >= 0
                    && (nx as usize) < nav.w && (ny as usize) < nav.h
                    && nav.passable(nx as usize, ny as usize)
                {
                    return vec2(
                        (nx as f32 + 0.5) * map::TILE_SIZE,
                        (ny as f32 + 0.5) * map::TILE_SIZE,
                    );
                }
            }
        }
    }
    pos
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
        let _ = world.insert_one(e, MoveOrder { flow: flow.clone(), goal, anchor: click, seek, arrive, attack_move: false });
        let _ = world.insert_one(e, components::Formation { offset: goal - click });
        let _ = world.remove_one::<components::OrderQueue>(e);
    }
}

/// Same as `issue_move` but sets the attack_move flag when targeting an enemy.
fn issue_move_with_flags(world: &mut hecs::World, nav: &NavGrid, cache: &mut FlowCache, units: &[Entity], click: Vec2, attack_move: bool) {
    let (tx, ty) = ((click.x / map::TILE_SIZE) as i32, (click.y / map::TILE_SIZE) as i32);
    if units.is_empty() || tx < 0 || ty < 0 || tx as usize >= nav.w || ty as usize >= nav.h || !nav.passable(tx as usize, ty as usize) { return; }
    let flow = cache.get_or_build(nav, (tx as usize, ty as usize));
    let n = units.len();
    let cols = (n as f32).sqrt().ceil().max(1.0) as i32;
    let rows = ((n as i32 + cols - 1) / cols).max(1);
    let spacing = movement::UNIT_RADIUS * 2.2;
    let slots: Vec<Vec2> = (0..n as i32).map(|i| {
        let cx = (i % cols) as f32 - (cols - 1) as f32 * 0.5;
        let cy = (i / cols) as f32 - (rows - 1) as f32 * 0.5;
        click + vec2(cx * spacing, cy * spacing)
    }).collect();
    let half = vec2(cols as f32, rows as f32) * spacing * 0.5;
    let seek = half.length() + spacing * 2.0;
    let arrive = spacing * 2.0;
    let positions: Vec<(Entity, Vec2)> = units.iter().filter_map(|&e| {
        let pos = world.query_one_mut::<&Position>(e).ok()?.0;
        Some((e, pos))
    }).collect();
    let mut taken = vec![false; slots.len()];
    for (e, p) in positions {
        let best = slots.iter().enumerate().filter(|(si, _)| !taken[*si]).min_by(|(_, a), (_, b)| p.distance(**a).partial_cmp(&p.distance(**b)).unwrap()).map(|(si, _)| si);
        let goal = best.map(|si| { taken[si] = true; slots[si] }).unwrap_or(click);
        let _ = world.insert_one(e, MoveOrder { flow: flow.clone(), goal, anchor: click, seek, arrive, attack_move });
        let _ = world.insert_one(e, components::Formation { offset: goal - click });
        let _ = world.remove_one::<components::OrderQueue>(e);
    }
}

fn issue_leg(world: &mut hecs::World, nav: &NavGrid, cache: &mut FlowCache, e: Entity, anchor: Vec2, offset: Vec2) {
    let (tx, ty) = ((anchor.x / map::TILE_SIZE) as i32, (anchor.y / map::TILE_SIZE) as i32);
    if tx < 0 || ty < 0 || tx as usize >= nav.w || ty as usize >= nav.h || !nav.passable(tx as usize, ty as usize) { return; }
    let flow = cache.get_or_build(nav, (tx as usize, ty as usize));
    let spacing = movement::UNIT_RADIUS * 2.2;
    let seek = offset.length() + spacing * 2.0;
    let arrive = spacing * 2.0;
    let _ = world.insert_one(e, MoveOrder { flow, goal: anchor + offset, anchor, seek, arrive, attack_move: false });
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

    let mut sprites = Sprites::load();
    let defs = data::load_definitions();
    let (map, _regions) = map::TileMap::generate(256, 256);
    let map_px = map.size_px();
    let mut nav = NavGrid::from_map(&map);
    let mut flow_cache = FlowCache::new(64);
    let minimap = minimap::Minimap::build(&map);

    let mut fog = fog::FogGrid::new(map.width, map.height);
    let (ptx, pty) = map.player_spawn_tile();
    fog.reveal_tile(ptx as i32, pty as i32, fog::HQ_REVEAL_TILES);
    let (etx, ety) = map.enemy_spawn_tile();
    // Enemy also gets its HQ pre-revealed (symmetric)
    fog.reveal_tile(etx as i32, ety as i32, fog::HQ_REVEAL_TILES);

    let mut world = ecs::new_world();
    let count: usize = std::env::var("COLDWAR_UNITS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(30); // smaller default; quality over quantity now
    macroquad::rand::srand(miniquad::date::now().to_bits());
    let player_tint = Color::new(0.85, 0.92, 1.0, 1.0);
    let enemy_tint = Color::new(1.0, 0.55, 0.55, 1.0);

    let mut groups = combat_group::GroupRegistry::new();
    let mut ai = ai_brain::AiBrain::new(ENEMY_FACTION, PLAYER_FACTION);

    spawn_scenario(&mut world, &mut nav, &mut groups, &defs, &sprites, &map, count, player_tint, enemy_tint);

    let mut cam = camera::GameCamera { center: map.player_spawn(), scale: 1.0 };
    if let Ok(z) = std::env::var("COLDWAR_ZOOM") {
        if let Ok(s) = z.parse::<f32>() {
            cam.scale = s;
        }
    }

    let mut grid = SpatialGrid::new(map_px, 24.0);
    let mut drag_start: Option<Vec2> = None;
    let mut last_click: (f64, Option<Entity>) = (0.0, None); // (time, entity) for double-click
    let mut placing: Option<usize> = None; // index into defs.buildings while in placement mode
    let mut build_panel_open = false;     // whether the build panel UI is visible
    let mut event_log = debug::EventLog::new();
    let mut stats = debug::Stats::new();
    let mut ui = ui::Ui::new();
    let mut control_groups = groups::ControlGroups::new();

    let mut resupply_tracker = resupply::ResupplyTracker::new();
    let mut routes = supply_route::RouteRegistry::new();
    // Route-drawing state: None=idle, Some(None)=picking origin, Some(Some(e))=origin chosen
    let mut route_origin: Option<Option<hecs::Entity>> = None;
    // Resource type cycling for new routes
    let mut route_resource_idx: usize = 0;
    // Selected depot for inspection panel (None = panel closed)
    let mut selected_depot: Option<hecs::Entity> = None;
    // Building/unit selected for context panel (right-click)
    let mut context_entity: Option<hecs::Entity> = None;

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

    // Headless assertion runner: COLDWAR_ASSERT=<scenario>. Prints PASS/FAIL, exits 0/1.
    if let Ok(scenario) = std::env::var("COLDWAR_ASSERT") {
        debug::run_assert(&scenario);
    }

    // Headless world-state query: COLDWAR_QUERY=<fields> [COLDWAR_QTICKS=N].
    // Runs N ticks of a two-army battle, prints one JSON line, exits 0.
    if let Ok(fields) = std::env::var("COLDWAR_QUERY") {
        let ticks: u32 = std::env::var("COLDWAR_QTICKS").ok().and_then(|s| s.parse().ok()).unwrap_or(300);
        // Kick off AI advance so both armies move toward each other in query mode.
        {
            let enemies: Vec<Entity> = world.query::<&Faction>().iter()
                .filter(|(_, f)| f.0 == ENEMY_FACTION).map(|(e, _)| e).collect();
            let player_center = {
                let mut s = Vec2::ZERO; let mut n = 0u32;
                for (_, (f, p)) in world.query::<(&Faction, &Position)>().iter() {
                    if f.0 == PLAYER_FACTION { s += p.0; n += 1; }
                }
                if n > 0 { s / n as f32 } else { map_px * 0.5 }
            };
            if !enemies.is_empty() { issue_move(&mut world, &nav, &mut flow_cache, &enemies, player_center); }
        }
        let mut t0_sum = 0.0f64;
        let mut query_ai = 0.0f32;
        for tick_i in 0..ticks {
            let t0 = std::time::Instant::now();
            // Periodically refresh enemy orders toward the player.
            query_ai += tick_dt;
            if query_ai >= 2.0 || tick_i == 0 {
                query_ai = 0.0;
                let mut s = Vec2::ZERO; let mut n = 0u32;
                for (_, (f, p)) in world.query::<(&Faction, &Position)>().iter() {
                    if f.0 == PLAYER_FACTION { s += p.0; n += 1; }
                }
                if n > 0 {
                    let target = s / n as f32;
                    let enemies: Vec<Entity> = world.query::<&Faction>().iter()
                        .filter(|(_, f)| f.0 == ENEMY_FACTION).map(|(e, _)| e).collect();
                    if !enemies.is_empty() { issue_move(&mut world, &nav, &mut flow_cache, &enemies, target); }
                }
            }
            let tracers_before = world.query::<&components::Tracer>().iter().count();
            let alive_before = world.query::<&components::Health>().iter().count();
            grid.rebuild(&world);
            movement::step(&mut world, &grid, &nav, map_px, tick_dt);
            grid.rebuild(&world);
            movement::resolve_collisions(&mut world, &grid, &nav, map_px, 2);
            movement::settle_arrivals(&mut world);
            grid.rebuild(&world);
            combat::step(&mut world, &grid, tick_dt, PLAYER_FACTION);
            // Count new tracers as shots fired this tick
            let new_shots = world.query::<&components::Tracer>().iter().count().saturating_sub(tracers_before);
            for _ in 0..new_shots { stats.record_shot(PLAYER_FACTION); }
            // Count kills (units whose health entity disappeared)
            let alive_after = world.query::<&components::Health>().iter().count();
            if alive_before > alive_after {
                for _ in 0..(alive_before - alive_after) { stats.record_kill("any"); }
            }
            let elapsed = t0.elapsed().as_secs_f64() * 1000.0;
            stats.record_tick(elapsed);
            t0_sum += elapsed;
            event_log.advance();
        }
        let tick_ms = (t0_sum / ticks as f64) as f32;
        println!("{}", stats.query_json(&world, &fields, tick_ms));
        std::process::exit(0);
    }

    let mut loop_frame: u32 = 0;
    loop {
        // Load hull/turret sprites on frame 1 — after the first next_frame().await.
        // By then macroquad's font atlas has been created by the first draw_text call
        // in frame 0, so loading extra textures no longer corrupts the GL font state.
        if loop_frame == 1 && !sprites.has_hull_turrets() {
            sprites.load_hull_turrets(&defs.units);
        }

        let (mx, my) = mouse_position();
        let mp = vec2(mx, my);
        let sw = screen_width();
        let sh = screen_height();
        ui.begin();
        // Input layering: compute HUD panel rects up front (anchored to window size) and
        // gate world input on them, so clicks on any panel never fall through to the world.
        let input_layout = {
            let base = hud::HudLayout::compute(&world, sw, sh);
            let base = if build_panel_open { base.with_build_panel(defs.buildings.len(), sw, sh) } else { base };
            let base = base.with_route_panel(routes.all().len(), route_origin.is_some(), sh);
            if selected_depot.is_some() { base.with_depot_panel(sw, sh) } else { base }
        };
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
        // B opens/closes the build panel; click a button to pick a type; Esc exits.
        if is_key_pressed(KeyCode::B) {
            build_panel_open = !build_panel_open;
            if !build_panel_open { placing = None; }
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
                let btx = tx as usize;
                let bty = ty as usize;
                let bp_color = Color::from_rgba(def.color.0, def.color.1, def.color.2, 120);
                let bp_cx = (btx as f32 + def.w as f32 * 0.5) * map::TILE_SIZE;
                let bp_cy = (bty as f32 + def.h as f32 * 0.5) * map::TILE_SIZE;
                // Spawn a Blueprint entity instead of an instant building.
                // Engineers will auto-claim it and build it.
                world.spawn((
                    components::Building { tx: btx, ty: bty, w: def.w, h: def.h, color: bp_color },
                    components::Blueprint {
                        building_id: def.id.clone(),
                        tx: btx, ty: bty, w: def.w, h: def.h,
                        progress: 0.0,
                        required_supplies: def.required_supplies,
                        supplies_consumed: 0,
                        faction: PLAYER_FACTION.to_string(),
                        supply_timer: 0.0,
                    },
                    components::Faction(PLAYER_FACTION.to_string()),
                    components::Position(vec2(bp_cx, bp_cy)),
                ));
                for dy in 0..def.h {
                    for dx in 0..def.w {
                        nav.set_blocked(btx + dx, bty + dy);
                    }
                }
                flow_cache.clear(); // nav changed: stale routes must not be reused
                event_log.building(&def.id, btx, bty);
            }
            if is_mouse_button_pressed(MouseButton::Right) || is_key_pressed(KeyCode::Escape) {
                placing = None;
                build_panel_open = false;
            }
        }
        let placing_active = placing.is_some();

        // --- Route drawing mode (T key) ---
        // T: toggle route mode. Click depot A → origin, click depot B → create route.
        // Tab cycles the resource type. Escape cancels.
        if is_key_pressed(KeyCode::T) {
            route_origin = if route_origin.is_none() { Some(None) } else { None };
        }
        if is_key_pressed(KeyCode::Tab) {
            route_resource_idx = (route_resource_idx + 1) % 4;
        }
        let route_mode_active = route_origin.is_some();
        if route_mode_active && is_key_pressed(KeyCode::Escape) {
            route_origin = None;
        }
        if route_mode_active && is_mouse_button_pressed(MouseButton::Left) && !over_ui {
            let click = cam2d.screen_to_world(mp);
            // Find which depot was clicked (within 80px of depot centre).
            let hit_depot: Option<hecs::Entity> = {
                let mut found = None;
                let mut best = f32::MAX;
                for (e, (pos, _depot, fac)) in world.query::<(&components::Position, &depot::Depot, &components::Faction)>().iter() {
                    if fac.0 != PLAYER_FACTION { continue; }
                    let d = pos.0.distance(click);
                    if d < 80.0 && d < best { best = d; found = Some(e); }
                }
                found
            };
            if let Some(depot_e) = hit_depot {
                match route_origin {
                    Some(None) => {
                        // First click: set origin
                        route_origin = Some(Some(depot_e));
                    }
                    Some(Some(origin_e)) if origin_e != depot_e => {
                        // Second click: create route with default desired stock 500.
                        // Player can adjust in the route panel later.
                        let res = [
                            depot::ResourceType::Ammo,
                            depot::ResourceType::Fuel,
                            depot::ResourceType::BuildingSupplies,
                            depot::ResourceType::WeaponParts,
                        ][route_resource_idx];
                        routes.add(origin_e, depot_e, res, 500, supply_route::RoutePriority::High);
                        route_origin = Some(None); // stay in mode for another route
                    }
                    _ => {}
                }
            }
            // Right-click cancels route mode
        }
        if route_mode_active && is_mouse_button_pressed(MouseButton::Right) {
            route_origin = None;
        }

        // --- Depot click (normal mode, not route mode) ---
        // Left-clicking near a depot opens its inspection panel.
        if is_mouse_button_pressed(MouseButton::Left) && !over_ui && !placing_active && !route_mode_active {
            let click = cam2d.screen_to_world(mp);
            let hit: Option<hecs::Entity> = {
                let mut found = None;
                let mut best = f32::MAX;
                for (e, pos) in world.query::<(&components::Position, &depot::Depot)>().iter().map(|(e,(p,_))|(e,p)) {
                    let d = pos.0.distance(click);
                    if d < 80.0 && d < best { best = d; found = Some(e); }
                }
                found
            };
            if let Some(depot_e) = hit {
                selected_depot = Some(depot_e);
            } else if selected_depot.is_some() {
                // Click elsewhere closes the panel
                selected_depot = None;
            }
        }

        // --- Selection (left mouse) ---
        if is_mouse_button_pressed(MouseButton::Left) && !over_ui && !placing_active && !route_mode_active {
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
                // Only player's own units are selectable — buildings, depots, and NonSelectable excluded.
                to_sel.retain(|&e| {
                    world.get::<&Faction>(e).map(|f| f.0 == PLAYER_FACTION).unwrap_or(false)
                        && world.get::<&components::Renderable>(e).is_ok()
                        && world.get::<&components::NonSelectable>(e).is_err()
                });
                for e in to_sel {
                    let _ = world.insert_one(e, Selected);
                }
            }
        }

        // --- Move / Attack-move order (right mouse) ---
        // Right-click ground → formation move.
        // Right-click: open building context panel OR issue move order.
        if is_mouse_button_pressed(MouseButton::Right) && !over_ui && !placing_active {
            let click = cam2d.screen_to_world(mp);

            // Check if click is on a Building entity or engineer with IsBuilding.
            let hit_building: Option<hecs::Entity> = {
                let mut found = None;
                let mut best = f32::MAX;
                // Buildings (footprint centre within 80px)
                for (e, pos) in world.query::<(&components::Position, &components::Building)>()
                    .iter().map(|(e,(p,_))|(e,p))
                {
                    let d = pos.0.distance(click);
                    if d < 80.0 && d < best { best = d; found = Some(e); }
                }
                // Engineers with IsBuilding
                for (e, (pos, uk)) in world.query::<(&components::Position, &components::UnitKind)>().iter() {
                    if uk.id != "engineer" { continue; }
                    if world.get::<&components::IsBuilding>(e).is_err() { continue; }
                    let d = pos.0.distance(click);
                    if d < 40.0 && d < best { best = d; found = Some(e); }
                }
                found
            };
            if let Some(e) = hit_building {
                context_entity = Some(e);
            } else {
                context_entity = None;
                // No building hit — process as a move/attack order
                let shift = is_key_down(KeyCode::LeftShift) || is_key_down(KeyCode::RightShift);
                let sel: Vec<Entity> = world.query::<&Selected>().iter().map(|(e, _)| e).collect();
                let attack_target = {
                    let mut found = false;
                    let mut best_d = 24.0f32 * 24.0;
                    for (_e, (pos, fac)) in world.query::<(&Position, &Faction)>().iter() {
                        if fac.0 == PLAYER_FACTION { continue; }
                        let d2 = pos.0.distance_squared(click);
                        if d2 < best_d { best_d = d2; found = true; }
                    }
                    found
                };
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
                    event_log.move_order(sel.len(), click);
                    issue_move_with_flags(&mut world, &nav, &mut flow_cache, &sel, click, attack_target);
                }
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

        // --- Restart (R) ---
        if is_key_pressed(KeyCode::R) {
            let all: Vec<Entity> = world.iter().map(|e| e.entity()).collect();
            for e in all { let _ = world.despawn(e); }
            nav = NavGrid::from_map(&map);
            flow_cache.clear();
            placing = None;
            groups.clear();
            routes.clear();
            ai = ai_brain::AiBrain::new(ENEMY_FACTION, PLAYER_FACTION);
            spawn_scenario(&mut world, &mut nav, &mut groups, &defs, &sprites, &map, count, player_tint, enemy_tint);
        }

        // --- AI brain tick (runs on fixed sim cadence, not render) ---
        // Executed once per render frame for now; will move into sim tick loop.
        {
            // Fan out group orders to individual units.
            let orders: Vec<_> = groups.all().iter().map(|g| (g.faction.clone(), g.order.clone(), g.members.clone())).collect();
            for (_faction, order, members) in &orders {
                let living: Vec<Entity> = members.iter().copied().filter(|&e| world.contains(e)).collect();
                match order {
                    combat_group::GroupOrder::AdvanceTo(goal) => {
                        issue_move(&mut world, &nav, &mut flow_cache, &living, *goal);
                    }
                    combat_group::GroupOrder::Hold => {
                        for &e in &living { let _ = world.remove_one::<MoveOrder>(e); }
                    }
                    combat_group::GroupOrder::Withdraw(goal) => {
                        issue_move(&mut world, &nav, &mut flow_cache, &living, *goal);
                    }
                    combat_group::GroupOrder::Idle => {}
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
            extraction::step(&mut world, tick_dt);
            processing::step(&mut world, tick_dt);
            resupply_tracker.step(&mut world);

            // ── Truck dispatch ────────────────────────────────────────────
            let dispatches = supply_route::dispatch_needed(&mut routes, &world);
            for (route_id, origin_e, dest_e, resource, amount) in dispatches {
                let origin_centre = match world.get::<&components::Position>(origin_e) {
                    Ok(p) => p.0, Err(_) => continue,
                };
                let dest_centre = match world.get::<&components::Position>(dest_e) {
                    Ok(p) => p.0, Err(_) => continue,
                };
                let faction = world.get::<&components::Faction>(origin_e)
                    .map(|f| f.0.clone())
                    .unwrap_or_else(|_| PLAYER_FACTION.to_string());
                let tint = if faction == PLAYER_FACTION {
                    Color::new(0.85, 0.92, 1.0, 1.0)
                } else {
                    Color::new(1.0, 0.55, 0.55, 1.0)
                };
                // Spawn outside the source building footprint, drive to tile adjacent to destination.
                let spawn_pos = nearest_passable(origin_centre, &nav);
                let move_target = nearest_passable(dest_centre, &nav);
                let truck_e = truck::spawn_truck(
                    &mut world, &sprites, route_id,
                    resource, amount, origin_e, dest_e,
                    spawn_pos, &faction, tint,
                );
                issue_move(&mut world, &nav, &mut flow_cache, &[truck_e], move_target);
            }

            // ── Truck step — handle arrivals and destruction ───────────────
            let truck_events = truck::step(&mut world);
            let mut trucks_to_despawn: Vec<hecs::Entity> = Vec::new();
            for (truck_e, event) in truck_events {
                match event {
                    truck::TruckEvent::Delivered { route_id, resource, amount, dest } => {
                        // Transfer cargo to destination depot
                        if let Ok(dest_ref) = world.entity(dest) {
                            if let Some(mut depot) = dest_ref.get::<&mut depot::Depot>() {
                                depot.add(resource, amount);
                            }
                        }
                        // Issue return trip
                        let origin_pos = {
                            let origin = world.get::<&truck::Truck>(truck_e)
                                .map(|t| t.origin)
                                .ok();
                            origin.and_then(|o| world.get::<&components::Position>(o).map(|p| p.0).ok())
                        };
                        if let Some(target) = origin_pos {
                            let passable_target = nearest_passable(target, &nav);
                            issue_move(&mut world, &nav, &mut flow_cache, &[truck_e], passable_target);
                        }
                        if let Ok(mut t) = world.get::<&mut truck::Truck>(truck_e) {
                            t.state = truck::TruckState::DrivingBack;
                            t.cargo_amount = 0;
                        }
                        let _ = route_id; // bookkeeping via active_trucks; decremented on Returned
                    }
                    truck::TruckEvent::Returned { route_id } => {
                        trucks_to_despawn.push(truck_e);
                        if let Some(r) = routes.get_mut(route_id) {
                            r.active_trucks = r.active_trucks.saturating_sub(1);
                        }
                    }
                    truck::TruckEvent::Destroyed { route_id } => {
                        trucks_to_despawn.push(truck_e);
                        if let Some(r) = routes.get_mut(route_id) {
                            r.active_trucks = r.active_trucks.saturating_sub(1);
                        }
                    }
                }
            }
            for e in trucks_to_despawn {
                let _ = world.despawn(e);
            }

            // Construction: engineers advance blueprints toward completion.
            let completed_blueprints = construction::step(&mut world, tick_dt);
            for bp_entity in completed_blueprints {
                // Collect blueprint data before despawning.
                let bp_data = {
                    let Ok(bp)  = world.get::<&components::Blueprint>(bp_entity) else { continue; };
                    let Ok(bld) = world.get::<&components::Building>(bp_entity) else { continue; };
                    let Ok(fac) = world.get::<&components::Faction>(bp_entity) else { continue; };
                    (bp.building_id.clone(), bld.tx, bld.ty, fac.0.clone())
                };
                let (building_id, btx, bty, bfaction) = bp_data;
                // Clear IsBuilding from any engineers assigned to this blueprint.
                let clear_engineers: Vec<hecs::Entity> = world
                    .query::<&components::IsBuilding>()
                    .iter()
                    .filter(|(_, ib)| ib.blueprint == bp_entity)
                    .map(|(e, _)| e)
                    .collect();
                for e in clear_engineers { let _ = world.remove_one::<components::IsBuilding>(e); }
                // Despawn the blueprint, then spawn the real building.
                let _ = world.despawn(bp_entity);
                if let Some(def) = defs.buildings.iter().find(|b| b.id == building_id) {
                    let bx = (btx as f32 + def.w as f32 * 0.5) * map::TILE_SIZE;
                    let by = (bty as f32 + def.h as f32 * 0.5) * map::TILE_SIZE;
                    let building_e = spawn_building::spawn_building(&mut world, def, btx, bty, &bfaction);

                    // Attach extraction/processing components that need a co-located depot.
                    match building_id.as_str() {
                        "mine" | "oil_pump" => {
                            // Spawn a small local depot next to the extractor.
                            let local_depot = world.spawn((
                                components::Position(vec2(bx, by)),
                                components::Faction(bfaction.clone()),
                                depot::Depot::new(&bfaction, 300.0),
                            ));
                            let kind = if building_id == "mine" {
                                extraction::ExtractorKind::Mine
                            } else {
                                extraction::ExtractorKind::OilPump
                            };
                            let _ = world.insert_one(building_e, extraction::Extractor {
                                kind, attached_depot: local_depot, cooldown: 0.0,
                            });
                        }
                        "processing" => {
                            let local_depot = world.spawn((
                                components::Position(vec2(bx, by)),
                                components::Faction(bfaction.clone()),
                                depot::Depot::new(&bfaction, 300.0),
                            ));
                            let _ = world.insert_one(building_e, processing::Processor {
                                kind: processing::ProcessorKind::ProcessingFacility,
                                attached_depot: local_depot, cooldown: 0.0,
                            });
                        }
                        "refinery" => {
                            let local_depot = world.spawn((
                                components::Position(vec2(bx, by)),
                                components::Faction(bfaction.clone()),
                                depot::Depot::new(&bfaction, 300.0),
                            ));
                            let _ = world.insert_one(building_e, processing::Processor {
                                kind: processing::ProcessorKind::FuelRefinery,
                                attached_depot: local_depot, cooldown: 0.0,
                            });
                        }
                        "ammo_factory" => {
                            let local_depot = world.spawn((
                                components::Position(vec2(bx, by)),
                                components::Faction(bfaction.clone()),
                                depot::Depot::new(&bfaction, 300.0),
                            ));
                            let _ = world.insert_one(building_e, processing::Processor {
                                kind: processing::ProcessorKind::AmmoFactory,
                                attached_depot: local_depot, cooldown: 0.0,
                            });
                        }
                        _ => {}
                    }
                }
            }
            // AI brain tick — uses same systems as player
            ai.tick(&mut world, &mut groups);
            groups.prune_all(&world);
            // Fog update from player units
            fog.update(&world, PLAYER_FACTION);
            event_log.advance();
            stats.record_tick(tick_dt as f64 * 1000.0);
            sim.tick();
            accumulator -= tick_dt;
        }
        advance_queues(&mut world, &nav, &mut flow_cache);
        let tick_ms = ((get_time() - t0) * 1000.0) as f32;

        // Engineer auto-move: engineers with IsBuilding but no MoveOrder walk to their blueprint.
        {
            let engineer_moves: Vec<(hecs::Entity, Vec2)> = world
                .query::<(&components::IsBuilding, &components::Position)>()
                .without::<&MoveOrder>()
                .iter()
                .filter_map(|(e, (ib, _pos))| {
                    world.get::<&components::Position>(ib.blueprint).ok().map(|bp_pos| (e, bp_pos.0))
                })
                .collect();
            for (e, target) in engineer_moves {
                issue_move(&mut world, &nav, &mut flow_cache, &[e], target);
            }
        }

        // Turrets track enemies every render frame for smooth rotation.
        combat::update_turrets(&mut world, &grid, get_frame_time());

        let drag_box = drag_start.map(|s| (s, mp));
        render::present(&world, &map, &fog, &cam, &sim, &sprites, drag_box, ghost, tick_ms, None);
        // Draw supply routes + route-drawing overlay in world space before switching to screen cam.
        render::draw_route_overlay(&world, &routes, route_origin, &cam, sw, sh);

        // Aggregate live economy from all player depots for the HUD top bar.
        let economy = economy::aggregate(&world, PLAYER_FACTION);
        // Recompute layout after this frame's input so panels reflect current selection.
        let hud_layout = {
            let base = hud::HudLayout::compute(&world, sw, sh);
            let base = if build_panel_open { base.with_build_panel(defs.buildings.len(), sw, sh) } else { base };
            let base = base.with_route_panel(routes.all().len(), route_origin.is_some(), sh);
            if selected_depot.is_some() { base.with_depot_panel(sw, sh) } else { base }
        };
        match hud::draw(&mut ui, &world, &economy, &ai, &groups, &hud_layout, &hud::BuildState { buildings: &defs.buildings, placing, panel_open: build_panel_open }) {
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
                if s == stance::Stance::HoldGround {
                    let sel: Vec<Entity> = world.query::<&Selected>().iter().map(|(e, _)| e).collect();
                    for e in sel {
                        let _ = world.remove_one::<MoveOrder>(e);
                        let _ = world.remove_one::<components::OrderQueue>(e);
                    }
                }
            }
            hud::HudAction::SelectGroup(id) => {
                clear_selection(&mut world);
                if let Some(g) = groups.get_mut(id) {
                    g.select_members(&mut world);
                    if let Some(c) = g.centroid(&world) {
                        cam.center = c;
                    }
                }
            }
            hud::HudAction::PlaceBuilding(idx) => {
                placing = Some(idx);
                // Keep panel open so player can switch type without re-opening
            }
            hud::HudAction::CloseBuildPanel => {
                build_panel_open = false;
                placing = None;
            }
            hud::HudAction::None => {}
        }

        // G key: cycle through player combat groups (SupCom-style group cycling).
        if is_key_pressed(KeyCode::G) {
            let player_groups: Vec<u32> = groups.all().iter()
                .filter(|g| g.faction == PLAYER_FACTION)
                .map(|g| g.id)
                .collect();
            if !player_groups.is_empty() {
                let current = groups.selected_group_id(&world);
                let next_id = match current {
                    Some(id) => {
                        let pos = player_groups.iter().position(|&x| x == id).unwrap_or(0);
                        player_groups[(pos + 1) % player_groups.len()]
                    }
                    None => player_groups[0],
                };
                clear_selection(&mut world);
                if let Some(g) = groups.get_mut(next_id) {
                    g.select_members(&mut world);
                    if let Some(c) = g.centroid(&world) {
                        cam.center = c;
                    }
                }
            }
        }

        minimap.draw(&world, &fog, view, hud_layout.minimap);

        // Route management panel
        if let Some(del_id) = hud::draw_route_panel(&mut ui, &routes, &world, route_mode_active, route_resource_idx, &hud_layout) {
            routes.remove(del_id);
            // Despawn any trucks still driving this route — cargo is lost.
            let stale: Vec<hecs::Entity> = world.query::<&truck::Truck>()
                .iter()
                .filter(|(_, t)| t.route_id == del_id)
                .map(|(e, _)| e)
                .collect();
            for e in stale { let _ = world.despawn(e); }
        }

        // Depot inspection panel (left-click)
        if let Some(depot_e) = selected_depot {
            let gone = !world.contains(depot_e);
            let closed = !gone && hud::draw_depot_panel(&mut ui, depot_e, &world, &hud_layout);
            if gone || closed { selected_depot = None; }
        }

        // Building/engineer context panel (right-click)
        if let Some(ctx_e) = context_entity {
            let gone = !world.contains(ctx_e);
            let closed = !gone && hud::draw_building_context(&mut ui, ctx_e, &world, sw, sh);
            if gone || closed { context_entity = None; }
        }

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
                // Log once and flush the event log
                if text == "VICTORY" || text == "DEFEAT" {
                    event_log.game_over(text);
                    event_log.flush();
                }
            }
        }

        next_frame().await;
        loop_frame = loop_frame.saturating_add(1);

        if let Some(path) = &capture_path {
            frame += 1;
            if frame >= capture_frames {
                let rt = render_target(screen_width() as u32, screen_height() as u32);
                render::present(&world, &map, &fog, &cam, &sim, &sprites, None, None, tick_ms, Some(rt.clone()));
                let mut uicam = Camera2D::from_display_rect(Rect::new(0.0, 0.0, sw, sh));
                uicam.render_target = Some(rt.clone());
                set_camera(&uicam);
                let cap_layout = hud::HudLayout::compute(&world, sw, sh);
                let cap_economy = economy::aggregate(&world, PLAYER_FACTION);
                let _ = hud::draw(&mut ui, &world, &cap_economy, &ai, &groups, &cap_layout, &hud::BuildState { buildings: &defs.buildings, placing: None, panel_open: false });
                minimap.draw(&world, &fog, cam.view_rect(sw, sh), cap_layout.minimap);
                set_default_camera();
                rt.texture.get_texture_data().export_png(path);
                break;
            }
        }
    }
}
