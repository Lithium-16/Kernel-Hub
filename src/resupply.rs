//! Resupply system: transfers Ammo and Fuel from nearby same-faction Depots
//! into units that carry AmmoStorage / FuelTank components.
//!
//! Physical-logistics rules:
//! - No teleportation. Transfer only happens when the unit is within the depot's supply_range.
//! - The depot stock must actually hold the resource; nothing is conjured.
//! - Each unit resupplies at most once every RESUPPLY_INTERVAL_TICKS (100 ticks = 5 s at 20 Hz).

use std::collections::HashMap;

use hecs::{Entity, World};
use macroquad::prelude::Vec2;

use crate::components::{AmmoStorage, Faction, FuelTank, Position};
use crate::depot::{Depot, ResourceType};

// ─── tunables ────────────────────────────────────────────────────────────────

pub const RESUPPLY_INTERVAL_TICKS: u32 = 100; // 5 s at 20 Hz
pub const RESUPPLY_AMMO_BATCH: u32 = 20;      // shots transferred per event
pub const RESUPPLY_FUEL_BATCH: f32 = 50.0;    // fuel units transferred per event

// ─── tracker ────────────────────────────────────────────────────────────────

/// Holds per-entity cooldown counters so resupply events are rate-limited.
pub struct ResupplyTracker {
    /// Remaining ticks before the entity may resupply again.
    /// Entities absent from the map are implicitly at 0 (ready).
    cooldowns: HashMap<Entity, u32>,
}

impl ResupplyTracker {
    pub fn new() -> Self {
        Self { cooldowns: HashMap::new() }
    }

    /// Call once per sim tick (20 Hz).  Finds units near depots and transfers resources.
    pub fn step(&mut self, world: &mut World) {
        // ── pass 1: decrement cooldowns ──────────────────────────────────────
        for ticks in self.cooldowns.values_mut() {
            *ticks = ticks.saturating_sub(1);
        }

        // ── pass 2: collect units that want resupply ─────────────────────────
        // We read Position, Faction, AmmoStorage, FuelTank in separate queries
        // to avoid aliasing the borrow checker would reject if we held mutable
        // references to multiple component types simultaneously.

        // Gather (entity, world-pos, faction-id, needs_ammo, needs_fuel).
        // A unit "needs" a resource when it is not already full.
        struct UnitNeed {
            entity: Entity,
            pos: Vec2,
            faction: String,
            needs_ammo: bool,
            ammo_free: u32,
            needs_fuel: bool,
            fuel_free: f32,
        }

        let mut needs: Vec<UnitNeed> = Vec::new();

        // Units that have AmmoStorage (fuel optional)
        for (e, (pos, fac, ammo)) in world
            .query::<(&Position, &Faction, &AmmoStorage)>()
            .iter()
        {
            // Skip if on cooldown
            if self.cooldowns.get(&e).copied().unwrap_or(0) > 0 {
                continue;
            }
            let needs_ammo = !ammo.is_full();
            // Check fuel too if present (done below in separate query)
            if needs_ammo {
                needs.push(UnitNeed {
                    entity: e,
                    pos: pos.0,
                    faction: fac.0.clone(),
                    needs_ammo,
                    ammo_free: ammo.free(),
                    needs_fuel: false, // filled in next pass
                    fuel_free: 0.0,
                });
            }
        }

        // Units that have FuelTank (ammo may already be in the list above)
        // We collect fuel-only units here; ammo+fuel overlap handled by merging.
        let mut fuel_entries: Vec<UnitNeed> = Vec::new();
        for (e, (pos, fac, tank)) in world
            .query::<(&Position, &Faction, &FuelTank)>()
            .iter()
        {
            if self.cooldowns.get(&e).copied().unwrap_or(0) > 0 {
                continue;
            }
            if !tank.is_full() {
                fuel_entries.push(UnitNeed {
                    entity: e,
                    pos: pos.0,
                    faction: fac.0.clone(),
                    needs_ammo: false,
                    ammo_free: 0,
                    needs_fuel: true,
                    fuel_free: tank.free(),
                });
            }
        }

        // Merge: if an entity appears in both lists, set needs_fuel on the existing entry.
        for fe in fuel_entries {
            if let Some(existing) = needs.iter_mut().find(|u| u.entity == fe.entity) {
                existing.needs_fuel = true;
                existing.fuel_free = fe.fuel_free;
            } else {
                needs.push(fe);
            }
        }

        if needs.is_empty() {
            return;
        }

        // ── pass 3: collect depot snapshots (pos, faction, entity, supply_range) ──
        // We collect identifiers and ranges here; mutations happen per-unit below
        // through a separate targeted world.get_mut.
        struct DepotInfo {
            entity: Entity,
            pos: Vec2,
            faction: String,
            supply_range: f32,
        }

        let depot_infos: Vec<DepotInfo> = world
            .query::<(&Position, &Depot)>()
            .iter()
            .map(|(e, (p, d))| DepotInfo {
                entity: e,
                pos: p.0,
                faction: d.faction.clone(),
                supply_range: d.supply_range,
            })
            .collect();

        // ── pass 4: for each needy unit find nearest depot and transfer ───────
        let mut resupplied: Vec<(Entity, u32, f32)> = Vec::new(); // (entity, ammo_given, fuel_given)

        for unit in &needs {
            // Find nearest same-faction depot within its supply_range.
            let best = depot_infos
                .iter()
                .filter(|d| d.faction == unit.faction)
                .filter(|d| unit.pos.distance_squared(d.pos) <= d.supply_range * d.supply_range)
                .min_by(|a, b| {
                    let da = unit.pos.distance_squared(a.pos);
                    let db = unit.pos.distance_squared(b.pos);
                    da.partial_cmp(&db).unwrap_or(std::cmp::Ordering::Equal)
                });

            let Some(depot_info) = best else { continue };

            let mut ammo_given = 0u32;
            let mut fuel_given = 0.0f32;

            // Borrow depot mutably for transfer.
            if let Ok(mut depot) = world.get::<&mut Depot>(depot_info.entity) {
                if unit.needs_ammo && unit.ammo_free > 0 {
                    let want = unit.ammo_free.min(RESUPPLY_AMMO_BATCH);
                    ammo_given = depot.withdraw(ResourceType::Ammo, want);
                }
                if unit.needs_fuel && unit.fuel_free > 0.0 {
                    let want_u = (unit.fuel_free.min(RESUPPLY_FUEL_BATCH).ceil()) as u32;
                    let taken_u = depot.withdraw(ResourceType::Fuel, want_u);
                    fuel_given = taken_u as f32;
                }
            }

            if ammo_given > 0 || fuel_given > 0.0 {
                resupplied.push((unit.entity, ammo_given, fuel_given));
            }
        }

        // ── pass 5: apply transfers to unit components ───────────────────────
        for (entity, ammo_given, fuel_given) in resupplied {
            if ammo_given > 0 {
                if let Ok(mut ammo) = world.get::<&mut AmmoStorage>(entity) {
                    ammo.shots = (ammo.shots + ammo_given).min(ammo.capacity);
                }
            }
            if fuel_given > 0.0 {
                if let Ok(mut tank) = world.get::<&mut FuelTank>(entity) {
                    tank.fuel = (tank.fuel + fuel_given).min(tank.capacity);
                }
            }
            // Reset cooldown regardless (prevents spam even if transfer was partial)
            self.cooldowns.insert(entity, RESUPPLY_INTERVAL_TICKS);
        }
    }
}
