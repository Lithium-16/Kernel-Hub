# PLAN.md — Implementation Plan

> Companion to **PROJECT.md** (which holds the vision/design/decisions). This file is the
> *actionable build plan*: milestones, the systems each phase delivers, concrete tasks, key
> data types, and acceptance criteria. Update checkboxes and notes as work progresses.
>
> **Last updated:** 2026-06-22 · **Status:** M0 + Phase 1 + Phase 2 done; Phase 2.5 UI substantially
> done; Phase 3 core (combat groups, buildings, combat, minimap, debug suite) in progress.
>
> **DIRECTION UPDATE 2026-06-22** — Game is now a tile-based operational RTS. Primary interface is
> Combat Groups, not individual units. Logistics is intent-based (player defines routes/depots,
> system executes). Expansion is the core progression loop. See PROJECT.md §2, §3, §7.17-7.19, and
> Decision Log entry 2026-06-22 for the full update. Existing code (movement, combat, ECS, buildings)
> is compatible — new build priorities are the Combat Group layer, logistics intent UI, expansion flow,
> and geographic terrain. See "Next phase priorities" section below.

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
- [ ] **Recon Group unit type** — high vision radius, fast, light armor; purpose-built for reconnaissance.

**Key types:** `CombatGroup`, `GroupOrder`, `FogOfWar`, `TileProperties`, `VisionRadius`.

**Acceptance:** player forms a group, orders it to advance, it engages enemies en route; fog hides the map until explored; radar reveals a large area; geographic tiles affect movement; `COLDWAR_ASSERT=formation_fills` still passes.

---

## Phase 4 — Physical Resources + Extraction
**Goal:** the first economic loop. Resources are physical quantities in the world, not numbers in a spreadsheet. Weapons stop firing when ammo runs out. Vehicles stop when fuel runs out.

**Build:**
- [ ] **Resource types** — Ore, Oil (strategic); Ammo, Fuel, Building Supplies, Weapon Parts (logistics). All stored as integer quantities in depots.
- [ ] **Extraction buildings** — Mine (on Ore Basin tile), Oil Pump (on Oil Field tile). Engineers build them from blueprints. Produce resource over time into nearest depot.
- [ ] **Processing buildings** — Processing Facility (Ore → Building Supplies + Weapon Parts), Fuel Refinery (Oil → Fuel), Ammo Factory (Weapon Parts → Ammo). Engineers build from blueprints.
- [ ] **Depot** — stores all resource types; visible stockpile bar per resource (amber=low, red=empty); supply radius around it.
- [ ] **Ammo consumption** — units draw ammo from nearby depot before firing; weapons stop when ammo=0.
- [ ] **Fuel consumption** — vehicles consume fuel on movement; stop when fuel=0.
- [ ] **Geographic resource regions** — Ore Basin, Oil Field as named tile regions; visible on map and Region View.
- [ ] **Construction from blueprints** — Engineer Group assigned to blueprint auto-paths to it and builds, consuming Building Supplies from nearest depot.

**Key types:** `ResourceType`, `Stockpile`, `Depot`, `Extractor`, `ProcessingBuilding`, `OreBasin`, `OilField`.

**Acceptance:** place a mine blueprint, assign engineers, mine builds and ore flows into depot; ammo factory consumes weapon parts and produces ammo; a unit runs out of ammo and stops firing; a vehicle runs out of fuel and stops moving; depot bars visible and accurate.

---

## Phase 5 — Roads + Supply Routes + Trucks
**Goal:** the logistics layer. The player draws routes; the system executes. A cut road immediately reduces throughput.

**Build:**
- [ ] **Road blueprint tool** — player clicks two points, ghost preview shown, confirm places road tiles; engineers auto-claim and build using Building Supplies.
- [ ] **Road tiers** — off-road (1× speed), dirt road (2×), paved (3×); road tile type determines convoy speed.
- [ ] **Road damage + repair** — artillery damages road segments; damaged road reverts toward off-road speed; engineers auto-repair if Building Supplies available and repair job assigned.
- [ ] **Supply Route tool** — player selects origin depot, destination depot, resource type, priority; system creates the route.
- [ ] **Truck system** — trucks spawn from origin depot, follow road network (pathfind along road tiles), deliver resource to destination, return; visible as vehicle sprites.
- [ ] **Route display** — active routes shown as colored lines on map; alert icon when route disrupted (road cut, depot empty, trucks destroyed).
- [ ] **Convoy ambush** — enemy units can attack trucks; destroyed truck loses cargo; player sees notification.

