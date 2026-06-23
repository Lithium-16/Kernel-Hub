# PLAN.md — Implementation Plan

> Companion to **PROJECT.md** (which holds the vision/design/decisions). This file is the
> *actionable build plan*: milestones, the systems each phase delivers, concrete tasks, key
> data types, and acceptance criteria. Update checkboxes and notes as work progresses.
>
> **Last updated:** 2026-06-22 · **Status:** M0 + Phase 1 + Phase 2 + Phase 3 done. Phase 4 next.
>
> **LOGISTICS DESIGN (2026-06-22)** — Resources are fully physical. No teleportation. Every resource
> exists at a location, must be driven to its destination by a truck, and can be intercepted or
> destroyed en route. The player manages *routes, priorities, stockpiles, infrastructure* — the
> game manages individual trucks, deliveries, loading/unloading, and route execution. The player
> should never feel like they're playing Factorio. See "Logistics System" section below.

## How to use this plan
- Phases are **sequential** and each ends in a **verifiable, runnable build**. Do not start a phase
  before the prior one's acceptance criteria pass.
- **Verify every phase** two ways: `cargo test` (logic) + a screenshot (WASM-in-browser or native
  window). Never mark a phase done on "it compiles."
- **Stress-test scale early and continuously** — from Phase 2 on, every build runs a 1,000+-unit
  scene and reports frame/tick timings. Scale is a feature, not an afterthought.
- Keep everything **data-driven** (units/buildings/factions in data files) from Phase 1 so content
  is authored, not coded.
- **Use version control properly** — branch per phase/feature, small Conventional Commits, merge to
  `main` via PR only when CI is green; update PROJECT.md/PLAN.md in the same PR as the work.

## Guiding technical principles
- **Fixed-timestep sim (20 Hz) decoupled from render**, with render interpolation. Deterministic-friendly.
- **Data-oriented ECS (hecs)** — components are plain data; systems are functions over queries.
- **No O(n²) anywhere** — all neighbor/range queries go through the spatial grid.
- **Pluggable boundaries** — renderer, pathfinding, and AI behind clean interfaces so pieces can be
  swapped (e.g. renderer → custom WebGL, or macroquad → Bevy) without rewrites.
- **Extensible by construction** — data-driven content, ECS composition + ability lists, trait-based
  system boundaries, registries (no central match-statements), an event bus, and versioned data/map
  schemas. Goal: add or overhaul a system by adding code/data, not rewriting existing systems.
- **Target build matrix:** native (`cargo run`, performance) + WASM (`wasm32-unknown-unknown`,
  browser verification). Both must build at every phase.

---

## Next phase priorities (post direction update 2026-06-22)
These are the highest-value next builds given the new operational RTS direction. In order:

1. **Combat Group system** — group entity that owns a list of unit entities; group-level UI (assign
   objective, set stance, request support, reinforce); individual units still use flow-field movement +
   utility AI inside the group. This is the new primary player interface.

2. **Geographic terrain** — mountain, river, pass, valley, and chokepoint tiles in the map generator
   (noise + rules); terrain affects movement speed + passability; chokepoints are narrow tiles.
   Rivers require crossing tiles. This makes geography matter strategically.

3. **Logistics intent UI** — player draws a route between two points; depots auto-placed along it;
   the system dispatches trucks automatically. First version: single corridor, one resource type.

4. **Expansion flow** — resource regions marked on the map; player secures one → can build an extractor;
   extractor feeds the logistics network. The core gameplay loop.

5. **Reconnaissance** — fog of war (tiles unseen until a unit moves nearby); Combat Groups have a
   vision radius; the player discovers the map through movement.

6. **Automated rear defense** — threat notification when an enemy enters a rear zone; a designated
   QRF group auto-responds; patrols along defined corridors.

---

## Milestone 0 — Project setup & tooling
**Goal:** a runnable empty macroquad window, building to native + WASM, with a test harness.

- [x] Install Rust toolchain (`rustup` 1.29) + VS Build Tools (MSVC); add `wasm32-unknown-unknown`. ✓
- [x] Cargo project (`coldwar-rts`) with `macroquad` 0.4 + `hecs` 0.10, versions pinned. ✓
- [x] "Hello window" — macroquad window + fixed-timestep loop + placeholder scene/overlay. ✓
- [x] WASM build pipeline — `scripts/build-wasm.sh`; runs in browser (console-verified, no errors). ✓
      Note: needed `.cargo/config.toml` linker flag (`--import-undefined`) for macroquad on recent Rust.
- [x] Screenshot verification — via native offscreen render-target capture (`COLDWAR_CAPTURE`). ✓
      Note: browser preview can't screenshot a continuously-animating canvas; native capture is the loop.
