//! ECS components (plain data). Grows each phase.

use std::sync::Arc;

use macroquad::prelude::{Color, Vec2};

use crate::nav::FlowField;

/// World-space position (in world pixels).
pub struct Position(pub Vec2);

/// Current velocity (world px/s) — used for smoothing and facing.
pub struct Velocity(pub Vec2);

/// Facing angle in radians (velocity-derived); persists when idle.
pub struct Heading(pub f32);

/// How an entity draws: a sprite index (into `Sprites`), a tint, and a world size.
pub struct Renderable {
    pub sprite: usize,
    pub tint: Color,
    pub size: f32,
}

/// Which faction an entity belongs to (faction id from the data definitions).
pub struct Faction(pub String);

/// Marker: entity is currently selected by the player.
pub struct Selected;

/// A per-unit move order: the flow field to follow, the goal, and the arrival
/// radius. Each unit carries its own (the `Arc` shares one field across a group),
/// so issuing a new order to other units never hijacks this one.
pub struct MoveOrder {
    pub flow: Arc<FlowField>,
    pub goal: Vec2,
    pub arrive: f32,
}
