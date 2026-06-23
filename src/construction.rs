//! Blueprint construction system.
//!
//! Manages the two-phase lifecycle of placed buildings:
//!   1. Auto-claim: idle same-faction engineers are assigned to unclaimed blueprints.
//!   2. Build progress: assigned engineers that have arrived tick the blueprint forward,
//!      consuming Building Supplies from the nearest depot.
//!
//! `step()` returns the list of completed blueprint entities; main.rs is responsible for
//! despawning each one and calling `spawn_building()` to produce the real building entity.

use hecs::{Entity, World};
use macroquad::prelude::Vec2;

use crate::components::{AutoBuildMode, Blueprint, Building, Faction, IsBuilding, MoveOrder, Position, UnitKind};
use crate::depot::{Depot, ResourceType};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

const CLAIM_RANGE: f32 = 2000.0;
/// Distance at which an engineer counts as "on site" and contributes build rate.
/// Large enough for engineers to stop outside the nav-blocked blueprint footprint.
pub const ARRIVE_RANGE: f32 = 160.0;
/// Cap on simultaneous builders per blueprint (prevents runaway stacking).
const MAX_BUILDERS_PER_BLUEPRINT: u32 = 5;

const BUILD_RATE_PER_ENGINEER: f32 = 0.06; // progress per second per on-site engineer
const SUPPLY_INTERVAL: f32 = 5.0;          // seconds between each supply withdrawal
const SUPPLIES_PER_INTERVAL: u32 = 5;      // building supplies consumed per interval

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

pub struct ConstructionSystem;

