# PROJECT.md â€” Large-Scale Logistics RTS (working title: TBD)

> **Purpose of this file:** Single source of truth for the project. It is the handoff
> document for any engineer, agent, or new Claude instance picking this up cold.
> Keep it updated as decisions are made and phases complete. When something changes,
> update the relevant section AND add a line to the Decision Log.
>
> **Last updated:** 2026-06-17
> **Status:** Phase 2 in progress â€” flow-field pathfinding + avoidance + placeholder sprites; 1,200 units at ~2.3ms/tick
> **Repository:** https://github.com/B0RE16/laughing-waffle (private) Â· local folder: `coldwar-rts/`

---

## 1. One-line vision
A **tile-based operational RTS** focused on **military campaigns, logistics, and expansion**. The
player is a theater commander directing a military machine. Victories are won through preparation,
logistics, infrastructure, and operational planning â€” not unit micromanagement. The game sits
between a traditional RTS and grand strategy.

## 2. Core pillars (the non-negotiable identity)
1. **Theater commander, not a pilot.** The player commands Combat Groups, not individual vehicles.
   Individual units are still simulated and rendered, but the primary interface is the group layer.
2. **Logistics intent, not logistics micromanagement.** The player defines intent (routes, depots,
   corridors, plans). The system executes automatically. Players should never manually route a truck.
3. **Expansion is the core progression loop.** Resource deposits are infinite. Players fight for
   territory because resource *regions* are valuable, not because resources deplete. The map is
   worth owning.
4. **Geography matters.** Tile-based terrain with mountains, passes, rivers, valleys, and chokepoints
   that create real strategic decisions. The map is not decorative.
5. **Automated rear defense.** Radar, patrols, quick reaction forces, and automatic repairs handle
   rear threats. The player focuses on fronts and operations.
6. **Rewards preparation over APM.** Every system should reward planning, positioning, and logistics
   more than reaction speed or individual unit control.
7. **Built to extend.** Data-driven, modular, ECS. New content = author data, not edit code.

## 3. Control philosophy â€” the player commands at 3 levels
The low-micro promise is structural. Most play happens at levels 1â€“2.
1. **Strategic (policy):** logistics network, routes, depots, corridors, expansion plans, defense
   zones, doctrine presets. Set once, runs itself.
2. **Operational (intent):** assign objectives to Combat Groups, set behavior priorities, request
   support, reinforce groups. The player is a theater commander, not a unit controller.
3. **Tactical (optional):** *draft* a group for direct control at a key moment. Never required.

### 3.1 The war loop (five phases)
Every operation follows this cycle â€” the game should support all five, not just Execution:
1. **Reconnaissance** â€” discover terrain, enemy positions, resource regions.
2. **Planning** â€” assign objectives, choose routes, prepare logistics.
3. **Preparation** â€” pre-position supplies, build roads/depots, move forces to staging.
4. **Execution** â€” Combat Groups advance and engage per their orders.
5. **Consolidation** â€” secure taken territory, establish logistics, repair, reinforce.

### 3.2 Combat Groups (the primary player-facing unit)
Groups are collections of individual units organized by role:
- **Armored Group** (tanks, heavy vehicles)
- **Mechanized Group** (infantry + vehicles)
- **Artillery Group** (indirect fire support)
- Future: Recon Group, Engineer Group, Air Defense Group

Players interact with groups to:
- Assign objectives (advance to, hold, attack, withdraw)
- Set behavior priorities (aggressive / defensive / hold)
- Request support (artillery, air, supply)
- Reinforce (attach replacement units from the rear)

Individual units within a group handle execution autonomously via the utility AI + standing orders.

## 4. Tech stack & tooling
| Concern | Choice | Rationale |
|---|---|---|
| Language | **Rust** | Best perf for 1,000+ units; no GC pauses; real multithreading; fits theme |
| Window/render/input | **macroquad** | Thin; compiles to **native (perf)** AND **WASM (browser-verifiable)** |
| ECS | **hecs** | Fast archetype ECS, data-oriented, scales to thousands |
| Everything else | **custom** | Pathfinding, logistics, AI, sim â€” this is "our own engine" |
| Alternative (if needed) | Bevy | More batteries + parallel systems, but heavier / less "own engine" |
| Version control | **Git + GitHub** | Source history, branch per phase/feature, PR review |
| CI | **GitHub Actions** | Auto `build`/`test`/`clippy` + WASM build on every push/PR |

**Why Rust over web/native alternatives:** chosen by project owner for raw performance.
Rusted Warfare itself is Java/LibGDX; Rust was selected to exceed that ceiling. Decision is
revisitable (see Decision Log) but currently firm.

### How verification works (important for handoff)
- **Logic:** `cargo test` â€” fully automated, headless.
- **Visuals (autonomous):** build to WebAssembly and run in a browser; screenshot via preview
  tooling. This is the primary way the AI engineer confirms rendering without human help.
- **Visuals (alt):** run native build; screenshot the OS window via desktop control.
- **Performance:** stress-test with 1,000+ dummy units starting in Phase 2 â€” never guess about scale.

### Version control & engineering practices
- **Git + GitHub** from Milestone 0. `main` stays green (always builds + passes tests).
- **Branch per phase/feature** (e.g. `phase-1-engine-skeleton`, `feat/flow-fields`); merge via PR â€”
  good discipline even solo, and it documents *why* changes happened.
- **Conventional Commits** (`feat:`, `fix:`, `refactor:`, `docs:`, `test:`, `perf:`) for readable history.
- **GitHub Actions CI** runs `cargo build`, `cargo test`, `cargo clippy`, and the WASM build on every
  push/PR â€” the verification harness, automated.
- **Update PROJECT.md/PLAN.md in the same PR** as the change they describe, so docs never drift.
- `.gitignore` excludes `target/`, build artifacts, and local cruft.

## 5. Engine architecture (modules / frameworks to build)
Built as clean, separable systems. Foundational systems (must exist early, painful to retrofit)
are marked â˜….

