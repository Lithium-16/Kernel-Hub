# PLAN.md — Implementation Plan

> Companion to **PROJECT.md** (which holds the vision/design/decisions). This file is the
> *actionable build plan*: milestones, the systems each phase delivers, concrete tasks, key
> data types, and acceptance criteria. Update checkboxes and notes as work progresses.
>
> **Last updated:** 2026-06-18 · **Status:** M0 + Phase 1 + Phase 2 done; movement / perf / art /
> pathfinding polished. **Roadmap restructured core-first — canonical phase ORDER is now PROJECT.md §8.**
> Next: **Phase 2.5 (UI toolkit)**. The detailed phase sections below predate the restructure; treat
> PROJECT.md §8 as the source of truth for order — they're re-sequenced/migrated as each phase begins.

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
- [x] **Formation slots** — group moves assign each unit its own slot in a packed block (greedy nearest) and seek it once near the formation anchor, instead of all crushing one point. Killed the long-standing packed-group jitter: 600-unit residual motion 2.30 → 0.06 px/tick. `COLDWAR_SETTLE` headless jitter metric. ✓ 2026-06-18
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
- [~] **Panel registry** — all panels consolidated in `hud.rs` (top resource bar w/ hover tooltips + overdraw warning, bottom command bar, selection panel by type); `economy::Economy` stub backs the bar. Real register-don't-switch registry deferred to Phase 3 (command card). ✓ panels 2026-06-20
- [ ] **UI icon atlas** + batched draw.

**Key types:** `Ui`, `Widget`, `Layout`, `Theme`, `PanelId`, `InputCapture`.

**Acceptance:** a themed HUD with a working button/panel that captures its own clicks (no leak to the
world); resizes cleanly; screenshot; tests for layout + input-capture logic.

---

## Phase 3 — Units & Buildings infrastructure + autonomy core
**Goal:** the full **unit AND building** object model + autonomy — Forms/abilities/transitions,
building placement/construction/production, utility AI + job system + squads — so minimal player input
produces sensible behavior for both units and buildings.

**Build:**
- [ ] **Registry + event bus + system traits** — the extensibility scaffolding (Phase-1 debt); the
      dispatch layer abilities/effects/conditions/transitions register into.
- [ ] **Ability framework** — `AbilityDef` + effect registry; **auto-cast policies** (Manual/Auto/Off
      + `AutoRule{condition, target, priority}`) scored inside the utility AI.
- [ ] **Forms & transitions** — unit state machine (mobile / sieged / deploy / construction phases);
      a building is just an immobile Form; HP carries over as %.
- [~] **Command-card UI** — `hud.rs` command card (bottom-left, appears on selection) with Stop +
      stance buttons (active stance outlined) done 2026-06-21; ability buttons (auto-generated from
      Form abilities), cooldown/disabled/toggle states, and targeting modes still to add.
- [ ] **Utility AI** — per-unit scorer over candidate actions (idle, take-job, move-to, engage,
      retreat, resupply); pick highest; standing orders bias weights. Runs on staggered schedule.
- [~] **Standing orders & stances** — `Stance` component (Aggressive / Defensive / Hold-Ground) set
      per selection via the command card, unit-tested (`dominant`/`set_selected`), Hold-Ground halts
      movement, done 2026-06-21; Hold-fire / Cautious, retreat-at-X%-HP, auto-resupply, and combat
      effects land with the combat system.
- [ ] **Job system** — global job board (haul, build-assist, repair, garrison, reinforce); idle
      units claim by priority + proximity; jobs have state (open/claimed/done) and re-queue on fail.
- [ ] **Squad/formation layer** — named squads; **formations** (line/column/wedge/spread); squad-level
      orders fan out; shared flow-field target; **squad templates** define desired composition;
      auto-reinforce hook (stubbed until production exists).