- [x] **Git init** + `.gitignore` + `.gitattributes`; initial commit of PROJECT.md/PLAN.md. ✓ 2026-06-17
- [x] **GitHub repo** (private) created & pushed → https://github.com/B0RE16/laughing-waffle ✓ 2026-06-17
- [x] **Branch/commit conventions** adopted: branch per phase/feature, Conventional Commits, PR merges.
- [x] **GitHub Actions CI** added (build/test/clippy/wasm on push/PR); running on GitHub. ✓
- [x] Repo hygiene: module layout (`sim`, `render`, `ecs`, `data`) + `.cargo/config.toml`. ✓

**Acceptance:** native + WASM both open a window; `cargo test` runs (even if empty); a screenshot
of the running app is captured; the repo is on GitHub and CI is green.

---

## Phase 1 — Engine skeleton
**Goal:** a fixed-timestep engine drawing a large tilemap with placeholder sprites and a pannable
camera; unit/building definitions loaded from data.

**Build:**
- [x] **Core loop** — fixed-timestep accumulator (20 Hz) + variable-rate render. ✓ (interpolation deferred until entities move)
- [x] **ECS bootstrap** — hecs world; starter components `Position`, `Renderable`, `Faction` (Velocity later). ✓
- [x] **Renderer** — view-culled tile draw + entity draw through the game camera; offscreen-capture path. ✓ (sprite batching when needed)
- [x] **Camera** — WASD/arrow pan + mouse-wheel zoom, clamped to map bounds. ✓
- [x] **Tilemap** — 256×256 grid (ground/water/cliff/resource); only visible tiles drawn. ✓
- [x] **Data layer scaffold** — units/factions loaded from `assets/definitions.ron` (versioned); entities spawned from defs. ✓
- [ ] **Map format (v1)** — versioned, layered map *file* (procedural test map for now; file format slated for Phase 1.5).
- [ ] **Extensibility scaffolding** — registries + event bus + swappable-system traits. *Moved to Phase 3* (now a prerequisite for the ability/transition system).
- [x] **Debug overlay** — FPS, tick, entity count, camera pos/zoom, map size. ✓

**Key types:** `Tile`, `TileMap`, `Camera2D`, `UnitDef`/`BuildingDef`/`FactionDef`, `World` wrapper.

**Acceptance:** pan/zoom over a 256×256 map; placeholder units rendered from data definitions;
debug overlay live; native + WASM screenshots match; `cargo test` covers map + def loading.

---

## Phase 1.5 — Map system & editor
**Goal:** a usable in-engine map editor and a solid, versioned map format — so every later system is
easy to test on purpose-built maps. The editor is expanded in later phases as new placeables appear.

**Build:**
- [ ] **Map format hardening** — finalize layered, versioned format (terrain, elevation, passability/
      cost, resource nodes, spawns, markers/triggers, pre-placed infra slots); load/save round-trip.
- [ ] **Editor core** — paint terrain & elevation, place/erase resource nodes & spawn points, set map
      size, undo/redo, save/load.
- [ ] **Test-play** — launch a skirmish on the current map directly from the editor.
- [ ] **Validation** — reachability, resource balance, spawn fairness warnings.
- [ ] **Tile/terrain data-driven** — new terrain types added in data appear in the editor palette.
- [ ] (Stretch) **Procedural generator** emitting the same format.

**Key types:** `MapFile` (versioned), `MapLayer`, `EditorState`, `BrushTool`, `MapValidator`.

**Acceptance:** author a map in-editor, save, reload, and test-play it; a new terrain type added via
data shows up in the palette; editor screenshot; tests for save/load round-trip + validation.

> Note: the editor is a living tool — Phases 4–7 add resource/infrastructure/zone/trigger placement
> to it as those systems land.

---

## Phase 2 — Pathfinding at scale
**Goal:** hundreds of units move smoothly to a destination using flow fields, with local avoidance —
proven under a 1,000+-unit stress test.

**Build:**
- [x] **Spatial grid** — uniform bucket grid; radius neighbor queries; rebuilt per tick. ✓
- [x] **Navigation grid** — passability derived from the tilemap (cost uniform for now). ✓
- [x] **Flow fields** — 8-neighbour Dijkstra cost field → gradient (wall-aware) per-tile direction, bilinearly sampled; the primary mover. ✓
- [x] **Flow-field cache** — `FlowCache` reuses fields by goal tile (LRU), so converging/repeated orders skip recompute. Decision: stay on flow fields (best for large-RTS groups); HPA*-portal + sector-flow hybrid deferred until map size demands it. ✓ 2026-06-18
- [ ] **A\*** — single-unit fallback for stragglers/special cases (deferred; flow field covers group moves).
- [x] **Local avoidance** — separation + "around" steering via the spatial grid; units don't stack. ✓
- [x] **Movement system** — turn-then-move steering (flow/slot dir + avoidance); positional collision pass; stall-based arrival. ✓
- [x] **Formation slots** — group moves assign each unit its own slot in a packed block (greedy nearest) and seek it once near the formation anchor, instead of all crushing one point. Movers shove idle/arrived units aside so the block actually fills (no edge-jamming). Killed the packed-group jitter AND made formations settle perfectly: 200/600/1200-unit residual motion now 0.00 px/tick, 0 units left moving. `COLDWAR_SETTLE` headless jitter metric. ✓ 2026-06-18, push-through 2026-06-21
- [ ] **Staggered ticks** — not needed yet (1,200-unit tick ≈ 2.3 ms); available lever if sim/render grows.
- [x] **Stress harness** — `COLDWAR_UNITS` spawns N; overlay reports tick ms + counts (1,200 verified). ✓