- â˜… **Core loop** â€” fixed-timestep sim (~20Hz) + interpolated rendering; deterministic-friendly.
- â˜… **ECS (data-oriented)** â€” components in flat arrays; systems are functions over them. Locked
  in early because converting later is a rewrite. Required for scale.
- â˜… **Pathfinding service** â€” **flow fields as the primary mover** (one field for a whole army),
  grid A* for stragglers/special cases, local avoidance (steering/separation). Pluggable.
- â˜… **Autonomy: Utility AI** â€” each unit scores options (engage / take cover / resupply / retreat /
  pull a job / idle) and picks the best. Standing orders bias the scoring. This is *why* the player
  doesn't babysit units.
- â˜… **Job system** ("work-giver" model, RimWorld-style) â€” global queue of jobs (haul, repair,
  garrison, reinforce). Idle units claim by priority + proximity. Makes base & logistics self-manage.
- â˜… **Squad / formation hierarchy** â€” command groups as single entities; shared flow-field movement;
  auto-reinforce from production. Essential for commanding hundreds.
- â˜… **Spatial grid** â€” uniform grid for all neighbor/range queries (targeting, cover, vision,
  supply range). No O(nÂ²) scans, ever.
- â˜… **Supply/network graph** â€” nodes (depots, extractors, conduits) + edges; carries resources &
  power; holds throughput + coverage + route-security data.
- **Renderer** â€” sprite batching via macroquad; layered draws (terrain â†’ buildings â†’ units â†’
  projectiles â†’ UI); camera transform. Kept behind a clean boundary.
- **Command system** â€” all actions as queued commands (supports waypoint queueing, debuggable).
- **Event bus** â€” decoupled messaging (unit died, building complete, node depleted).
- â˜… **Data-driven definitions & faction system** â€” units, weapons, buildings, and factions defined
  as composable data (stats + component lists), not hardcoded. Lets us author *any* kind of unit
  and support **multiple factions** from one engine. Ship one Cold-War-era faction first; the
  architecture supports N. (Internal data-driven design; player-facing modding still not a goal.)
- **Victory-conditions / match-rules system** â€” configurable win/lose objectives (annihilation,
  Command-Center kill, economic, survival/time, or custom combinations) selected at match setup.
- â˜… **Map system** â€” versioned map format (layered: terrain, elevation, resources, spawns, markers,
  pre-placed infrastructure) + an **in-engine map editor**. Tile/terrain types are data-driven. The
  editor grows as new placeable types (resources, infra, zones, triggers) are added.

### 5.1 Extensibility & scalability patterns (so major changes stay easy)
- **Data-driven content** â€” units, buildings, weapons, resources, tech-gating, factions, tiles, and
  victory conditions all defined in versioned data files. New content = author data, not edit code.
- **Composition over inheritance** â€” capabilities are ECS components + an **ability list** per unit;
  new behavior = a new component/ability + system, never editing existing types.
- **Trait-based system boundaries** â€” major systems (renderer, pathfinder, AI behavior, resource
  type, victory condition, job type) sit behind Rust traits so implementations are swappable.
- **Registries, not match-statements** â€” unit/ability/job/victory/AI types register at startup; the
  engine iterates registries, so adding a type never touches a central switch.
- **Event bus** â€” systems react to events (unit died, building complete, route cut) so new systems
  hook in without modifying emitters.
- **Versioned schemas** â€” data/map/save formats carry a version + migration path; old content keeps working.
- **No magic numbers in code** â€” all tunables live in data.

## 6. Scale & performance strategy (target: 1,000+ units)
- **Data-oriented ECS** â€” tight numeric loops over typed/flat storage.
- **Flow fields default**, per-unit A* only as fallback.
- **Staggered AI ticks** â€” units "think" every N ticks on a rotating schedule (~50/tick at 1,000 units).
- **Simulation LOD** â€” rear-area/idle units simulate coarsely; combat units fully.
- **Spatial grid** for every neighbor query.
- **Renderer kept swappable** â€” if rendering throughput becomes the wall, upgrade the batcher;
  WASM build perf is validated continuously.

## 7. Game design

> **Superseded 2026-06-22.** All prior Â§7 content is replaced by this version.
> This is the canonical game design. Follow it exactly.

---

### 7.1 Resources â€” physical, not abstract

Resources are **physical quantities** that exist in the world. They are produced at specific locations, stored in depots, transported by trucks, and consumed at the front. Every step is visible, attackable, and meaningful.

There are no abstract economy numbers. When a depot runs out of ammo, it is empty. When a road is cut, convoys slow or stop. The player feels the supply chain.

#### Strategic Resources (extracted from territory)
Deposits are **infinite**. Players fight for regions because the region produces â€” not because it depletes.

| Resource | Extracted from | Produces |
|---|---|---|
| **Ore** | Ore Basin (mine) | Building Supplies, Weapon Parts |
| **Oil** | Oil Field (pump) | Fuel |

#### Logistics Resources (physically transported)
These resources fuel military operations. They move through the world on trucks along roads.

| Resource | Consumed by | Effect when empty |
|---|---|---|
| **Ammo** | Infantry, tanks, artillery, defenses firing | Weapons stop firing |
| **Fuel** | Vehicles moving, trucks driving | Units stop moving |
| **Building Supplies** | Construction, road building, repairs | Construction and repairs stop |
| **Weapon Parts** | Unit production, group reinforcement | No replacements available |

#### Resource visibility
Every depot shows a bar for each resource type it holds. Bars turn **amber** when low, **red** when empty. The player reads supply health at a glance without opening menus.

---

### 7.2 Roads â€” the most important strategic asset

Roads determine the speed and capacity of every supply route, reinforcement, and advance. A road network is a military asset. It must be built, maintained, and defended.

#### Road tiers
| Type | Speed multiplier | Notes |
|---|---|---|
| Off-road | 1Ã— | Passable but slow; no convoys |
| Dirt road | 2Ã— | Basic supply route; can be built quickly |
| Paved road | 3Ã— | Full capacity; required for heavy supply |

