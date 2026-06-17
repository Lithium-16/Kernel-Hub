//! ECS components (plain data). Phase 1 starter set; grows each phase.

use macroquad::prelude::{Color, Vec2};

/// World-space position (in world pixels).
pub struct Position(pub Vec2);

/// How an entity draws (placeholder primitive until sprites exist).
pub struct Renderable {
    pub color: Color,
    pub radius: f32,
}

/// Which faction an entity belongs to (faction id from the data definitions).
pub struct Faction(pub String);
