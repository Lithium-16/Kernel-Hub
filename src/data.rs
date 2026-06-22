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
    /// Weapon damage per second (0 = unarmed).
    pub dps: f32,
    /// Turret rotation speed (rad/s). 0 = no turret (infantry fire direction = hull).
    pub turret_turn_rate: f32,
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
