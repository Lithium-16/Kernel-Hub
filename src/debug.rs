//! Debugging suite designed for AI (Claude) use. Every check is one env-var command →
//! one short structured output line → exit. No screenshots, no prose, no reading code.
//!
//! Commands (all headless, native-only):
//!   COLDWAR_QUERY=<fields>        run N ticks, print JSON, exit 0
//!   COLDWAR_ASSERT=<scenario>     run scenario, print PASS/FAIL, exit 0/1
//!   COLDWAR_EVENTLOG=1            write debug/events.jsonl during a normal run
//!
//! COLDWAR_QUERY fields (comma-separated):
//!   shots_fired  kills  alive  mean_hp  moving  tracers  tick_ms  entities
//!
//! COLDWAR_ASSERT scenarios:
//!   combat_discrete    one attacker, one target → exactly 1 tracer per shot
//!   no_friendly_fire   all same faction → 0 damage after 10 ticks
//!   turret_delays      tank faces wrong way → no damage tick 1, damage by tick 20
//!   formation_fills    25 units group move → 0 still-moving after 1200 ticks

use std::collections::HashMap;
use hecs::{Entity, World};
use macroquad::prelude::Vec2;

use crate::components::{Faction, Health, MoveOrder, Tracer, Weapon};

// ── Event log ────────────────────────────────────────────────────────────────

pub struct EventLog {
    enabled: bool,
    lines: Vec<String>,
    pub tick: u32,
}

impl EventLog {
    pub fn new() -> Self {
        Self {
            enabled: std::env::var("COLDWAR_EVENTLOG").is_ok(),
            lines: Vec::new(),
            tick: 0,
        }
    }

    pub fn advance(&mut self) { self.tick += 1; }

