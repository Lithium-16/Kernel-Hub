//! ECS layer. Milestone 0 establishes the dependency on `hecs`; Phase 1 builds the
//! component set and systems on top (data-oriented — see PLAN.md).

pub use hecs::World;

/// Create an empty engine world. Placeholder until Phase 1.
pub fn new_world() -> World {
    World::new()
}
