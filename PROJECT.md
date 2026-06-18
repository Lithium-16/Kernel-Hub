# PROJECT.md — Large-Scale Logistics RTS (working title: TBD)

> **Purpose of this file:** Single source of truth for the project. It is the handoff
> document for any engineer, agent, or new Claude instance picking this up cold.
> Keep it updated as decisions are made and phases complete. When something changes,
> update the relevant section AND add a line to the Decision Log.
>
> **Last updated:** 2026-06-17
> **Status:** Phase 2 in progress — flow-field pathfinding + avoidance + placeholder sprites; 1,200 units at ~2.3ms/tick
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

### 7.3 Combat — flow, damage model, suppression (abstracted, logistics-fed)
Combat **auto-resolves** from positioning, supply, and stats; the player commands intent, not shots.

**Per-tick pipeline** (throttled on the staggered think-tick):
1. **Acquire** — an armed unit with no valid target scans the spatial grid within `range + margin`
   for enemies it can damage (domain check); picks best by `target_priority` + threat + distance.
2. **Decide** (stance/utility AI) — Aggressive: chase & fire · Defensive: fire in range, don't chase ·
   Hold-ground: fire, never move · Hold-fire: never fire · plus retreat-at-X%-HP.
3. **Fire** — if in range, off cooldown, **has ammo**, arc/LoS ok → spawn projectile (travel time) or
   hitscan; consume ammo; reset cooldown; roll accuracy (cover / elevation / target-motion / suppression).
4. **Resolve** — on hit: `damage = base × armor_mult[damage_type][armor_class]` (+ splash); emit `DamageEvent`.
5. **Apply** — subtract HP; add suppression; HP ≤ 0 → death (wreck + event + freed upkeep).
6. **Suppression** — incoming fire lowers accuracy/speed; high suppression forces caution/retreat; decays.
7. **Logistics** — firing drains onboard **ammo**, moving drains **fuel**; empty → can't act → auto-requests
   resupply (job). **Combat effectiveness is a readout of supply.**

**Damage model (locked):** one small `armor_mult[damage_type][armor_class]` table — not per-unit matrices.
- damage types: `small_arms | AP | HE | AA` · armor classes: `infantry | light | heavy | air | structure`
- This **subsumes the anti-air rule** (AA weapons: high vs `air`, ~0 vs ground; ground weapons: 0 vs `air`)
  and gives counters (AP vs heavy, small-arms vs infantry, HE splash) — the rock-paper-scissors from one table.

**Suppression (locked in):** units accumulate suppression under fire → reduced accuracy + speed; high →
forced caution/retreat; decays when not under fire. (HP-only fallback if it proves fiddly in playtest.)

**Upkeep (locked in):** units consume a trickle of ammo/fuel/supply over time (not just build cost), so a
big army you can't supply degrades — reinforcing the logistics identity.

### 7.4 Unit model — Forms, Abilities, Transitions, Upgrades, Auto-cast
A **unit is a persistent, faction-owned entity whose capabilities come from its active _Form_.** A
`UnitDef` is a small **state machine**: a set of Forms + Transitions. **A building is just a Form**
(immobile + structure/producer components), so unit↔building conversion and construction phases use
the same mechanism — no separate building system.

- **Form (mode / phase):** what the entity is right now — `chassis(Mobile{speed,turn_rate,collision,
  terrain_mask} | Immobile) + durability(hp, armor_class) + armament[Weapon(+turret mount)] +
  abilities[id…] + sprite/turret layers + role components (Builder | Producer | Storage | Cargo …)`.
  One Form = simple unit; multiple Forms = siege/deploy/morph/construction phases.
- **Turret / mount:** a weapon may carry `mount{turret_sprite, traverse_rate, arc (360=full, 0=fixed),
  pivot}`, rendered as an independently-rotating layer that tracks the target while the hull moves.
- **Ability (verbs a Form grants, from a registry of effect types):** `trigger(active|passive|auto) +
  target_mode(self|unit|point|area|none|toggle) + cost(ammo|energy|resources|hp) + cooldown +
  effect[…](fire_special, transform, build, repair, haul, deploy, cloak, toggle_stance, spawn,
  area_effect, self_destruct, capture …) + ui(icon, hotkey, name, tooltip, slot, confirm?)`. Compose
  existing effects in data; a new effect type is one small registered handler (no scripting language).
- **Transition (change Form):** `from→to, trigger(ability|build_complete|timer|hp_threshold|condition),
  duration, cost, reversible?, interruptible?`. HP carries over as a **percentage**.
