//! Movement: per-unit flow-field steering with gentle local avoidance + velocity
//! smoothing + facing, plus a positional collision pass. All position changes are
//! filtered through the nav grid so units can't be pushed onto impassable terrain.

use std::collections::HashSet;

use hecs::{Entity, World};
use macroquad::prelude::*;

use crate::components::{Heading, Mobility, MoveOrder, MoveState, Position, Velocity};
use crate::map::TILE_SIZE;
use crate::nav::NavGrid;
use crate::spatial::SpatialGrid;

/// Collision radius per unit (uniform for now). Sized close to the sprite half-width
/// so units don't visually clip into each other when packed.
pub const UNIT_RADIUS: f32 = 14.0;
const COLLISION_DIAM: f32 = UNIT_RADIUS * 2.0;
/// Speed smoothing rate (higher = snappier accel/decel).
const ACCEL: f32 = 9.0;
/// Local-avoidance look radius; `sep` pushes apart, `around` steers past units ahead.
const AVOID_R: f32 = 34.0;
const SEP_WEIGHT: f32 = 0.9;
const AROUND_WEIGHT: f32 = 1.3;
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

fn wrap_angle(a: f32) -> f32 {
    use std::f32::consts::{PI, TAU};
    let mut a = a % TAU;
    if a > PI {
        a -= TAU;
    } else if a < -PI {
        a += TAU;
    }
    a
}

/// Steer units with a `MoveOrder`: pick a desired direction (flow + local avoidance),
/// turn the hull toward it at the unit's turn rate, then drive forward scaled by how
/// aligned the hull is — so slow-turning units (tanks) pivot before moving. Arrival is
/// handled by `settle_arrivals`.
pub fn step(world: &mut World, grid: &SpatialGrid, nav: &NavGrid, map_px: Vec2, dt: f32) {
    for (e, (pos, vel, head, state, mob, order)) in world
        .query::<(&mut Position, &mut Velocity, &mut Heading, &mut MoveState, &Mobility, &MoveOrder)>()
        .iter()
    {
        state.last = pos.0; // pre-move position; settle_arrivals compares against it
        let facing = vec2(head.0.cos(), head.0.sin());

        // Base direction: follow the shared flow field until we reach the formation
        // anchor's neighbourhood, then seek our OWN slot directly. The switch is keyed on
        // distance to the anchor (sized to the formation in `issue_move`), not to the
        // slot — otherwise outer-slot units funnel into the central pile and never get
        // close enough to their slot to break away. Distinct slots → the group fans into
        // a block instead of crushing one point → no packed-group jitter.
        let to_goal = order.goal - pos.0;
        let dist_goal = to_goal.length();
        let near_formation = pos.0.distance(order.anchor) < order.seek;
        let base_dir = if near_formation && dist_goal > 0.001 {
            to_goal / dist_goal
        } else {
            order.flow.dir_at(pos.0)
        };
        let mut sep = Vec2::ZERO;
        let mut around = Vec2::ZERO;
        grid.for_neighbors(pos.0, AVOID_R, |other, op| {
            if other == e {
                return;
            }
            let d = pos.0 - op;
            let dist = d.length();
            if dist > 0.001 && dist < AVOID_R {
                let w = (AVOID_R - dist) / AVOID_R;
                sep += d / dist * w;
                // If the neighbour is ahead of us, steer sideways to go around it.
                let to_other = -d / dist;
                let ahead = facing.dot(to_other);
                if ahead > 0.2 {
                    let perp = vec2(-facing.y, facing.x);
                    let side = if perp.dot(to_other) > 0.0 { -1.0 } else { 1.0 };
                    around += perp * (side * w * ahead);
                }
            }
        });
        let desired = base_dir + sep * SEP_WEIGHT + around * AROUND_WEIGHT;
        let desired_dir = if desired.length_squared() > 1e-4 {
            desired.normalize()
        } else {
            facing
        };

        // Turn toward the desired direction at the unit's turn rate (tanks pivot first).
        let target_angle = desired_dir.y.atan2(desired_dir.x);
        let diff = wrap_angle(target_angle - head.0);
        let max_turn = mob.turn_rate * dt;
        head.0 = wrap_angle(head.0 + diff.clamp(-max_turn, max_turn));

        // Drive forward along the (new) facing, scaled by alignment → turn-then-move.
        let new_facing = vec2(head.0.cos(), head.0.sin());
        let align = new_facing.dot(desired_dir).max(0.0);
        let target_speed = mob.speed * align * align;
        let cur = vel.0.length();
        let speed = cur + (target_speed - cur) * (ACCEL * dt).min(1.0);
        vel.0 = new_facing * speed;

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
