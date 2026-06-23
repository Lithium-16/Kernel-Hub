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
    pub sprite: usize,          // atlas fallback index
    pub tint: Color,
    pub size: f32,
    /// Hull sprite base name from definitions.ron (e.g. "hull_tank").
    /// Empty = use placeholder atlas sprite.
    pub hull_sprite: String,
    /// Turret sprite base name (e.g. "turret_tank"). Empty = no separate turret.
    pub turret_sprite: String,
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

/// Marker: entity cannot be selected by the player (supply trucks, AI units, etc.).
pub struct NonSelectable;

/// Marker: this engineer belongs to a group with auto-build enabled.
/// Set/cleared each frame by main.rs based on group.auto_build flag.
/// construction::step() treats these engineers as available for blueprint
/// claim even when they have an active MoveOrder.
pub struct AutoBuildMode;

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
    /// If true the unit keeps moving toward its goal even while engaging enemies (attack-move).
    /// If false (default formation move) the unit stops once engaged.
    pub attack_move: bool,
}

/// Vision radius for this unit (world pixels). Determines how much fog it reveals.
pub struct VisionRange(pub f32);

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

/// Onboard ammo for armed units. Gun cannot fire when shots == 0.
pub struct AmmoStorage {
    pub shots: u32,
    pub capacity: u32,
}

impl AmmoStorage {
    pub fn new(capacity: u32) -> Self {
        Self { shots: capacity, capacity }
    }
    pub fn is_full(&self) -> bool { self.shots >= self.capacity }
    pub fn free(&self) -> u32 { self.capacity.saturating_sub(self.shots) }
}

/// The building type this entity represents (e.g. "hq", "gun_turret", "depot").
/// Drives combat targeting (buildings are valid targets when this is present + Health).
pub struct BuildingKind(pub String);

/// A placed blueprint waiting to be constructed. Engineers claim this and advance
/// `progress` toward 1.0, withdrawing BuildingSupplies from the nearest depot along
/// the way. When progress reaches 1.0 the blueprint despawns and the real building spawns.
pub struct Blueprint {
    /// Which building definition to spawn on completion.
    pub building_id: String,
    /// Footprint tile origin (mirrors the co-located Building component).
    pub tx: usize,
    pub ty: usize,
    pub w: usize,
    pub h: usize,
    /// Construction progress 0.0 → 1.0.
    pub progress: f32,
    /// Total building supplies required to complete.
    pub required_supplies: u32,
    /// Building supplies already consumed.
    pub supplies_consumed: u32,
    /// Which faction owns this blueprint.
    pub faction: String,
    /// Seconds since last supply withdrawal (triggers withdrawal every SUPPLY_INTERVAL seconds).
    pub supply_timer: f32,
}

/// Marker placed on an Engineer entity to indicate it is actively working on a blueprint.
pub struct IsBuilding {
    pub blueprint: hecs::Entity,
}

/// Onboard fuel for vehicles. Engine stops when fuel <= 0.
pub struct FuelTank {
    pub fuel: f32,
    pub capacity: f32,
    pub burn_rate: f32,   // fuel units consumed per world-pixel travelled
}

impl FuelTank {
    pub fn new(capacity: f32) -> Self {
        Self { fuel: capacity, capacity, burn_rate: 0.05 }
    }
    pub fn is_full(&self) -> bool { self.fuel >= self.capacity }
    pub fn free(&self) -> f32 { (self.capacity - self.fuel).max(0.0) }
}
