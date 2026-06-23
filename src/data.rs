//! Data-driven definitions (units / buildings / factions) loaded from versioned
//! files. Authored content lives in `assets/*.ron`, embedded at build time so it
//! works identically on native and WASM. The versioned schema is the extension
//! point (see PLAN.md, Extensibility).

use serde::Deserialize;

/// Schema version for on-disk definition files. Bump + migrate on breaking changes.
pub const DEFINITIONS_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Deserialize)]
pub struct Definitions {
    pub version: u32,
    pub factions: Vec<FactionDef>,
    pub units: Vec<UnitDef>,
    pub buildings: Vec<BuildingDef>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct BuildingDef {
    pub id: String,
    pub name: String,
    pub color: (u8, u8, u8),
    /// Footprint size in tiles.
    pub w: usize,
    pub h: usize,

    /// Health pool; 0 = indestructible.
    #[serde(default)]
    pub hp: f32,

    /// Whether this building has a defensive turret.
    #[serde(default)]
    pub has_turret: bool,

    /// Turret weapon range in world pixels.
    #[serde(default)]
    pub turret_range: f32,

    /// Turret damage per shot.
    #[serde(default)]
    pub turret_damage: f32,

    /// Turret shots per second.
    #[serde(default)]
    pub turret_fire_rate: f32,

    /// Turret rotation speed (rad/s).
    #[serde(default)]
    pub turret_turn_rate: f32,

    /// Whether this building acts as a supply depot.
    #[serde(default)]
    pub has_depot: bool,

    /// Depot resupply radius in world pixels; 0 = use default 600.0.
    #[serde(default)]
    pub depot_supply_range: f32,

    /// Vision radius in tiles for radar/observation; 0 = no radar.
    #[serde(default)]
    pub vision_range_tiles: f32,

    /// Building Supplies required to construct; 0 = instant (HQ / scenario-start buildings).
    #[serde(default)]
    pub required_supplies: u32,

    /// Construction progress per engineer per second (0.0 = use default 0.1).
    #[serde(default)]
    pub build_rate: f32,

    /// Hull sprite base name (future rendering use). Empty = placeholder rectangle.
    #[serde(default)]
    pub hull_sprite: String,

    /// Turret sprite base name (future rendering use). Empty = no separate turret sprite.
    #[serde(default)]
    pub turret_sprite: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct FactionDef {
    pub id: String,
    pub name: String,
    pub color: (u8, u8, u8),
}

#[derive(Debug, Clone, Deserialize)]
pub struct UnitDef {
    pub id: String,
    pub name: String,
    pub faction: String,
    pub sprite: String,
    pub color: (u8, u8, u8),
    pub radius: f32,
    pub speed: f32,
    pub turn_rate: f32,
    pub hp: f32,
    /// Weapon range in world px (0 = unarmed).
    pub range: f32,
    /// Damage per shot (0 = unarmed).
    pub damage: f32,
    /// Shots per second (0 = unarmed).
    pub fire_rate: f32,
    /// Turret rotation speed (rad/s). 0 = no turret (infantry fire direction = hull).
    pub turret_turn_rate: f32,
    /// Vision radius in world px. 0 = use the global default (fog::VISION_RADIUS_PX).
    #[serde(default)]
    pub vision_range: f32,
    /// Onboard ammo capacity (shots). 0 = unit has no ammo storage (unarmed or infinite).
    #[serde(default)]
    pub ammo_capacity: u32,
    /// Onboard fuel capacity. 0 = unit has no fuel tank (foot soldiers).
    #[serde(default)]
    pub fuel_capacity: f32,
    /// Fuel burn rate (units per world-pixel moved). 0 = use default (0.05).
    #[serde(default)]
    pub fuel_burn_rate: f32,
    /// Hull sprite base name. e.g. "hull_tank" → hull_tank_player.png + hull_tank_enemy.png.
    /// Empty string = use placeholder atlas sprite.
    #[serde(default)]
    pub hull_sprite: String,
    /// Turret sprite base name. e.g. "turret_tank". Empty = no separate turret sprite.
    #[serde(default)]
    pub turret_sprite: String,
}

/// Load and validate the bundled definitions.
pub fn load_definitions() -> Definitions {
    const SRC: &str = include_str!("../assets/definitions.ron");
    let defs: Definitions = ron::from_str(SRC).expect("failed to parse assets/definitions.ron");
    assert_eq!(
        defs.version, DEFINITIONS_SCHEMA_VERSION,
        "definitions.ron schema version mismatch"
    );
    defs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn definitions_parse_and_validate() {
        let defs = load_definitions();
        assert_eq!(defs.version, DEFINITIONS_SCHEMA_VERSION);
        assert!(!defs.factions.is_empty(), "expected at least one faction");
        assert!(!defs.units.is_empty(), "expected at least one unit");
        // Every unit references a defined faction.
        for u in &defs.units {
            assert!(
                defs.factions.iter().any(|f| f.id == u.faction),
                "unit {} references unknown faction {}",
                u.id,
                u.faction
            );
        }
    }
}
