//! Combat (Phase 3 core). Direct-fire: each armed unit damages the nearest *enemy*
//! (different faction) within its weapon range every tick — hitscan, no projectiles yet.
//! Units at 0 HP despawn. Stance-driven movement and projectiles come later; this is
//! enough to make two armies actually fight.

use hecs::{Entity, World};
use macroquad::prelude::{Color, Vec2};

use crate::components::{Faction, Health, Position, Tracer, Turret, Weapon, TRACER_TTL};
use crate::spatial::SpatialGrid;

fn wrap_angle(a: f32) -> f32 {
    use std::f32::consts::{PI, TAU};
    let a = a % TAU;
    if a > PI { a - TAU } else if a < -PI { a + TAU } else { a }
}

/// Per-frame: rotate every turret toward its nearest enemy so the barrel tracks
/// smoothly. Called every render frame (not just sim ticks) for smooth animation.
pub fn update_turrets(world: &mut World, grid: &SpatialGrid, dt: f32) {
    let mut rotations: Vec<(Entity, f32)> = Vec::new();
    for (e, (pos, wpn, fac, turret)) in world
        .query::<(&Position, &Weapon, &Faction, &Turret)>()
        .iter()
    {
        let mut best_pos: Option<Vec2> = None;
        let mut best_d = wpn.range * wpn.range;
        grid.for_neighbors(pos.0, wpn.range, |other, op| {
            if other == e { return; }
            let d2 = pos.0.distance_squared(op);
            if d2 > best_d { return; }
            let is_enemy = world.get::<&Faction>(other).map(|f| f.0 != fac.0).unwrap_or(false)
                && world.get::<&Health>(other).is_ok();
            if is_enemy { best_d = d2; best_pos = Some(op); }
        });
        let target_angle = if let Some(tp) = best_pos {
            let d = tp - pos.0;
            d.y.atan2(d.x)
        } else {
            turret.angle // no target: hold current
        };
        let diff = wrap_angle(target_angle - turret.angle);
        let max_turn = turret.turn_rate * dt;
        let new_angle = wrap_angle(turret.angle + diff.clamp(-max_turn, max_turn));
        rotations.push((e, new_angle));
    }
    for (e, angle) in rotations {
        if let Ok(t) = world.query_one_mut::<&mut Turret>(e) {
            t.angle = angle;
        }
    }
}

/// One combat tick: age tracers, decrement weapon cooldowns, fire discrete shots (one
/// tracer per shot — no continuous spray), remove the dead.
/// Units with a Turret only fire once the barrel is aimed within ~12° of the target.
pub fn step(world: &mut World, grid: &SpatialGrid, dt: f32, player_faction: &str) {
    // Age and retire old tracers.
    let mut expired: Vec<Entity> = Vec::new();
    for (e, t) in world.query::<&mut Tracer>().iter() {
        t.ttl -= dt;
        if t.ttl <= 0.0 { expired.push(e); }
    }
    for e in expired { let _ = world.despawn(e); }

    // Collect shooter info before mutating weapons/health.
    struct Shot { target: Entity, damage: f32, from: Vec2, to: Vec2, color: Color }
    let mut shots: Vec<Shot> = Vec::new();

    let friendly = Color::new(0.72, 0.88, 1.0, 1.0);
    let hostile  = Color::new(1.0, 0.65, 0.35, 1.0);

    // Step 1: find targets + check cooldowns (read-only borrows on world).
    let mut ready: Vec<(Entity, Entity, Vec2, Vec2, Color, f32)> = Vec::new(); // (shooter, target, from, to, color, dmg)
    for (e, (pos, wpn, fac)) in world.query::<(&Position, &Weapon, &Faction)>().iter() {
        if wpn.fire_rate <= 0.0 || wpn.cooldown > 0.0 { continue; }
        let mut best: Option<Entity> = None;
        let mut best_pos = Vec2::ZERO;
        let mut best_d = wpn.range * wpn.range;
        grid.for_neighbors(pos.0, wpn.range, |other, op| {
            if other == e { return; }
            let d2 = pos.0.distance_squared(op);
            if d2 >= best_d { return; }
            let is_enemy = world.get::<&Faction>(other).map(|f| f.0 != fac.0).unwrap_or(false)
                && world.get::<&Health>(other).is_ok();
            if is_enemy { best_d = d2; best = Some(other); best_pos = op; }
        });
        let Some(target) = best else { continue };
        // Turreted units must be aimed within ~12° before firing.
        let aimed = if let Ok(turret) = world.get::<&Turret>(e) {
            let desired = (best_pos - pos.0).y.atan2((best_pos - pos.0).x);
            wrap_angle(desired - turret.angle).abs() < 0.21
        } else { true };
        if !aimed { continue; }
        // Muzzle origin = turret tip (barrel length ≈ half sprite radius ahead).
        let muzzle = if let Ok(turret) = world.get::<&Turret>(e) {
            let barrel = 14.0; // world px from center to barrel tip
            pos.0 + Vec2::new(turret.angle.cos(), turret.angle.sin()) * barrel
        } else { pos.0 };
        let color = if fac.0 == player_faction { friendly } else { hostile };
        ready.push((e, target, muzzle, best_pos, color, wpn.damage));
    }

    // Step 2: apply cooldown resets and collect shot data.
    for (shooter, target, from, to, color, damage) in ready {
        if let Ok(wpn) = world.query_one_mut::<&mut Weapon>(shooter) {
            wpn.cooldown = 1.0 / wpn.fire_rate; // reset cooldown
        }
        shots.push(Shot { target, damage, from, to, color });
    }

    // Step 3: decrement all cooldowns.
    for (_e, wpn) in world.query::<&mut Weapon>().iter() {
        wpn.cooldown = (wpn.cooldown - dt).max(0.0);
    }

    // Step 4: apply damage + spawn one tracer per shot.
    for s in shots {
        if let Ok(h) = world.query_one_mut::<&mut Health>(s.target) {
            h.cur -= s.damage;
        }
        world.spawn((Tracer { from: s.from, to: s.to, color: s.color, ttl: TRACER_TTL },));
    }

    // Step 5: remove dead units.
    let dead: Vec<Entity> = world.query::<&Health>().iter()
        .filter(|(_, h)| h.cur <= 0.0).map(|(e, _)| e).collect();
    for e in dead { let _ = world.despawn(e); }
}

