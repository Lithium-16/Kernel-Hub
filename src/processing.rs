//! Resource processing buildings. Each converts one resource type into another
//! via a timed cycle using a co-located attached depot.
//!
//! Physical logistics rules: the processor withdraws from its attached depot
//! and deposits back into the same depot. Trucks carry resources between depots.
//!
//! Processing chains:
//!   ProcessingFacility : BuildingSupplies(10) → WeaponParts(5)     every 15s
//!   FuelRefinery       : (oil implicit)       → Fuel(12)            every 12s
//!   AmmoFactory        : WeaponParts(3)       → Ammo(20)            every 10s

use hecs::{Entity, World};

use crate::depot::{Depot, ResourceType};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ProcessorKind {
    ProcessingFacility, // BuildingSupplies → WeaponParts
    FuelRefinery,       // oil (implicit) → Fuel
    AmmoFactory,        // WeaponParts → Ammo
}

pub struct Processor {
    pub kind: ProcessorKind,
    /// Depot that feeds this processor and receives its output.
    pub attached_depot: Entity,
    pub cooldown: f32,
}

impl ProcessorKind {
    pub fn cycle_secs(self) -> f32 {
        match self {
            ProcessorKind::ProcessingFacility => 15.0,
            ProcessorKind::FuelRefinery       => 12.0,
            ProcessorKind::AmmoFactory        => 10.0,
        }
    }

    /// Returns (input_resource, input_amount) or None if no input consumed.
    pub fn input(self) -> Option<(ResourceType, u32)> {
        match self {
            ProcessorKind::ProcessingFacility => Some((ResourceType::BuildingSupplies, 10)),
            ProcessorKind::FuelRefinery       => None, // crude oil is implicit
            ProcessorKind::AmmoFactory        => Some((ResourceType::WeaponParts, 3)),
        }
    }

    pub fn output(self) -> (ResourceType, u32) {
        match self {
            ProcessorKind::ProcessingFacility => (ResourceType::WeaponParts, 5),
            ProcessorKind::FuelRefinery       => (ResourceType::Fuel, 12),
            ProcessorKind::AmmoFactory        => (ResourceType::Ammo, 20),
        }
    }
}

/// Run all processors one simulation tick.
/// Two-pass to avoid simultaneous mutable borrows.
pub fn step(world: &mut World, dt: f32) {
    // Pass 1: collect work — which processors fire this tick.
    struct Work { entity: Entity, kind: ProcessorKind, depot: Entity, new_cooldown: f32, fires: bool }
    let work: Vec<Work> = world.query::<&Processor>().iter().map(|(e, p)| {
        let new_cd = p.cooldown - dt;
        Work { entity: e, kind: p.kind, depot: p.attached_depot, new_cooldown: new_cd, fires: new_cd <= 0.0 }
    }).collect();

    // Pass 2: update cooldowns, then do depot I/O.
    for w in work {
        if let Ok(mut proc) = world.get::<&mut Processor>(w.entity) {
            proc.cooldown = if w.fires { w.kind.cycle_secs() } else { w.new_cooldown };
        }
        if !w.fires { continue; }

        let Ok(mut depot) = world.get::<&mut Depot>(w.depot) else { continue };

        // Consume input (if any).
        if let Some((in_res, in_amt)) = w.kind.input() {
            let taken = depot.withdraw(in_res, in_amt);
            if taken < in_amt {
                // Not enough input — put back what we took and skip this cycle.
                depot.add(in_res, taken);
                continue;
            }
        }

        // Produce output.
        let (out_res, out_amt) = w.kind.output();
        depot.add(out_res, out_amt);
    }
}
