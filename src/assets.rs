//! Sprite system.
//!
//! Tiles use a small placeholder atlas loaded at startup (safe — font atlas not yet created).
//! Unit hull + turret textures are loaded DEFERRED on frame 1, after macroquad's font atlas
//! has been initialised by the first draw_text call in frame 0.
//!
//! # Adding a new sprite
//! 1. Drop `{name}_player.png` and `{name}_enemy.png` into `assets/sprites/`.
//! 2. Add two lines to `sprite_bytes()` below.
//! 3. Set `hull_sprite`/`turret_sprite` on the unit in `assets/definitions.ron`.
//!    Only step 2 requires a Rust change; everything else is pure data.

use std::collections::HashMap;

use macroquad::prelude::*;

use crate::data::UnitDef;
use crate::map::Tile;

// ── Tile atlas (placeholder) ──────────────────────────────────────────────────

const CELL: f32 = 64.0;
const COLS: usize = 4;
const SLOT_GROUND: usize = 6;
const SLOT_WATER: usize = 7;
const SLOT_CLIFF: usize = 8;
const SLOT_RESOURCE: usize = 9;
const SLOT_SELECTION: usize = 10;

fn slot_rect(i: usize) -> Rect {
    Rect::new((i % COLS) as f32 * CELL, (i / COLS) as f32 * CELL, CELL, CELL)
}

const UNIT_ORDER: [&str; 6] = ["infantry", "engineer", "tank", "artillery", "aa", "truck"];

// ── Compile-time sprite registry ──────────────────────────────────────────────
//
// WASM requires textures to be embedded at compile time via include_bytes!.
// This match is the only place to update when adding new PNGs.
// Adding a new sprite pair: two lines here, plus the PNG files.

fn sprite_bytes(name: &str) -> Option<&'static [u8]> {
    match name {
        "hull_tank_player"     => Some(include_bytes!("../assets/sprites/hull_tank_player.png")),
        "hull_tank_enemy"      => Some(include_bytes!("../assets/sprites/hull_tank_enemy.png")),
        "hull_arty_player"     => Some(include_bytes!("../assets/sprites/hull_arty_player.png")),
        "hull_arty_enemy"      => Some(include_bytes!("../assets/sprites/hull_arty_enemy.png")),
        "hull_aa_player"       => Some(include_bytes!("../assets/sprites/hull_aa_player.png")),
        "hull_aa_enemy"        => Some(include_bytes!("../assets/sprites/hull_aa_enemy.png")),
        "hull_engineer_player" => Some(include_bytes!("../assets/sprites/hull_engineer_player.png")),
        "hull_engineer_enemy"  => Some(include_bytes!("../assets/sprites/hull_engineer_enemy.png")),
        "hull_truck_player"    => Some(include_bytes!("../assets/sprites/hull_truck_player.png")),
        "hull_truck_enemy"     => Some(include_bytes!("../assets/sprites/hull_truck_enemy.png")),
        "hull_infantry_player" => Some(include_bytes!("../assets/sprites/hull_infantry_player.png")),
        "hull_infantry_enemy"  => Some(include_bytes!("../assets/sprites/hull_infantry_enemy.png")),
        "hull_scout_player"    => Some(include_bytes!("../assets/sprites/hull_scout_player.png")),
        "hull_scout_enemy"     => Some(include_bytes!("../assets/sprites/hull_scout_enemy.png")),
        "turret_tank_player"   => Some(include_bytes!("../assets/sprites/turret_tank_player.png")),
        "turret_tank_enemy"    => Some(include_bytes!("../assets/sprites/turret_tank_enemy.png")),
        "turret_arty_player"   => Some(include_bytes!("../assets/sprites/turret_arty_player.png")),
        "turret_arty_enemy"    => Some(include_bytes!("../assets/sprites/turret_arty_enemy.png")),
        "turret_aa_player"     => Some(include_bytes!("../assets/sprites/turret_aa_player.png")),
        "turret_aa_enemy"      => Some(include_bytes!("../assets/sprites/turret_aa_enemy.png")),
        _ => None,
    }
}

fn load_png(bytes: &[u8]) -> Texture2D {
    // No set_filter call here — calling it during startup corrupts macroquad's
    // font atlas GL state, causing draw_text to produce black boxes.
    Texture2D::from_file_with_format(bytes, Some(ImageFormat::Png))
}