#[cfg(test)]
mod tests {
    use super::*;
    use macroquad::prelude::vec2;

    fn grid_for(world: &World) -> SpatialGrid {
        let mut g = SpatialGrid::new(vec2(256.0, 256.0), 24.0);
        g.rebuild(world);
        g
    }

    #[test]
    fn armed_unit_damages_then_kills_enemy() {
        let mut world = World::new();
        // fire_rate=1 shot/s, damage=10 per shot, cooldown=0 (fires immediately)
        let attacker = world.spawn((
            Position(vec2(0.0, 0.0)),
            Faction("a".into()),
            Weapon { range: 50.0, damage: 10.0, fire_rate: 1.0, cooldown: 0.0 },
            Health { cur: 100.0, max: 100.0 },
        ));
        let target = world.spawn((Position(vec2(20.0, 0.0)), Faction("b".into()), Health { cur: 25.0, max: 25.0 }));

        let grid = grid_for(&world);
        step(&mut world, &grid, 1.0, "a"); // fires 1 shot = 10 damage
        assert!((world.get::<&Health>(target).unwrap().cur - 15.0).abs() < 0.001);

        // Run enough ticks (each 1s) that cooldown resets and fires again
        for _ in 0..4 {
            let grid = grid_for(&world);
            step(&mut world, &grid, 1.0, "a");
        }
        assert!(world.get::<&Health>(target).is_err(), "target should have died and despawned");
        assert!(world.get::<&Health>(attacker).is_ok(), "attacker should be unharmed");
    }

    #[test]
    fn does_not_damage_same_faction() {
        let mut world = World::new();
        world.spawn((
            Position(vec2(0.0, 0.0)),
            Faction("a".into()),
            Weapon { range: 50.0, damage: 10.0, fire_rate: 1.0, cooldown: 0.0 },
            Health { cur: 100.0, max: 100.0 },
        ));
        let friend = world.spawn((Position(vec2(15.0, 0.0)), Faction("a".into()), Health { cur: 30.0, max: 30.0 }));

        let grid = grid_for(&world);
        step(&mut world, &grid, 1.0, "a");
        assert_eq!(world.get::<&Health>(friend).unwrap().cur, 30.0, "friendlies must not take fire");
    }

    #[test]
    fn one_tracer_spawned_per_shot() {
        let mut world = World::new();
        world.spawn((
            Position(vec2(0.0, 0.0)),
            Faction("a".into()),
            Weapon { range: 60.0, damage: 10.0, fire_rate: 2.0, cooldown: 0.0 },
            Health { cur: 100.0, max: 100.0 },
        ));
        world.spawn((Position(vec2(30.0, 0.0)), Faction("b".into()), Health { cur: 100.0, max: 100.0 }));

        let grid = grid_for(&world);
        step(&mut world, &grid, 0.05, "a"); // one tick at 20 Hz

        let tracers = world.query::<&Tracer>().iter().count();
        assert_eq!(tracers, 1, "one shot per tick = one tracer, got {}", tracers);
    }

    #[test]
    fn cooldown_prevents_immediate_refire() {
        let mut world = World::new();
        world.spawn((
            Position(vec2(0.0, 0.0)),
            Faction("a".into()),
            Weapon { range: 60.0, damage: 10.0, fire_rate: 1.0, cooldown: 0.0 },
            Health { cur: 100.0, max: 100.0 },
        ));
        world.spawn((Position(vec2(30.0, 0.0)), Faction("b".into()), Health { cur: 100.0, max: 100.0 }));

        // Tick 1: fires (cooldown=0 → resets to 1.0s)
        let grid = grid_for(&world);
        step(&mut world, &grid, 0.05, "a");
        // Tick 2 immediately after: cooldown > 0, should NOT fire again
        let grid = grid_for(&world);
        step(&mut world, &grid, 0.05, "a");
        // There should be 2 tracers total (tick1 tracer still alive, NOT a second shot)
        // Actually tick1 tracer TTL=0.20s, still alive. Tick2 should not produce a new one.
        // Count non-expired tracers (ttl > 0)
        let live_tracers = world.query::<&Tracer>().iter().filter(|(_, t)| t.ttl > 0.0).count();
        assert_eq!(live_tracers, 1, "cooldown should prevent second shot, got {} live tracers", live_tracers);
    }

    #[test]
    fn out_of_range_enemy_is_safe() {
        let mut world = World::new();
        world.spawn((
            Position(vec2(0.0, 0.0)),
            Faction("a".into()),
            Weapon { range: 30.0, damage: 10.0, fire_rate: 1.0, cooldown: 0.0 },
            Health { cur: 100.0, max: 100.0 },
        ));
        let far = world.spawn((Position(vec2(100.0, 0.0)), Faction("b".into()), Health { cur: 20.0, max: 20.0 }));

        let grid = grid_for(&world);
        step(&mut world, &grid, 1.0, "a");
        assert_eq!(world.get::<&Health>(far).unwrap().cur, 20.0, "out-of-range enemy must be untouched");
    }
}
