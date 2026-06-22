//! ECS components (plain data). Grows each phase.

use std::collections::VecDeque;
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

/// A placed building: footprint tile origin `(tx, ty)` and size in tiles, plus a draw
/// color. Its tiles are marked impassable in the nav grid when placed.
pub struct Building {
    pub tx: usize,
    pub ty: usize,
    pub w: usize,
    pub h: usize,
    pub color: Color,
}

/// The unit type this entity was spawned from (data-definition id + display name).
/// Drives the selection readout now and the command card / production later.
pub struct UnitKind {
    pub id: String,
    pub name: String,
}

/// Hit points. Entities die (despawn) when `cur` reaches 0.
pub struct Health {
    pub cur: f32,
    pub max: f32,
}

/// A direct-fire weapon. Fires discrete shots at `fire_rate` shots/sec, each dealing
/// `damage` HP. `cooldown` counts down between shots (starts at 0 so first shot fires
/// immediately when a target enters range). One tracer per shot = no line clutter.
pub struct Weapon {
    pub range: f32,
    pub damage: f32,
    pub fire_rate: f32, // shots per second
    pub cooldown: f32,  // seconds until next shot (mutable, decremented each tick)
}

/// A rotating turret (vehicles). `angle` is the current barrel direction (radians); the
/// unit only fires once the turret has swung to within the fire arc of its target. Units
/// without a turret (e.g. infantry) fire as soon as a target is in range.
pub struct Turret {
    pub angle: f32,
    pub turn_rate: f32,
}

/// A short-lived shot tracer (shooter→target), colored by the shooter's side and faded
/// over `ttl`. Purely visual; carries no Position/Faction so other systems ignore it.
pub struct Tracer {
    pub from: Vec2,
    pub to: Vec2,
    pub color: Color,
    pub ttl: f32,
}

/// Bullet flight time in seconds (shooter→target); also used for trail/progress.
pub const TRACER_TTL: f32 = 0.20;

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

/// A unit's fixed offset from the group's formation anchor. Held across a whole order
/// chain so the formation shape translates along queued waypoints (each leg's goal is
/// `anchor + offset`).
pub struct Formation {
    pub offset: Vec2,
}

/// Pending move waypoints (group anchor points). When a unit's current `MoveOrder`
/// completes and this queue is non-empty, the next anchor becomes the next leg. Built
/// by Shift+right-click.
pub struct OrderQueue {
    pub anchors: VecDeque<Vec2>,
}
