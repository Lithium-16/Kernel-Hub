//! Combat Groups — the primary command layer. Individual units are still simulated and
//! fight autonomously; the player (and AI) command groups, not individuals.
//!
//! A CombatGroup owns a list of unit entities and a current order. When the order changes,
//! the system fans it out to all living members as individual MoveOrders. Groups are plain
//! Rust structs (not ECS entities) stored alongside the World.

use hecs::{Entity, World};
use macroquad::prelude::Vec2;

use crate::components::{Faction, Health, Position, Selected};

/// The order a group is currently executing.
#[derive(Clone, Debug)]
pub enum GroupOrder {
    /// No order — hold in place.
    Idle,
    /// Move to position AND engage enemies encountered en route (attack-move).
    AdvanceTo(Vec2),
    /// Stay in current position; fire on enemies in range but don't move.
    Hold,
    /// Fall back toward a rally point.
    Withdraw(Vec2),
}

#[derive(Debug)]
pub struct CombatGroup {
    pub id: u32,
    pub name: String,
    pub faction: String,
    pub members: Vec<Entity>,
    pub order: GroupOrder,
    /// Original member count (at formation time). Used for loss display.
    pub original_strength: usize,
}

impl CombatGroup {
    pub fn new(id: u32, name: impl Into<String>, faction: impl Into<String>, members: Vec<Entity>) -> Self {
        let n = members.len();
        Self {
            id,
            name: name.into(),
            faction: faction.into(),
            members,
            order: GroupOrder::Idle,
            original_strength: n,
        }
    }

    /// Remove despawned entities from the member list.
    pub fn prune(&mut self, world: &World) {
        self.members.retain(|&e| world.contains(e));
    }

    /// How many members are still alive.
    pub fn strength(&self) -> usize { self.members.len() }

    /// Center of mass of all living members.
    pub fn centroid(&self, world: &World) -> Option<Vec2> {
        let mut sum = Vec2::ZERO;
        let mut count = 0;
        for &e in &self.members {
            if let Ok(p) = world.get::<&Position>(e) {
                sum += p.0;
                count += 1;
            }
        }
        if count > 0 { Some(sum / count as f32) } else { None }
    }

    /// Select all living members of this group.
    pub fn select_members(&self, world: &mut World) {
        for &e in &self.members {
            if world.contains(e) {
                let _ = world.insert_one(e, Selected);
            }
        }
    }

    /// True if the player has selected any member of this group.
    pub fn any_selected(&self, world: &World) -> bool {
        self.members.iter().any(|&e| world.get::<&Selected>(e).is_ok())
    }
}

/// Registry holding all groups for both factions.
pub struct GroupRegistry {
    groups: Vec<CombatGroup>,
    next_id: u32,
}

impl GroupRegistry {
    pub fn new() -> Self {
        Self { groups: Vec::new(), next_id: 1 }
    }

    pub fn add(&mut self, name: impl Into<String>, faction: impl Into<String>, members: Vec<Entity>) -> u32 {
        let id = self.next_id;
        self.next_id += 1;
        self.groups.push(CombatGroup::new(id, name, faction, members));
        id
    }

    pub fn all(&self) -> &[CombatGroup] { &self.groups }
    pub fn all_mut(&mut self) -> impl Iterator<Item = &mut CombatGroup> { self.groups.iter_mut() }
    pub fn clear(&mut self) { self.groups.clear(); }

    pub fn get_mut(&mut self, id: u32) -> Option<&mut CombatGroup> {
        self.groups.iter_mut().find(|g| g.id == id)
    }

    /// Prune dead members from all groups.
    pub fn prune_all(&mut self, world: &World) {
        for g in self.groups.iter_mut() { g.prune(world); }
    }

    /// Find which group (if any) a player-selected group is in.
    pub fn selected_group_id(&self, world: &World) -> Option<u32> {
        self.groups.iter()
            .filter(|g| g.any_selected(world))
            .map(|g| g.id)
            .next()
    }

    /// All enemy groups (for AI targeting).
    pub fn enemy_groups<'a>(&'a self, player_faction: &str) -> impl Iterator<Item = &'a CombatGroup> {
        let pf = player_faction.to_string();
        self.groups.iter().filter(move |g| g.faction != pf)
    }
}

/// Generate a group name based on faction and index.
pub fn auto_name(faction: &str, index: usize, kind: &str) -> String {
    let ordinal = match index + 1 {
        1 => "1st".to_string(),
        2 => "2nd".to_string(),
        3 => "3rd".to_string(),
        n => format!("{}th", n),
    };
    format!("{} {} {}", ordinal, faction_short(faction), kind)
}

fn faction_short(faction: &str) -> &str {
    match faction {
        "vanguard" => "Armored",
        "crimson"  => "Armored",
        _          => "Combat",
    }
}