**Key types:** `SpatialGrid`, `NavGrid`, `FlowField`, `PathRequest`, `Movement` component.

**Acceptance:** 1,000+ units path around obstacles to a shared goal without stacking; tick time
within budget (recorded baseline); screenshot of a mass move; tests for flow-field correctness &
grid queries.

---

## Phase 2.5 — Modular & scalable UI system
**Goal:** a reusable UI toolkit every later panel is built on (command card, build menu, economy
readouts, minimap, modals) — consistent, themeable, resolution-scalable, with proper input layering.

**Build:**
- [~] **Immediate-mode widget core** — Panel, Button, Label, Bar, Tooltip done; IconButton, Grid,
      ScrollList, ContextMenu, Modal still to add. Drawn in the screen-space pass.
- [~] **Layout** — panels anchor to window size (reflow on resize) off `hud::TOP_H`/`BAR_H`; full stack/grid helpers + DPI handling still to add.
- [~] **Theming** — `Theme` struct (colors/font) applied everywhere incl. colored labels; not yet data-driven or icon-atlas backed.
- [x] **Input layering** — world input gated on computed `HudLayout` rects (no click leak to the map). ✓ 2026-06-18
- [~] **Panel registry** — panels consolidated in `hud.rs` (top resource bar w/ hover tooltips + overdraw warning, bottom command bar, selection panel by type, command card) + **minimap** (`minimap.rs`: baked terrain texture, live unit dots, viewport box, click/drag-to-pan); `economy::Economy` stub backs the bar. Real register-don't-switch registry deferred to Phase 3. ✓ panels 2026-06-21
- [ ] **UI icon atlas** + batched draw.

**Key types:** `Ui`, `Widget`, `Layout`, `Theme`, `PanelId`, `InputCapture`.

**Acceptance:** a themed HUD with a working button/panel that captures its own clicks (no leak to the
world); resizes cleanly; screenshot; tests for layout + input-capture logic.

---

## Phase 3 — Combat Groups + Reconnaissance (current)
**Goal:** the two features the new direction depends on most before logistics can be layered on top.
Individual units still exist and fight — Combat Groups are a command layer above them.
Fog of war makes geography matter.

**Build:**
- [~] **Combat core** — `Health`, `Weapon`, discrete shots, turret aiming, two factions, health bars. ✓ 2026-06-21
- [~] **Selection & command UI** — drag-box, Shift-add, double-click-type, control groups 1–9, waypoint queueing. ✓ 2026-06-21
- [~] **Building placement** — ghost preview, grid snap, validity check, nav-blocking. ✓ 2026-06-21
- [~] **Stances** — Aggressive / Defensive / Hold-Ground on command card. ✓ 2026-06-21
- [x] **Combat Group entity** — `CombatGroup` owns unit entities; clickable group cards in HUD (name, strength bar, losses %, green highlight when selected); G key cycles groups; click card → select members + center camera. ✓ 2026-06-22
- [x] **Group orders** — AdvanceTo/Hold/Withdraw wired in main loop; AI brain uses groups; individual unit moves fan out from group orders. ✓ 2026-06-22
- [x] **Attack-move** — `attack_move` flag on `MoveOrder`; units fire at enemies in range while advancing; stall-settle disabled so they push through to goal. ✓ 2026-06-22
- [x] **Fog of war** — `FogGrid` (Hidden/LastSeen/Visible per tile); updated per tick from player unit VisionRange; rendered as black/dimmed overlay; enemy units hidden in fog; HQ pre-revealed 20-tile radius. ✓ 2026-06-22
- [x] **Geographic terrain tiles** — Mountain/Cliff (impassable), MountainPass (2× movement cost), River (impassable), RiverCrossing (2.5× cost), Chokepoint; NavGrid stores per-tile cost used in FlowField Dijkstra so paths correctly prefer flat ground over passes. ✓ 2026-06-22
- [x] **Recon Group unit type** — `scout` unit def (120 px/s, 35 HP, 480px vision = 3× default); `vision_range` field on UnitDef with serde default; 4 scouts per side spawn as "1st Recon Group". ✓ 2026-06-22

**Key types:** `CombatGroup`, `GroupOrder`, `FogOfWar`, `TileProperties`, `VisionRadius`.

**Acceptance:** player forms a group, orders it to advance, it engages enemies en route; fog hides the map until explored; radar reveals a large area; geographic tiles affect movement; `COLDWAR_ASSERT=formation_fills` still passes.

---

