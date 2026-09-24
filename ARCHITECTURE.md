# Hub — architecture plan (v4)

A Windows desktop hub for everything I run: game servers, PCs, local AI tools and
storage. It is built from **modules**, and it has an **assistant** that can operate
any of them. Every feature works by hand too: each thing the assistant can do is a
button first.

Status: plan. Design mockups: *Claude Agent Desktop* canvas (Assistant, Minecraft,
PC monitor, Automations, Palette, Phone, Settings).

---

## 1. Goals and non-goals

**Goals**

- One place to control my utilities: the Minecraft server, my PCs, AI media on Pluto, and files.
- **Modular.** A new utility is a new module folder, with no core changes.
- **Buttons first, AI second.** Every module action is a button, a palette command, a
  phone button and an assistant tool, all calling the same code.
- An **assistant** (Claude by default, a local model optional) that can use every module,
  with permissions I control.
- **Automations** (schedules and event triggers) that keep running when my main PC is off.
- A **phone** web app, reachable only over my private network.
- Usable by me every day: robust, with logs, installers and updates.

**Non-goals (for now)**

- Public release, a module marketplace, or third-party modules. Modules are written by me (and Claude).
- Guest or friend access, and multiple users.
- Video or music generation (a GTX 1080 Ti can't do these well).
- Anything reachable from the public internet.

## 2. Machines

| Node | Hardware | Role |
|---|---|---|
| **Main PC** | Windows, decent GPU | Desktop app (UI, palette, tray, mic/wake word), local modules (PC monitor, files) |
| **Pluto** | Windows, GTX 1080 Ti (11 GB), 64 GB RAM | **Home node**: assistant runtime, cross-node automations, activity log, phone web app, library and storage, AI media (Forge, TTS, STT) |
| **mc-vm** | VM (hypervisor TBD) | Minecraft (Forge) only, isolated. Runs a minimal node with just the Minecraft module |

Pluto is the **home node** because it's meant to stay on. The phone app and AI
automations need a machine that's awake, and the main PC is often off.

## 3. Big picture

```
 Main PC                                   Pluto (home node)                      mc-vm
┌──────────────────────────┐   Tailscale  ┌──────────────────────────────┐       ┌────────────────┐
│ Desktop app (Tauri 2)     │◄────────────►│ Hub services                  │◄─────►│ Node (minimal)  │
│  React UI · palette · tray│   WSS + MCP  │  assistant runtime (Agent SDK)│  WSS  │  minecraft      │
│  voice capture/wake word  │              │  automation engine            │  +MCP │   module        │
│ Node (local modules)      │              │  activity log · API proxy     │       └────────────────┘
│  pc-monitor · files       │              │  phone web app                │
└──────────────────────────┘              │ Node (modules)                │
                                           │  ai-media · storage ·          │
                                           │  pc-monitor · vm-power         │
                                           └──────────────────────────────┘
```

- **The desktop app** is a client: UI, windows, tray, the Alt+Space palette and voice capture.
  If Pluto is unreachable, it still drives local modules directly (degraded mode).
- **Nodes** run modules and report their status. Each node is a single Rust binary
  with a tray icon that starts at sign-in.
- **Hub services** on the home node coordinate everything that spans nodes: the assistant,
  cross-node automations, the activity log, the phone app, and the Claude API proxy.

## 4. Modules

A module is a folder with a manifest plus code (**Python or TypeScript**, with a small
SDK for each). The node runs each module as its **own process**, so one crashing
never takes others down. A module is an MCP server over stdio, plus a manifest the node reads.

```toml
# modules/minecraft/module.toml
id       = "minecraft"
name     = "Minecraft"
icon     = "box"
runtime  = "python"
entry    = "main.py"

[status]                     # polled/streamed; shown in sidebar, tiles, assistant context
fields = ["state", "players", "max_players", "tps", "memory_mb", "uptime_s"]
sidebar = "{players}/{max_players}"

[[actions]]
id = "server.start"          # → button, palette command, phone button, AI tool
label = "Start"
icon = "play"
ai = "safe"                  # safe | confirm | never
enabled_when = "state == 'stopped'"

[[actions]]
id = "server.stop"
label = "Stop"
icon = "square"
ai = "confirm"
confirm_when = "players > 0" # humans get a confirm too when players are online
params = { delay_min = { type = "int", default = 0 } }

[[actions]]
id = "world.restore"
label = "Restore backup"
ai = "never"                 # only a human can press this

[[events]]                   # usable as automation triggers
id = "server.crashed"

[[views]]                    # built from the hub's own blocks; no module UI code
blocks = ["toolbar", "tiles", "console:rcon", "table:players", "list:backups"]
```

**How an action flows:** a button click, palette command, phone tap, assistant tool
call or automation step all become the **same** `action.invoke(module, id, params)`.
The node checks permissions, runs it, and logs it to the activity feed along with
*who* started it (you, the assistant, an automation, or the phone).

**UI blocks** (drawn by the hub in its square Raycast-style look): toolbar buttons,
stat tiles, meters, tables, a console/log view with an input, lists, forms and a media
gallery. Later, a module that really needs a custom panel can ship one in a sandboxed
iframe.

### First modules

| Module | Node | Actions (buttons) | Status / views | Events |
|---|---|---|---|---|
| **Minecraft** (Forge) | mc-vm | start, stop (with delay), restart, backup now, say, kick, op, whitelist add/remove, restore backup | state, players, TPS, memory, uptime, RCON console, players table, backups list, mods count | started, stopped, crashed, player joined/left, backup done |
| **VM power** | Pluto | start VM, stop VM, restart VM | VM state, CPU, RAM | VM stopped unexpectedly |
| **PC monitor** | every node | sleep, restart, shut down (all *confirm*), wake Pluto (Wake-on-LAN, from the main PC) | CPU, GPU, temperatures, RAM, disks | threshold crossed |
| **AI media** | Pluto | generate / edit / upscale image (Forge `--api`), speak (TTS), transcribe (STT) | GPU job queue, models and LoRAs, library gallery | job done |
| **Files / storage** | Pluto (+ main PC) | list, read, write, move between PCs, back up folder | allowed folders, usage | backup done / failed |

The Minecraft module talks to the server over **RCON bound to localhost inside the
VM** (setup turns it on if needed). The VM keeps its isolation: its node exposes only
the Minecraft module, and has no access to Pluto's files or GPU.

## 5. Assistant

- **Brain:** Claude through the Claude Agent SDK, running on the home node. A local model
  (Ollama) is an option, labeled experimental.
- **Tools:** the assistant sees one merged tool list with every module action from every
  online node. Installing a module teaches it new skills with no core changes.
- **Context:** each module's live status is included, so "what's running?" doesn't need
  a tool call.
- **Permissions** come from each action's `ai` tier:
  - `safe` runs immediately.
  - `confirm` shows an inline **Approve / Deny** prompt in the chat, the palette and the phone.
  - `never` is refused, and the assistant tells you which button to press.
- Anything coming from the web can never, on its own, trigger a `confirm` action without you.
- **Chats become automations:** you can save a conversation as an automation, and the
  tool calls turn into steps.
- **Voice:** a wake word ("Hey hub") runs locally on the main PC, and nothing leaves the
  mic before it. Audio streams to speech-to-text on Pluto, the brain replies, and TTS on
  Pluto speaks it. Push-to-talk works in the palette.

### API proxy (on the home node)

The agent never sees the Anthropic key. The Agent SDK is pointed at
`ANTHROPIC_BASE_URL=http://127.0.0.1:<port>` with a short-lived session token. The proxy:

- adds the real key
- streams responses unchanged, so prompt caching still works
- meters tokens and cost
- enforces budget caps by blocking the next call
- shares one rate limiter across everything, backing off on 429/529 errors

## 6. Automations

- **Triggers:** schedules (cron), module events (`minecraft.server.crashed`), status
  conditions (`disk.used_pct > 90`) and voice phrases.
- **Steps:** module actions, `wait until` / `wait for`, `notify`, and optionally an
  assistant step (the only kind that costs money).
- **Where they run:** automations that only touch one node's modules run **on that node**,
  so the Minecraft crash restart, stop-when-empty and backups still work if Pluto and the
  main PC are both down. Cross-node or AI automations run on the home node.
- Every run is logged to the activity feed, with its result and duration.

Starter automations:

- restart the Minecraft server on crash, and notify me
- stop the server 15 minutes after the last player leaves
- back up the world nightly at 03:00, to Pluto storage
- alert me when any disk goes above 90%

## 7. Phone

- A mobile web app served by the home node, reachable **only over Tailscale**. Installable
  as a home-screen app (PWA).
- The same square style with big tap targets:
  - module dashboards (Minecraft Start / Stop / Restart, node status)
  - approval prompts
  - recent activity
  - assistant chat
- Limitation: if Pluto (the home node) is asleep, the phone app is down. The main PC's
  node can serve a read-only fallback, and Wake-on-LAN from the phone needs an always-on
  device on the LAN (future: a Raspberry Pi or the router).

## 8. Data and secrets

| Data | Owner |
|---|---|
| Activity log, automations, chats, module settings, library catalog | SQLite on the home node (versioned migrations) |
| UI state, window layout, local cache (thumbnails, recent files) | SQLite in `%APPDATA%\Hub` on the main PC |
| Library files (images, audio), world backups | Pluto storage, e.g. `D:\Hub\Library`, `D:\Hub\Backups` |
| Claude session transcripts | The Agent SDK's own store on the home node |
| Anthropic key, node pairing tokens, module secrets (RCON password) | Windows Credential Manager on the node that uses them |

**Backups:** one disk isn't a backup. There's an optional nightly copy of the library,
world backups and the home node database to a second drive or the main PC. It's off by
default, and the app nags you until it's configured.

## 9. Security

- Nodes listen only on the LAN and Tailscale addresses. A Windows Firewall rule
  allows only the **Private** network profile and the Tailscale adapter.
- Nodes are **paired** with a one-time code and use per-node tokens with mutual
  authentication after that.
- Files modules only see folders you've explicitly allowed. Writing and deleting are `confirm`.
- PC power actions (sleep, restart, shut down) are always `confirm`, even when you press
  the button yourself while things are running.
- The Minecraft VM stays isolated: its node exposes one module and nothing else.
- Modules are local code I trust, but each still runs as its own process with only its
  own settings.

## 10. Reliability and diagnostics

- The node supervisor restarts crashed modules with backoff, and a module stuck
  crash-looping is marked **failed** and shown in the UI.
- The desktop app reconnects automatically. When a node is offline, its modules show
  offline and their buttons are disabled with a reason.
- Every action has explicit states: queued, running, done, failed, needs approval.
- Structured logs (Rust `tracing`, Python `structlog`, Node `pino`) go to rotating files
  on each node.
- **Help → Export diagnostics** zips up the logs and versions, with secrets removed.

## 11. Tech stack

| Part | Choice |
|---|---|
| Desktop shell | **Tauri 2** (Rust), WebView2 |
| UI | React 19, TypeScript, Vite, Tailwind v4, Zustand, TanStack Query, CodeMirror 6, xterm.js, **Lucide** icons (1.5px, square caps), Geist fonts |
| Node daemon | Rust (tokio, axum for HTTPS/WSS, `rusqlite`, `keyring`, `tracing`), tray icon, starts at sign-in |
| Module SDKs | Python (MCP Python SDK + pydantic), TypeScript (MCP TS SDK + zod) |
| Assistant runtime | Node (TypeScript) + Claude Agent SDK, bundled Node runtime |
| Local brain (optional) | Ollama |
| Voice | openWakeWord (main PC), faster-whisper + Kokoro/Piper/XTTS (Pluto) |
| Transport | Tailscale; WSS for status/events, MCP (streamable HTTP) for tools |
| Protocol source of truth | zod schemas in `packages/protocol` → JSON Schema → Rust types at build time |
| Packaging | Tauri installer (per-user) for the app; per-user installer for nodes; GitHub Releases + updater; unsigned for now |

## 12. Repository layout

```
apps/desktop/          Tauri app: src-tauri/ (Rust) + src/ (React UI)
apps/phone/            mobile web app (served by the home node)
crates/node/           node daemon
crates/protocol/       generated Rust types
packages/protocol/     zod schemas (source of truth)
packages/assistant/    assistant runtime + API proxy client
packages/sdk-ts/       TypeScript module SDK
sdk/python/            Python module SDK
modules/minecraft/     first modules
modules/pc-monitor/
modules/vm-power/
modules/ai-media/
modules/storage/
docs/                  this plan, module authoring guide
```

## 13. Roadmap

Rough estimates for one person working focused.

| Phase | Deliverable | Rough time |
|---|---|---|
| **0 · Test run** | Tauri shell; node daemon + pairing over Tailscale; module SDK "hello world"; **test that the Agent SDK runs from bundled Node on Windows** | 1–2 wk |
| **1 · Buttons** | **Minecraft**, **VM power** and **PC monitor** modules with full UI: tiles, console, tables, buttons, palette commands. Activity log. **Useful with no AI.** | 3–4 wk |
| **2 · Assistant** | Assistant on the home node, module tools, permission tiers, approval prompts, API proxy + cost meter | 3 wk |
| **3 · Automations + phone** | Automation engine on nodes, starter automations, phone web app | 3 wk |
| **4 · AI media + storage** | Forge/TTS/STT module, library on Pluto, files module, backups | 3–4 wk |
| **5 · Voice + polish** | Wake word, voice replies, local brain, installers, updater, diagnostics | 3 wk |

## 14. Risks

- **Packaging the Agent SDK:** it runs the Claude Code engine as a child process, so ship
  a real Node runtime. Verify in phase 0.
- **The home node is a single point of failure:** if Pluto is down, the assistant, phone and
  cross-node automations are down. Single-node automations and local buttons still work.
- **The 1080 Ti is aging:** pin CUDA and PyTorch versions that still support Pascal cards.
  One GPU means a job queue with priorities: voice > images > batch jobs.
- **Hands-free voice:** false triggers and latency. Ship push-to-talk first.
- **Prompt injection:** limit the damage (permission tiers, `never` actions, no key in the
  agent). Nobody can fully prevent it.

## 15. Open questions

- The Minecraft VM: which hypervisor, which guest OS, how the server is started today, and whether RCON is on.
- Can Pluto stay awake 24/7 as the home node, or do we need a low-power always-on box?
- Which TTS voice(s), and is voice cloning (XTTS) worth its GPU memory?
