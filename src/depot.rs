//! Physical resource framework. Resources are quantities stored in Depot entities,
//! transported by trucks (Phase 5), and consumed by combat and movement.
//! This module defines the types and the depot component — production/routing is Phase 4+.

use hecs::World;
use macroquad::prelude::Vec2;

use crate::components::Position;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum ResourceType {
    Ammo,
    Fuel,
    BuildingSupplies,
    WeaponParts,
}

impl ResourceType {
    pub const ALL: [ResourceType; 4] = [
        ResourceType::Ammo,
        ResourceType::Fuel,
        ResourceType::BuildingSupplies,
        ResourceType::WeaponParts,
    ];

    pub fn label(self) -> &'static str {
        match self {
            ResourceType::Ammo             => "Ammo",
            ResourceType::Fuel             => "Fuel",
            ResourceType::BuildingSupplies => "Supplies",
            ResourceType::WeaponParts      => "Parts",
        }
    }
}

/// A physical depot: stores multiple resource types. Units within `supply_range` can draw from it.
pub struct Depot {
    pub stockpile: std::collections::HashMap<ResourceType, u32>,
    /// World-px radius within which units can draw from this depot.
    pub supply_range: f32,
    pub faction: String,
}

impl Depot {
    pub fn new(faction: impl Into<String>, supply_range: f32) -> Self {
        Self {
            stockpile: ResourceType::ALL.iter().map(|&r| (r, 0)).collect(),
            supply_range,
            faction: faction.into(),
        }
    }

    pub fn with_stock(mut self, r: ResourceType, amount: u32) -> Self {
        self.stockpile.insert(r, amount);
        self
    }

    pub fn get(&self, r: ResourceType) -> u32 {
        self.stockpile.get(&r).copied().unwrap_or(0)
    }

    pub fn add(&mut self, r: ResourceType, amount: u32) {
        *self.stockpile.entry(r).or_default() += amount;
    }

    /// Attempt to withdraw `amount` of resource. Returns how much was actually taken.
    pub fn withdraw(&mut self, r: ResourceType, amount: u32) -> u32 {
        let have = self.stockpile.entry(r).or_default();
        let taken = (*have).min(amount);
        *have -= taken;
        taken
    }

    pub fn is_empty(&self, r: ResourceType) -> bool { self.get(r) == 0 }

    /// Fill fraction 0..1 relative to a soft cap (for UI bar display).
    pub fn fill_frac(&self, r: ResourceType, cap: u32) -> f32 {
        if cap == 0 { return 0.0; }
        (self.get(r) as f32 / cap as f32).clamp(0.0, 1.0)
    }
}

/// Find the nearest depot of the given faction within `max_range` world pixels.
pub fn nearest_depot<'w>(
    world: &'w World,
    pos: Vec2,
    faction: &str,
    max_range: f32,
) -> Option<hecs::EntityRef<'w>> {
    let mut best_dist = max_range * max_range;
    let mut best = None;
    for (e, (dpos, depot)) in world.query::<(&Position, &Depot)>().iter() {
        if depot.faction != faction { continue; }
        let d2 = pos.distance_squared(dpos.0);
        if d2 < best_dist {
            best_dist = d2;
            best = Some(e);
        }
    }
    best.and_then(|e| world.entity(e).ok())
}
