# PROJECT.md — Large-Scale Logistics RTS (working title: TBD)

> **Purpose of this file:** Single source of truth for the project. It is the handoff
> document for any engineer, agent, or new Claude instance picking this up cold.
> Keep it updated as decisions are made and phases complete. When something changes,
> update the relevant section AND add a line to the Decision Log.
>
> **Last updated:** 2026-06-17
> **Status:** Milestone 0 complete — engine scaffold runs (native + WASM); Phase 1 next
> **Repository:** https://github.com/B0RE16/laughing-waffle (private) · local folder: `coldwar-rts/`

---

## 1. One-line vision
A **macro-scale, low-micro real-time strategy game** — Rusted Warfare's spirit, but at a far
larger scale (hundreds of units *per side*), where the player commands by setting *policy and
logistics* rather than micromanaging units. **The depth lives in the supply chain, especially
the backline.** Battles are won upstream, in industry and logistics, before the front ever fires.

## 2. Core pillars (the non-negotiable identity)
1. **Large scale.** 1,000+ active units total (hundreds per side), both armies fully simulated.
   This is a hard architectural target from day one, not an aspiration.
2. **Low micromanagement.** The player sets standing orders, zones, and production policy; units
   execute autonomously. Direct unit control is *opt-in*, never required.
3. **Deep logistics, especially the backline.** Multi-stage supply chains, self-running but deep
   to optimize and defend. RimWorld influence = logistics/jobs/zones model (NOT combat granularity).
4. **Abstracted combat.** HP + cover + suppression + range. Combat is mostly the *demand signal*
   the logistics system must feed. No per-hit/body-part detail.
5. **Infrastructure is core gameplay.** Planning and building roads, rail, power, and supply
   networks is a major strategic layer with its own *blueprint/planning mode* — not a side mechanic.
6. **Built to extend.** Every system is data-driven and modular so expanding it or making major
   changes later is easy. Strong first-class **map-making** support is part of this.

## 3. Control philosophy — the player commands at 3 levels
The "low micro" promise is delivered by this structure. Most play happens at levels 1–2.
1. **Strategic (set-and-forget policy):** production orders/bills, resource priorities, supply
   network design, doctrines, defense zones. Set once, runs itself.
2. **Operational (set intent):** "hold this front," "secure that region," assign a squad to a
   sector. Squads handle tactical execution themselves.
3. **Tactical (optional micro):** *draft* a squad for hands-on control during a key moment.
   The only manual layer, used by choice.

## 4. Tech stack & tooling
| Concern | Choice | Rationale |
|---|---|---|
| Language | **Rust** | Best perf for 1,000+ units; no GC pauses; real multithreading; fits theme |
| Window/render/input | **macroquad** | Thin; compiles to **native (perf)** AND **WASM (browser-verifiable)** |
| ECS | **hecs** | Fast archetype ECS, data-oriented, scales to thousands |
| Everything else | **custom** | Pathfinding, logistics, AI, sim — this is "our own engine" |
| Alternative (if needed) | Bevy | More batteries + parallel systems, but heavier / less "own engine" |
| Version control | **Git + GitHub** | Source history, branch per phase/feature, PR review |
| CI | **GitHub Actions** | Auto `build`/`test`/`clippy` + WASM build on every push/PR |

**Why Rust over web/native alternatives:** chosen by project owner for raw performance.
Rusted Warfare itself is Java/LibGDX; Rust was selected to exceed that ceiling. Decision is
revisitable (see Decision Log) but currently firm.

### How verification works (important for handoff)
- **Logic:** `cargo test` — fully automated, headless.
- **Visuals (autonomous):** build to WebAssembly and run in a browser; screenshot via preview
  tooling. This is the primary way the AI engineer confirms rendering without human help.
- **Visuals (alt):** run native build; screenshot the OS window via desktop control.
- **Performance:** stress-test with 1,000+ dummy units starting in Phase 2 — never guess about scale.

### Version control & engineering practices
- **Git + GitHub** from Milestone 0. `main` stays green (always builds + passes tests).
- **Branch per phase/feature** (e.g. `phase-1-engine-skeleton`, `feat/flow-fields`); merge via PR —
  good discipline even solo, and it documents *why* changes happened.
- **Conventional Commits** (`feat:`, `fix:`, `refactor:`, `docs:`, `test:`, `perf:`) for readable history.
- **GitHub Actions CI** runs `cargo build`, `cargo test`, `cargo clippy`, and the WASM build on every
  push/PR — the verification harness, automated.
