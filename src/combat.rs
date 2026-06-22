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

/// One combat tick: age old tracers, acquire targets, apply damage, spawn tracers,
/// remove the dead. Units with a `Turret` only fire once aimed within the fire cone.
/// `player_faction` only picks the tracer color (friendly vs hostile).
pub fn step(world: &mut World, grid: &SpatialGrid, dt: f32, player_faction: &str) {
    // Age and retire tracers from previous ticks.
    let mut expired: Vec<Entity> = Vec::new();
    for (e, t) in world.query::<&mut Tracer>().iter() {
        t.ttl -= dt;
        if t.ttl <= 0.0 {
            expired.push(e);
        }
    }
    for e in expired {
        let _ = world.despawn(e);
    }

    let mut damage: Vec<(Entity, f32)> = Vec::new();
    // (from, to, color) for each shot fired this tick.
    let mut shots: Vec<(Vec2, Vec2, Color)> = Vec::new();
    let friendly = Color::new(0.55, 0.85, 1.0, 1.0);
    let hostile = Color::new(1.0, 0.6, 0.45, 1.0);

    for (e, (pos, wpn, fac)) in world.query::<(&Position, &Weapon, &Faction)>().iter() {
        if wpn.dps <= 0.0 {
            continue;
        }
        let mut best: Option<Entity> = None;
        let mut best_pos = Vec2::ZERO;
        let mut best_d = wpn.range * wpn.range;
        grid.for_neighbors(pos.0, wpn.range, |other, op| {
            if other == e {
                return;
            }
            let d2 = pos.0.distance_squared(op);
            if d2 > best_d {
                return;
            }
            // Enemy = has a Faction that differs from ours (no friendly fire), and is
            // damageable.
            let is_enemy = world.get::<&Faction>(other).map(|f| f.0 != fac.0).unwrap_or(false)
                && world.get::<&Health>(other).is_ok();
            if is_enemy {
                best_d = d2;
                best = Some(other);
                best_pos = op;
            }
        });
        if let Some(t) = best {
            // Units with a turret must be aimed within 12° before firing.
            let aimed = if let Ok(turret) = world.get::<&Turret>(e) {
                let desired = (best_pos - pos.0).y.atan2((best_pos - pos.0).x);
                wrap_angle(desired - turret.angle).abs() < 0.21 // ~12 degrees
            } else {
                true // no turret (infantry) = always ready
            };
            if aimed {
                damage.push((t, wpn.dps * dt));
                let color = if fac.0 == player_faction { friendly } else { hostile };
                shots.push((pos.0, best_pos, color));
            }
        }
    }

    for (t, amt) in damage {
        if let Ok(h) = world.query_one_mut::<&mut Health>(t) {
            h.cur -= amt;
        }
    }

    for (from, to, color) in shots {
        world.spawn((Tracer { from, to, color, ttl: TRACER_TTL },));
    }

    let dead: Vec<Entity> = world
        .query::<&Health>()
        .iter()
        .filter(|(_, h)| h.cur <= 0.0)
        .map(|(e, _)| e)
        .collect();
    for e in dead {
        let _ = world.despawn(e);
    }
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
        let attacker = world.spawn((
            Position(vec2(0.0, 0.0)),
            Faction("a".into()),
            Weapon { range: 50.0, dps: 10.0 },
            Health { cur: 100.0, max: 100.0 },
        ));
        let target = world.spawn((Position(vec2(20.0, 0.0)), Faction("b".into()), Health { cur: 25.0, max: 25.0 }));

        let grid = grid_for(&world);
        step(&mut world, &grid, 1.0, "a"); // 1s * 10 dps = 10 damage
        assert!((world.get::<&Health>(target).unwrap().cur - 15.0).abs() < 0.001);

        for _ in 0..5 {
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
            Weapon { range: 50.0, dps: 10.0 },
            Health { cur: 100.0, max: 100.0 },
        ));
        let friend = world.spawn((Position(vec2(15.0, 0.0)), Faction("a".into()), Health { cur: 30.0, max: 30.0 }));

        let grid = grid_for(&world);
        step(&mut world, &grid, 1.0, "a");
        assert_eq!(world.get::<&Health>(friend).unwrap().cur, 30.0, "friendlies must not take fire");
    }

    #[test]
    fn out_of_range_enemy_is_safe() {
        let mut world = World::new();
        world.spawn((
            Position(vec2(0.0, 0.0)),
            Faction("a".into()),
            Weapon { range: 30.0, dps: 10.0 },
            Health { cur: 100.0, max: 100.0 },
        ));
        let far = world.spawn((Position(vec2(100.0, 0.0)), Faction("b".into()), Health { cur: 20.0, max: 20.0 }));

        let grid = grid_for(&world);
        step(&mut world, &grid, 1.0, "a");
        assert_eq!(world.get::<&Health>(far).unwrap().cur, 20.0, "out-of-range enemy must be untouched");
    }
}
