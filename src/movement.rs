//! Movement system: units following the active flow field, with local avoidance
//! (separation) so they don't stack. Reads neighbor positions from the spatial grid
//! snapshot, so it can mutate positions without aliasing.

use hecs::World;
use macroquad::prelude::*;

use crate::components::{Moving, Position};
use crate::nav::FlowField;
use crate::spatial::SpatialGrid;

pub const SPEED: f32 = 72.0;
const NEIGHBOR_R: f32 = 18.0;
const SEP_WEIGHT: f32 = 1.4;
const ARRIVE: f32 = 22.0;

/// Advance all `Moving` entities one tick. Entities that reach the goal stop.
pub fn step(world: &mut World, grid: &SpatialGrid, flow: &FlowField, goal: Vec2, map_px: Vec2, dt: f32) {
    let mut arrived = Vec::new();

    for (e, (pos, _)) in world.query::<(&mut Position, &Moving)>().iter() {
        let p = pos.0;
        let mut v = flow.dir_at(p) * SPEED;

        // Local avoidance: push away from nearby units.
        let mut sep = Vec2::ZERO;
        grid.for_neighbors(p, NEIGHBOR_R, |other, op| {
            if other == e {
                return;
            }
            let delta = p - op;
            let dist = delta.length();
            if dist > 0.001 && dist < NEIGHBOR_R {
                sep += delta / dist * (NEIGHBOR_R - dist);
            }
        });
        v += sep * SEP_WEIGHT;

        let speed = v.length();
        if speed > SPEED {
            v = v / speed * SPEED;
        }

        let np = (p + v * dt).clamp(Vec2::ZERO, map_px);
        pos.0 = np;
        if np.distance(goal) < ARRIVE {
            arrived.push(e);
        }
    }

    for e in arrived {
        let _ = world.remove_one::<Moving>(e);
    }
}
