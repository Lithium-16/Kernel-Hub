//! Combat (Phase 3 core). Direct-fire: each armed unit damages the nearest *enemy*
//! (different faction) within its weapon range every tick — hitscan, no projectiles yet.
//! Units at 0 HP despawn. Stance-driven movement and projectiles come later; this is
//! enough to make two armies actually fight.

use hecs::{Entity, World};

use crate::components::{Faction, Health, Position, Weapon};
use crate::spatial::SpatialGrid;

/// One combat tick: acquire targets, apply damage, remove the dead.
pub fn step(world: &mut World, grid: &SpatialGrid, dt: f32) {
    let mut damage: Vec<(Entity, f32)> = Vec::new();

    for (e, (pos, wpn, fac)) in world.query::<(&Position, &Weapon, &Faction)>().iter() {
        if wpn.dps <= 0.0 {
            continue;
        }
        let mut best: Option<Entity> = None;
        let mut best_d = wpn.range * wpn.range;
        grid.for_neighbors(pos.0, wpn.range, |other, op| {
            if other == e {
                return;
            }
            let d2 = pos.0.distance_squared(op);
            if d2 > best_d {
                return;
            }
            // Enemy = has a Faction that differs from ours, and is damageable.
            let is_enemy = world.get::<&Faction>(other).map(|f| f.0 != fac.0).unwrap_or(false)
                && world.get::<&Health>(other).is_ok();
            if is_enemy {
                best_d = d2;
                best = Some(other);
            }
        });
        if let Some(t) = best {
            damage.push((t, wpn.dps * dt));
        }
    }

    for (t, amt) in damage {
        if let Ok(h) = world.query_one_mut::<&mut Health>(t) {
            h.cur -= amt;
        }
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
        step(&mut world, &grid, 1.0); // 1s * 10 dps = 10 damage
        assert!((world.get::<&Health>(target).unwrap().cur - 15.0).abs() < 0.001);

        for _ in 0..5 {
            let grid = grid_for(&world);
            step(&mut world, &grid, 1.0);
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
        step(&mut world, &grid, 1.0);
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
        step(&mut world, &grid, 1.0);
        assert_eq!(world.get::<&Health>(far).unwrap().cur, 20.0, "out-of-range enemy must be untouched");
    }
}
