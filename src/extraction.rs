//! Extraction buildings: Mine (ore → BuildingSupplies) and OilPump (oil → Fuel).
//! Each extractor is an ECS entity with an Extractor component. It feeds one
//! specific attached depot — no depot search, no teleportation.

use crate::depot::{Depot, ResourceType};
use hecs::World;

// ─── Types ───────────────────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ExtractorKind {
    Mine,     // produces BuildingSupplies
    OilPump,  // produces Fuel
}

impl ExtractorKind {
    pub fn cycle_secs(self) -> f32 {
        match self {
            ExtractorKind::Mine    => 8.0,
            ExtractorKind::OilPump => 10.0,
        }
    }

    pub fn output_resource(self) -> ResourceType {
        match self {
            ExtractorKind::Mine    => ResourceType::BuildingSupplies,
            ExtractorKind::OilPump => ResourceType::Fuel,
        }
    }

    pub fn output_amount(self) -> u32 {
        match self {
            ExtractorKind::Mine    => 15,
            ExtractorKind::OilPump => 10,
        }
    }
}

pub struct Extractor {
    pub kind: ExtractorKind,
    /// The specific depot this extractor feeds. Set at spawn time by main.rs.
    /// The extractor does NOT search for a nearest depot — it uses only this one.
    pub attached_depot: hecs::Entity,
    /// Seconds remaining until the next production tick.
    pub cooldown: f32,
}

// ─── System step ─────────────────────────────────────────────────────────────

/// Advance all extractors by `dt` seconds and deposit resources into their
/// attached depots when a production cycle completes.
///
/// Two-pass design avoids simultaneous mutable borrows:
///   Pass 1 – iterate extractors, tick cooldown, collect pending deposits.
///   Pass 2 – apply deposits to depot components.
pub fn step(world: &mut World, dt: f32) {
    // Pass 1: collect work items.
    // Each item: (extractor_entity, kind, attached_depot, new_cooldown, should_produce)
    let mut work: Vec<(hecs::Entity, ExtractorKind, hecs::Entity, f32, bool)> = Vec::new();

    for (entity, extractor) in world.query::<&Extractor>().iter() {
        let new_cooldown = extractor.cooldown - dt;
        let should_produce = new_cooldown <= 0.0;
        work.push((
            entity,
            extractor.kind,
            extractor.attached_depot,
            new_cooldown,
            should_produce,
        ));
    }

    // Pass 2: update cooldowns and deposit resources.
    for (extractor_entity, kind, depot_entity, new_cooldown, should_produce) in work {
        // Update cooldown on the extractor.
        if let Ok(mut extractor) = world.get::<&mut Extractor>(extractor_entity) {
            if should_produce {
                // Reset cooldown for the next cycle.
                extractor.cooldown = kind.cycle_secs();
            } else {
                extractor.cooldown = new_cooldown;
            }
        }

        // Deposit into the attached depot if a cycle just completed.
        if should_produce {
            // If the depot entity no longer exists, skip silently.
            if let Ok(mut depot) = world.get::<&mut Depot>(depot_entity) {
                depot.add(kind.output_resource(), kind.output_amount());
            }
        }
    }
}