/// Run once per simulation tick (20 Hz).
///
/// Returns the list of blueprint entities whose `progress >= 1.0`. The caller
/// (main.rs) must:
///   1. Collect the `Blueprint` data for `spawn_building()`.
///   2. Clear `IsBuilding` from any engineer that was assigned to it.
///   3. Despawn the blueprint entity.
///   4. Spawn the real building entity.
pub fn step(world: &mut World, dt: f32) -> Vec<Entity> {
    // -----------------------------------------------------------------------
    // Phase 1 — Engineer auto-claim
    // -----------------------------------------------------------------------
    // Collect all unclaimed blueprints (no engineer holds IsBuilding pointing to them).
    // Then collect idle engineers per faction.
    // Finally assign the closest idle engineer to each unclaimed blueprint.

    // Pass 1a: find every blueprint entity and its world position + faction.
    let blueprints: Vec<(Entity, Vec2, String)> = world
        .query::<(&Blueprint, &Position, &Faction)>()
        .iter()
        .map(|(e, (_, pos, fac))| (e, pos.0, fac.0.clone()))
        .collect();

    // Pass 1b: count how many engineers are already assigned per blueprint.
    // (Replaces the old single-claim set — we now allow up to MAX_BUILDERS_PER_BLUEPRINT.)

    // Pass 1c: collect idle engineers (same-faction, no IsBuilding, no MoveOrder).
    // "idle" = UnitKind id == "engineer" and has neither IsBuilding nor MoveOrder.
    struct EngineerInfo {
        entity: Entity,
        pos: Vec2,
        faction: String,
    }

    // Idle engineers: no IsBuilding, no MoveOrder.
    // AutoBuildMode engineers: included even with a MoveOrder — they'll have it cleared on claim.
    let idle_engineers: Vec<EngineerInfo> = world
        .query::<(&UnitKind, &Position, &Faction)>()
        .without::<&IsBuilding>()
        .iter()
        .filter(|(e, (uk, _, _))| {
            uk.id == "engineer" && (
                world.get::<&MoveOrder>(*e).is_err()          // truly idle
                || world.get::<&AutoBuildMode>(*e).is_ok()   // or auto-build mode
            )
        })
        .map(|(e, (_, pos, fac))| EngineerInfo {
            entity: e,
            pos: pos.0,
            faction: fac.0.clone(),
        })
        .collect();

    // Pass 1d: for each unclaimed blueprint find the closest idle same-faction engineer.
    // Track which engineers we assign in this pass so one engineer isn't double-assigned.
    let mut assigned_engineers: std::collections::HashSet<Entity> = std::collections::HashSet::new();

    let mut new_assignments: Vec<(Entity /* engineer */, Entity /* blueprint */)> = Vec::new();

    // Count how many engineers are already assigned per blueprint.
    let mut current_builders: std::collections::HashMap<Entity, u32> =
        std::collections::HashMap::new();
    for (_, ib) in world.query::<&IsBuilding>().iter() {
        *current_builders.entry(ib.blueprint).or_default() += 1;
    }

    for (bp_entity, bp_pos, bp_faction) in &blueprints {
        // Allow multiple engineers per blueprint, up to the cap.
        let slots_taken = *current_builders.get(bp_entity).unwrap_or(&0);
        let slots_available = MAX_BUILDERS_PER_BLUEPRINT.saturating_sub(slots_taken);
        if slots_available == 0 { continue; }

        // Assign up to `slots_available` idle engineers, nearest-first.
        let mut candidates: Vec<&EngineerInfo> = idle_engineers
            .iter()
            .filter(|eng| {
                eng.faction == *bp_faction
                    && !assigned_engineers.contains(&eng.entity)
                    && eng.pos.distance(*bp_pos) <= CLAIM_RANGE
            })
            .collect();
        candidates.sort_by(|a, b| {
            a.pos.distance_squared(*bp_pos)
                .partial_cmp(&b.pos.distance_squared(*bp_pos))
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        for eng in candidates.into_iter().take(slots_available as usize) {
            assigned_engineers.insert(eng.entity);
            new_assignments.push((eng.entity, *bp_entity));
            *current_builders.entry(*bp_entity).or_default() += 1;
        }
    }

    // Pass 1e: apply IsBuilding markers. Clear any existing MoveOrder so the
    // engineer immediately starts pathing to the blueprint (important for AutoBuildMode).
    for (engineer_e, blueprint_e) in new_assignments {
        let _ = world.insert_one(engineer_e, IsBuilding { blueprint: blueprint_e });
        let _ = world.remove_one::<MoveOrder>(engineer_e);
        let _ = world.remove_one::<crate::components::OrderQueue>(engineer_e);
    }

    // -----------------------------------------------------------------------
    // Phase 2 — Build progress
    // -----------------------------------------------------------------------

    // Pass 2a: for each blueprint, count how many assigned engineers are within ARRIVE_RANGE.
    // We need blueprint positions and the IsBuilding assignments.

    // Collect (engineer_entity, blueprint_entity, engineer_pos) for engineers currently building.
    let building_engineers: Vec<(Entity, Vec2)> = world
        .query::<(&IsBuilding, &Position)>()
        .iter()
        .map(|(_, (ib, pos))| (ib.blueprint, pos.0))
        .collect();

    // Map blueprint_entity → builder count (engineers within arrive range).
    let mut builder_counts: std::collections::HashMap<Entity, u32> =
        std::collections::HashMap::new();
    for (bp_entity, _, _) in &blueprints {
        builder_counts.insert(*bp_entity, 0);
    }

    // We need blueprint positions keyed by entity for the arrive-range check.
    let bp_pos_map: std::collections::HashMap<Entity, Vec2> = blueprints
        .iter()
        .map(|(e, pos, _)| (*e, *pos))
        .collect();

    for (bp_e, eng_pos) in &building_engineers {
        if let Some(&bp_pos) = bp_pos_map.get(bp_e) {
            if eng_pos.distance(bp_pos) <= ARRIVE_RANGE {
                *builder_counts.entry(*bp_e).or_default() += 1;
            }
        }
    }

    // Pass 2b: collect blueprint mutable data for progress update.
    // We need to update Blueprint fields, so collect entities + current values,
    // then apply. Also need faction for depot lookup.
    struct BpUpdate {
        entity: Entity,
        pos: Vec2,
        faction: String,
        builder_count: u32,
        supply_timer: f32,
        progress: f32,
    }

    let updates: Vec<BpUpdate> = world
        .query::<(&Blueprint, &Position, &Faction)>()
        .iter()
        .map(|(e, (bp, pos, fac))| BpUpdate {
            entity: e,
            pos: pos.0,
            faction: fac.0.clone(),
            builder_count: *builder_counts.get(&e).unwrap_or(&0),
            supply_timer: bp.supply_timer,
            progress: bp.progress,
        })
        .collect();

    // Pass 2c: for each active blueprint compute new progress / supply consumption.
    struct BpResult {
        entity: Entity,
        new_progress: f32,
        new_supply_timer: f32,
        supplies_consumed_delta: u32,
        stall: bool, // true if we tried to pull supplies but couldn't
    }

    let mut results: Vec<BpResult> = Vec::new();

    for upd in updates {
        if upd.builder_count == 0 {
            // No engineer on-site yet — no progress.
            results.push(BpResult {
                entity: upd.entity,
                new_progress: upd.progress,
                new_supply_timer: upd.supply_timer,
                supplies_consumed_delta: 0,
                stall: false,
            });
            continue;
        }

        let mut new_timer = upd.supply_timer + dt;
        let mut supplies_delta: u32 = 0;
        let mut stall = false;

        if new_timer >= SUPPLY_INTERVAL {
            new_timer -= SUPPLY_INTERVAL;

            // Find nearest same-faction depot and try to withdraw.
            // We need to do the depot lookup and mutation here (one query at a time).
            let faction = upd.faction.clone();
            let pos = upd.pos;

            // Find nearest depot entity.
            let depot_entity: Option<Entity> = {
                let mut best_dist = f32::MAX;
                let mut best: Option<Entity> = None;
                for (de, (dpos, depot)) in world.query::<(&Position, &Depot)>().iter() {
                    if depot.faction != faction {
                        continue;
                    }
                    let d2 = dpos.0.distance_squared(pos);
                    if d2 < best_dist {
                        best_dist = d2;
                        best = Some(de);
                    }
                }
                best
            };

            if let Some(de) = depot_entity {
                // Mutably borrow the depot to withdraw.
                if let Ok(mut depot) = world.get::<&mut Depot>(de) {
                    let taken = depot.withdraw(ResourceType::BuildingSupplies, SUPPLIES_PER_INTERVAL);
                    if taken < SUPPLIES_PER_INTERVAL {
                        // Partial or no supply — stall this tick.
                        // (We still consumed what we got; partial progress is fine.)
                        stall = taken == 0;
                        supplies_delta = taken;
                    } else {
                        supplies_delta = taken;
                    }
                } else {
                    stall = true;
                }
            } else {
                stall = true;
            }
        }

        // Progress only advances when not fully stalled (no supplies at all).
        let effective_builders = upd.builder_count.min(MAX_BUILDERS_PER_BLUEPRINT) as f32;
        let new_progress = if stall {
            upd.progress
        } else {
            upd.progress + BUILD_RATE_PER_ENGINEER * effective_builders * dt
        };

        results.push(BpResult {
            entity: upd.entity,
            new_progress: new_progress.min(1.0),
            new_supply_timer: new_timer,
            supplies_consumed_delta: supplies_delta,
            stall,
        });
    }

    // Pass 2d: apply progress/timer mutations to Blueprint components.
    for res in &results {
        if let Ok(mut bp) = world.get::<&mut Blueprint>(res.entity) {
            bp.progress = res.new_progress;
            bp.supply_timer = res.new_supply_timer;
            bp.supplies_consumed += res.supplies_consumed_delta;
        }
    }

    // Pass 2e: collect completed blueprint entities.
    results
        .into_iter()
        .filter(|r| r.new_progress >= 1.0)
        .map(|r| r.entity)
        .collect()
}

/// Despawn a blueprint and release its assigned engineer(s).
///
/// Call this when the player cancels a blueprint, or right before `spawn_building()`
/// finalises a completed one.
pub fn cancel_blueprint(world: &mut World, blueprint_e: Entity) {
    // Collect engineers that point to this blueprint.
    let assigned: Vec<Entity> = world
        .query::<&IsBuilding>()
        .iter()
        .filter(|(_, ib)| ib.blueprint == blueprint_e)
        .map(|(e, _)| e)
        .collect();

    // Remove the IsBuilding marker from each assigned engineer.
    for eng_e in assigned {
        let _ = world.remove_one::<IsBuilding>(eng_e);
    }

    // Despawn the blueprint entity itself (ignore error if already gone).
    let _ = world.despawn(blueprint_e);
}