#### Building roads
1. Player selects the Road blueprint tool.
2. Player clicks a start tile, drags to an end tile.
3. A ghost preview shows the road path.
4. Player confirms. Blueprint placed.
5. Nearest available Engineers automatically claim the build job.
6. Engineers consume **Building Supplies** from the nearest depot as they work.
7. Road tiles upgrade to the new speed tier when complete.

#### Road damage and repair
- Artillery and strikes damage road tiles. Damaged roads revert toward off-road speed.
- A damaged road segment is highlighted on the map.
- Engineers with Building Supplies auto-repair assigned roads.
- A cut road is an immediate logistics crisis â€” supply throughput drops.

#### Strategic decisions roads create
- *Single road:* cheap and fast to build, fragile under attack.
- *Two parallel roads:* redundancy, survives one being cut, costs more.
- *Road network:* multiple routes between hubs, rerouting possible when one segment fails.
- *Road as objective:* capturing or cutting an enemy road is a valid operational strategy.

---

### 7.3 Buildings

Players place **blueprints**. Engineers execute them. No instant construction.

Every building is placed on the tile grid with a ghost preview showing validity (green = valid, red = blocked). Engineers nearby automatically claim construction jobs and build using Building Supplies from the nearest depot.

#### Extraction
| Building | Size | Placed on | Produces |
|---|---|---|---|
| **Mine** | 2Ã—2 | Ore Basin tile | Ore â†’ flows to nearest Processing Facility |
| **Oil Pump** | 1Ã—1 | Oil Field tile | Oil â†’ flows to nearest Fuel Refinery |

#### Processing
| Building | Size | Input â†’ Output |
|---|---|---|
| **Processing Facility** | 3Ã—3 | Ore â†’ Building Supplies + Weapon Parts |
| **Fuel Refinery** | 2Ã—2 | Oil â†’ Fuel |
| **Ammo Factory** | 3Ã—3 | Weapon Parts â†’ Ammo |

#### Storage and distribution
| Building | Size | Function |
|---|---|---|
| **Depot** | 2Ã—2 | Stores all resource types; trucks deliver/collect here; visible stockpile bars |

#### Intelligence
| Building | Size | Function |
|---|---|---|
| **Radar Station** | 2Ã—2 | Permanently reveals a large area; triggers threat alerts on enemy incursion |

#### Fortifications
| Building | Size | Function |
|---|---|---|
| **Bunker** | 1Ã—1 | Garrisoned infantry gain cover bonus, reduced damage taken |
| **Gun Turret** | 1Ã—1 | Automated defense; fires on enemies in range; requires ammo from nearby depot |

#### Production
| Building | Size | Produces |
|---|---|---|
| **Barracks** | 2Ã—2 | Infantry squads |
| **Vehicle Factory** | 3Ã—3 | Tanks, IFVs, trucks, artillery |

---

### 7.4 Supply Routes â€” player sets intent, trucks execute

The player creates a route between two points. The system dispatches trucks. The player never drives a truck.

#### Creating a route
1. Player opens the Supply Route tool.
2. Clicks an **origin** (depot, factory, or processing facility).
3. Clicks a **destination** (depot or forward position).
4. Sets **resource type** (Ammo / Fuel / Building Supplies / Weapon Parts).
5. Sets **priority** (Low / Medium / High â€” determines how many trucks are assigned).
6. Route is created. Trucks immediately begin dispatching along the road network.

#### How trucks behave
- Trucks follow roads. Road quality determines their speed.
- If a road segment is damaged, trucks reroute automatically or slow to off-road speed.
- Trucks are visible in the world as vehicle sprites moving along roads.
- Trucks can be attacked. A destroyed truck loses its cargo.
- Truck convoys can be ambushed â€” this is a legitimate enemy strategy.

#### Route management
- Players see active routes on the map as colored lines.
- A route with problems (road cut, depot empty, trucks destroyed) shows an alert icon.
- Players can delete, pause, or reprioritize routes at any time.

#### Strategic depth
- *High priority* routes assign more trucks but require more vehicles.
- *Too many routes* = vehicles spread thin, all routes slow.
- *Chokepoints* = one road segment failing collapses multiple routes.
- *Defending routes* is a real operational task, not background noise.

---

### 7.5 Combat Groups â€” the primary command structure

Individual units are still simulated, rendered, and fight autonomously. But players issue orders to **Combat Groups**, not individuals.

#### Group types
| Group | Typical composition | Role |
|---|---|---|
| **Armored Group** | 20â€“50 tanks | Heavy assault, breakthrough |
| **Mechanized Group** | 10â€“20 infantry squads + 5â€“10 IFVs | Combined arms, flexible |
| **Artillery Group** | 5â€“15 artillery pieces | Fire support, interdiction |
| **Engineer Group** | 5â€“10 engineers | Construction, road building, repair |
| **Recon Group** | 5â€“10 light vehicles | Reconnaissance, screening |

#### Creating a group
1. Player selects individual units.
2. Presses **G** or clicks "Form Group."
3. Group auto-named (1st Armored Group, 2nd Mechanized Group, etc.).
4. Player can rename it.

#### Group UI card (shown when a group is selected)
```
1st Armored Group
â”â”â”â”â”â”â”â”â”â”â”â”â”â”â”â”â”â”â”â”â”â”â”â”
 Tanks:    34 / 50    â–ˆâ–ˆâ–ˆâ–ˆâ–ˆâ–ˆâ–ˆâ–ˆâ–‘â–‘
 Ammo:     Adequate   â–ˆâ–ˆâ–ˆâ–ˆâ–ˆâ–ˆâ–ˆâ–ˆâ–‘â–‘
 Fuel:     Low        â–ˆâ–ˆâ–ˆâ–ˆâ–‘â–‘â–‘â–‘â–‘â–‘
â”â”â”â”â”â”â”â”â”â”â”â”â”â”â”â”â”â”â”â”â”â”â”â”
 Stance:  [Aggressive] [Defensive] [Hold]
 [Advance To] [Hold Position] [Withdraw]
 [Request Arty Support]  [Reinforce]
```