- **Update PROJECT.md/PLAN.md in the same PR** as the change they describe, so docs never drift.
- `.gitignore` excludes `target/`, build artifacts, and local cruft.

## 5. Engine architecture (modules / frameworks to build)
Built as clean, separable systems. Foundational systems (must exist early, painful to retrofit)
are marked ★.

- ★ **Core loop** — fixed-timestep sim (~20Hz) + interpolated rendering; deterministic-friendly.
- ★ **ECS (data-oriented)** — components in flat arrays; systems are functions over them. Locked
  in early because converting later is a rewrite. Required for scale.
- ★ **Pathfinding service** — **flow fields as the primary mover** (one field for a whole army),
  grid A* for stragglers/special cases, local avoidance (steering/separation). Pluggable.
- ★ **Autonomy: Utility AI** — each unit scores options (engage / take cover / resupply / retreat /
  pull a job / idle) and picks the best. Standing orders bias the scoring. This is *why* the player
  doesn't babysit units.
- ★ **Job system** ("work-giver" model, RimWorld-style) — global queue of jobs (haul, repair,
  garrison, reinforce). Idle units claim by priority + proximity. Makes base & logistics self-manage.
- ★ **Squad / formation hierarchy** — command groups as single entities; shared flow-field movement;
  auto-reinforce from production. Essential for commanding hundreds.
- ★ **Spatial grid** — uniform grid for all neighbor/range queries (targeting, cover, vision,
  supply range). No O(n²) scans, ever.
- ★ **Supply/network graph** — nodes (depots, extractors, conduits) + edges; carries resources &
  power; holds throughput + coverage + route-security data.
- **Renderer** — sprite batching via macroquad; layered draws (terrain → buildings → units →
  projectiles → UI); camera transform. Kept behind a clean boundary.
- **Command system** — all actions as queued commands (supports waypoint queueing, debuggable).
- **Event bus** — decoupled messaging (unit died, building complete, node depleted).
- ★ **Data-driven definitions & faction system** — units, weapons, buildings, and factions defined
  as composable data (stats + component lists), not hardcoded. Lets us author *any* kind of unit
  and support **multiple factions** from one engine. Ship one Cold-War-era faction first; the
  architecture supports N. (Internal data-driven design; player-facing modding still not a goal.)
- **Victory-conditions / match-rules system** — configurable win/lose objectives (annihilation,
  Command-Center kill, economic, survival/time, or custom combinations) selected at match setup.
- ★ **Map system** — versioned map format (layered: terrain, elevation, resources, spawns, markers,
  pre-placed infrastructure) + an **in-engine map editor**. Tile/terrain types are data-driven. The
  editor grows as new placeable types (resources, infra, zones, triggers) are added.

### 5.1 Extensibility & scalability patterns (so major changes stay easy)
- **Data-driven content** — units, buildings, weapons, resources, tech-gating, factions, tiles, and
  victory conditions all defined in versioned data files. New content = author data, not edit code.
- **Composition over inheritance** — capabilities are ECS components + an **ability list** per unit;
  new behavior = a new component/ability + system, never editing existing types.
- **Trait-based system boundaries** — major systems (renderer, pathfinder, AI behavior, resource
  type, victory condition, job type) sit behind Rust traits so implementations are swappable.
- **Registries, not match-statements** — unit/ability/job/victory/AI types register at startup; the
  engine iterates registries, so adding a type never touches a central switch.
- **Event bus** — systems react to events (unit died, building complete, route cut) so new systems
  hook in without modifying emitters.
- **Versioned schemas** — data/map/save formats carry a version + migration path; old content keeps working.
- **No magic numbers in code** — all tunables live in data.

## 6. Scale & performance strategy (target: 1,000+ units)
- **Data-oriented ECS** — tight numeric loops over typed/flat storage.
- **Flow fields default**, per-unit A* only as fallback.
- **Staggered AI ticks** — units "think" every N ticks on a rotating schedule (~50/tick at 1,000 units).
- **Simulation LOD** — rear-area/idle units simulate coarsely; combat units fully.
- **Spatial grid** for every neighbor query.
- **Renderer kept swappable** — if rendering throughput becomes the wall, upgrade the batcher;
  WASM build perf is validated continuously.

## 7. Game design

