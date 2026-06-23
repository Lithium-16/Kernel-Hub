//! Sprite system. Tiles use a small placeholder atlas; units use real tank sprites
//! from the asset pack (256×256 hull + 256×256 turret, both centred on the same pivot).
//!
//! Player faction → Color_A sprites  (olive green)
//! Enemy faction  → Color_D sprites  (dark/sand)
//!
//! Unit index mapping (stable — used in Renderable.sprite):
//!   0=tank  1=arty  2=aa  3=engineer  4=truck  5=infantry  6=scout

use macroquad::prelude::*;

use crate::map::Tile;

// ── Tile atlas (placeholder, unchanged) ──────────────────────────────────────

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

// ── Unit sprite index ─────────────────────────────────────────────────────────

const UNIT_NAMES: [&str; 7] = ["tank", "arty", "aa", "engineer", "truck", "infantry", "scout"];

fn unit_idx(name: &str) -> usize {
    // legacy aliases kept so old definitions.ron sprite names still work
    let name = match name {
        "artillery" => "arty",
        "engineer"  => "engineer",
        other       => other,
    };
    UNIT_NAMES.iter().position(|&n| n == name).unwrap_or(0)
}

// ── Texture loader helper ─────────────────────────────────────────────────────

fn load_tex(bytes: &[u8]) -> Texture2D {
    let t = Texture2D::from_file_with_format(bytes, Some(ImageFormat::Png));
    t.set_filter(FilterMode::Linear);
    t
}

// ── Sprites struct ────────────────────────────────────────────────────────────

pub struct Sprites {
    // Tile atlas (placeholder art, still used for terrain tiles)
    pub atlas: Texture2D,

    // Per-unit hull textures (256×256, top-down, pointing UP in the image).
    // [unit_idx][0=player, 1=enemy]
    hulls: [[Texture2D; 2]; 7],

    // Per-unit turret textures (256×256, same canvas/pivot as hull).
    // Only armed units have one; index matches hulls.
    // Inner Option is None for unarmed units.
    turrets: [Option<[Texture2D; 2]>; 7],
}

impl Sprites {
    pub fn load() -> Self {
        let atlas = Texture2D::from_file_with_format(
            include_bytes!("../assets/sprites/atlas.png"),
            Some(ImageFormat::Png),
        );
        atlas.set_filter(FilterMode::Nearest);

        // Hull textures — order must match UNIT_NAMES
        let hulls = [
            // 0: tank
            [load_tex(include_bytes!("../assets/sprites/hull_tank_player.png")),
             load_tex(include_bytes!("../assets/sprites/hull_tank_enemy.png"))],
            // 1: arty
            [load_tex(include_bytes!("../assets/sprites/hull_arty_player.png")),
             load_tex(include_bytes!("../assets/sprites/hull_arty_enemy.png"))],
            // 2: aa
            [load_tex(include_bytes!("../assets/sprites/hull_aa_player.png")),
             load_tex(include_bytes!("../assets/sprites/hull_aa_enemy.png"))],
            // 3: engineer
            [load_tex(include_bytes!("../assets/sprites/hull_engineer_player.png")),
             load_tex(include_bytes!("../assets/sprites/hull_engineer_enemy.png"))],
            // 4: truck
            [load_tex(include_bytes!("../assets/sprites/hull_truck_player.png")),
             load_tex(include_bytes!("../assets/sprites/hull_truck_enemy.png"))],
            // 5: infantry
            [load_tex(include_bytes!("../assets/sprites/hull_infantry_player.png")),
             load_tex(include_bytes!("../assets/sprites/hull_infantry_enemy.png"))],
            // 6: scout
            [load_tex(include_bytes!("../assets/sprites/hull_scout_player.png")),
             load_tex(include_bytes!("../assets/sprites/hull_scout_enemy.png"))],
        ];

        // Turret textures — None for units without weapons
        let turrets = [
            Some([load_tex(include_bytes!("../assets/sprites/turret_tank_player.png")),
                  load_tex(include_bytes!("../assets/sprites/turret_tank_enemy.png"))]),  // tank
            Some([load_tex(include_bytes!("../assets/sprites/turret_arty_player.png")),
                  load_tex(include_bytes!("../assets/sprites/turret_arty_enemy.png"))]),  // arty
            Some([load_tex(include_bytes!("../assets/sprites/turret_aa_player.png")),
                  load_tex(include_bytes!("../assets/sprites/turret_aa_enemy.png"))]),    // aa
            None,  // engineer — unarmed
            None,  // truck — unarmed
            None,  // infantry — no separate turret sprite (fires from hull direction)
            None,  // scout — unarmed
        ];

        Self { atlas, hulls, turrets }
    }

    /// Stable index for a unit sprite name. Used in Renderable.sprite.
    pub fn unit_index(&self, name: &str) -> usize {
        unit_idx(name)
    }

    /// Hull texture for `unit_idx` and `is_enemy` flag.
    pub fn hull(&self, idx: usize, is_enemy: bool) -> &Texture2D {
        &self.hulls[idx.min(6)][is_enemy as usize]
    }

    /// Turret texture for `unit_idx` and `is_enemy` flag. None for unarmed units.
    pub fn turret(&self, idx: usize, is_enemy: bool) -> Option<&Texture2D> {
        self.turrets[idx.min(6)].as_ref().map(|pair| &pair[is_enemy as usize])
    }

    // ── Tile helpers (unchanged) ──────────────────────────────────────────────

    pub fn tile_rect(&self, t: Tile) -> Rect {
        slot_rect(match t {
            Tile::Ground | Tile::Chokepoint => SLOT_GROUND,
            Tile::Water | Tile::River | Tile::RiverCrossing => SLOT_WATER,
            Tile::Cliff | Tile::MountainPass => SLOT_CLIFF,
            Tile::OreBasin | Tile::OilField => SLOT_RESOURCE,
            Tile::Road => SLOT_GROUND,
        })
    }

    pub fn selection_rect(&self) -> Rect { slot_rect(SLOT_SELECTION) }

    // Kept for any remaining callers
    pub fn unit_rect(&self, _idx: usize) -> Rect { slot_rect(0) }
}