// ── Sprites ───────────────────────────────────────────────────────────────────

pub struct Sprites {
    pub atlas: Texture2D,
    unit_index: HashMap<String, usize>,
    /// Keyed by sprite base name (e.g. "hull_tank"); value = [player_tex, enemy_tex].
    /// Empty until load_hull_turrets() is called on frame 1.
    hulls:   HashMap<String, [Texture2D; 2]>,
    turrets: HashMap<String, [Texture2D; 2]>,
}

impl Sprites {
    /// Call once at startup — loads only the tile atlas.
    /// Safe before macroquad's font atlas exists.
    pub fn load() -> Self {
        let atlas = Texture2D::from_file_with_format(
            include_bytes!("../assets/sprites/atlas.png"),
            Some(ImageFormat::Png),
        );
        atlas.set_filter(FilterMode::Nearest);

        let mut unit_index = HashMap::new();
        for (i, name) in UNIT_ORDER.iter().enumerate() {
            unit_index.insert((*name).to_string(), i);
        }
        Self { atlas, unit_index, hulls: HashMap::new(), turrets: HashMap::new() }
    }

    /// Call on frame 1 (after the first next_frame().await).
    /// By then macroquad's font atlas exists, so loading extra textures is GL-safe.
    /// Reads hull_sprite / turret_sprite from the unit definitions and loads only the
    /// sprites that are actually referenced — no hardcoded list.
    pub fn load_hull_turrets(&mut self, units: &[UnitDef]) {
        if !self.hulls.is_empty() { return; } // already loaded

        for unit in units {
            // Hull
            if !unit.hull_sprite.is_empty() && !self.hulls.contains_key(&unit.hull_sprite) {
                let p_key = format!("{}_player", unit.hull_sprite);
                let e_key = format!("{}_enemy",  unit.hull_sprite);
                if let (Some(p), Some(e)) = (sprite_bytes(&p_key), sprite_bytes(&e_key)) {
                    self.hulls.insert(unit.hull_sprite.clone(), [load_png(p), load_png(e)]);
                }
            }
            // Turret
            if !unit.turret_sprite.is_empty() && !self.turrets.contains_key(&unit.turret_sprite) {
                let p_key = format!("{}_player", unit.turret_sprite);
                let e_key = format!("{}_enemy",  unit.turret_sprite);
                if let (Some(p), Some(e)) = (sprite_bytes(&p_key), sprite_bytes(&e_key)) {
                    self.turrets.insert(unit.turret_sprite.clone(), [load_png(p), load_png(e)]);
                }
            }
        }
    }

    /// True once hull/turret textures have been loaded (after frame 1).
    pub fn has_hull_turrets(&self) -> bool { !self.hulls.is_empty() }

    /// Hull texture for a sprite base name + faction. None if not yet loaded or unknown.
    pub fn hull(&self, name: &str, is_enemy: bool) -> Option<&Texture2D> {
        self.hulls.get(name).map(|pair| &pair[is_enemy as usize])
    }

    /// Turret texture. None if not loaded, unknown, or unit has no turret sprite.
    pub fn turret(&self, name: &str, is_enemy: bool) -> Option<&Texture2D> {
        self.turrets.get(name).map(|pair| &pair[is_enemy as usize])
    }

    // ── Atlas helpers (tile rendering + fallback) ─────────────────────────────

    pub fn unit_index(&self, name: &str) -> usize {
        *self.unit_index.get(name).unwrap_or(&0)
    }

    pub fn unit_rect(&self, idx: usize) -> Rect {
        slot_rect(idx.min(UNIT_ORDER.len() - 1))
    }

    pub fn tile_rect(&self, t: Tile) -> Rect {
        slot_rect(match t {
            Tile::Ground | Tile::Chokepoint              => SLOT_GROUND,
            Tile::Water  | Tile::River | Tile::RiverCrossing => SLOT_WATER,
            Tile::Cliff  | Tile::MountainPass             => SLOT_CLIFF,
            Tile::OreBasin | Tile::OilField               => SLOT_RESOURCE,
            Tile::Road                                    => SLOT_GROUND,
        })
    }

    pub fn selection_rect(&self) -> Rect { slot_rect(SLOT_SELECTION) }
}