#### Group orders
| Order | Effect |
|---|---|
| **Advance To** | Group moves to target tile; engages enemies en route (attack-move) |
| **Hold Position** | Group stays in place; fires on enemies in range |
| **Withdraw** | Group retreats toward the rear along its last route |
| **Request Artillery Support** | Nearest Artillery Group fires on designated target |
| **Reinforce** | Opens reinforcement panel to source replacement units |

#### Groups and supply
Groups automatically request resupply from the nearest depot when ammo or fuel runs low. If no supply route reaches the group's position, it runs dry. Tanks stop moving. Infantry stop firing. This is why pushing logistics forward matters â€” advancing groups need depots to follow them.

---

### 7.6 Reinforcements â€” tracking losses, sourcing replacements

Destroyed units are permanent losses against a group's roster until reinforced.

#### Loss tracking
The group card always shows current vs original strength (34/50 tanks). The player sees at a glance which groups are degraded.

#### Reinforcing a group
1. Player selects a group and clicks **Reinforce**.
2. Panel shows available sources:
```
Reinforce 1st Armored Group
â”â”â”â”â”â”â”â”â”â”â”â”â”â”â”â”â”â”â”â”â”â”â”â”â”â”â”
Vehicle Factory Alpha
  12 tanks ready now
  Travel time: ~3 min

Vehicle Factory East
  Build time: 8 min (queue: 3)

Reserve Pool â€” Depot C
  6 tanks stored
  Travel time: ~5 min
```
3. Player selects a source. Replacement units drive from source to group automatically.
4. Until they arrive, the group fights at reduced strength.

#### Strategic implications
- A group that takes heavy losses and can't be reinforced will eventually stop functioning.
- Enemy groups can be ground down â€” forcing them to consume their reserve pool.
- The player who controls resource regions and factories can sustain reinforcement longer.
- Cutting supply routes starves the enemy's ability to replace losses.

---

### 7.7 Reconnaissance and Fog of War

The map starts hidden. Players discover it through movement.

#### Fog states
| State | Visual | Information |
|---|---|---|
| **Unexplored** | Black | Nothing visible |
| **Last-seen** | Dimmed overlay | Terrain and buildings as last seen; no unit info |
| **Visible** | Full | Everything in real time |

#### Vision sources
| Source | Vision radius | Notes |
|---|---|---|
| Infantry | Small | Short range; good in urban/forest terrain |
| Tanks | Medium | Standard ground unit vision |
| Recon vehicles | Large | Built for this; advance screening |
| Artillery | Small | Long-range weapons, short vision |
| Radar Station | Very large (fixed) | Permanently reveals area; shows enemy blips, not unit types |
| High ground | Bonus | Elevation tiles extend vision of units standing on them |

#### What reconnaissance enables
- Finding enemy depots, roads, and factories (interdiction targets).
- Identifying chokepoints before committing a group.
- Locating enemy supply routes to plan ambushes.
- Discovering resource regions worth capturing.

#### Enemy intelligence
When a Radar station detects an enemy unit, the player sees a blip and a threat level â€” not a detailed roster. The player knows something is there, not exactly what. Committing a scout group to investigate is a real decision.

---

### 7.8 The Nine-Phase Combat Loop

This is the core gameplay pattern. Every major operation should move through these phases. The **Eastern Pass** scenario below is the benchmark for all design decisions.

```
Phase 1: RECONNAISSANCE
  Locate enemy defenses, roads, depots, supply routes, troop positions.
  Commit recon groups. Place radar. Build a picture before acting.

Phase 2: PREPARATION
  Attacker: build roads toward the objective, create forward depots,
            move artillery groups to range, stage combat groups, stockpile.
  Defender: build bunkers, stock ammo depots, position troops,
            create backup supply routes, designate QRF.

Phase 3: LOGISTICS INTERDICTION
  Attacker targets enemy supply routes.
  Artillery hits roads. Recon groups ambush convoys.
  Goal: reduce enemy throughput before the main assault.
  Defender scrambles to repair roads and reroute convoys.

Phase 4: ATTRITION
  Defender consumes ammo defending. Bunkers take damage. Engineers repair.
  Repairs consume Building Supplies. Stockpiles shrink.
  Both sides racing: attacker to exhaust defender, defender to resupply.

Phase 5: CRITICAL DECISIONS
  Defender: reinforce troops / repair roads / create alternate route /
            counterattack enemy artillery / commit reserves.
  Attacker: continue attrition / escalate / destroy depot / cut road.

Phase 6: MAIN ASSAULT
  Combat groups advance. Artillery fires. Infantry clears positions.
  Outcome determined by: supplies remaining, terrain, road access,
  stockpile depth, reinforcement availability, preparation quality.
  NOT by unit count alone.

Phase 7: SUPPLY FAILURE
  Ammo depleted â†’ defenses stop firing.
  Building Supplies depleted â†’ repairs stop, bunkers degrade.
  Fuel depleted â†’ vehicles can't reposition or retreat.
  Position begins collapsing from the inside.

Phase 8: BREAKTHROUGH
  Attacker captures the objective.
  Defender's remaining units withdraw or are destroyed.

Phase 9: CONSOLIDATION
  Attacker repairs roads, establishes depot at captured position,
  extends supply routes, repairs buildings.
  Position becomes operational. Next expansion begins.
```

**Design rule:** every major system should make at least one phase of this loop deeper or more interesting. If a new feature doesn't affect any phase, question whether it belongs.

---

### 7.9 Region System â€” strategic zoom layer

When the player zooms out past a threshold, the map transitions to **Region View**. Individual units become dots. Regions become the primary information layer.

