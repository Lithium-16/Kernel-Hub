//! Movement: flow-field steering with velocity smoothing + facing, plus positional
//! collision resolution so units occupy space and don't overlap.

use hecs::{Entity, World};
use macroquad::prelude::*;

use crate::components::{Heading, Moving, Position, Velocity};
use crate::nav::FlowField;
use crate::spatial::SpatialGrid;

pub const SPEED: f32 = 72.0;
/// Collision radius per unit (uniform for now).
pub const UNIT_RADIUS: f32 = 10.0;
const COLLISION_DIAM: f32 = UNIT_RADIUS * 2.0;
/// Velocity smoothing rate (higher = snappier).
const ACCEL: f32 = 9.0;

/// Steer `Moving` units along the flow field (smoothed), update facing, and stop
/// units once they enter the order's arrival disk.
pub fn step(world: &mut World, flow: &FlowField, goal: Vec2, arrive_radius: f32, map_px: Vec2, dt: f32) {
    let mut arrived = Vec::new();

    for (e, (pos, vel, head, _)) in
        world.query::<(&mut Position, &mut Velocity, &mut Heading, &Moving)>().iter()
    {
        let desired = flow.dir_at(pos.0) * SPEED;
        vel.0 += (desired - vel.0) * (ACCEL * dt).min(1.0);

        // Face the direction of travel (only when actually moving).
        if vel.0.length_squared() > 4.0 {
            head.0 = vel.0.y.atan2(vel.0.x);
        }

        pos.0 = (pos.0 + vel.0 * dt).clamp(Vec2::ZERO, map_px);
        if pos.0.distance(goal) < arrive_radius {
            arrived.push(e);
        }
    }

    for e in arrived {
        let _ = world.remove_one::<Moving>(e);
        if let Ok(v) = world.query_one_mut::<&mut Velocity>(e) {
            v.0 = Vec2::ZERO;
        }
    }
}

/// Keep all unit centers at least `COLLISION_DIAM` apart (positional correction).
/// Applies to every unit so idle ones get shoved aside by moving groups.
pub fn resolve_collisions(world: &mut World, grid: &mut SpatialGrid, map_px: Vec2, iters: u32) {
    for _ in 0..iters {
        grid.rebuild(world);

        let mut corrections: Vec<(Entity, Vec2)> = Vec::new();
        for (e, pos) in world.query::<&Position>().iter() {
            let mut push = Vec2::ZERO;
            grid.for_neighbors(pos.0, COLLISION_DIAM, |other, op| {
                if other == e {
                    return;
                }
                let d = pos.0 - op;
                let dist = d.length();
                if dist > 0.0001 && dist < COLLISION_DIAM {
                    push += d / dist * (COLLISION_DIAM - dist) * 0.5;
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