**Key types:** `RoadTile`, `RoadTier`, `SupplyRoute`, `Truck`, `RouteAlert`.

**Acceptance:** player draws a road, engineers build it; player creates a route, trucks drive it; artillery damages the road, throughput drops, player sees alert; engineers repair the road, throughput recovers; trucks can be destroyed.

---

## Phase 6 — Reinforcements + Region System + Interdiction
**Goal:** the operational layer. Groups track losses, regions appear on the strategic map, supply lines can be interdicted.

**Build:**
- [ ] **Loss tracking** — Combat Group card shows current/original strength (34/50 tanks); tracks kills against the group's roster.
- [ ] **Reinforce panel** — player clicks Reinforce on a group; panel shows available sources (factories, reserve depots, build queues) with travel time estimates; player chooses; replacement units path to group automatically.
- [ ] **Region system** — named strategic areas (Ore Basin, Mountain Pass, Oil Field, Valley, etc.); zoom out past threshold → Region View; each region shows ownership, military presence, stockpile levels, threat level, resource output.
- [ ] **Region ownership** — region controlled by faction with military presence + a depot there; contested when both factions present; losing a region cuts resource output immediately.
- [ ] **Logistics interdiction** — Artillery Group can be ordered to fire on road segments (destroying them); Recon and light groups can attack convoys; enemy can do the same.
- [ ] **Alternate route** — if primary road cut, player can draw alternate route around it; system switches trucks automatically.

**Key types:** `CombatGroupRoster`, `ReinforceSource`, `Region`, `RegionOwnership`, `InterdictionTarget`.

**Acceptance:** group takes losses and card shows reduced strength; player reinforces from a factory and units arrive; zooming out shows Region View with cards; enemy cuts a road and trucks slow; player creates alternate route and trucks reroute.

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

**Have:** flow-field movement · formation slots with push-through · control groups 1–9 · shift-add/double-click-type · command card + stances · waypoint queueing · minimap · building placement (ghost+snap) · discrete combat (turrets aim, per-shot tracers, range circles) · health bars · win/lose + restart · geographic terrain (mountains, river, passes, chokepoints) with flow-field terrain cost · fog of war (Hidden/LastSeen/Visible per tile) · combat groups with clickable HUD cards (G key cycles) · attack-move · AI brain with 5-min prep → group advance · debug suite (COLDWAR_ASSERT/QUERY/EVENTLOG).

**High-impact gaps vs reference games (reprioritized for operational RTS direction):**
1. **Combat Groups** (SupCom group control, CoH unit cohesion) — *the* defining feature of the new
   direction; players command groups not individuals. Build this before any other Phase 3+ work.
2. **Geographic terrain** (every serious RTS) — mountains, rivers, passes, chokepoints. Currently
   flat noise map. Geography is a core pillar.
3. **Fog of war / reconnaissance** (all RTS) — currently full visibility. Recon is phase 1 of the
   war loop. *Biggest gameplay gap given the new direction.*
4. **Logistics intent / auto-routing** (unique to this game) — player draws routes, system runs trucks.
5. **Expansion flow** (unique) — resource regions, secure → build → integrate loop.
6. **Attack-move** (SC2/SupCom) — units currently only fire in place; attack-move = advance and engage.
7. **Automated rear defense** (They Are Billions / CoH) — QRF, patrols, threat alerts.
8. **Strategic zoom** (SupCom) — zoom-to-whole-map-overview; essential for theater command.
9. **Suppression / cover / retreat** (CoH) — abstracted combat depth (later phase).
10. **Audio/feedback** (all) — command acks, fire/impact SFX; none yet.

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
