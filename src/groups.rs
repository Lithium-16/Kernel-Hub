//! Control groups (1–9): assign the current selection to a numbered group (Ctrl+N)
//! and recall it (N). Groups store entity ids and prune dead entities on recall, so a
//! group shrinks as its units die rather than re-selecting stale handles.

use hecs::{Entity, World};

use crate::components::Selected;

/// Ten slots; index 0 is unused so the digit keys map straight to indices 1..=9.
pub struct ControlGroups {
    groups: [Vec<Entity>; 10],
}

impl Default for ControlGroups {
    fn default() -> Self {
        Self { groups: std::array::from_fn(|_| Vec::new()) }
    }
}

impl ControlGroups {
    pub fn new() -> Self {
        Self::default()
    }

    /// Snapshot the currently-selected entities into group `n` (1..=9).
    pub fn assign(&mut self, n: usize, world: &World) {
        if !(1..=9).contains(&n) {
            return;
        }
        self.groups[n] = world.query::<&Selected>().iter().map(|(e, _)| e).collect();
    }

    /// Make group `n` the selection: drop dead members, clear the current selection,
    /// then select the group's survivors. Returns how many were selected.
    pub fn select(&mut self, n: usize, world: &mut World) -> usize {
        if !(1..=9).contains(&n) {
            return 0;
        }
        self.groups[n].retain(|&e| world.contains(e));
        let current: Vec<Entity> = world.query::<&Selected>().iter().map(|(e, _)| e).collect();
        for e in current {
            let _ = world.remove_one::<Selected>(e);
        }
        for &e in &self.groups[n] {
            let _ = world.insert_one(e, Selected);
        }
        self.groups[n].len()
    }

    /// Number of (last-known) members in group `n`.
    pub fn len(&self, n: usize) -> usize {
        self.groups.get(n).map_or(0, |g| g.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn assign_recall_and_prune() {
        let mut world = World::new();
        let a = world.spawn((Selected,));
        let b = world.spawn((Selected,));
        let c = world.spawn(()); // not selected

        let mut groups = ControlGroups::new();
        groups.assign(1, &world);
        assert_eq!(groups.len(1), 2, "group 1 should hold the two selected entities");

        // Clear selection, then recall the group.
        let _ = world.remove_one::<Selected>(a);
        let _ = world.remove_one::<Selected>(b);
        let n = groups.select(1, &mut world);
        assert_eq!(n, 2);
        assert!(world.get::<&Selected>(a).is_ok() && world.get::<&Selected>(b).is_ok());
        assert!(world.get::<&Selected>(c).is_err(), "c was never in the group");

        // A dead member is pruned on the next recall.
        world.despawn(a).unwrap();
        let n = groups.select(1, &mut world);
        assert_eq!(n, 1, "dead entity should be pruned");
        assert_eq!(groups.len(1), 1);
    }

    #[test]
    fn out_of_range_is_ignored() {
        let mut world = World::new();
        world.spawn((Selected,));
        let mut groups = ControlGroups::new();
        groups.assign(0, &world);
        groups.assign(10, &world);
        assert_eq!(groups.len(0), 0);
        assert_eq!(groups.select(0, &mut world), 0);
    }
}