### 7.1 Resources (multi-stage — refined tier locked in)
Raw resources are extracted, then **refined and manufactured** before use — this is the backbone of
the deep backline logistics.
- **Ore** (raw, stockpile) — mined from ore nodes.
- **Crude** (raw, stockpile) — from oil/geyser nodes.
- **Metal** (refined) — Ore → Refinery → Metal. Primary build material.
- **Refined Fuel** (refined) — Crude → Refinery → Fuel. Powers vehicles/aircraft and convoy movement.
- **Components** (manufactured) — Metal (+Power) → Foundry → Components; gate advanced units/buildings.
- **Power** (flow rate, not bank) — buildings stall if the grid goes negative. Generators produce it.
Raw → refined → manufactured means you build an *industrial base*, not just plop extractors.

### 7.2 Logistics — the deep system (rear → front)
```
EXTRACT          REFINE/MAKE         STORE              DISTRIBUTE          FRONT
ore/fuel nodes → refineries,    →  stockpile zones  →  haulers/convoys  →  forward supply
(rear)           factories          + warehouses        along roads/rail    dumps → units
                 (ammo,fuel,        (priorities,        (auto-dispatched    consume (ammo,
                  parts, units)      capacity)           by job system)      fuel, reinforce)
```
Depth mechanics:
- **Pull-based, demand-driven hauling** — dumps/stockpiles have target stock levels; shortfalls
  generate haul jobs; job system dispatches nearest free hauler. Player sets policy, not routes.
- **Throughput & infrastructure** — routes have bandwidth; roads/rail raise convoy speed + capacity;
  bottlenecks are real. Building/upgrading infrastructure is a core strategic lever.
- **Supply range / coverage** — units far from supply resupply slowly or not at all. Advancing
  requires *pushing logistics forward* (new dumps, extended roads), not just moving units.
- **Route security** — convoys travel real paths; can be raided. Defending corridors (and cutting
  the enemy's) is strategic. A backline cut off from ammo collapses without direct losses → a win path.
- **Stockpile zones & production bills** (RimWorld layer) — designate storage zones w/ priorities &
  capacity; standing production orders ("keep 200 shells, then pause"). Manage policy, not items.

### 7.3 Combat (abstracted, plugs into logistics)
- HP + cover (terrain) + suppression + range. Auto-fire on targets in range; auto-seek cover.
- **Anti-air rule:** AA units/turrets hit only air; everything else hits only ground. Single rule,
  whole rock-paper-scissors, no armor matrix.
- **Projectiles are real entities** (travel time, can miss movers). Some weapons have splash.
- Units **burn ammo per volley, fuel per move**; low units auto-pull from nearest forward dump.
  Well-supplied front = full effectiveness; starved front can't fire/maneuver. **Combat = readout of logistics.**
- Standing orders: aggressive / defensive / hold-fire / retreat-at-X%-HP.

### 7.4 Units (v1 placeholder roster — Cold-War-era theming, data-driven)
Units are **defined as composable data** (chassis/movement + weapon(s) + armor + abilities) so any
kind is authorable and factions can share or differ. Starter roster (Cold-War flavor, names per
faction): Engineer (builder/repair) · Infantry · Main Battle Tank · Artillery (splash, fragile) ·
AA Vehicle (anti-air only) · Jet/Helicopter (air) · Supply Truck (logistics transport).
Naval deferred (framework supports movement layers).

### 7.5 Buildings (v1 placeholder)
Command Center · Extractor (on node) · Refinery · Barracks (infantry) · Factory (vehicles) ·
Airbase (aircraft) · Depot/Warehouse (storage) · Forward Supply Dump · Turret · AA Turret ·
Generator (power). Infrastructure: Roads/Rail.

### 7.6 Tech progression
**Build-gated, not research-gated** (v1): access gated by what you've built (need a Factory to
make Tanks, etc.). No tech-tree screen yet. Intuitive, self-pacing. Real upgrade tree = later.

### 7.7 Vision / fog of war
Two-state fog (unexplored → explored/dimmed showing last-seen → visible). Per-unit sight radius
on the spatial grid. Data stubbed early, rendered in Phase 7.

### 7.8 Enemy AI
Commander-level **macro AI** using the same systems the player does: maintain economy + logistics,
run production loops, mass to a threshold, attack-move toward objectives, defend if attacked, and
(stretch) raid supply lines. Difficulty = economy multiplier + attack threshold.

### 7.9 Win / lose — configurable match conditions
All objective types are selectable at match setup (and combinable): **annihilation** (destroy all
buildings), **decapitation** (destroy Command Center), **economic/territorial**, and
**survival/time** goals. Logistics strangulation remains a viable indirect path under most rulesets.
Driven by the victory-conditions system (see §5).

### 7.10 Controls
LMB select / drag-box · RMB contextual command · Shift+RMB queue waypoints · double-click select
all of type on screen · control groups 1–9 · building hotkeys. Plus level-1/2 policy UI (bills,
zones, network) — design TBD.