- **Upgrades / research (faction-wide, build-gated):** `UpgradeDef{ cost, research_time,
  prerequisites(building/upgrade), affects(unit tags), effects[stat_delta | unlock_ability |
  unlock_form | unlock_unit | cost_reduction] }`. A unit's **effective stat = base + Σ active faction
  upgrades matching its tags** (present + future units). Unit *access* stays build-gated; upgrades
  layer on top. (Per-unit **veterancy** = same mechanism, later.)
- **Auto-cast policies (the low-micro layer):** each ability runs **Manual | Auto | Off** (toggle on
  the command card). An auto ability has `AutoRule{ condition, target_selector, priority }`; on the
  staggered think-tick the unit fires its highest-priority rule whose **condition** holds (resources/
  cooldown/target permitting). Conditions from a registry: `self.hp<X · ammo==0 · enemy_in_range ·
  enemy_count_within(r)>=N · ally_wounded_within(r) · suppression>X · stationary_for(t) · …`. Sensible
  per-ability defaults + doctrine presets + stance gating = **set policy once, no per-cast micro**.
  Auto-cast is scored inside the utility AI, not a separate loop.
- **Command-card UI:** auto-generated from the selection's active-Form abilities + standard commands
  (Move / Stop / Hold / Attack-move). Buttons reflect cooldown / disabled (no ammo·energy·res) / toggle
  state; click or hotkey → instant fire or targeting mode; multi-select shows shared abilities, applies
  to all. New unit/ability/upgrade → its button appears automatically. Icons live in an icon atlas.

**v1 roster (Cold-War flavor, data-driven, names per faction):** Engineer (builder/repair) · Infantry ·
Main Battle Tank (turret) · Artillery (deploy Form, splash) · AA Vehicle · Jet/Helicopter (air) ·
Supply Truck (hauler). Naval deferred (movement layer supports it).

**Example data — illustrative; pins the shape, not final field names:**
```ron
UnitDef(
  id: "mbt", name: "Main Battle Tank", faction: "vanguard", default_form: "mobile",
  forms: {
    "mobile": Form(
      chassis: Mobile(speed: 70, turn_rate: 180, collision: 11, terrain: [Ground]),
      durability: (hp: 400, armor: Heavy),
      armament: [ Weapon(
        damage: 90, damage_type: AP, target_domains: [Ground], range: 220, cooldown: 2.2,
        projectile_speed: 600, splash: 0, accuracy: 0.9, ammo_per_shot: 1,
        mount: Some(Turret(sprite: "mbt_turret", traverse_rate: 120, arc: 360)) ) ],
      abilities: ["siege", "detonate"],
    ),
    "sieged": Form(
      chassis: Immobile, durability: (hp: 400, armor: Heavy),
      armament: [ Weapon( damage: 160, damage_type: HE, range: 420, cooldown: 5.0,
        splash: 48, accuracy: 0.95, ammo_per_shot: 2 ) ],
      abilities: ["unsiege"],
    ),
  },
  transitions: [
    (from: "mobile", to: "sieged", trigger: Ability("siege"), duration: 1.5, reversible: true),
    (from: "sieged", to: "mobile", trigger: Ability("unsiege"), duration: 1.2),
  ],
  logistics: (ammo_capacity: 24, fuel_capacity: 100, fuel_per_tile: 0.2, upkeep: (fuel: 0.05)),
  cost: (metal: 600, components: 40), build_time: 18, requires: "factory",
)

AbilityDef(
  id: "detonate", trigger: Active, target_mode: SelfArea, cooldown: 0,
  effect: [SelfDestruct(damage: 250, damage_type: HE, radius: 64)],
  default_autocast: Off,                          // dangerous -> manual by default
  ui: (icon: "ic_detonate", hotkey: "T", name: "Detonate", confirm: true),
)

UpgradeDef(
  id: "composite_armor", name: "Composite Armor", requires: "research_bay",
  cost: (metal: 400, components: 80), research_time: 40,
  affects: ["tank", "heavy_vehicle"], effects: [StatDelta(armor_bonus: 1, hp_mult: 1.15)],
)
```

### 7.5 Buildings (v1 placeholder)
Command Center · Extractor (on node) · Refinery · Barracks (infantry) · Factory (vehicles) ·
Airbase (aircraft) · Depot/Warehouse (storage) · Forward Supply Dump · Turret · AA Turret ·
Generator (power). Infrastructure: Roads/Rail.

