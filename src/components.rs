//! ECS components (plain data). Grows each phase.

use std::sync::Arc;

use macroquad::prelude::{Color, Vec2};

use crate::nav::FlowField;

/// World-space position (in world pixels).
pub struct Position(pub Vec2);

/// Current velocity (world px/s) — used for smoothing and facing.
pub struct Velocity(pub Vec2);

/// Facing angle in radians; turned toward the move direction at `Mobility.turn_rate`.
pub struct Heading(pub f32);

/// Per-unit mobility: max speed (world px/s) and turn rate (rad/s). Slow turn rate =
/// the unit pivots toward its destination before driving off (tanks).
pub struct Mobility {
    pub speed: f32,
    pub turn_rate: f32,
}

/// How an entity draws: a sprite index (into `Sprites`), a tint, and a world size.
pub struct Renderable {
    pub sprite: usize,
    pub tint: Color,
    pub size: f32,
}

/// Which faction an entity belongs to (faction id from the data definitions).
pub struct Faction(pub String);

/// Per-unit arrival bookkeeping: last tick's position + a stall counter (consecutive
/// near-goal ticks with little real progress → the unit has effectively arrived).
pub struct MoveState {
    pub last: Vec2,
    pub stall: u8,
}

/// Marker: entity is currently selected by the player.
pub struct Selected;

/// A per-unit move order. The shared `flow` field routes the unit to the formation
/// `anchor` (the click point); once within `seek` distance of the anchor it heads
/// straight for its own `goal` slot, so the group fans into a block instead of all
/// piling onto one point. `arrive` is the stall-window radius around the slot. Each
/// unit carries its own order (the `Arc` shares one field across a group), so issuing
/// a new order to other units never hijacks this one.
pub struct MoveOrder {
    pub flow: Arc<FlowField>,
    pub goal: Vec2,
    pub anchor: Vec2,
    pub seek: f32,
    pub arrive: f32,
}
