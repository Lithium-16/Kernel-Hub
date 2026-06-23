//! Spawn one Depot entity per faction near the HQ spawn point at scenario start.
//! The starting stock represents the pre-war stockpile; no delivery needed for Phase 4.

use hecs::World;
use macroquad::prelude::Vec2;

use crate::components::{Faction, Position};
use crate::depot::{Depot, ResourceType};

/// Ammo stocked in each depot at game start.
const START_AMMO: u32 = 2000;
/// Fuel stocked in each depot at game start.
const START_FUEL: u32 = 1500;
/// Building supplies stocked in each depot at game start.
const START_SUPPLIES: u32 = 800;
/// Weapon parts stocked in each depot at game start.
const START_PARTS: u32 = 400;

/// Distance the depot is placed from the faction's HQ spawn point (px).
const DEPOT_OFFSET: f32 = 200.0;

/// Units within this many world-pixels can resupply from the depot.
const SUPPLY_RANGE: f32 = 600.0;

fn make_depot(faction: &str) -> Depot {
    Depot::new(faction, SUPPLY_RANGE)
        .with_stock(ResourceType::Ammo, START_AMMO)
        .with_stock(ResourceType::Fuel, START_FUEL)
        .with_stock(ResourceType::BuildingSupplies, START_SUPPLIES)
        .with_stock(ResourceType::WeaponParts, START_PARTS)
}

/// Spawn one depot per side and return `(player_depot, enemy_depot)`.
///
/// Each depot is placed `DEPOT_OFFSET` pixels below the faction's spawn point so it
/// does not block unit pathfinding around the HQ.
pub fn spawn_starting_depots(
    world: &mut World,
    player_spawn: Vec2,
    enemy_spawn: Vec2,
    player_faction: &str,
    enemy_faction: &str,
) -> (hecs::Entity, hecs::Entity) {
    let player_pos = Vec2::new(player_spawn.x, player_spawn.y + DEPOT_OFFSET);
    let enemy_pos  = Vec2::new(enemy_spawn.x,  enemy_spawn.y  + DEPOT_OFFSET);

    let player_depot = world.spawn((
        Position(player_pos),
        Faction(player_faction.to_string()),
        make_depot(player_faction),
    ));

    let enemy_depot = world.spawn((
        Position(enemy_pos),
        Faction(enemy_faction.to_string()),
        make_depot(enemy_faction),
    ));

    (player_depot, enemy_depot)
}