### 7.11 Factions
The engine is **multi-faction from the architecture down**, even though v1 ships a single
**Cold-War-era** faction. A faction = a data bundle: available units/buildings, tech-gating, and
global modifiers (e.g. cheaper logistics, tougher armor, faster production). Later factions (e.g.
Eastern/Western bloc analogues) are added as data, not code. The "in-depth unit manipulation" goal
is met by composable unit definitions — new unit types/variants are authored in data, not hardcoded.

### 7.12 Unit control & command (detailed)
Layers map to §3 (strategic / operational / tactical). Concretely:
- **Selection:** click, drag-box, double-click select-type-on-screen, control groups (1–9), select-all-army.
- **Order types:** move, attack-move, patrol (waypoint loop), hold position, guard/follow unit,
  garrison, retreat, use-ability, set rally point. **Shift queues** any order into a waypoint chain.
- **Stances / standing orders:** Aggressive / Defensive / Hold-fire / Hold-ground / Cautious, plus
  retreat-at-X%-HP and auto-resupply toggles. Set per unit or per squad; persist until changed.
- **Squads:** group units into a named squad with a **formation** (line/column/wedge/spread) and a
  squad stance; orders fan out to members; squads **auto-reinforce** from production (squad templates
  define desired composition). Squads are the main unit of operational command.
- **Drafting (opt-in micro):** draft a unit/squad to take full manual control for a moment, then
  release it back to autonomy. Never required.
- **Doctrine presets:** save/apply policy bundles (stances + bills + priorities) so a new force
  inherits sensible behavior with one click.
- **In-depth manipulation:** because units are composable data, variants, loadouts, and role
  assignments are defined without engine changes.

### 7.13 Infrastructure (a core pillar)
Building and *planning* infrastructure is a major strategic layer, not flavor.
- **Blueprint / planning mode:** lay out a whole network as ghost placements first (roads, power
  lines, depots, defenses), validate it, then commit — construction units build it over time. Plans
  can be saved/copied. (Factorio/RimWorld-style planning, RTS-paced.)
- **Roads & rail:** segment/tile-built. Roads raise unit move speed + supply throughput; **rail** is
  a high-capacity backbone with stations/depots. Terrain-aware (bridges over water, cuts through cliffs).
- **Power grid:** generators + transmission lines/pylons + substations; coverage radius; buildings
  stall without power. A real network you plan and defend.
- **Supply network:** depots, distribution hubs, pipelines/conduits — the graph resources flow
  through, with throughput and coverage (see §7.2).
- **Fortifications:** walls, bunkers, defensive lines as buildable infrastructure.
- **Construction units & queues:** engineers/construction vehicles execute build/upgrade/repair jobs
  via the job system; bigger projects = more builders or more time.
- **Throughput, bottlenecks & upgrades:** links have capacity; upgrading infrastructure scales the
  economy. Planning *good* infrastructure (not just more) is the skill.
- **Vulnerability:** infrastructure can be damaged/destroyed and must be repaired; cutting enemy
  roads/power/supply is a strategic objective.

### 7.14 Maps & map-making
First-class map support is a project goal (see §5 Map system).
- **Layered map format:** terrain, elevation, passability/cost, resource nodes, spawn points,
  scripted markers/triggers, optional pre-placed infrastructure. Human-readable metadata + efficient
  tile storage; **versioned** for forward-compatibility.
- **In-engine editor:** paint terrain/elevation, place resources & spawns, define zones and triggers,
  pre-place infrastructure, test-play instantly. Grows alongside the game as new placeables appear.
- **Large maps** (256×256+) via chunking; designed so size can scale up later.
- **Validation:** reachability, resource balance, spawn fairness checks.
- **Procedural hooks:** optional generators emit the same format (not v1-critical, but supported).

## 8. Roadmap (phased; playable/verifiable at each step)
- **Phase 1 — Engine skeleton:** core loop, data-oriented ECS, macroquad renderer + camera, large
  tilemap, render placeholder sprites, + data-driven definition loader scaffold (one faction),
  versioned map format, and extensibility scaffolding (registries, event bus, traits).
  Goal: something on screen (native + WASM screenshot).
- **Phase 1.5 — Map system & editor:** versioned layered map format + in-engine editor (paint
  terrain/elevation, place resources/spawns, validate, test-play). The editor grows in later phases.
- **Phase 2 — Pathfinding at scale:** flow fields (primary), A* fallback, local avoidance, staggered
  ticks. **Stress-test 1,000+ dummy units here.**