## Phase 3.5 — Building Framework (NEXT)
**Goal:** Buildings become real game entities with health, type identity, optional turrets, and
storage. The placement system bridges into functional components. HQ becomes a proper entity
whose destruction is a win condition. Units and buildings share the same combat system — no
special targeting code needed.

### Design

**Every placed building is a single ECS entity with:**
- `Building { tx, ty, w, h }` — footprint + nav blocking (already exists, keep)
- `BuildingKind(String)` — type id matching BuildingDef (new)
- `Faction(String)` — ownership
- `Health { cur, max }` — makes it targetable by the existing combat system automatically;
  units in Aggressive stance will attack enemy buildings in weapon range just like enemy units
- `Position(Vec2)` — world centre of the footprint (needed for range queries, turret rotation)
- Optional functional components added at spawn based on kind:
  - `Depot` — HQ, Supply Depot (resource storage + resupply radius)
  - `Turret + Weapon` — Gun Turret, Bunker, AA Tower (auto-fires at enemies in range)
  - `AmmoStorage` — armed buildings draw from nearest friendly depot to reload
  - `VisionRange` — Radar Station (large fog reveal radius)
  - `Extractor` — Mine, Oil Pump (already exists)

### Building types to define in definitions.ron

| id | Name | Size | Function | Turret |
|---|---|---|---|---|
| `hq` | Headquarters | 3×3 | Depot (large); win-condition target | No |
| `depot` | Supply Depot | 2×2 | Depot (standard) | No |
| `bunker` | Bunker | 2×2 | Health 500; infantry garrison cover bonus | MG (range 140, dmg 8, rate 3/s) |
| `gun_turret` | Gun Turret | 1×1 | Standalone heavy turret | Cannon (range 200, dmg 40, rate 0.6/s) |
| `aa_tower` | AA Tower | 2×2 | Anti-vehicle/air turret | AA gun (range 200, dmg 20, rate 2/s) |
| `radar` | Radar Station | 2×2 | Vision radius 40 tiles (fog reveal) | No |
| `mine` | Mine | 2×2 | Extracts Ore (already exists) | No |
| `oil_pump` | Oil Pump | 2×2 | Extracts Oil (already exists) | No |
| `processing` | Processing Facility | 3×3 | Ore → Supplies + Parts | No |
| `refinery` | Fuel Refinery | 2×2 | Oil → Fuel | No |
| `ammo_factory` | Ammo Factory | 2×2 | Parts → Ammo | No |

### BuildingDef additions (definitions.ron)
```ron
hp: 200.0          // health pool; 0 = indestructible (for map decorations)
has_turret: false  // whether to attach Turret + Weapon at spawn
turret_range: 0.0  turret_damage: 0.0  turret_fire_rate: 0.0  turret_turn_rate: 0.0
has_depot: false   depot_supply_range: 0.0  depot_start_stock: false
vision_range: 0.0  // > 0 → VisionRange component (Radar)
hull_sprite: ""    turret_sprite: ""  // future: building sprites
```

### HQ entity
Spawned at scenario start for each faction at `map.player_spawn()` / `map.enemy_spawn()`.
Has `Building + BuildingKind("hq") + Health(1000) + Depot + Faction`. Destroying the enemy HQ
triggers the Decapitation win condition (Phase 7 checks for `Health.cur <= 0`).

### Placement system
When the player places a building (B key), `spawn_building()` reads the BuildingDef and attaches
the correct functional components automatically — no manual wiring per type.

### Targeting
No new targeting code needed. The existing `combat::step` already finds the nearest enemy entity
with `Health` in weapon range. Buildings with `Health + Faction + Position` are automatically
valid targets. Units will attack enemy buildings they encounter while advancing.

**Build:**
- [ ] **BuildingDef extended** — add `hp`, turret fields, depot fields, `vision_range`, sprite names to BuildingDef; update definitions.ron with all building types.
- [ ] **BuildingKind component** — `pub struct BuildingKind(pub String)` in components.rs; all placed buildings get this.
- [ ] **Health on buildings** — all placed buildings get `Health { cur, max }` from BuildingDef.hp; health bar drawn for damaged buildings.
- [ ] **Position on buildings** — centre-of-footprint Vec2; needed for range queries and turret aiming.
- [ ] **Armed buildings** — buildings with `has_turret: true` get `Turret + Weapon + AmmoStorage`; existing `combat::step` handles firing automatically.
- [ ] **Depot buildings** — buildings with `has_depot: true` get `Depot` component; resupply system works unchanged.
- [ ] **Radar buildings** — buildings with `vision_range > 0` get `VisionRange`; fog system works unchanged.
- [ ] **`spawn_building()` function** — reads BuildingDef, spawns entity with all appropriate components, nav-blocks footprint.
- [ ] **HQ entities** — spawned at scenario start for both factions; player HQ health shown in HUD.
- [ ] **Building render** — draw health bars on damaged buildings; future: hull sprites from BuildingDef.
- [ ] **Building select + info** — clicking a building shows its type, health, and (if depot) stockpile in the selection panel.
- [ ] **`COLDWAR_ASSERT=hq_targetable`** — unit attacks enemy HQ, HQ health decreases.
- [ ] **`COLDWAR_ASSERT=turret_fires`** — gun turret auto-fires at enemy unit in range.

