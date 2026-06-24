//! Supply truck entities and their tick system.
//!
//! Trucks are physical units that ferry resources between depots.  They can be
//! attacked and destroyed; when that happens the cargo is lost.  The caller
//! (route registry / main loop) is responsible for acting on the returned
//! [`TruckEvent`]s — transferring stock into the destination depot on delivery,
//! and despawning the entity on `Returned` or `Destroyed`.

use std::f32::consts::PI;

use hecs::{Entity, World};
use macroquad::prelude::*;

use crate::assets::Sprites;
use crate::components::{
    Faction, FuelTank, Health, Heading, LoadingZone, Mobility, MoveState, NonSelectable, Position,
    Renderable, UnitKind, Velocity,
};
use crate::depot::ResourceType;

// ── Constants ─────────────────────────────────────────────────────────────────

pub const TRUCK_SPEED: f32 = 90.0;
pub const TRUCK_TURN_RATE: f32 = 4.0;
pub const TRUCK_RADIUS: f32 = 11.0;
/// Distance (world px) at which a truck counts as arrived.
/// Uses LoadingZone position if available; 200px covers the largest buildings.
pub const TRUCK_ARRIVAL_RANGE: f32 = 200.0;

// ── Component ─────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TruckState {
    /// Driving loaded to the destination depot.
    DrivingToDestination,
    /// Empty return trip back to the origin depot; despawn on arrival.
    DrivingBack,
}

/// ECS component attached to every supply truck entity.
pub struct Truck {
    pub route_id: u32,
    pub cargo_resource: ResourceType,
    /// How many units of `cargo_resource` are being carried (zero on the return leg).
    pub cargo_amount: u32,
    /// The depot entity to deliver to.
    pub destination: Entity,
    /// The depot entity to return to (and then despawn).
    pub origin: Entity,
    pub state: TruckState,
}

// ── Spawn helper ──────────────────────────────────────────────────────────────

/// Spawn a truck entity at `spawn_pos`.
///
/// The truck starts with full fuel and is ready to move.  The caller must
/// issue a [`crate::components::MoveOrder`] (e.g. via `issue_move`) after
/// calling this function so the truck actually drives to its destination.
///
/// Returns the new entity handle.
#[allow(clippy::too_many_arguments)]
pub fn spawn_truck(
    world: &mut World,
    sprites: &Sprites,
    route_id: u32,
    cargo_resource: ResourceType,
    cargo_amount: u32,
    origin: Entity,
    destination: Entity,
    spawn_pos: Vec2,
    faction: &str,
    tint: Color,
) -> Entity {
    world.spawn((
        Position(spawn_pos),
        Velocity(Vec2::ZERO),
        Heading(-PI / 2.0),
        MoveState {
            last: spawn_pos,
            stall: 0,
        },
        Mobility {
            speed: TRUCK_SPEED,
            turn_rate: TRUCK_TURN_RATE,
        },
        Renderable {
            sprite: sprites.unit_index("truck"),
            tint,
            size: TRUCK_RADIUS * 2.6,
            hull_sprite: "hull_truck".into(),
            turret_sprite: "".into(),
        },
        Faction(faction.to_string()),
        Health { cur: 70.0, max: 70.0 },
        UnitKind {
            id: "truck".into(),
            name: "Supply Truck".into(),
        },
        FuelTank {
            fuel: 500.0,
            capacity: 500.0,
            burn_rate: 0.04,
        },
        Truck {
            route_id,
            cargo_resource,
            cargo_amount,
            destination,
            origin,
            state: TruckState::DrivingToDestination,
        },
        NonSelectable,  // trucks are AI-driven; player cannot select them
    ))
}

// ── Events ────────────────────────────────────────────────────────────────────

/// Events emitted by [`step`].  The caller must handle all mutations that
/// follow from each event (depot transfers, entity despawns, route bookkeeping).
pub enum TruckEvent {
    /// Truck reached its destination and is ready to unload.
    Delivered {
        route_id: u32,
        resource: ResourceType,
        amount: u32,
        dest: Entity,
    },
    /// Truck returned to the origin depot and should be despawned.
    Returned { route_id: u32 },
    /// Truck was destroyed in combat; cargo is lost.
    Destroyed { route_id: u32 },
}

// ── System ────────────────────────────────────────────────────────────────────

/// Run truck arrival and death checks for one simulation tick.
///
/// Two-pass design: events are collected first (read-only world queries), then
/// the caller applies all resulting mutations so this function never aliases
/// borrows.
///
/// **Caller responsibilities per event:**
/// - `Delivered` → call `dest_depot.add(resource, amount)`, remove the truck's
///   `MoveOrder`, then issue a new `MoveOrder` back to the origin depot.
/// - `Returned`  → `world.despawn(entity)`.
/// - `Destroyed` → `world.despawn(entity)` (cargo is already lost).
pub fn step(world: &mut World) -> Vec<(Entity, TruckEvent)> {
    // ── Pass 1: read-only — gather position snapshots and truck states ────────

    // Collect depot positions and their LoadingZone overrides up front.
    let depot_positions: std::collections::HashMap<Entity, Vec2> = world
        .query::<&Position>()
        .iter()
        .map(|(e, pos)| (e, pos.0))
        .collect();
    // LoadingZone overrides: if a depot building has one, use that for arrival checks.
    let loading_zones: std::collections::HashMap<Entity, Vec2> = world
        .query::<&LoadingZone>()
        .iter()
        .map(|(e, lz)| (e, lz.world_pos))
        .collect();

    // ── Pass 2: iterate trucks, emit events ──────────────────────────────────

    let mut events: Vec<(Entity, TruckEvent)> = Vec::new();
    let mut to_remove_move_order: Vec<Entity> = Vec::new();

    for (entity, (truck, pos, health)) in
        world.query::<(&Truck, &Position, &Health)>().iter()
    {
        // ── Death check ───────────────────────────────────────────────────
        if health.cur <= 0.0 {
            events.push((entity, TruckEvent::Destroyed { route_id: truck.route_id }));
            continue;
        }

        match truck.state {
            TruckState::DrivingToDestination => {
                // Prefer LoadingZone (exact stop point) over raw depot centre.
                let dest_check = loading_zones.get(&truck.destination)
                    .or_else(|| depot_positions.get(&truck.destination)).copied();
                if let Some(dest_pos) = dest_check {
                    if pos.0.distance(dest_pos) < TRUCK_ARRIVAL_RANGE {
                        events.push((
                            entity,
                            TruckEvent::Delivered {
                                route_id: truck.route_id,
                                resource: truck.cargo_resource,
                                amount: truck.cargo_amount,
                                dest: truck.destination,
                            },
                        ));
                        to_remove_move_order.push(entity);
                    }
                }
            }

            TruckState::DrivingBack => {
                let origin_check = loading_zones.get(&truck.origin)
                    .or_else(|| depot_positions.get(&truck.origin)).copied();
                if let Some(origin_pos) = origin_check {
                    if pos.0.distance(origin_pos) < TRUCK_ARRIVAL_RANGE {
                        events.push((entity, TruckEvent::Returned { route_id: truck.route_id }));
                    }
                }
            }
        }
    }

    // Remove MoveOrders from trucks that just delivered (so they stop at depot
    // until the caller issues the return-leg MoveOrder).
    for entity in to_remove_move_order {
        let _ = world.remove_one::<crate::components::MoveOrder>(entity);
    }

    events
}