#### What a region is
A named strategic area with defined boundaries: *Northern Ore Basin, Eastern Mountain Pass, Southern Oil Field, Central Valley, River Crossing.* Each region has strategic significance from its terrain, resources, and position.

#### Region card (visible when zoomed out)
```
â”Œâ”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”
â”‚ NORTHERN ORE BASIN          â”‚
â”‚ â— Controlled: Player        â”‚
â”œâ”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”¤
â”‚ Infrastructure: 67%         â”‚
â”‚ Military: 1st Mechanized    â”‚
â”‚                             â”‚
â”‚ Stockpiles                  â”‚
â”‚  Ammo:              400 â–ˆâ–ˆâ–ˆâ–ˆâ”‚
â”‚  Fuel:              250 â–ˆâ–ˆâ–ˆ â”‚
â”‚  Building Supplies: 800 â–ˆâ–ˆâ–ˆâ–ˆâ”‚
â”œâ”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”¤
â”‚ Threat Level: LOW    â—â—‹â—‹â—‹â—‹  â”‚
â”‚ Output: 45 Ore/min          â”‚
â””â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”˜
```

#### Region ownership
- A region is controlled by the faction with military presence and infrastructure in it.
- Contested regions flash. Control requires both holding the ground AND having a depot there.
- Losing a region immediately reduces resource output.

---

### 7.10 Automated Rear Defense â€” policy, not babysitting

The player sets up rear defense as a policy. The system executes. The player is never expected to personally chase raiders.

#### Radar and threat alerts
```
âš  THREAT DETECTED
Supply Convoy Ambushed â€” Eastern Corridor
Enemy light vehicles â€” 3 km south of Depot Alpha

[Dismiss]  [Mark as Priority]  [Send QRF]
```

#### Quick Reaction Forces
1. Player designates a Combat Group as QRF for a zone.
2. When a threat fires in that zone, QRF automatically moves to intercept.
3. After clearing the threat, QRF returns to its designated position.

#### Automated repairs
Engineers with assigned repair tasks and Building Supplies repair damaged roads and buildings automatically. If supplies run out, repairs stop.

#### Patrol routes
Player draws a patrol path, assigns a group to it. They cycle the route continuously.

**Failure is a policy failure, not a micro failure.** A raider destroying a depot means no radar covered the area, no QRF was assigned, no patrol was running â€” a policy choice, not a reaction-time failure.

---

### 7.11 Win Conditions

Three paths to victory, selectable at match setup:

| Condition | How |
|---|---|
| **Decapitation** | Destroy the enemy HQ |
| **Economic Collapse** | Enemy Weapon Parts reach 0 AND their factory is destroyed â€” they can no longer reinforce; force collapses |
| **Territorial Control** | Control all named resource regions simultaneously for 10 minutes |

All three reward logistics and infrastructure. Decapitation requires sustained assault through a defended network. Economic collapse requires supply line interdiction. Territorial control requires expansion and defense across multiple fronts.

---

### 7.12 How the game feels â€” the progression arc

#### First 10 minutes
The player starts with: HQ, 1 Armored Group (20 tanks), 1 Engineer Group (5 engineers), 1 Depot (200 Ammo, 100 Fuel, 500 Building Supplies). A nearby Ore Basin is visible but uncontrolled. Enemy is off-screen.

First decisions: move the Armored Group to secure the basin â†’ place a Mine blueprint â†’ draw a road from HQ to the basin â†’ place a Depot near the basin â†’ place a Processing Facility once the road is done â†’ create a supply route from the Processing Facility back to the HQ Depot.

The first supply truck appears on the road. Ore starts flowing. Building Supplies accumulate. The player has their first economic loop.

#### Mid-game
A visible road network. Multiple depots. Several supply routes as colored lines. Trucks moving. Two or three Combat Groups at different positions. A front line forming.

The player reads the map like an operational commander: where are my groups, where are my routes, what's threatened, what needs attention. Most systems run autonomously. The player intervenes when something changes.

#### Late-game
A complex logistics network supports multiple fronts. Regions are contested. The enemy is cutting supply lines. The player is deciding: repair the eastern road or reroute the convoys? Reinforce the armored group or hold for a counterattack? Commit reserves to push through the pass?

The best player doesn't have the most units. They have the most resilient military machine.

#### The emotional core
The player should feel the satisfaction of a system working â€” convoys moving, depots filling, groups advancing on good logistics. And they should feel the stress of a system failing â€” a road cut, a depot raided, a group running dry mid-assault. These two feelings, alternating, are the game.

**Strategic resources** (extracted from territory, infinite deposits â€” fight for the region, not depletion):
- **Ore** â€” mined from ore regions; feeds construction.
- **Oil** â€” from oil regions; feeds fuel production.

**Logistics resources** (consumed by operations, generated from strategic resources):
- **Building Supplies** â€” from Ore; used for construction and fortifications.
- **Weapon Parts** â€” from Ore + manufacturing; enables unit production and repairs.
- **Ammo** â€” manufactured; consumed by combat. Empty = reduced effectiveness.
- **Fuel** â€” from Oil; consumed by vehicle movement and operations. Empty = immobile.

**Why simple:** the depth is in *logistics* (routing, distribution, security), not production chains.
Resources should be instantly legible so the player focuses on the operational layer.

**Expansion beats depletion:** resource deposits are infinite. The player fights for territory because
ore/oil *regions* are strategically valuable, not because individual nodes run out.

### 7.13 The Eastern Pass â€” reference scenario for all design decisions

This scenario is the benchmark. When evaluating any new system, ask: does it make this scenario deeper or more interesting?

**Setup:** A mountain pass controls the only viable route between the player's industrial heartland and the northern ore basins. The enemy has fortified it. The player must take it.

**Player builds:** bunkers at the southern approach, a radar station to cover the valley, an ammo depot supplied from the central hub, a fuel depot, two supply routes (primary + backup), 1 Armored Group + 1 Mechanized Group staging south of the pass.

**Enemy holds:** bunkers in the pass, a gun turret covering the chokepoint, ammo depot supplied from the north, 1 Mechanized Group defending.

