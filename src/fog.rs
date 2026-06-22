//! Fog of war. Each tile has three states: Hidden (black), LastSeen (dimmed, shows
//! last-known terrain), Visible (live). Updated every sim tick from player unit positions.
//! Enemy units only appear in Visible tiles — they vanish when out of sight.

use hecs::World;
use macroquad::prelude::Vec2;

use crate::components::{Faction, Position};
use crate::map::TILE_SIZE;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FogState {
    Hidden,
    LastSeen,
    Visible,
}

pub struct FogGrid {
    pub w: usize,
    pub h: usize,
    tiles: Vec<FogState>,
}

impl FogGrid {
    pub fn new(w: usize, h: usize) -> Self {
        Self { w, h, tiles: vec![FogState::Hidden; w * h] }
    }

    pub fn state(&self, x: usize, y: usize) -> FogState {
        if x < self.w && y < self.h { self.tiles[y * self.w + x] } else { FogState::Hidden }
    }

    /// Reveal a circular area around a tile coordinate.
    pub fn reveal_tile(&mut self, cx: i32, cy: i32, radius_tiles: i32) {
        let r2 = radius_tiles * radius_tiles;
        for dy in -radius_tiles..=radius_tiles {
            for dx in -radius_tiles..=radius_tiles {
                if dx * dx + dy * dy <= r2 {
                    let x = cx + dx;
                    let y = cy + dy;
                    if x >= 0 && y >= 0 && (x as usize) < self.w && (y as usize) < self.h {
                        self.tiles[y as usize * self.w + x as usize] = FogState::Visible;
                    }
                }
            }
        }
    }

    /// Reveal a world-space circle (converts px to tiles).
    pub fn reveal_world(&mut self, center: Vec2, radius_px: f32) {
        let cx = (center.x / TILE_SIZE) as i32;
        let cy = (center.y / TILE_SIZE) as i32;
        let r = (radius_px / TILE_SIZE).ceil() as i32;
        self.reveal_tile(cx, cy, r);
    }

    /// Age: tick all Visible → LastSeen before revealing this tick's vision.
    fn age(&mut self) {
        for s in self.tiles.iter_mut() {
            if *s == FogState::Visible {
                *s = FogState::LastSeen;
            }
        }
    }

    /// Full update: age then reveal from every player-faction unit.
    pub fn update(&mut self, world: &World, player_faction: &str) {
        self.age();
        for (_e, (pos, fac)) in world.query::<(&Position, &Faction)>().iter() {
            if fac.0 == player_faction {
                self.reveal_world(pos.0, VISION_RADIUS_PX);
            }
        }
    }

    /// Returns true if a world-space point is currently visible.
    pub fn visible_world(&self, p: Vec2) -> bool {
        let x = (p.x / TILE_SIZE) as usize;
        let y = (p.y / TILE_SIZE) as usize;
        self.state(x, y) == FogState::Visible
    }
}

/// Default unit vision radius in world pixels. Will become a component later.
pub const VISION_RADIUS_PX: f32 = 5.0 * TILE_SIZE; // 5 tiles
/// HQ pre-reveal radius in tiles.
pub const HQ_REVEAL_TILES: i32 = 20;