### 7.6 Tech progression
**Build-gated access + upgrade research.** Unit/building *access* is gated by what you've built (need
a Factory to make Tanks). On top of that, **upgrades/research** (see §7.4) improve produced units
faction-wide and can unlock abilities/forms. No full free-form tech-tree *screen* in v1, but the
upgrade system is in scope (no longer deferred).

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

### 7.15 UI system (modular & scalable)
All in-game UI is built on **one reusable toolkit**, not ad-hoc draw calls — so panels are consistent,
themeable, resolution-scalable, and cheap to add.
- **Immediate-mode widget core** over the renderer's screen-space pass: composable widgets — Panel,
  Button, IconButton, Label, Bar, Grid, ScrollList, Tooltip, ContextMenu, Modal.
- **Layout:** anchors + stack/grid that reflow on resolution/DPI change (scales to any window).
- **Theming:** data-driven theme (colors, fonts, padding, icon set) — restyle everything in one place.
- **Input layering:** UI consumes mouse/keys **first**; the world only sees input the UI didn't handle
  (a click on the command card never leaks a move order to the map). Hotkeys routed through the UI.
- **Panel registry:** HUD panels register themselves (no central switch) — command card, resource/power
  bar, selection/portrait panel, build menu, minimap slot, notifications/alerts, match-setup/confirm modals.
- **Batched + icon atlas:** UI draws batch through an icon atlas (same idea as the sprite atlas).
- This toolkit is what the **command card** (§7.4), **economy UI** (Phase 4), and **minimap / match
  setup** (Phase 7) are built on.

### 7.16 Buildings — placement, construction, production (first-class; a building = a Form)
A building is an entity in an **immobile Form** (§7.4), so it shares the unit ability/command-card model.
Its lifecycle is its own infrastructure (built in Phase 3 alongside units). Authored as a `BuildingDef`
— the same Form machinery as units, plus a footprint:
- **Footprint / custom size:** each building declares a tile **footprint** — `W×H` (1×1 gun turret,
  2×2 factory, 3×3 HQ, 1×N walls) or an irregular **tile mask**. Drives placement, occupied tiles, and
  nav blocking. (Mobile units use a collision radius; buildings use a footprint.)
- **Turrets:** buildings use the same weapon **`mount`** as units (§7.4) — defensive turrets rotate to
  track targets; multiple mounts (different pivot offsets) make a multi-gun fort.
- **Placement:** ghost/blueprint preview **snapped to the tile grid** (footprint-aligned); validity
  check (every footprint tile passable & clear, within builder/build-radius, on a resource node for
  extractors); rotation; multi-place blueprint mode (§7.13). Placed footprints become **impassable** on
  the nav grid so units path around them.
- **Construction:** a builder takes a **build job**; the building advances through Forms
  (site → frame → complete) as work is applied; resources drain gradually; cancel (refund) / repair.
- **Production:** producer buildings (factory / barracks / airbase) have a **production queue** + **rally
  point** + exit; built units spawn at the exit and move to rally. Production **bills** (§7.2) and
  **research** (§7.4) run through the same queue. *(Resource costs are wired when the economy lands in
  Phase 4; the queue / placement / construction systems are built in Phase 3.)*
- **Deploy / undeploy:** mobile ↔ building via transitions (MCV → HQ), reusing the Form mechanism.
- **Command card:** buildings use the same auto-generated card (produce, set rally, research, toggle).

**Example (building) — illustrative; a turreted 1×1 defense and a 2×2 producer:**
```ron
BuildingDef(
  id: "gun_turret", name: "Gun Turret", faction: "vanguard",
  footprint: (1, 1),                          // tiles; use a tile mask for irregular shapes
  default_form: "built",
  forms: {
    "site":  Form(chassis: Immobile, durability: (hp: 80,  armor: Structure)),   // under construction
    "built": Form(
      chassis: Immobile, durability: (hp: 600, armor: Structure),
      armament: [ Weapon(damage: 40, damage_type: AP, target_domains: [Ground], range: 200,
        cooldown: 1.0, mount: Some(Turret(sprite: "turret_gun", traverse_rate: 240, arc: 360))) ],
    ),
  },
  transitions: [ (from: "site", to: "built", trigger: BuildComplete) ],
  cost: (metal: 250, components: 10), build_time: 12, requires: "command_center",
)

BuildingDef(
  id: "vehicle_factory", name: "Vehicle Factory", footprint: (2, 2), default_form: "built",
  forms: { "built": Form(chassis: Immobile, durability: (hp: 1400, armor: Structure),
           role: Producer(produces: ["mbt", "aa_vehicle"], rally: true)) },
  cost: (metal: 800, components: 30), build_time: 25, requires: "command_center",
)
```