- [~] **Selection & command UI** — click, drag-box, **Shift-add**, **double-click select-type-on-screen**,
      and **control groups 1–9** (Ctrl+N assign, N recall, dead-member pruning) done 2026-06-20, unit-tested;
      order types (move / attack-move / patrol / hold / guard / garrison / retreat / rally / ability) with
      **Shift to queue waypoints** and **opt-in squad drafting** still to add.
- [ ] **Zones** — paint defense/staging/no-go zones that orders and jobs reference.
- [ ] **Doctrine presets** — save/apply policy bundles (stances + priorities) to a force in one action.
- [ ] **Building placement** — ghost/blueprint preview, grid snap, validity (terrain / overlap /
      build-radius / resource node), rotation; multi-place blueprint mode.
- [ ] **Construction** — builders take build jobs; site → frame → complete Forms; gradual drain;
      cancel (refund) / repair.
- [ ] **Production** — producer buildings: queue + rally point + exit; bills; research queue.
      (Resource *costs* wired in Phase 4; queue / placement / construction *systems* built here.)
- [ ] **Building command card + deploy/undeploy** — buildings use the same card; MCV↔HQ transitions.

**Key types:** `Form`, `Ability`/`AbilityDef`, `AutoRule`, `Transition`, `UtilityAgent`, `StandingOrder`,
`Job`/`JobBoard` (incl. `BuildJob`), `Squad`, `Zone`, `Selection`, `Placement`, `ProductionQueue`.

**Acceptance:** undrafted units idle→claim jobs and defend zones with no per-unit input; a squad
moves/holds as one; drafting a squad gives direct control; **a builder constructs a placed building
(site→complete) and a producer building queues + rallies a unit; deploy↔undeploy works**; an auto-cast
ability fires on its condition; screenshot; tests for utility scoring, job claim/release, and a transition.

---

## Phase 4 — Economy, Logistics & Infrastructure (the identity phase)
**Goal:** the full multi-stage, self-running supply chain — extraction → refining → manufacturing →
storage → distribution → front — plus **infrastructure as a core build/plan pillar** (roads, rail,
power, supply networks) with a blueprint/planning mode, throughput, and coverage.

**Build:**
- [ ] **Resources** — Ore, Crude (raw); Metal, Fuel (refined); Components (manufactured); Power (flow).
- [ ] **Production buildings** — Extractor, Refinery, Foundry, factories; **production bills**
      (standing orders: "keep N, then pause"); gradual resource drain while producing.
- [ ] **Supply/network graph** — depots/conduits as nodes, in-range/connected edges; carries
      resources + power; throughput (bandwidth) per edge; coverage radius.
- [ ] **Power grid** — production vs consumption balance per tick; buildings stall on deficit.
- [ ] **Storage** — stockpile zones + warehouses/depots with priorities & capacity.
- [ ] **Pull-based hauling** — dumps/stockpiles have target levels; shortfalls emit haul jobs;
      Supply Trucks (from Phase 3 job system) fulfill them. Convoys burn Fuel.
- [ ] **Infrastructure construction** — roads (speed + throughput), **rail backbone** with stations,
      **power transmission lines/pylons**, depots/hubs, pipelines, fortifications; built by
      construction units via the job system; terrain-aware (bridges/cuts).
- [ ] **Blueprint / planning mode** — ghost-place a whole network, validate, then commit to build;
      save/copy plans. (A headline feature — infrastructure planning is a core pillar.)
- [ ] **Throughput, upgrades & vulnerability** — links have capacity; upgrade to scale; infra can be
      damaged/destroyed and repaired; cutting enemy roads/power/supply is a strategic objective.
- [ ] **Supply coverage** — "is tile X supplied?" query (used by combat resupply in Phase 5).
- [ ] **Resource flow solver** — deterministic per-tick balance pass across the network.
- [ ] **Research / upgrades** — research buildings produce `UpgradeDef`s (faction-wide, build-gated);
      effective stat = base + active upgrades; effects can unlock abilities/forms.
- [ ] **Economy UI** — resource readouts, power balance, bills, network overlay.