**The attacker's campaign:**
1. Recon reveals the road from the northern depot to the pass.
2. Artillery Group shells the road â€” throughput drops.
3. Recon vehicles ambush a convoy â€” the pass depot's ammo starts running low.
4. Armored Group advances under artillery fire.
5. Defender's guns slow as ammo runs short.
6. Mechanized Group flanks through a valley.
7. Pass falls.

**The defender's response:** repair the road, reroute convoys over rougher terrain (slower but intact), commit a reserve armored group, counterattack the artillery position.

**Why this matters:** this one scenario exercises every major system: fog of war, reconnaissance, roads, supply routes, depots, stockpiles, combat groups, reinforcement, terrain, automated defense, and the nine-phase loop. If all systems work, this scenario plays itself.

---

**What the player creates:**
- **Routes** â€” define a supply corridor between two points (rear depot â†’ forward area).
- **Depots** â€” designate a tile or area as a storage/distribution point.
- **Logistics corridors** â€” broader zones where supply trucks operate automatically.
- **Expansion plans** â€” mark a resource region for future integration into the supply network.

**What the system manages automatically:**
- Truck routing and dispatch.
- Delivery scheduling and load balancing.
- Re-routing when a road is cut or depot destroyed.
- Resource movement along defined corridors.

**Depth mechanics:**
- **Infrastructure matters** â€” roads raise convoy speed + capacity; building roads is strategic investment.
- **Supply range** â€” units far from a depot resupply slowly. Advancing requires *pushing logistics
  forward*: new depots, extended roads. You can't just move the army; you have to move the supply chain.
- **Route security** â€” corridors can be raided. Cutting the enemy's supply line is a win condition;
  defending yours is rear-area defense. A front that runs out of ammo collapses on its own.
- **Pull-based** â€” depots have target stock levels; shortfalls trigger automatic resupply from the rear.
  Players set policy (keep 200 shells at depot X), not individual hauls.

```
STRATEGIC     â†’  LOGISTICS HUB  â†’  CORRIDOR  â†’  FORWARD DEPOT  â†’  COMBAT GROUPS
(ore / oil)      (refines into      (roads,       (ammo, fuel,       (consume ammo
                  supplies/ammo/     trucks,        supplies)          fuel; auto-
                  fuel; auto)        auto-          (player-placed)    request resupply)
                                     dispatch)
```

## 8. Roadmap (canonical — updated 2026-06-22)

**Done:** Milestone 0 · Phase 1 (engine skeleton) · Phase 2 (pathfinding, perf, movement) · Phase 2.5 (UI toolkit, HUD, minimap, command card) · Phase 3 partial (combat, buildings, debug suite, discrete shots, turrets).

**Phase 3 — Combat Groups + Reconnaissance (current)**
- Combat Group entity: owns a unit list; group card UI (name, strength, supply status, losses).
- Group orders: Advance To (attack-move), Hold Position, Withdraw, Request Artillery Support, Reinforce.
- Fog of war: tiles hidden until revealed by unit vision; last-seen dimmed; Radar clears large areas.
- Recon unit type: high vision radius, fast, light armor.
- Attack-move order: group advances and engages enemies en route.

**Phase 4 — Physical Resources + Extraction**
- Mine (Ore Basin) and Oil Pump (Oil Field) blueprints; engineers build them.
- Processing Facility (Ore → Building Supplies + Weapon Parts), Fuel Refinery (Oil → Fuel), Ammo Factory (Weapon Parts → Ammo).
- Depot: stores resources; visible stockpile bars (amber=low, red=empty).
- Resources consumed: ammo by combat, fuel by movement. Empty = weapons stop / units stop.
- Geographic terrain: Ore Basin, Oil Field, Mountain Pass, River Crossing, Valley, Chokepoint tile types with strategic properties.

**Phase 5 — Roads + Supply Routes + Trucks**
- Road blueprint: player draws start→end, engineers build, road tiles placed.
- Road tiers: off-road (1x), dirt (2x), paved (3x). Damageable, repairable.
- Supply Route tool: origin depot → destination → resource type → priority. Trucks dispatch automatically.
- Trucks visible in the world, can be attacked. Route alert when disrupted.

**Phase 6 — Reinforcements + Region System + Interdiction**
- Group loss tracking and reinforce panel (choose source, units path automatically).
- Region system: named strategic areas; zoom-out transitions to Region View with region cards.
- Logistics interdiction: artillery damages roads, groups can ambush convoys.

**Phase 7 — Automated Rear Defense + Win Conditions**
- Radar threat alerts with location. QRF designation. Patrol routes.
- Bunkers garrisoned for cover bonus. Gun turrets consuming ammo from depots.
- Win conditions: Decapitation / Economic Collapse / Territorial Control.
- Enemy AI at operational level: expands, builds routes, attacks objectives.
- Eastern Pass reference scenario fully playable.

**Phase 8 — Content, Polish, Campaign**
- Full unit and building roster. Designed campaign map (Eastern Pass).
- Audio: command acks, fire, impact, ambient.
- Full UI polish and performance pass.


## 9. Decision log
- **2026-06-17** â€” Genre set: macro-scale, low-micro logistics RTS (not a direct RW clone).
- **2026-06-17** â€” Single-player only; no multiplayer (sim still built deterministic-friendly).
- **2026-06-17** â€” Modding not a goal (but unit/building defs kept data-driven internally).
- **2026-06-17** â€” Scale target raised to **hundreds per side / 1,000+ total**; drove data-oriented
  ECS + flow-fields + staggered ticks + sim LOD as mandatory.
- **2026-06-17** â€” RimWorld influence scoped to **logistics/jobs/zones**, NOT combat granularity.
  Combat is abstracted. Backline logistics is the primary depth.