### Blueprint → Engineer construction

**How it works:**
1. Player places a blueprint (B key, ghost outline as now). Blueprint is a lightweight ECS entity:
   `Blueprint { building_id: String, tx, ty, w, h, progress: f32, required_supplies: u32, faction }`
2. Blueprint appears on the map as a translucent footprint with a progress bar. It nav-blocks
   immediately (no unit can walk through it), but is not yet functional.
3. Any Engineer unit within range of the blueprint auto-claims it if idle — no manual assignment
   needed. Engineer pathfinds to the blueprint, then stands adjacent and builds.
4. Each sim tick an Engineer is building: `progress += build_rate * dt`. Building Supplies are
   consumed from the nearest depot (1 supply per N progress, checked every second).
   If the depot runs dry, progress pauses until resupplied.
5. When `progress >= 1.0`: `spawn_building()` is called, the Blueprint entity is despawned,
   the real building entity replaces it. The Engineer becomes idle.
6. Multiple Engineers on one blueprint stack build rates (2 engineers = 2× speed).
7. Blueprint can be cancelled (right-click → despawn; nav-block removed).

**Key types:**
```
Blueprint {
    building_id: String,      // which BuildingDef to build
    tx: usize, ty: usize,    // footprint origin
    w: usize,  h: usize,
    progress: f32,            // 0.0 → 1.0
    required_supplies: u32,   // total supplies to complete
    supplies_consumed: u32,   // running total drawn so far
    faction: String,
}
```

**Build:**
- [ ] **Blueprint component + entity** — spawned when player commits a ghost placement (replaces the immediate `spawn_building` call for player-placed buildings; HQ/starting depots still spawn directly).
- [ ] **Engineer auto-claim** — idle Engineers of the same faction within map range pathfind to nearest unclaimed Blueprint and begin building.
- [ ] **Build progress system** — `construction::step()` per sim tick: for each Blueprint with an adjacent Builder engineer, increment progress, withdraw Building Supplies from nearest depot.
- [ ] **Blueprint render** — translucent footprint + progress bar; colour shifts from ghost-white to faction colour as progress increases.
- [ ] **Completion** — on `progress >= 1.0`, despawn Blueprint, call `spawn_building()`.
- [ ] **Supply gate** — if no Building Supplies available at nearest depot, progress halts; amber warning icon on blueprint.
- [ ] **`COLDWAR_ASSERT=blueprint_builds`** — place blueprint, spawn engineer near it with depot stocked, verify building exists after N ticks.

**Key types:** `Blueprint`, `construction::step()`.

**Acceptance:** place a Gun Turret → it auto-fires at approaching enemies; place an HQ → it has
health that decrements when attacked; place a Depot → it stores resources and resupplies nearby
units; place a blueprint with Engineers nearby → building completes as supplies are consumed;
all 6 existing ASSERT scenarios still pass.

---

## Logistics System (canonical design — governs Phases 4–6)

> **Core philosophy: resources are physical.** Every resource exists at a specific location.
> It must be driven through the world by a truck. It can be stored, transported, intercepted,
> or destroyed. Nothing teleports.

### What the player manages
- **Routes** — origin depot → destination depot, resource type, priority (High/Med/Low)
- **Desired stockpiles** — per depot per resource (e.g. Eastern Pass Depot: Ammo desired 5000, current 2700); the system sends trucks to fill the gap
- **Infrastructure** — roads determine convoy speed and throughput; damaged roads choke supply
- **Depot placement** — every region should have a depot; depots are regional warehouses

### What the game manages automatically
- Individual trucks (spawned, routed, loaded, driven, unloaded, returned)
- Convoy grouping (multiple trucks on same route naturally form a convoy)
- Loading/unloading at depot
- Route execution and rerouting around damage

### Resource types

| Resource | Produced by | Consumed by | When exhausted |
|---|---|---|---|
| Ore | Mine (on OreBasin tile) | Processing Facility | no Parts/Supplies output |
| Oil | Oil Pump (on OilField tile) | Fuel Refinery | no Fuel output |
| Ammo | Ammo Factory (Parts→Ammo) | All armed units (per shot, from onboard storage) | unit cannot fire |
| Fuel | Fuel Refinery (Oil→Fuel) | Trucks, tanks, vehicles (continuous burn while moving) | vehicle stranded |
| Building Supplies | Processing Facility (Ore→Supplies) | Construction, repairs, roads | no building/repair |
| Weapon Parts | Processing Facility (Ore→Parts) | Ammo Factory input, reinforcements | no replacement units |

