//! Unit stances (Phase 3 autonomy groundwork). A stance is standing guidance the
//! future utility AI / combat reads to decide how a unit behaves without per-unit
//! orders. Stored as a component; the command card sets it for the selection. Combat
//! effects land with the combat system — for now Hold-Ground also halts movement.

use hecs::{Entity, World};

use crate::components::Selected;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Stance {
    /// Engage and pursue threats.
    Aggressive,
    /// Engage in range but don't chase.
    Defensive,
    /// Stay put; never move on its own.
    HoldGround,
}

impl Stance {
    pub fn label(self) -> &'static str {
        match self {
            Stance::Aggressive => "Aggressive",
            Stance::Defensive => "Defensive",
            Stance::HoldGround => "Hold",
        }
    }

    fn index(self) -> usize {
        match self {
            Stance::Aggressive => 0,
            Stance::Defensive => 1,
            Stance::HoldGround => 2,
        }
    }

    fn from_index(i: usize) -> Stance {
        match i {
            1 => Stance::Defensive,
            2 => Stance::HoldGround,
            _ => Stance::Aggressive,
        }
    }

    pub const ALL: [Stance; 3] = [Stance::Aggressive, Stance::Defensive, Stance::HoldGround];
}

/// The most common stance among the current selection (None if nothing is selected).
pub fn dominant(world: &World) -> Option<Stance> {
    let mut counts = [0u32; 3];
    let mut any = false;
    for (_e, (_s, st)) in world.query::<(&Selected, &Stance)>().iter() {
        any = true;
        counts[st.index()] += 1;
    }
    if !any {
        return None;
    }
    let mut best = 0;
    for i in 1..3 {
        if counts[i] > counts[best] {
            best = i;
        }
    }
    Some(Stance::from_index(best))
}

/// Set the stance of every selected unit.
pub fn set_selected(world: &mut World, s: Stance) {
    let sel: Vec<Entity> = world.query::<&Selected>().iter().map(|(e, _)| e).collect();
    for e in sel {
        let _ = world.insert_one(e, s);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dominant_picks_majority_and_set_updates_it() {
        let mut world = World::new();
        world.spawn((Selected, Stance::Aggressive));
        world.spawn((Selected, Stance::Defensive));
        world.spawn((Selected, Stance::Defensive));
        assert_eq!(dominant(&world), Some(Stance::Defensive));

        set_selected(&mut world, Stance::HoldGround);
        assert_eq!(dominant(&world), Some(Stance::HoldGround));
    }

    #[test]
    fn dominant_is_none_without_selection() {
        let mut world = World::new();
        world.spawn((Stance::Aggressive,)); // present but not selected
        assert_eq!(dominant(&world), None);
    }
}