- **2026-06-17** â€” **Language: Rust** (owner's call, for performance). Chosen over TS/web despite
  slightly slower iteration; macroquad's WASM target preserves autonomous visual verification.
- **2026-06-17** â€” Engine libs **confirmed**: macroquad + hecs + custom systems. Bevy = documented fallback.
- **2026-06-17** â€” Economy: **refined tier locked in** (Ore/Crude â†’ Metal/Fuel â†’ Components). Deep
  multi-stage backline supply chain is the core depth.
- **2026-06-17** â€” Theme: **start Cold-War-era, single faction**, but engine is **multi-faction +
  data-driven composable units** from the architecture down (any unit type authorable in data).
- **2026-06-17** â€” Victory: **fully configurable match conditions** (annihilation / decapitation /
  economic / survival / custom combos) via a match-rules system.
- **2026-06-17** â€” Micro floor: **opt-in squad drafting** (standing orders default; direct control optional).
- **2026-06-17** â€” **Extensibility is a first-class requirement**: data-driven content, ECS
  composition, trait boundaries, registries, event bus, versioned schemas (see Â§5.1).
- **2026-06-17** â€” **Infrastructure (roads/rail/power/supply) elevated to a core pillar**, with a
  blueprint/planning mode (see Â§7.13).
- **2026-06-17** â€” **First-class map-making**: versioned layered map format + in-engine editor
  (new Phase 1.5; see Â§7.14).
- **2026-06-17** â€” **Version control: Git + GitHub** with branch-per-feature, Conventional Commits,
  PR merges, and GitHub Actions CI (build/test/clippy/WASM) as standing engineering practice.
- **2026-06-17** â€” **Milestone 0 complete**: macroquad 0.4 + hecs 0.10 scaffold builds native + WASM;
  fixed-timestep sim loop + placeholder render verified. WASM needs a `--import-undefined` linker flag
  (`.cargo/config.toml`); visual verification is via native offscreen render-target capture
  (`COLDWAR_CAPTURE` env var) since the preview tool can't screenshot a live animation loop.
- **2026-06-17** â€” **Unit model = Forms + Abilities + Transitions** (a state machine); a building is
  just a Form. Enables siege/deploy modes, unitâ†”building conversion, and construction phases from one
  mechanism (see Â§7.4).
- **2026-06-17** â€” **Combat: small `armor_mult[type][class]` damage table** (supersedes the bare
  AA-only rule); **suppression** and **continuous upkeep** locked in. Stays abstracted, gains counters.
- **2026-06-17** â€” **Upgrades/research promoted from parking lot to a real system** (faction-wide,
  build-gated); buffs stats and unlocks abilities/forms. Unit *access* stays build-gated.
- **2026-06-17** â€” **Command-card UI + auto-cast policies**: UI auto-generated from a Form's abilities;
  abilities self-trigger by condition (Manual/Auto/Off) â€” the low-micro ability layer.
- **2026-06-17** â€” **Registry + event-bus scaffolding elevated to a Phase-3 prerequisite** â€” it's the
  dispatch layer for abilities / effects / conditions / transitions.
- **2026-06-17** â€” **Added Phase 2.5 â€” a modular/scalable UI toolkit** (widgets, layout, theming, input
  layering, icon atlas) before Phase 3, since the command card and later panels build on it (Â§7.15).
- **2026-06-17** â€” **Phase 3 expanded to full unit AND building infrastructure** â€” placement,
  construction, production queues/rally, deployâ†”undeploy â€” all on the Forms model (Â§7.16).
- **2026-06-18** â€” **Roadmap restructured to core-first** (see Â§8): UI + core unit/building gameplay +
  combat + a polish pass (a complete, polished vertical slice) come BEFORE major features. Deep
  logistics/infrastructure (the identity) â†’ Phase 6; a minimal economy stays in Phase 3; map editor â†’ Phase 8.
- **2026-06-22** â€” **Major game direction update** (owner instruction). Key changes recorded here:
  1. **Combat Groups** replace individual unit control as the primary player-facing layer. Individuals
     still simulated + rendered, but players issue orders to groups (Armored / Mechanized / Artillery),
     not vehicles. Groups have objectives, behavior priorities, support requests, and reinforcement.
  2. **The five-phase war loop** (Recon â†’ Planning â†’ Preparation â†’ Execution â†’ Consolidation) is the
     design spine. Every system should support all five phases, not just Execution.
  3. **Logistics intent replaces logistics micromanagement.** Player creates routes, depots, corridors,
     and expansion plans; the system manages trucks, deliveries, and routing automatically.
  4. **Resources simplified** to Strategic (Ore, Oil) + Logistics (Building Supplies, Weapon Parts,
     Ammo, Fuel). No Factorio-style chains. Economy is military-focused and legible.
  5. **Expansion is the primary progression mechanic.** Deposits are infinite; fight for regions,
     not depletion. The seven-step expansion flow (Discover â†’ Secure â†’ Plan â†’ Build â†’ Connect â†’
     Activate â†’ Integrate) is the core gameplay loop.
  6. **Geography is strategic.** Tile-based maps with mountains, passes, rivers, chokepoints, valleys,
     and resource basins that create real decisions. Terrain is not decorative.
  7. **Rear defense is automated.** Radar, patrols, QRF, auto-repair handle rear threats. Player
     focuses on fronts and operations; never chases individual raiders.
  8. **Design principle:** consistently rewards preparation, planning, logistics, and positioning more
     than APM, micromanagement, or individual vehicle control.
  **Existing code compatibility:** flow-field movement, individual unit sim/render, building placement,
  faction system, health/combat, ECS â€” all compatible. New build priorities: Combat Group layer,
  logistics intent UI, expansion flow, geographic terrain generation.
- **2026-06-18** â€” **Pathfinding upgraded** to 8-neighbour Dijkstra + gradient flow + bilinear sampling
  (natural, anticipatory routing). **Placeholder art** swapped to Kenney "Top-down Tanks Redux" (CC0).
  **Camera Y-pan** fixed (world is Y-up via from_display_rect; derive screen-relative dirs from the camera).

## 10. Open questions (need owner input)
Resolved 2026-06-17: theme (Cold-War start, multi-faction architecture), economy depth (refined
tier), victory (configurable conditions), micro floor (opt-in drafting), engine libs (macroquad+hecs).
Remaining:
1. **Working title / game name** â€” still TBD (not blocking).
2. **Concrete numbers** â€” map size, per-side soft cap, tick rate: starting targets set in PLAN.md
   (256Ã—256, ~500/side, 20Hz), tuned during Phase 2 stress tests.
3. **Faction list & asymmetry** â€” which factions beyond the first, and how they differ (later phase).
4. **Specific unit stats / costs / tech-gating tables** â€” filled in during Phases 4â€“5.

## 11. Working-style notes (for any agent picking this up)
- Owner wants **minimal interference**: make the engineering calls yourself; surface only genuine
  taste/design decisions. Plan features before implementing them. Build & verify your own work.
- Prefer prose planning the owner can steer in chat over heavy questionnaires.
- This is on **Windows** (PowerShell primary; Bash available). Game repo dir:
  `C:\Users\patri\Downloads\ClaudeCode\coldwar-rts` (the parent folder holds unrelated projects).
- **Always use the debug suite before reading code to diagnose a problem** (see Â§13).
  One command â†’ one short output â†’ act. Never read source files to understand runtime behaviour
  when a debug command can answer the question in one line.

## 13. Debug suite (AI-first, one command â†’ one line output)
Built in `src/debug.rs`. Every check is a single env-var command that runs headlessly, prints one
short structured result, and exits. **Always prefer these over reading source files or screenshots.**

### COLDWAR_ASSERT=\<scenario\> â€” pass/fail invariant checks (exits 0/1)
```
COLDWAR_ASSERT=combat_discrete     # 1 shot = 1 tracer (no DPS spray)
COLDWAR_ASSERT=no_friendly_fire    # 0 damage to same-faction units
COLDWAR_ASSERT=turret_delays       # turret must aim before firing
COLDWAR_ASSERT=formation_fills     # 25-unit group move â†’ 0 stuck after 1200 ticks
```
Add new scenarios to `debug::run_assert` whenever a new system needs regression coverage.

### COLDWAR_QUERY=\<fields\> â€” one JSON line of world state after N ticks
```
COLDWAR_UNITS=40 COLDWAR_QUERY="shots_fired,kills,alive,mean_hp,moving,tick_ms" COLDWAR_QTICKS=600
â†’ {"shots_fired":168,"kills":{"any":28},"alive":{"vanguard":33,"crimson":18},...}
```
Fields: `shots_fired` `kills` `alive` `mean_hp` `moving` `tracers` `tick_ms` `entities`
`COLDWAR_QTICKS` sets the number of ticks to run (default 300).

### COLDWAR_EVENTLOG=1 â€” write debug/events.jsonl during a normal run
Each significant event (shot, kill, move order, building placed, game over) is appended as a JSON
line. Grep/tail it to answer "what happened to unit X" without reading code.
```
grep '"type":"kill"' debug/events.jsonl | tail -5
```

### Existing headless commands (same pattern)
```
COLDWAR_BENCH=400                  # sim perf: ms/tick at N ticks
COLDWAR_SETTLE=1000                # jitter metric: still_moving + mean_disp px/tick
COLDWAR_UNITS=N                    # spawn N units per side
COLDWAR_CAPTURE=out.png            # headless screenshot after N frames
```

### When to use which
| Question | Command |
|---|---|
| Did combat fire correctly? | `COLDWAR_QUERY="shots_fired,kills"` |
| Is there friendly fire? | `COLDWAR_ASSERT=no_friendly_fire` |
| Are bullets discrete (not DPS spray)? | `COLDWAR_ASSERT=combat_discrete` |
| Does turret gate fire? | `COLDWAR_ASSERT=turret_delays` |
| Do formations fill? | `COLDWAR_ASSERT=formation_fills` |
| Is there jitter after a group move? | `COLDWAR_SETTLE=1000` |
| What's the sim perf at 1200 units? | `COLDWAR_UNITS=1200 COLDWAR_BENCH=400` |
| Why did units die unexpectedly? | `COLDWAR_EVENTLOG=1` then grep kills |

## 12. RTS benchmarking & competitive analysis (standing practice)
**Standing instruction (owner, 2026-06-21):** continuously test and compare our features and
code against well-known RTS games, and use those comparisons to keep proposing features. Every
iteration / increment should ask: *"how do the games that solved this already do it, and are we
matching or deliberately diverging?"* â€” then surface concrete suggestions.

**Reference games & what to mine from each:**
- **Supreme Commander / Total Annihilation / Planetary Annihilation** â€” flow-field movement at huge
  scale, strategic zoom, queued/patrol/factory build orders, eco as flow rates (our economy model),
  area commands. *Closest spiritual reference for scale + macro.*
- **StarCraft II** â€” control-group/hotkey muscle memory, control feel, command-card clarity,
  selection ergonomics (we mirror: groups, double-click-type, shift-add, command card).
- **Company of Heroes** â€” cover/suppression, squad cohesion, retreat behavior; informs our
  abstracted combat + stances.
- **Age of Empires IV / II** â€” formations (line/box/wedge), gather/drop-off logistics loops, rally
  points, idle-villager management â†’ our job board & rally/production.
- **Command & Conquer / Red Alert** â€” base building feel, power as a global resource that gates
  production (we already model power), MCV deployâ†”undeploy (our buildingâ†”unit transitions).
- **They Are Billions** â€” large defensive lines, wall/zone painting, pause-and-plan against waves.
- **RimWorld / Factorio** â€” the logistics/jobs/zones/throughput identity (our core pillar), not
  combat micro.

**How to apply:** when building or polishing a system, note in the commit / PLAN how it compares to
the reference (matching, simplified, or intentionally different and why). Keep the comparative
feature backlog in PLAN.md fresh and pull the next-most-impactful idea from it each iteration.