### Per-unit onboard storage (physical, not a depot gate)
- Every armed unit has `AmmoStorage { shots: u32, capacity: u32 }` — its own shells onboard
- Every vehicle has `FuelTank { fuel: f32, capacity: f32 }` — its own fuel onboard
- Unit fires → `shots -= 1`; unit moves → `fuel -= burn_rate * dt`
- When `shots == 0`: gun is physically empty; unit cannot fire until restocked
- When `fuel == 0.0`: engine dead; unit cannot move until refueled
- **Restocking**: unit drives within range of a Depot → depot transfers a batch to the unit's storage. OR a supply truck drives to the unit. The depot must have stock; the resource must have physically arrived there by truck.

### Trucks
- Physical vehicles with `Health`, `Position`, `Faction`, `Cargo { resource: ResourceType, amount: u32 }`
- Follow flow-field routes between depots; visible on map; can be destroyed
- On arrival at destination: transfer cargo to depot's stockpile
- Return to origin when empty
- Multiple trucks on same route → natural convoy; can be escorted by combat groups

### Throughput model
- Road tile quality determines convoy speed (already in nav: off-road 1×, pass 0.5×)
- Damaged road tiles (future: artillery craters) reduce speed further → throughput drops
- Throughput = trucks/minute × cargo per truck; if consumption > throughput, depot drains

### Logistics warfare
- Attack roads → trucks slow, throughput drops, frontline depot drains
- Attack convoys → cargo lost, trucks gone
- Attack depots → stockpile destroyed
- Player defends by: escorts, alternate routes, QRF response, road repair engineers

---

## Phase 4 — Per-unit Storage + Depots + Extraction
**Goal:** resources exist physically. Units carry onboard ammo/fuel and run dry. Depots store resources. Extractors produce into local depots. No trucks yet — starting stock only.

**Build:**
- [x] **Resource types defined** — `ResourceType` enum (Ammo/Fuel/BuildingSupplies/WeaponParts) in depot.rs. ✓
- [x] **Depot entity** — `Depot` struct with stockpile HashMap, supply_range, faction. `depot.rs` complete. ✓
- [x] **Per-unit ammo storage** — `AmmoStorage { shots: u32, capacity: u32 }` component on all armed units; spawned with full load; `combat::step` decrements on each shot; at 0 gun cannot fire. ✓ 2026-06-22
- [x] **Per-unit fuel tank** — `FuelTank { fuel: f32, capacity: f32 }` on all mobile units; `movement::step` burns fuel proportional to speed; at 0 unit cannot move. ✓ 2026-06-22
- [x] **Depot resupply radius** — unit within `supply_range` of same-faction Depot with stock → transfers batch to unit's AmmoStorage/FuelTank (once per ~5s per unit, not per tick). ✓ 2026-06-22
- [x] **Starting depot spawn** — one Depot per side near HQ with starting stock (Ammo 2000, Fuel 1500, Supplies 800, Parts 400); HUD top bar shows real totals from player depots (amber <200, red =0). ✓ 2026-06-22
- [x] **Extraction buildings** — Mine → produces BuildingSupplies into attached depot every 8s; Oil Pump → produces Fuel every 10s. Placed via `extraction::Extractor` component; no Engineers required yet. ✓ 2026-06-22
- [ ] **Processing buildings** — Processing Facility (Ore→Supplies+Parts, 15s), Fuel Refinery (Oil→Fuel, 12s), Ammo Factory (Parts→Ammo, 10s). Each attached to a local depot.
- [ ] **HUD depot panel** — click a Depot to see per-resource stockpile bars (current vs desired); amber <25%, red =0.

**Key types:** `AmmoStorage`, `FuelTank`, `Extractor { kind, attached_depot: Entity, cooldown }`, `Processor { kind, attached_depot: Entity, cooldown }`.

**Acceptance (COLDWAR_ASSERT scenarios to add):**
- `ammo_drains`: armed unit fires until AmmoStorage.shots==0, then stops firing for 10 ticks.
- `fuel_drains`: vehicle moves until FuelTank.fuel==0.0, then stops.
- `depot_resupply`: unit near depot gets ammo transferred after 5s.

---

## Phase 5 — Supply Routes + Trucks + Roads
**Goal:** the physical logistics layer. Player draws routes between depots; system dispatches trucks; trucks drive, deliver, return. Roads determine throughput. Cutting a road starves a depot.

