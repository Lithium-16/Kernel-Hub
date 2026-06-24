//! Unified building spawner. Call `spawn_building` for all building types;
//! call `spawn_hq` for the HQ which also pre-stocks its Depot.

use std::f32::consts::FRAC_PI_2;

use hecs::World;
use macroquad::prelude::{Color, Vec2};

use crate::components::{
    AmmoStorage, Building, BuildingKind, Faction, Health, LoadingZone, Position, Turret, Weapon,
    VisionRange,
};
use crate::data::BuildingDef;
use crate::depot::{Depot, ResourceType};
use crate::map::TILE_SIZE;

/// Spawn a fully-functional building entity from its definition.
///
/// Functional components (Depot, Turret/Weapon/AmmoStorage, VisionRange) are attached
/// based on the flags in `def`. Extractor is intentionally NOT added here — the caller
/// must attach it after spawn because it needs the depot entity handle.
pub fn spawn_building(
    world: &mut World,
    def: &BuildingDef,
    tx: usize,
    ty: usize,
    faction: &str,
) -> hecs::Entity {
    let cx = (tx as f32 + def.w as f32 * 0.5) * TILE_SIZE;
    let cy = (ty as f32 + def.h as f32 * 0.5) * TILE_SIZE;
    let pos = Vec2::new(cx, cy);

    let (r, g, b) = def.color;
    let color = Color::from_rgba(r, g, b, 255);

    // Loading zone: one tile below the bottom-centre of the building footprint.
    // This is always passable (buildings can't be placed on impassable tiles),
    // and it's the exact position trucks will stop at for pickup/dropoff.
    let lz_x = (tx as f32 + def.w as f32 * 0.5) * TILE_SIZE;
    let lz_y = (ty as f32 + def.h as f32 + 0.5) * TILE_SIZE; // one tile below bottom edge
    let loading_zone_pos = Vec2::new(lz_x, lz_y);

    // ── Always-present components ─────────────────────────────────────────────
    let entity = world.spawn((
        Building { tx, ty, w: def.w, h: def.h, color },
        BuildingKind(def.id.clone()),
        Faction(faction.to_owned()),
        Position(pos),
        LoadingZone { world_pos: loading_zone_pos },
    ));

    // ── Health ────────────────────────────────────────────────────────────────
    if def.hp > 0.0 {
        world.insert_one(entity, Health { cur: def.hp, max: def.hp })
            .expect("spawn_building: insert Health");
    }

    // ── Depot ─────────────────────────────────────────────────────────────────
    if def.has_depot {
        let range = if def.depot_supply_range > 0.0 { def.depot_supply_range } else { 600.0 };
        world.insert_one(entity, Depot::new(faction, range))
            .expect("spawn_building: insert Depot");
    }

    // ── Gun turret ────────────────────────────────────────────────────────────
    if def.has_turret {
        world.insert(
            entity,
            (
                Turret { angle: -FRAC_PI_2, turn_rate: def.turret_turn_rate },
                Weapon {
                    range: def.turret_range,
                    damage: def.turret_damage,
                    fire_rate: def.turret_fire_rate,
                    cooldown: 0.0,
                },
                AmmoStorage::new(200),
            ),
        )
        .expect("spawn_building: insert turret components");
    }

    // ── Vision ────────────────────────────────────────────────────────────────
    if def.vision_range_tiles > 0.0 {
        world.insert_one(entity, VisionRange(def.vision_range_tiles * TILE_SIZE))
            .expect("spawn_building: insert VisionRange");
    }

    entity
}

/// Spawn the HQ building and pre-stock its Depot with the given starting resources.
///
/// `spawn_pos` is a world-space pixel position (e.g. `map.player_spawn()`). The HQ
/// footprint is centred on the nearest tile that keeps the building within the map.
#[allow(clippy::too_many_arguments)]
pub fn spawn_hq(
    world: &mut World,
    def: &BuildingDef,
    spawn_pos: Vec2,
    faction: &str,
    start_ammo: u32,
    start_fuel: u32,
    start_supplies: u32,
    start_parts: u32,
) -> hecs::Entity {
    // Convert world-space spawn point to tile origin (top-left of footprint).
    let tx = ((spawn_pos.x / TILE_SIZE) as usize).saturating_sub(def.w / 2);
    let ty = ((spawn_pos.y / TILE_SIZE) as usize).saturating_sub(def.h / 2);

    let entity = spawn_building(world, def, tx, ty, faction);

    // Pre-stock the depot (def.has_depot must be true for HQ).
    if let Ok(mut depot) = world.get::<&mut Depot>(entity) {
        depot.add(ResourceType::Ammo,             start_ammo);
        depot.add(ResourceType::Fuel,             start_fuel);
        depot.add(ResourceType::BuildingSupplies, start_supplies);
        depot.add(ResourceType::WeaponParts,      start_parts);
    }

    entity
}