## 8. Roadmap (core-first, then content; playable/verifiable at each step)
> **Restructured 2026-06-18 — core-first.** Front-load UI + core unit/building gameplay + combat +
> a polish pass into a *complete, polished vertical slice*, THEN add major features/content. The deep
> logistics/infrastructure (our identity) moves to Phase 6; a **minimal** economy stays in Phase 3 to
> make the core loop playable. (Map editor → Phase 8.) This §8 list is the canonical phase order.

**Done:** Milestone 0 · Phase 1 (engine skeleton) · Phase 2 (pathfinding at scale).

**Core & polish — finish a polished game first:**
- **Phase 2.5 — Modular UI system:** reusable widget toolkit (panels, buttons, layout, theming, input
  layering, icon atlas) every later panel builds on.
- **Phase 3 — Core units & buildings:** registry/event-bus, ability framework, Forms/transitions,
  command-card UI, selection + control groups; building placement / construction / production +
  build/repair jobs; a **minimal economy** (simple resource(s) to gate building/production). → build-and-produce loop.
- **Phase 4 — Combat & a fightable opponent:** damage table (subsumes AA), projectiles, suppression,
  death, health bars/FX, ammo basics + a basic skirmish enemy. → a playable battle.
- **Phase 5 — Polish / "game-ready":** full HUD, fog of war, minimap, audio, win/lose + match setup,
  controls & feel polish, performance pass. → a complete, polished core game (vertical slice).

**Major features & content — built on the polished core:**
- **Phase 6 — Deep logistics & infrastructure (the identity depth):** refined chain (Ore/Crude →
  Metal/Fuel → Components), supply/network graph, self-running haulers, production bills, power grid,
  roads/rail/power construction + blueprint mode, throughput/coverage, upkeep.
- **Phase 7 — Research/upgrades + advanced autonomy:** upgrade/research buildings, auto-cast policies,
  doctrines, deeper utility AI, formations.
- **Phase 8 — Content & factions:** full unit/building rosters, multiple factions, the in-engine map
  editor (content tool), maps, scenarios/campaign.
- **Phase 9 — Balance & final polish.**

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
- **2026-06-17** — **Unit model = Forms + Abilities + Transitions** (a state machine); a building is
  just a Form. Enables siege/deploy modes, unit↔building conversion, and construction phases from one
  mechanism (see §7.4).
- **2026-06-17** — **Combat: small `armor_mult[type][class]` damage table** (supersedes the bare
  AA-only rule); **suppression** and **continuous upkeep** locked in. Stays abstracted, gains counters.
- **2026-06-17** — **Upgrades/research promoted from parking lot to a real system** (faction-wide,
  build-gated); buffs stats and unlocks abilities/forms. Unit *access* stays build-gated.
- **2026-06-17** — **Command-card UI + auto-cast policies**: UI auto-generated from a Form's abilities;
  abilities self-trigger by condition (Manual/Auto/Off) — the low-micro ability layer.
- **2026-06-17** — **Registry + event-bus scaffolding elevated to a Phase-3 prerequisite** — it's the
  dispatch layer for abilities / effects / conditions / transitions.
- **2026-06-17** — **Added Phase 2.5 — a modular/scalable UI toolkit** (widgets, layout, theming, input
  layering, icon atlas) before Phase 3, since the command card and later panels build on it (§7.15).
- **2026-06-17** — **Phase 3 expanded to full unit AND building infrastructure** — placement,
  construction, production queues/rally, deploy↔undeploy — all on the Forms model (§7.16).
- **2026-06-18** — **Roadmap restructured to core-first** (see §8): UI + core unit/building gameplay +
  combat + a polish pass (a complete, polished vertical slice) come BEFORE major features. Deep
  logistics/infrastructure (the identity) → Phase 6; a minimal economy stays in Phase 3; map editor → Phase 8.
- **2026-06-18** — **Pathfinding upgraded** to 8-neighbour Dijkstra + gradient flow + bilinear sampling
  (natural, anticipatory routing). **Placeholder art** swapped to Kenney "Top-down Tanks Redux" (CC0).
  **Camera Y-pan** fixed (world is Y-up via from_display_rect; derive screen-relative dirs from the camera).

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