**Build:**
- [ ] **Supply Route** — `SupplyRoute { id, origin: Entity, destination: Entity, resource: ResourceType, priority: Priority }`. Player creates via UI: click origin depot → click destination depot → pick resource + priority. Stored in a `RouteRegistry`.
- [ ] **Desired stockpile UI** — per depot, player sets desired amount per resource. System compares current vs desired and triggers dispatch when deficit > truck_capacity.
- [ ] **Truck entity** — `Truck { cargo_type: ResourceType, cargo_amount: u32, route_id: u32, state: TruckState }` with `Health`, `Position`, `Faction`, `Renderable`. TruckState: `{ Idle, DrivingToPickup, Loading, DrivingToDelivery, Unloading, Returning }`.
- [ ] **Truck dispatch** — each tick, RouteRegistry checks all routes: if destination below desired and origin has stock → spawn Truck at origin; truck uses flow-field to drive to destination.
- [ ] **Cargo transfer** — on arrival: `origin_depot.withdraw(resource, amount)` → `truck.cargo_amount`; on delivery: `destination_depot.add(resource, amount)`.
- [ ] **Convoy grouping** — trucks on same route dispatched within 10s of each other auto-form a convoy (shared flow field, travel together).
- [ ] **Route display** — active routes shown as colored lines on world map; trucks visible as sprites driving along them; click route → show throughput stats.
- [ ] **Truck destruction** — enemy units attack trucks; on death, cargo is lost; notification fires.
- [ ] **Road blueprint tool** — player draws road between two points; Engineers auto-build using Building Supplies; improves convoy speed on those tiles.
- [ ] **Route alert** — if truck destroyed or depot runs dry, amber alert on route line + notification.

**Key types:** `SupplyRoute`, `RouteRegistry`, `Truck`, `TruckState`, `Convoy`, `RouteAlert`.

**Acceptance:** create a route, trucks dispatch, cargo transfers; destroy a truck and cargo is lost; depot below desired triggers new dispatch; route line visible on map.

---

## Phase 6 — Interdiction + Region Ownership + Reinforcements
**Goal:** logistics warfare and the operational layer. Players attack and defend supply lines. Regions have ownership. Losses can be replaced if the supply chain is intact.

**Build:**
- [ ] **Loss tracking** — Combat Group card: current/original strength (34/50 tanks).
- [ ] **Reinforcement** — player presses Reinforce on group; system finds nearest factory/reserve with stock; replacement units physically drive from factory to group via road network.
- [ ] **Region ownership** — region held by faction with military presence + a depot; contested when both present; losing region → extraction stops.
- [ ] **Road damage** — artillery can target road tiles; damaged tiles reduce convoy speed; engineers repair using Building Supplies.
- [ ] **Alternate routes** — if route's road is severed, player draws alternate; trucks reroute automatically.
- [ ] **Convoy escort** — player assigns a combat group as escort for a route; escort follows convoys and engages attackers.
- [ ] **AI logistics** — enemy AI builds supply routes, dispatches trucks, attacks player supply lines when advantageous.
- [ ] **Region View** — zoom out past threshold → regional overlay showing ownership, stockpile levels, threat level per region.

**Key types:** `RoadDamage`, `RegionOwnership`, `ConvoyEscort`, `ReinforceJob`, `AiLogisticsPlanner`.

---

## Phase 7 — Automated Rear Defense + Win Conditions + Full Loop
**Goal:** the Eastern Pass reference scenario is fully playable. Automated systems handle rear threats without babysitting. Win conditions close the loop.

**Build:**
- [ ] **Radar threat alerts** — Radar Station detects enemy incursion in its area → notification with location, severity, "Send QRF" button.
- [ ] **QRF designation** — player marks a Combat Group as QRF for a zone; group auto-moves to respond to threats, returns to position after.
- [ ] **Patrol routes** — player draws patrol path, assigns group; group cycles continuously.
- [ ] **Bunker garrison** — infantry group assigned to a Bunker gains cover bonus and reduced damage.
- [ ] **Gun Turret ammo** — Gun Turrets draw ammo from nearest depot; stop firing when empty.
- [ ] **Win conditions** — Decapitation (destroy HQ), Economic Collapse (Weapon Parts=0 + factory destroyed), Territorial Control (hold all resource regions for 10 min). Selectable at match start.
- [ ] **Enemy AI — operational level** — enemy expands toward resource regions, builds supply routes, attacks objectives, interdicts player supply lines when advantageous.
- [ ] **Eastern Pass playable** — designed map with mountain pass, two resource regions, chokepoint; verifies entire nine-phase loop.

**Key types:** `QRFZone`, `PatrolRoute`, `GarrisonBonus`, `WinCondition`, `AiOperationalGoal`.

**Acceptance:** Eastern Pass scenario plays through all nine phases; radar alerts fire correctly; QRF responds and returns; win conditions trigger correctly; enemy AI builds a logistics network and attacks.

---

## Phase 8 — Content, Polish, Campaign
**Build:**
- [ ] Full unit and building roster (all types from §7.3–§7.4).
- [ ] Designed campaign map. Eastern Pass as the tutorial/first mission.
- [ ] Audio: command acks, weapon fire, impact, ambient.
- [ ] Full UI polish pass (icons, tooltips, animations).
- [ ] Performance pass at full scale (500+ units per side, complex road network).

---

## Debug suite (use this, not source reading — see PROJECT.md §13)
All commands are headless native-only, one env-var → one short output → exit.
**This is the primary verification tool for any agent working on this codebase.**

