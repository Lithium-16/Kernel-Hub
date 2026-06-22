//! Selection helpers (Phase 3). Pure queries over the world so the selection rules
//! (select-by-type, box select) are unit-testable without a window or input.

use hecs::{Entity, World};
use macroquad::prelude::Rect;

use crate::components::{Position, UnitKind};

/// All entities of the given unit-type id whose position falls inside `rect`. Used by
/// double-click "select all of type on screen" (pass the current view rect).
pub fn same_kind_in_rect(world: &World, kind_id: &str, rect: Rect) -> Vec<Entity> {
    world
        .query::<(&Position, &UnitKind)>()
        .iter()
        .filter(|(_, (p, k))| k.id == kind_id && rect.contains(p.0))
        .map(|(e, _)| e)
        .collect()
}

/// All entities whose position falls inside `rect` (box select).
pub fn in_rect(world: &World, rect: Rect) -> Vec<Entity> {
    world
        .query::<&Position>()
        .iter()
        .filter(|(_, p)| rect.contains(p.0))
        .map(|(e, _)| e)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::Faction;
    use macroquad::prelude::{vec2, Rect};

    fn spawn(world: &mut World, id: &str, x: f32, y: f32) -> Entity {
        world.spawn((
            Position(vec2(x, y)),
            UnitKind { id: id.to_string(), name: id.to_string() },
            Faction("test".to_string()),
        ))
    }

    #[test]
    fn same_kind_filters_by_type_and_bounds() {
        let mut world = World::new();
        let t1 = spawn(&mut world, "tank", 10.0, 10.0);
        let _t_far = spawn(&mut world, "tank", 999.0, 999.0); // out of rect
        let _inf = spawn(&mut world, "infantry", 12.0, 12.0); // wrong kind
        let t2 = spawn(&mut world, "tank", 20.0, 20.0);

        let rect = Rect::new(0.0, 0.0, 100.0, 100.0);
        let mut got = same_kind_in_rect(&world, "tank", rect);
        got.sort();
        let mut want = vec![t1, t2];
        want.sort();
        assert_eq!(got, want);
    }

    #[test]
    fn in_rect_collects_all_inside() {
        let mut world = World::new();
        let a = spawn(&mut world, "tank", 5.0, 5.0);
        let b = spawn(&mut world, "infantry", 50.0, 50.0);
        let _out = spawn(&mut world, "tank", 500.0, 5.0);
        let mut got = in_rect(&world, Rect::new(0.0, 0.0, 100.0, 100.0));
        got.sort();
        let mut want = vec![a, b];
        want.sort();
        assert_eq!(got, want);
    }
}