**Key types:** `Resource`, `Stockpile`, `ProductionBill`, `SupplyNode`/`SupplyEdge`/`SupplyGraph`,
`PowerGrid`, `HaulJob`, `Road`.

**Acceptance:** a base auto-refines raw → components and auto-distributes to a forward dump with
zero manual hauling; cutting a route starves the downstream dump; power deficit stalls buildings;
network overlay screenshot; a blueprinted road+power network builds out and a destroyed segment
cuts throughput; tests for flow solver + coverage + bill logic.

---

## Phase 5 — Combat (abstracted, logistics-fed)
**Goal:** auto-resolving combat driven by positioning, cover, and supply — combat as the demand
signal on the logistics system.

**Build:**
- [ ] **Health/damage** — HP, death, wreckage; damage application system.
- [ ] **Weapons & projectiles** — projectile entities (travel time, can miss movers); range, ROF,
      damage; **splash** flag (artillery).
- [ ] **Damage table** — `armor_mult[damage_type][armor_class]` (subsumes the AA rule; gives counters).
- [ ] **Targeting** — auto-acquire via spatial grid; threat/priority selection.
- [ ] **Cover & terrain** — accuracy/range modifiers from elevation/cover tiles; positioning matters.
- [ ] **Suppression** — incoming fire reduces effectiveness/forces caution (ties to utility AI).
- [ ] **Ammo, fuel & upkeep** — burn per volley/move + a continuous upkeep trickle; low units auto-pull
      resupply from nearest forward dump via job system; starved units can't fire/maneuver.
- [ ] **Damage/repair** — Engineers repair; wreck salvage (optional).
- [ ] **Combat feedback** — health bars, hit/explosion FX, suppression indicator.

**Key types:** `Health`, `Weapon`, `Projectile`, `Armament`, `Ammo`, `Suppression`, `DamageEvent`.

**Acceptance:** two armies auto-fight on positioning + supply with no micro; a unit cut off from
supply degrades and stops firing; AA/air interaction correct; screenshot of a supplied vs starved
engagement; tests for damage, AA targeting rules, ammo/resupply.

---

## Phase 6 — Enemy AI (commander-level)
**Goal:** a macro AI opponent that plays the same game you do — economy, logistics, and attacks.

**Build:**
- [ ] **Economic AI** — expand to nodes, build extractors/refineries/foundries, keep bills running.
- [ ] **Logistics AI** — build depots/roads, maintain forward supply, defend corridors.
- [ ] **Military AI** — mass to a threshold, form squads, attack-move toward objectives; defend if hit.
- [ ] **Strategic targeting** — value targets (incl. raiding enemy supply lines as a win path).
- [ ] **Difficulty knobs** — economy multiplier, aggression threshold, reaction time.
- [ ] **AI debug view** — show AI intent/state for tuning.

**Key types:** `AiBrain`, `AiGoal`, `ThreatMap`, difficulty config.

**Acceptance:** AI builds a functioning logistics economy and mounts coordinated attacks; raids
player supply when advantageous; a full match is playable start→finish; tests for AI decision steps
where feasible.

---

## Phase 7 — Polish, UI & match rules
**Goal:** a complete, playable single-player match with all the framing systems.

**Build:**
- [ ] **Fog of war** — unexplored/explored-dimmed/visible; per-unit sight on spatial grid.
- [ ] **Minimap** — terrain, units, supply network, alerts.
- [ ] **Command/policy UI** — bills, zones, network design, standing orders, drafting.
- [ ] **Configurable victory conditions** — annihilation / decapitation / economic / survival /
      custom combos selected at match setup.
- [ ] **Match setup** — pick faction, map, opponents, rules, difficulty.
- [ ] **Audio** — SFX + ambient (lightweight).
- [ ] **A real playable map** + a short scenario to validate the whole loop.
- [ ] **Performance pass** — confirm scale targets hold in a full match (sim LOD, render budget).

**Acceptance:** a full match is winnable/losable under at least two victory rulesets at the scale
target with acceptable performance; fog/minimap/UI functional; screenshots of a complete match.

---

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
