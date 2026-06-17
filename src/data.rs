//! Data-driven definitions (units / buildings / factions) loaded from versioned
//! files. Stub for Milestone 0 — establishes where authored content will live and
//! the versioned-schema convention (see PLAN.md, Extensibility).

/// Schema version for on-disk definition files. Bump + migrate on breaking changes.
pub const DEFINITIONS_SCHEMA_VERSION: u32 = 1;
