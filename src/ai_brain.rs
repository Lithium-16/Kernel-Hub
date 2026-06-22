//! AI Brain framework. The enemy commander uses the same game systems as the player:
//! it forms combat groups, issues group orders, and will place blueprints / create routes
//! in later phases. No special shortcuts — symmetric capability.
//!
//! Phase 3 behaviour: wait PREP_TICKS (5 min), then order all combat groups to advance
//! on the player's centroid and refresh that order every 2s as the player moves.

use hecs::{Entity, World};
use macroquad::prelude::Vec2;

use crate::combat_group::{GroupOrder, GroupRegistry};
use crate::components::{Faction, Position};

/// Ticks at 20 Hz before the AI launches its first advance.
/// 5 minutes × 60 s × 20 ticks = 6000.
pub const PREP_TICKS: u32 = 6000;

/// How often (in ticks) the AI refreshes its group orders.
const ORDER_REFRESH_TICKS: u32 = 40; // 2 seconds

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AiPhase {
    Preparing,  // building up before the timer
    Attacking,  // combat groups advancing
}

pub struct AiBrain {
    pub faction: String,
    pub player_faction: String,
    pub phase: AiPhase,
    prep_ticks_left: u32,
    order_refresh: u32,
}

impl AiBrain {
    pub fn new(faction: impl Into<String>, player_faction: impl Into<String>) -> Self {
        Self {
            faction: faction.into(),
            player_faction: player_faction.into(),
            phase: AiPhase::Preparing,
            prep_ticks_left: PREP_TICKS,
            order_refresh: 0,
        }
    }

    /// Run one sim tick. Call from the main sim loop after movement + combat.
    pub fn tick(&mut self, world: &mut World, groups: &mut GroupRegistry) {
        match self.phase {
            AiPhase::Preparing => {
                if self.prep_ticks_left == 0 {
                    self.phase = AiPhase::Attacking;
                } else {
                    self.prep_ticks_left -= 1;
                }
            }
            AiPhase::Attacking => {
                self.order_refresh = self.order_refresh.saturating_sub(1);
                if self.order_refresh == 0 {
                    self.order_refresh = ORDER_REFRESH_TICKS;
                    self.issue_attack_orders(world, groups);
                }
            }
        }
    }

    fn issue_attack_orders(&self, world: &World, groups: &mut GroupRegistry) {
        // Find the player's centroid.
        let mut sum = Vec2::ZERO;
        let mut n = 0u32;
        for (_e, (fac, pos)) in world.query::<(&Faction, &Position)>().iter() {
            if fac.0 == self.player_faction {
                sum += pos.0;
                n += 1;
            }
        }
        if n == 0 { return; }
        let target = sum / n as f32;

        // Order all AI combat groups to advance on the player centroid.
        for g in groups.all_mut() {
            if g.faction == self.faction {
                g.order = GroupOrder::AdvanceTo(target);
            }
        }
    }

    /// Prep time remaining in seconds (for UI display).
    pub fn prep_seconds_left(&self) -> f32 {
        self.prep_ticks_left as f32 / 20.0
    }

    pub fn is_preparing(&self) -> bool { self.phase == AiPhase::Preparing }
}
