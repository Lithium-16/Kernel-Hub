//! Movement: per-unit flow-field steering with gentle local avoidance + velocity
//! smoothing + facing, plus a positional collision pass. All position changes are
//! filtered through the nav grid so units can't be pushed onto impassable terrain.

use std::collections::HashSet;

use hecs::{Entity, World};
use macroquad::prelude::*;

use crate::components::{Heading, MoveOrder, MoveState, Position, Velocity};
use crate::map::TILE_SIZE;
use crate::nav::NavGrid;
use crate::spatial::SpatialGrid;

pub const SPEED: f32 = 72.0;
/// Collision radius per unit (uniform for now). Sized close to the sprite half-width
/// so units don't visually clip into each other when packed.
pub const UNIT_RADIUS: f32 = 14.0;
const COLLISION_DIAM: f32 = UNIT_RADIUS * 2.0;
/// Velocity smoothing rate (higher = snappier).
const ACCEL: f32 = 9.0;
/// Local-avoidance look radius and strength (steer around neighbors).
const AVOID_R: f32 = 28.0;
const AVOID_STRENGTH: f32 = 45.0;
/// Overlap below this is tolerated (stops dense crowds from buzzing).
const COLLISION_SLOP: f32 = 1.5;
/// Max positional correction applied to a unit per tick.
const MAX_PUSH: f32 = 5.0;

fn passable(nav: &NavGrid, p: Vec2) -> bool {
    let x = (p.x / TILE_SIZE) as i32;
    let y = (p.y / TILE_SIZE) as i32;
    x >= 0 && y >= 0 && (x as usize) < nav.w && (y as usize) < nav.h && nav.passable(x as usize, y as usize)
}

/// Move from `old` toward `new`, but never onto an impassable tile — slide along
/// one axis if the diagonal is blocked, else stay put.
fn try_move(nav: &NavGrid, old: Vec2, new: Vec2) -> Vec2 {
    if !passable(nav, old) {
        // Already off the mesh (shouldn't happen): allow any move to escape.
        return new;
    }
    if passable(nav, new) {
        return new;
    }
    let slide_x = vec2(new.x, old.y);
    if passable(nav, slide_x) {
        return slide_x;
    }
    let slide_y = vec2(old.x, new.y);
    if passable(nav, slide_y) {
        return slide_y;
    }
    old
}

/// Steer units with a `MoveOrder` along their flow field, with local avoidance.
/// (Arrival/settling is handled separately by `settle_arrivals`.)
pub fn step(world: &mut World, grid: &SpatialGrid, nav: &NavGrid, map_px: Vec2, dt: f32) {
    for (e, (pos, vel, head, state, order)) in
        world.query::<(&mut Position, &mut Velocity, &mut Heading, &mut MoveState, &MoveOrder)>().iter()
    {
        state.last = pos.0; // record pre-move position; settle_arrivals compares against it
        let mut steer = order.flow.dir_at(pos.0) * SPEED;

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

        let target = (pos.0 + vel.0 * dt).clamp(Vec2::ZERO, map_px);
        pos.0 = try_move(nav, pos.0, target);
    }
}

/// Settle move orders so packed groups don't jitter. A unit "arrives" (drops its order
/// and stops) when it reaches the goal core, OR when it's near the goal and makes almost
/// no real progress for several ticks (it's blocked by the crowd). Stall detection kills
/// the back-of-group shaking — a stuck unit stops pushing instead of oscillating.
/// Call after the collision pass.
const ARRIVE_CORE: f32 = UNIT_RADIUS * 1.6;
const MIN_PROGRESS: f32 = 0.6; // px/tick below which a near-goal unit counts as stalled
const STALL_TICKS: u8 = 3;

pub fn settle_arrivals(world: &mut World) {
    let mut arrived = Vec::new();
    for (e, (pos, state, order)) in world.query::<(&Position, &mut MoveState, &MoveOrder)>().iter() {
        let d = pos.0.distance(order.goal);
        if d < ARRIVE_CORE {
            arrived.push(e);
        } else if d < order.arrive {
            if pos.0.distance(state.last) < MIN_PROGRESS {
                state.stall = state.stall.saturating_add(1);
                if state.stall >= STALL_TICKS {
                    arrived.push(e);
                }
            } else {
                state.stall = 0;
            }
        } else {
            state.stall = 0;
        }
    }

    for e in arrived {
        let _ = world.remove_one::<MoveOrder>(e);
        if let Ok(v) = world.query_one_mut::<&mut Velocity>(e) {
            v.0 = Vec2::ZERO;
        }
    }
}

/// Keep unit centers apart. Moving units yield to idle ones; pushes never move a
/// unit onto impassable terrain.
pub fn resolve_collisions(world: &mut World, grid: &SpatialGrid, nav: &NavGrid, map_px: Vec2, passes: u32) {
    let moving: HashSet<Entity> = world.query::<&MoveOrder>().iter().map(|(e, _)| e).collect();
    for _ in 0..passes {
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
                let overlap = COLLISION_DIAM - dist;
                if dist > 0.0001 && overlap > COLLISION_SLOP {
                    let w = match (self_moving, moving.contains(&other)) {
                        (false, true) => 0.0,
                        (true, false) => 1.0,
                        _ => 0.5,
                    };
                    push += d / dist * (overlap - COLLISION_SLOP) * w;
                }
            });
            if push != Vec2::ZERO {
                corrections.push((e, push.clamp_length_max(MAX_PUSH)));
            }
        }

        for (e, push) in corrections {
            if let Ok(p) = world.query_one_mut::<&mut Position>(e) {
                let target = (p.0 + push).clamp(Vec2::ZERO, map_px);
                p.0 = try_move(nav, p.0, target);
            }
        }
    }
}
