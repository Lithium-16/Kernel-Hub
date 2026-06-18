//! ECS components (plain data). Grows each phase.

use macroquad::prelude::{Color, Vec2};

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

/// Marker: entity is following the active flow field toward the order goal.
pub struct Moving;