| Command | Output | Use for |
|---|---|---|
| `COLDWAR_ASSERT=combat_discrete` | `PASS/FAIL` | Bullets discrete (not continuous DPS) |
| `COLDWAR_ASSERT=no_friendly_fire` | `PASS/FAIL` | No faction-check regressions |
| `COLDWAR_ASSERT=turret_delays` | `PASS/FAIL` | Turret gates fire correctly |
| `COLDWAR_ASSERT=formation_fills` | `PASS/FAIL` | Formation slots all fill |
| `COLDWAR_QUERY="shots_fired,kills,alive,mean_hp"` | 1 JSON line | Runtime combat sanity |
| `COLDWAR_SETTLE=1000` | 1 line | Jitter metric after group moves |
| `COLDWAR_BENCH=400` | 1 line | Sim perf ms/tick |
| `COLDWAR_EVENTLOG=1` + run | `debug/events.jsonl` | Trace unexpected deaths/moves |
| `cargo test` | `N passed` | Logic unit tests (19 currently) |

Add a new `COLDWAR_ASSERT` scenario whenever a new system could silently regress.

## Cross-cutting systems (where they live)
- **Data-driven definitions & factions** — scaffold in Phase 1; populated through Phases 4–6;
  second faction is a later content pass once one faction is fully playable.
- **Victory conditions** — minimal hook can land in Phase 6 (for AI matches); full configurable
  system in Phase 7.
- **Determinism hygiene** — keep RNG seeded and centralized from Phase 1 (cheap insurance, keeps a
  future replay/multiplayer door open even though out of scope).
- **Map editor** — format + basic editor in Phase 1.5; placement of resources/infrastructure/zones/
  triggers is added to it as those systems land (Phases 4–7).
- **Extensibility** — registries, event bus, trait boundaries, and versioned schemas established in
  Phase 1 and maintained as a standing rule for every new system.

## RTS comparison & feature backlog (living — review every iteration)
Standing practice (owner, 2026-06-21): continuously test/compare features and code against
well-known RTS games (see PROJECT.md §12) and pull the next-most-impactful idea from here each
iteration. Keep this list fresh — add as we learn, check off as we ship.

**Have:** flow-field movement · formation slots with push-through · control groups 1–9 · shift-add/double-click-type · command card + stances · waypoint queueing · minimap · building placement (ghost+snap) · discrete combat (turrets aim, per-shot tracers, range circles) · health bars · win/lose + restart · geographic terrain (mountains, river, passes, chokepoints) with flow-field terrain cost · fog of war (Hidden/LastSeen/Visible per tile) · combat groups with clickable HUD cards (G key cycles) · attack-move · AI brain with 5-min prep → group advance · Recon Group (scout unit, 3× vision, spawns per side) · strategic zoom (0.08–5.0 scale, colored-dot overview below 0.22) · debug suite (COLDWAR_ASSERT/QUERY/EVENTLOG) · per-unit AmmoStorage (shots decrement per fire; gun empties) · per-unit FuelTank (burns per pixel moved; engine stops at 0) · ResupplyTracker (batch ammo/fuel transfer from depot within supply_range every 5s) · starting depots (Ammo 2000, Fuel 1500, Supplies 800, Parts 400 per side) · extraction buildings (Mine→Supplies, OilPump→Fuel into attached depot) · live HUD economy bar from depot aggregation (amber/red color thresholds).

**High-impact gaps vs reference games (reprioritized for operational RTS direction):**
~~1. Combat Groups~~ ✓ · ~~2. Geographic terrain~~ ✓ · ~~3. Fog of war / recon~~ ✓ · ~~6. Attack-move~~ ✓ · ~~8. Strategic zoom~~ ✓

1. **Logistics intent / auto-routing** (unique to this game) — player draws routes, system runs trucks. Phase 5 core.
2. **Expansion flow** (unique) — resource regions, secure → build extractor → depot integrates output. Phase 4 core.
3. **Automated rear defense** (They Are Billions / CoH) — QRF designation, patrol routes, threat alerts from radar. Phase 7.
~~4. Minimap enemy intel~~ ✓ — enemy dots fog-gated; player=blue, enemy=red, selected=green.
5. **Suppression / cover** (CoH) — moving under fire accrues suppression → slows + forces prone. Later phase.
6. **Audio/feedback** (all) — command acks, weapon fire, impact SFX; none yet. Phase 8.

## Parking lot (explicitly deferred)
Naval/amphibious units · multiplayer/netcode · replays · campaign/story · player-facing modding ·
additional factions beyond the first · full free-form tech-tree *screen* · per-unit veterancy (later) ·
in-game gambit editor for custom auto-cast rules (engine supports it; UI later).

## Risk register
- **Rendering throughput at 1,000+ sprites** — mitigate via batching; escalate to custom WebGL/Bevy
  if Phase 2 stress test fails. *Tripwire: Phase 2.*
- **Sim tick budget at scale** — mitigate via staggered AI + sim LOD + spatial grid. *Tripwire: Phase 2/5.*
- **Logistics complexity vs fun** — keep it self-running by default (pull-based); validate it's not
  tedious in Phase 4 playtests.
- **Scope** — large for a solo project; the phase gates keep a playable artifact at every step.
