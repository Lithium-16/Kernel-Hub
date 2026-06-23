//! Supply route registry and dispatch logic.
//!
//! A SupplyRoute describes an intent: keep `destination` depot stocked at `desired_stock`
//! of a given resource by dispatching physical Truck entities from `origin`.
//! Trucks are spawned by the caller after `dispatch_needed` returns pending dispatches.

use hecs::{Entity, World};

use crate::depot::{Depot, ResourceType};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Units of resource one truck carries per trip.
pub const TRUCK_CAPACITY: u32 = 100;

/// Maximum trucks simultaneously active on a single route.
pub const MAX_TRUCKS_PER_ROUTE: u32 = 3;

// ---------------------------------------------------------------------------
// Route types
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RoutePriority {
    High,
    Medium,
    Low,
}

pub struct SupplyRoute {
    pub id: u32,
    /// Depot entity that cargo is drawn from.
    pub origin: Entity,
    /// Depot entity that cargo is delivered to.
    pub destination: Entity,
    pub resource: ResourceType,
    pub priority: RoutePriority,
    /// Dispatch trucks until destination holds at least this much of `resource`.
    pub desired_stock: u32,
    /// Number of trucks currently travelling this route (in either direction).
    pub active_trucks: u32,
}

// ---------------------------------------------------------------------------
// Registry
// ---------------------------------------------------------------------------

pub struct RouteRegistry {
    routes: Vec<SupplyRoute>,
    next_id: u32,
}

impl RouteRegistry {
    pub fn new() -> Self {
        Self {
            routes: Vec::new(),
            next_id: 1,
        }
    }

    /// Register a new route and return its assigned id.
    pub fn add(
        &mut self,
        origin: Entity,
        destination: Entity,
        resource: ResourceType,
        desired_stock: u32,
        priority: RoutePriority,
    ) -> u32 {
        let id = self.next_id;
        self.next_id += 1;
        self.routes.push(SupplyRoute {
            id,
            origin,
            destination,
            resource,
            priority,
            desired_stock,
            active_trucks: 0,
        });
        id
    }

    pub fn all(&self) -> &[SupplyRoute] {
        &self.routes
    }

    pub fn all_mut(&mut self) -> &mut [SupplyRoute] {
        &mut self.routes
    }

    pub fn get_mut(&mut self, id: u32) -> Option<&mut SupplyRoute> {
        self.routes.iter_mut().find(|r| r.id == id)
    }

    /// Remove a route by id. Does not affect trucks already in flight.
    pub fn remove(&mut self, id: u32) {
        self.routes.retain(|r| r.id != id);
    }

    /// Remove all routes (e.g. on game restart).
    pub fn clear(&mut self) {
        self.routes.clear();
    }
}

impl Default for RouteRegistry {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Dispatch
// ---------------------------------------------------------------------------

/// Examine every route and decide which ones need a new truck dispatched.
///
/// For each eligible route this function:
///   1. Checks that destination stock < desired_stock.
///   2. Checks that origin has at least TRUCK_CAPACITY of the resource.
///   3. Checks that fewer than MAX_TRUCKS_PER_ROUTE trucks are already active.
///   4. Withdraws TRUCK_CAPACITY from the origin depot (cargo is now "in transit").
///   5. Increments route.active_trucks.
///
/// Returns a list of dispatches the caller must fulfil by spawning Truck entities.
/// Tuple: (route_id, origin_entity, destination_entity, resource, amount_to_carry).
///
/// Routes are evaluated in priority order (High -> Medium -> Low) so high-priority
/// logistics are served first when resources are scarce.
pub fn dispatch_needed(
    registry: &mut RouteRegistry,
    world: &World,
) -> Vec<(u32, Entity, Entity, ResourceType, u32)> {
    // Sort indices by priority so High routes are evaluated first.
    let mut indices: Vec<usize> = (0..registry.routes.len()).collect();
    indices.sort_by_key(|&i| match registry.routes[i].priority {
        RoutePriority::High   => 0u8,
        RoutePriority::Medium => 1,
        RoutePriority::Low    => 2,
    });

    let mut dispatches = Vec::new();

    for idx in indices {
        let route = &registry.routes[idx];

        // Guard: slot available?
        if route.active_trucks >= MAX_TRUCKS_PER_ROUTE {
            continue;
        }

        let resource      = route.resource;
        let desired_stock = route.desired_stock;
        let origin_ent    = route.origin;
        let dest_ent      = route.destination;
        let route_id      = route.id;

        // Guard: destination below desired level?
        let dest_stock = {
            let Ok(dest_ref) = world.entity(dest_ent) else { continue };
            let Some(depot) = dest_ref.get::<&Depot>() else { continue };
            depot.get(resource)
        };
        if dest_stock >= desired_stock {
            continue;
        }

        // Guard: origin has enough to fill a truck?
        let origin_stock = {
            let Ok(origin_ref) = world.entity(origin_ent) else { continue };
            let Some(depot) = origin_ref.get::<&Depot>() else { continue };
            depot.get(resource)
        };
        if origin_stock < TRUCK_CAPACITY {
            continue;
        }

        // Withdraw cargo from origin depot now — it is "in transit".
        {
            let Ok(origin_ref) = world.entity(origin_ent) else { continue };
            let Some(mut depot) = origin_ref.get::<&mut Depot>() else { continue };
            let taken = depot.withdraw(resource, TRUCK_CAPACITY);
            if taken < TRUCK_CAPACITY {
                // Race: stock dropped between the check and the withdraw. Skip.
                // Return what was taken so depot stays consistent.
                depot.add(resource, taken);
                continue;
            }
        }

        // Commit: increment active truck counter and record the dispatch.
        registry.routes[idx].active_trucks += 1;
        dispatches.push((route_id, origin_ent, dest_ent, resource, TRUCK_CAPACITY));
    }

    dispatches
}

// ---------------------------------------------------------------------------
// Truck arrival helper
// ---------------------------------------------------------------------------

/// Call this when a truck successfully delivers its cargo.
/// Deposits `amount` of `resource` into the destination depot and decrements
/// the route's active truck counter.
///
/// Returns `false` if the route id no longer exists (route was deleted mid-flight).
pub fn on_truck_delivered(
    registry: &mut RouteRegistry,
    world: &World,
    route_id: u32,
    destination: Entity,
    resource: ResourceType,
    amount: u32,
) -> bool {
    // Deposit cargo into destination depot.
    if let Ok(dest_ref) = world.entity(destination) {
        if let Some(mut depot) = dest_ref.get::<&mut Depot>() {
            depot.add(resource, amount);
        }
    }

    // Decrement active truck count (route may have been removed).
    if let Some(route) = registry.get_mut(route_id) {
        route.active_trucks = route.active_trucks.saturating_sub(1);
        true
    } else {
        false
    }
}

/// Call this when a truck is destroyed before delivery (cargo lost).
/// Decrements the route's active truck counter.
/// Returns `false` if the route no longer exists.
pub fn on_truck_destroyed(registry: &mut RouteRegistry, route_id: u32) -> bool {
    if let Some(route) = registry.get_mut(route_id) {
        route.active_trucks = route.active_trucks.saturating_sub(1);
        true
    } else {
        false
    }
}