- **Phase 3 — Autonomy core:** utility AI, job system, squad/formation layer. The "low micro" engine.
- **Phase 4 — Economy, logistics & infrastructure:** refining chain, supply/network graph,
  self-running haulers, production bills, power grid, **roads/rail/power construction with a
  blueprint/planning mode**, throughput/coverage. (The deep, identity-defining phase.)
- **Phase 5 — Combat:** auto-fire, cover, suppression, AA rule, splash, ammo/fuel consumption + resupply.
- **Phase 6 — Enemy AI:** commander-level macro AI managing a logistics economy.
- **Phase 7 — Polish:** fog of war, minimap, command/policy UI, configurable victory conditions,
  sound, a real playable map.

## 9. Decision log
- **2026-06-17** — Genre set: macro-scale, low-micro logistics RTS (not a direct RW clone).
- **2026-06-17** — Single-player only; no multiplayer (sim still built deterministic-friendly).
- **2026-06-17** — Modding not a goal (but unit/building defs kept data-driven internally).
- **2026-06-17** — Scale target raised to **hundreds per side / 1,000+ total**; drove data-oriented
  ECS + flow-fields + staggered ticks + sim LOD as mandatory.
- **2026-06-17** — RimWorld influence scoped to **logistics/jobs/zones**, NOT combat granularity.
  Combat is abstracted. Backline logistics is the primary depth.
- **2026-06-17** — **Language: Rust** (owner's call, for performance). Chosen over TS/web despite
  slightly slower iteration; macroquad's WASM target preserves autonomous visual verification.
- **2026-06-17** — Engine libs **confirmed**: macroquad + hecs + custom systems. Bevy = documented fallback.
- **2026-06-17** — Economy: **refined tier locked in** (Ore/Crude → Metal/Fuel → Components). Deep
  multi-stage backline supply chain is the core depth.
- **2026-06-17** — Theme: **start Cold-War-era, single faction**, but engine is **multi-faction +
  data-driven composable units** from the architecture down (any unit type authorable in data).
- **2026-06-17** — Victory: **fully configurable match conditions** (annihilation / decapitation /
  economic / survival / custom combos) via a match-rules system.
- **2026-06-17** — Micro floor: **opt-in squad drafting** (standing orders default; direct control optional).
- **2026-06-17** — **Extensibility is a first-class requirement**: data-driven content, ECS
  composition, trait boundaries, registries, event bus, versioned schemas (see §5.1).
- **2026-06-17** — **Infrastructure (roads/rail/power/supply) elevated to a core pillar**, with a
  blueprint/planning mode (see §7.13).
- **2026-06-17** — **First-class map-making**: versioned layered map format + in-engine editor
  (new Phase 1.5; see §7.14).
- **2026-06-17** — **Version control: Git + GitHub** with branch-per-feature, Conventional Commits,
  PR merges, and GitHub Actions CI (build/test/clippy/WASM) as standing engineering practice.
- **2026-06-17** — **Milestone 0 complete**: macroquad 0.4 + hecs 0.10 scaffold builds native + WASM;
  fixed-timestep sim loop + placeholder render verified. WASM needs a `--import-undefined` linker flag
  (`.cargo/config.toml`); visual verification is via native offscreen render-target capture
  (`COLDWAR_CAPTURE` env var) since the preview tool can't screenshot a live animation loop.

## 10. Open questions (need owner input)
Resolved 2026-06-17: theme (Cold-War start, multi-faction architecture), economy depth (refined
tier), victory (configurable conditions), micro floor (opt-in drafting), engine libs (macroquad+hecs).
Remaining:
1. **Working title / game name** — still TBD (not blocking).
2. **Concrete numbers** — map size, per-side soft cap, tick rate: starting targets set in PLAN.md
   (256×256, ~500/side, 20Hz), tuned during Phase 2 stress tests.
3. **Faction list & asymmetry** — which factions beyond the first, and how they differ (later phase).
4. **Specific unit stats / costs / tech-gating tables** — filled in during Phases 4–5.

## 11. Working-style notes (for any agent picking this up)
- Owner wants **minimal interference**: make the engineering calls yourself; surface only genuine
  taste/design decisions. Plan features before implementing them. Build & verify your own work.
- Prefer prose planning the owner can steer in chat over heavy questionnaires.
- This is on **Windows** (PowerShell primary; Bash available). Game repo dir:
  `C:\Users\patri\Downloads\ClaudeCode\coldwar-rts` (the parent folder holds unrelated projects).
