//! Movement: per-unit flow-field steering with gentle local avoidance + velocity
//! smoothing + facing, plus a positional collision pass so units occupy space.

use std::collections::HashSet;

use hecs::{Entity, World};
use macroquad::prelude::*;

use crate::components::{Heading, MoveOrder, Position, Velocity};
use crate::spatial::SpatialGrid;

pub const SPEED: f32 = 72.0;
/// Collision radius per unit (uniform for now).
pub const UNIT_RADIUS: f32 = 10.0;
const COLLISION_DIAM: f32 = UNIT_RADIUS * 2.0;
/// Velocity smoothing rate (higher = snappier).
const ACCEL: f32 = 9.0;
/// Local-avoidance look radius and strength (steer around neighbors).
const AVOID_R: f32 = 28.0;
const AVOID_STRENGTH: f32 = 45.0;

/// Steer units that have a `MoveOrder` along their own flow field, with gentle
/// avoidance so they route around each other; stop on arrival.
pub fn step(world: &mut World, grid: &SpatialGrid, map_px: Vec2, dt: f32) {
    let mut arrived = Vec::new();

    for (e, (pos, vel, head, order)) in
        world.query::<(&mut Position, &mut Velocity, &mut Heading, &MoveOrder)>().iter()
    {
        let mut steer = order.flow.dir_at(pos.0) * SPEED;

        // Gentle separation so units flow around neighbors instead of into them.
        let mut sep = Vec2::ZERO;
        grid.for_neighbors(pos.0, AVOID_R, |other, op| {
            if other == e {
                return;
            }
            let d = pos.0 - op;
            let dist = d.length();
            if dist > 0.001 && dist < AVOID_R {
                sep += d / dist * ((AVOID_R - dist) / AVOID_R);
            }
        });
        steer += sep * AVOID_STRENGTH;

        let s = steer.length();
        if s > SPEED {
            steer = steer / s * SPEED;
        }

        vel.0 += (steer - vel.0) * (ACCEL * dt).min(1.0);
        if vel.0.length_squared() > 4.0 {
            head.0 = vel.0.y.atan2(vel.0.x);
        }

        pos.0 = (pos.0 + vel.0 * dt).clamp(Vec2::ZERO, map_px);
        if pos.0.distance(order.goal) < order.arrive {
            arrived.push(e);
        }
    }

    for e in arrived {
        let _ = world.remove_one::<MoveOrder>(e);
        if let Ok(v) = world.query_one_mut::<&mut Velocity>(e) {
            v.0 = Vec2::ZERO;
        }
    }
}

/// Keep unit centers at least `COLLISION_DIAM` apart. Moving units yield to idle
/// ones (idle hold their ground) so groups don't bulldoze bystanders.
pub fn resolve_collisions(world: &mut World, grid: &mut SpatialGrid, map_px: Vec2, iters: u32) {
    for _ in 0..iters {
        grid.rebuild(world);
        let moving: HashSet<Entity> = world.query::<&MoveOrder>().iter().map(|(e, _)| e).collect();

        let mut corrections: Vec<(Entity, Vec2)> = Vec::new();
        for (e, pos) in world.query::<&Position>().iter() {
            let self_moving = moving.contains(&e);
            let mut push = Vec2::ZERO;
            grid.for_neighbors(pos.0, COLLISION_DIAM, |other, op| {
                if other == e {
                    return;
                }
                let d = pos.0 - op;
                let dist = d.length();
                if dist > 0.0001 && dist < COLLISION_DIAM {
                    let w = match (self_moving, moving.contains(&other)) {
                        (false, true) => 0.0, // idle holds against a mover
                        (true, false) => 1.0, // mover steps fully around idle
                        _ => 0.5,
                    };
                    push += d / dist * (COLLISION_DIAM - dist) * w;
                }
            });
            if push != Vec2::ZERO {
                corrections.push((e, push));
            }
        }

        for (e, push) in corrections {
            if let Ok(p) = world.query_one_mut::<&mut Position>(e) {
                p.0 = (p.0 + push).clamp(Vec2::ZERO, map_px);
            }
        }
    }
}
