//! Economy aggregator (Phase 2.5+). Sums the stockpiles of all Depot entities
//! belonging to a given faction so the HUD top bar can display live totals.
//! Production / supply-network logic arrives in Phase 4; right now the numbers
//! drain as combat consumes resources and refill only via truck delivery (Phase 5).

use hecs::World;

use crate::components::Faction;
use crate::depot::{Depot, ResourceType};

/// Aggregated resource totals for one faction, derived from all its depots.
#[derive(Clone, Copy, Debug, Default)]
pub struct Economy {
    pub ammo:     u32,
    pub fuel:     u32,
    pub supplies: u32,
    pub parts:    u32,
}

/// Walk every `(Depot, Faction)` entity and sum the stockpiles that match `faction`.
pub fn aggregate(world: &World, faction: &str) -> Economy {
    let mut eco = Economy::default();
    for (_e, (depot, fac)) in world.query::<(&Depot, &Faction)>().iter() {
        if fac.0 != faction { continue; }
        eco.ammo     += depot.get(ResourceType::Ammo);
        eco.fuel     += depot.get(ResourceType::Fuel);
        eco.supplies += depot.get(ResourceType::BuildingSupplies);
        eco.parts    += depot.get(ResourceType::WeaponParts);
    }
    eco
}