    pub fn log(&mut self, event: &str) {
        if self.enabled {
            self.lines.push(format!(r#"{{"tick":{},"event":{}}}"#, self.tick, event));
        }
    }

    pub fn shot(&mut self, faction: &str, damage: f32, dist: f32) {
        self.log(&format!(r#"{{"type":"shot","faction":"{}","damage":{:.1},"dist":{:.1}}}"#, faction, damage, dist));
    }

    pub fn kill(&mut self, faction: &str, unit_type: &str) {
        self.log(&format!(r#"{{"type":"kill","faction":"{}","unit":"{}"}}"#, faction, unit_type));
    }

    pub fn move_order(&mut self, count: usize, goal: Vec2) {
        self.log(&format!(r#"{{"type":"move","units":{},"goal":[{:.0},{:.0}]}}"#, count, goal.x, goal.y));
    }

    pub fn building(&mut self, kind: &str, tx: usize, ty: usize) {
        self.log(&format!(r#"{{"type":"building","kind":"{}","tile":[{},{}]}}"#, kind, tx, ty));
    }

    pub fn game_over(&mut self, winner: &str) {
        self.log(&format!(r#"{{"type":"game_over","winner":"{}"}}"#, winner));
    }

    pub fn flush(&self) {
        if !self.enabled || self.lines.is_empty() { return; }
        #[cfg(not(target_arch = "wasm32"))]
        {
            use std::io::Write;
            let _ = std::fs::create_dir_all("debug");
            if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open("debug/events.jsonl") {
                for line in &self.lines {
                    let _ = writeln!(f, "{}", line);
                }
            }
        }
    }
}

// ── World stats sampler ───────────────────────────────────────────────────────

pub struct Stats {
    pub shots_fired: u64,
    pub kills: HashMap<String, u64>,
    pub total_ticks: u32,
    pub total_tick_ms: f64,
}

impl Stats {
    pub fn new() -> Self {
        Self { shots_fired: 0, kills: HashMap::new(), total_ticks: 0, total_tick_ms: 0.0 }
    }

    pub fn record_shot(&mut self, _faction: &str) { self.shots_fired += 1; }
    pub fn record_kill(&mut self, faction: &str) { *self.kills.entry(faction.to_string()).or_default() += 1; }
    pub fn record_tick(&mut self, ms: f64) { self.total_ticks += 1; self.total_tick_ms += ms; }

    /// Collect a world-state snapshot and return as JSON for the requested comma-separated fields.
    pub fn query_json(&self, world: &World, fields: &str, tick_ms: f32) -> String {
        let mut alive: HashMap<String, u64> = HashMap::new();
        let mut hp_sum: HashMap<String, f64> = HashMap::new();
        for (_e, (fac, h)) in world.query::<(&Faction, &Health)>().iter() {
            *alive.entry(fac.0.clone()).or_default() += 1;
            *hp_sum.entry(fac.0.clone()).or_default() += h.cur as f64;
        }
        let moving = world.query::<&MoveOrder>().iter().count();
        let tracers = world.query::<&Tracer>().iter().count();
        let entities = world.len();

        let mut parts: Vec<String> = Vec::new();
        for f in fields.split(',').map(str::trim) {
            let v = match f {
                "shots_fired" => format!(r#""shots_fired":{}"#, self.shots_fired),
                "kills" => {
                    let inner: Vec<String> = self.kills.iter().map(|(k, v)| format!(r#""{}": {}"#, k, v)).collect();
                    format!(r#""kills":{{{}}}"#, inner.join(","))
                }
                "alive" => {
                    let inner: Vec<String> = alive.iter().map(|(k, v)| format!(r#""{}": {}"#, k, v)).collect();
                    format!(r#""alive":{{{}}}"#, inner.join(","))
                }
                "mean_hp" => {
                    let inner: Vec<String> = alive.iter().map(|(k, &n)| {
                        let mean = if n > 0 { hp_sum.get(k).copied().unwrap_or(0.0) / n as f64 } else { 0.0 };
                        format!(r#""{}": {:.1}"#, k, mean)
                    }).collect();
                    format!(r#""mean_hp":{{{}}}"#, inner.join(","))
                }
                "moving"   => format!(r#""moving":{}"#, moving),
                "tracers"  => format!(r#""tracers":{}"#, tracers),
                "tick_ms"  => format!(r#""tick_ms":{:.3}"#, tick_ms),
                "entities" => format!(r#""entities":{}"#, entities),
                other      => format!(r#""{}":"unknown""#, other),
            };
            parts.push(v);
        }
        format!("{{{}}}", parts.join(","))
    }
}

// ── COLDWAR_ASSERT scenarios ─────────────────────────────────────────────────

/// Run the named scenario headlessly, print PASS or FAIL:<reason>, exit 0/1.
pub fn run_assert(scenario: &str) {
    let result = match scenario {
        "combat_discrete"  => assert_combat_discrete(),
        "no_friendly_fire" => assert_no_friendly_fire(),
        "turret_delays"    => assert_turret_delays(),
        "formation_fills"  => assert_formation_fills(),
        "ammo_drains"      => assert_ammo_drains(),
        "fuel_drains"      => assert_fuel_drains(),
        other => Err(format!("unknown scenario '{}'", other)),
    };
    match result {
        Ok(msg) => { println!("PASS: {}", msg); std::process::exit(0); }
        Err(msg) => { println!("FAIL: {}", msg); std::process::exit(1); }
    }
}

// ── Scenario helpers ──────────────────────────────────────────────────────────

fn sim_grid(world: &World) -> crate::spatial::SpatialGrid {
    let mut g = crate::spatial::SpatialGrid::new(Vec2::new(8192.0, 8192.0), 32.0);
    g.rebuild(world);
    g
}

fn combat_tick(world: &mut World, grid: &crate::spatial::SpatialGrid, dt: f32) {
    crate::combat::step(world, grid, dt, "player");
}

// ── Scenario: one shot = one tracer ──────────────────────────────────────────

fn assert_combat_discrete() -> Result<String, String> {
    use crate::components::{Health, Weapon};
    let mut world = World::new();
    world.spawn((
        crate::components::Position(Vec2::new(0.0, 0.0)),
        Faction("player".into()),
        Weapon { range: 80.0, damage: 10.0, fire_rate: 2.0, cooldown: 0.0 },
        Health { cur: 100.0, max: 100.0 },
    ));
    world.spawn((
        crate::components::Position(Vec2::new(40.0, 0.0)),
        Faction("enemy".into()),
        Health { cur: 100.0, max: 100.0 },
    ));
    let grid = sim_grid(&world);
    combat_tick(&mut world, &grid, 0.05); // one tick at 20Hz
    let tracers = world.query::<&Tracer>().iter().count();
    if tracers == 1 {
        Ok(format!("1 tracer spawned for 1 shot (expected 1, got {})", tracers))
    } else {
        Err(format!("expected exactly 1 tracer per shot, got {} — DPS leak", tracers))
    }
}

// ── Scenario: no friendly fire ───────────────────────────────────────────────

fn assert_no_friendly_fire() -> Result<String, String> {
    use crate::components::{Health, Weapon};
    let mut world = World::new();
    for i in 0..4 {
        let offset = Vec2::new(i as f32 * 30.0, 0.0);
        world.spawn((
            crate::components::Position(offset),
            Faction("player".into()),
            Weapon { range: 120.0, damage: 10.0, fire_rate: 5.0, cooldown: 0.0 },
            Health { cur: 100.0, max: 100.0 },
        ));
    }
    for _ in 0..10 {
        let grid = sim_grid(&world);
        combat_tick(&mut world, &grid, 0.05);
    }
    let damage = world.query::<&Health>().iter()
        .filter(|(_, h)| h.cur < 100.0).count();
    if damage == 0 {
        Ok("0 friendly-fire damage events in 10 ticks".into())
    } else {
        Err(format!("{} units took damage from friendlies", damage))
    }
}

// ── Scenario: turret must aim before firing ───────────────────────────────────

fn assert_turret_delays() -> Result<String, String> {
    use crate::components::{Health, Turret, Weapon};
    let mut world = World::new();
    world.spawn((
        crate::components::Position(Vec2::new(0.0, 0.0)),
        Faction("player".into()),
        Weapon { range: 120.0, damage: 20.0, fire_rate: 2.0, cooldown: 0.0 },
        // Turret facing 180° away from the target (target is at +X, turret faces -X)
        Turret { angle: std::f32::consts::PI, turn_rate: 2.0 },
        Health { cur: 100.0, max: 100.0 },
    ));
    let target = world.spawn((
        crate::components::Position(Vec2::new(60.0, 0.0)),
        Faction("enemy".into()),
        Health { cur: 200.0, max: 200.0 },
    ));

    // Tick 1: turret not aimed yet, should not fire
    let grid = sim_grid(&world);
    let _g2 = sim_grid(&world);
    crate::combat::update_turrets(&mut world, &grid, 0.05);
    combat_tick(&mut world, &grid, 0.05);
    let hp_after_tick1 = world.get::<&Health>(target).map(|h| h.cur).unwrap_or(0.0);
    if hp_after_tick1 < 200.0 {
        return Err(format!("turret fired before aimed: target HP dropped to {:.0} on tick 1", hp_after_tick1));
    }

    // Run until aimed (≈ π/turn_rate seconds)
    for _ in 0..60 {
        let grid = sim_grid(&world);
        crate::combat::update_turrets(&mut world, &grid, 0.05);
        combat_tick(&mut world, &grid, 0.05);
    }
    let hp_final = world.get::<&Health>(target).map(|h| h.cur).unwrap_or(0.0);
    if hp_final < 200.0 {
        Ok(format!("turret waited to aim, then fired: target HP = {:.0}/200", hp_final))
    } else {
        Err("turret never fired even after 3s of rotation".into())
    }
}

// ── Scenario: formation fills (0 units stuck) ────────────────────────────────

fn assert_formation_fills() -> Result<String, String> {
    use crate::components::{Heading, Mobility, MoveState, Position, Velocity};
    use crate::movement::{UNIT_RADIUS};
    use macroquad::prelude::{vec2, Vec2};

    let map_px = Vec2::new(8192.0, 8192.0);
    let (nav_map, _) = crate::map::TileMap::generate(256, 256);
    let nav = crate::nav::NavGrid::from_map(&nav_map);
    let mut flow_cache = crate::nav::FlowCache::new(16);
    let mut world = World::new();
    let mut grid = crate::spatial::SpatialGrid::new(map_px, 24.0);
    let tick_dt = 1.0 / 20.0;

    // Spawn 25 units in a block on the left
    let center = map_px * 0.5 + vec2(-400.0, 0.0);
    let cols = 5;
    let mut entities = Vec::new();
    for i in 0..25usize {
        let gx = (i % cols) as f32 - 2.0;
        let gy = (i / cols) as f32 - 2.0;
        let pos = center + vec2(gx * 32.0, gy * 32.0);
        let e = world.spawn((
            Position(pos),
            Velocity(Vec2::ZERO),
            Heading(0.0f32),
            MoveState { last: pos, stall: 0 },
            Mobility { speed: 70.0, turn_rate: 9.0 },
        ));
        entities.push(e);
    }

    // Issue a group move to the right
    let goal = map_px * 0.5 + vec2(200.0, 0.0);
    let (tx, ty) = ((goal.x / crate::map::TILE_SIZE) as i32, (goal.y / crate::map::TILE_SIZE) as i32);
    let flow = flow_cache.get_or_build(&nav, (tx as usize, ty as usize));
    let n = entities.len();
    let cols_f = (n as f32).sqrt().ceil().max(1.0) as i32;
    let rows = ((n as i32 + cols_f - 1) / cols_f).max(1);
    let spacing = UNIT_RADIUS * 2.2;
    let half = vec2(cols_f as f32, rows as f32) * spacing * 0.5;
    let seek = half.length() + spacing * 2.0;
    let arrive = spacing * 2.0;
    for (i, &e) in entities.iter().enumerate() {
        let cx = (i as i32 % cols_f) as f32 - (cols_f - 1) as f32 * 0.5;
        let cy = (i as i32 / cols_f) as f32 - (rows - 1) as f32 * 0.5;
        let slot = goal + vec2(cx * spacing, cy * spacing);
        let _ = world.insert_one(e, crate::components::MoveOrder {
            flow: flow.clone(), goal: slot, anchor: goal, seek, arrive, attack_move: false,
        });
        let _ = world.insert_one(e, crate::components::Formation { offset: slot - goal });
    }

    for _ in 0..1200 {
        grid.rebuild(&world);
        crate::movement::step(&mut world, &grid, &nav, map_px, tick_dt);
        grid.rebuild(&world);
        crate::movement::resolve_collisions(&mut world, &grid, &nav, map_px, 2);
        crate::movement::settle_arrivals(&mut world);
    }

    let still_moving = world.query::<&crate::components::MoveOrder>().iter().count();
    if still_moving == 0 {
        Ok("all 25 units reached formation slots (0 stuck)".into())
    } else {
        Err(format!("{}/25 units still stuck after 1200 ticks", still_moving))
    }
}

// ── Scenario: ammo drains and gun goes silent ─────────────────────────────────

fn assert_ammo_drains() -> Result<String, String> {
    use crate::components::{AmmoStorage, Faction, Health, Position, Weapon};
    use macroquad::prelude::*;

    let mut world = World::new();

    // Shooter: 3 shells onboard, fires at 10/s so empties within ~0.3s of sim time.
    let shooter = world.spawn((
        Position(vec2(100.0, 100.0)),
        Faction("a".into()),
        Weapon { range: 200.0, damage: 1.0, fire_rate: 10.0, cooldown: 0.0 },
        AmmoStorage::new(3),
    ));
    // Target (won't fight back).
    world.spawn((
        Position(vec2(150.0, 100.0)),
        Faction("b".into()),
        Health { cur: 9999.0, max: 9999.0 },
    ));

    let grid = sim_grid(&world);
    // 60 ticks × dt=0.1 = 6 simulated seconds — plenty to exhaust 3 shells.
    for _ in 0..60 {
        combat_tick(&mut world, &grid, 0.1);
    }

    let shots_left = world.get::<&AmmoStorage>(shooter)
        .map(|a| a.shots)
        .unwrap_or(999);

    if shots_left != 0 {
        return Err(format!("expected AmmoStorage.shots == 0 after 60 ticks, got {}", shots_left));
    }

    // Gun should now be silent: no new tracers after ammo = 0.
    let before = world.query::<&crate::components::Tracer>().iter().count();
    combat_tick(&mut world, &grid, 0.1);
    let after = world.query::<&crate::components::Tracer>().iter().count();
    // Tracers age out; new ones only appear if the gun fired.
    if after <= before {
        Ok(format!("ammo drained to 0, gun silent (tracers {} → {})", before, after))
    } else {
        Err(format!("ammo = 0 but gun still fires (tracers {} → {})", before, after))
    }
}

// ── Scenario: fuel drains and vehicle stops ───────────────────────────────────

fn assert_fuel_drains() -> Result<String, String> {
    use crate::components::{FuelTank, Heading, Mobility, MoveOrder, MoveState, Position, Velocity, Formation};
    use macroquad::prelude::*;

    let (map, _) = crate::map::TileMap::generate(64, 64);
    let nav = crate::nav::NavGrid::from_map(&map);
    let map_px = map.size_px();
    let mut cache = crate::nav::FlowCache::new(8);
    let tick_dt = 1.0_f32 / 20.0;

    let mut world = World::new();
    let start = vec2(400.0, 400.0);
    let goal  = vec2(1600.0, 400.0);
    let (tx, ty) = (
        (goal.x / crate::map::TILE_SIZE) as usize,
        (goal.y / crate::map::TILE_SIZE) as usize,
    );
    let flow = cache.get_or_build(&nav, (tx, ty));

    // Very low fuel: burn_rate=1.0 px⁻¹, fuel=10 → stops after ~10px.
    let e = world.spawn((
        Position(start),
        Velocity(Vec2::ZERO),
        Heading(0.0_f32),
        MoveState { last: start, stall: 0 },
        Mobility { speed: 80.0, turn_rate: 5.0 },
        MoveOrder { flow, goal, anchor: goal, seek: 200.0, arrive: 20.0, attack_move: false },
        Formation { offset: Vec2::ZERO },
        FuelTank { fuel: 10.0, capacity: 200.0, burn_rate: 1.0 },
    ));

    let grid = crate::spatial::SpatialGrid::new(map_px, 24.0);
    for _ in 0..200 {
        crate::movement::step(&mut world, &grid, &nav, map_px, tick_dt);
    }

    let fuel_left  = world.get::<&FuelTank>(e).map(|f| f.fuel).unwrap_or(-1.0);
    let dist_moved = world.get::<&Position>(e).map(|p| p.0.distance(start)).unwrap_or(0.0);
    let dist_goal  = world.get::<&Position>(e).map(|p| p.0.distance(goal)).unwrap_or(0.0);

    if fuel_left <= 0.0 && dist_goal > 100.0 {
        Ok(format!("fuel drained to {:.1}, vehicle stranded {:.0}px short of goal (moved {:.0}px)", fuel_left, dist_goal, dist_moved))
    } else if fuel_left > 0.0 {
        Err(format!("fuel did not drain: {:.1} remaining after 200 ticks", fuel_left))
    } else {
        Err(format!("vehicle reached goal despite tiny fuel tank (fuel={:.1}, dist_goal={:.0})", fuel_left, dist_goal))
    }
}
