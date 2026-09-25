# Kernel — architecture plan (v4)

A Windows desktop app for everything I run: game servers, PCs, local AI tools and
storage. It is built from **modules**, and it has an **assistant** that can operate
any of them. Every feature works by hand too: each thing the assistant can do is a
button first.

Status: plan. The build plan (specs, data model, phases and acceptance criteria) is in
[PLAN.md](PLAN.md). Design mockups: *Claude Agent Desktop* canvas (Assistant, Minecraft,
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
| **Main PC** | Windows, GTX 1080 Ti | Desktop app (UI, palette, tray, mic/wake word), **speech-to-text** (faster-whisper on its own GPU), local modules (PC monitor, files) |
| **Pluto** | Windows, GTX 1080 Ti (11 GB), 64 GB RAM, **on 24/7** | **Home node**: assistant runtime, cross-node automations, activity log, phone web app, library and storage, AI media (Forge, TTS). Also my Roblox AFK machine, which stays on Windows because Roblox doesn't run on Linux. The **Minecraft server** runs in WSL Ubuntu here (systemd, playit.gg tunnel) |

Pluto is the **home node** because it's meant to stay on. The phone app and AI
automations need a machine that's awake, and the main PC is often off.

## 3. Big picture

```
 Main PC                                   Pluto (home node)
┌──────────────────────────┐   Tailscale  ┌──────────────────────────────┐       ┌────────────────┐
│ Desktop app (Tauri 2)     │◄────────────►│ Kernel services                  │
│  React UI · palette · tray│   WSS + MCP  │  assistant runtime (Agent SDK)│
│  voice capture/wake word  │              │  automation engine            │
│ Node (local modules)      │              │  activity log · API proxy     │
│  pc-monitor · files       │              │  phone web app                │
└──────────────────────────┘              │ Node (modules)                │
                                           │  ai-media · storage ·          │
                                           │  pc-monitor · minecraft ──► WSL│
                                           └──────────────────────────────┘
```

- **The desktop app** is a client: UI, windows, tray, the Alt+Space palette and voice capture.
  If Pluto is unreachable, it still drives local modules directly (degraded mode).
- **Nodes** run modules and report their status. Each node is a single Rust binary
  with a tray icon that starts at sign-in.
- **Kernel services** on the home node coordinate everything that spans nodes: the assistant,
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

[[views]]                    # built from Kernel's own blocks; no module UI code
blocks = ["toolbar", "tiles", "console:rcon", "table:players", "list:backups"]
```

**How an action flows:** a button click, palette command, phone tap, assistant tool
call or automation step all become the **same** `action.invoke(module, id, params)`.
The node checks permissions, runs it, and logs it to the activity feed along with
*who* started it (you, the assistant, an automation, or the phone).

**UI blocks** (drawn by Kernel in its square Raycast-style look): toolbar buttons,
stat tiles, meters, tables, a console/log view with an input, lists, forms and a media
gallery. Later, a module that really needs a custom panel can ship one in a sandboxed
iframe.

### First modules

| Module | Node | Actions (buttons) | Status / views | Events |
|---|---|---|---|---|
| **Minecraft** (NeoForge) | Pluto (drives WSL) | start, stop (with delay), restart, backup now, say, kick, op, whitelist add/remove, restore backup | state, players, memory, uptime, console, players table, backups list, mods count | started, stopped, crashed, player joined/left, backup done |
| **PC monitor** | every node | sleep, restart, shut down (all *confirm*), wake Pluto (Wake-on-LAN, from the main PC) | CPU, GPU, temperatures, RAM, disks | threshold crossed |
| **AI media** | Pluto | generate / edit / upscale image (Forge `--api`), speak (TTS), transcribe (STT) | GPU job queue, models and LoRAs, library gallery | job done |
| **Files / storage** | Pluto (+ main PC) | list, read, write, move between PCs, back up folder | allowed folders, usage | backup done / failed |
| **Roblox** | Pluto | relaunch client, rejoin the last place (both *confirm*) | client running, session length, memory, last disconnect | client crashed, disconnected |

The Roblox module only **watches** the client and relaunches it. It never automates
gameplay or sends input to the game.

**Minecraft backups:** weekly (Sunday 03:00), keeping only the newest. The old one is deleted
only **after** the new one is written and verified (the zip opens and `level.dat` reads), so
there's never a moment with zero good backups.

The Minecraft module runs on Pluto and drives the server inside **WSL Ubuntu** by piping
small bash scripts to `wsl.exe ... bash -s` (systemd, `mc-cmd`, `mc-ping`). It also holds a
WSL session open, because WSL shuts down (and kills the server) when nothing is attached.

## 5. Assistant

- **Brain:** Claude through the Claude Agent SDK, running on the home node. A local model
  (Ollama) is an option, labeled experimental.
- **Account:** an **Anthropic API key** (pay per use). The Agent SDK docs say apps built on
  it can't use claude.ai subscription login or rate limits without Anthropic's approval, so
  my Pro/Max subscription stays for the Claude apps and Claude Code. Default monthly cap
  **$10** with alerts at 80%, and routine turns use a cheaper model.
- **Persona: "Kernel".** A personality prompt makes it a tsundere catgirl, the same voice as
  the Claude that designed it. It's editable in Settings → Assistant. Branding line:
  "Kernel, powered by Claude". **The personality never touches safety text:** approval
  prompts, errors, and what an action will do are always plain and exact.
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
- **Voice:** a wake word ("Hey Kernel", a custom openWakeWord model) runs locally on the main
  PC, and nothing leaves the mic before it. Speech-to-text also runs **on the main PC's
  1080 Ti** (faster-whisper, int8), the brain replies, and **Kokoro** TTS on Pluto's CPU
  speaks it. Push-to-talk works in the palette.

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
- back up the world weekly (Sunday 03:00) to Pluto storage, replacing last week's once the new one is verified
- alert me when any disk goes above 90%
- alert me on my phone if the Roblox client crashes or disconnects

## 7. Phone

- A mobile web app served by the home node, reachable **only over Tailscale**. Installable
  as a home-screen app (PWA).
- The same square style with big tap targets:
  - module dashboards (Minecraft Start / Stop / Restart, node status)
  - approval prompts
  - recent activity
  - assistant chat
- **iPhone:** notifications need the app added to the Home Screen first (Safari → Share →
  Add to Home Screen, iOS 16.4+). Onboarding walks through it.
- Pluto runs 24/7, so the phone app is normally always up. If Pluto is down anyway (updates,
  power cut), the main PC's node serves a read-only fallback.

## 8. Data and secrets

| Data | Owner |
|---|---|
| Activity log, automations, chats, module settings, library catalog | SQLite on the home node (versioned migrations) |
| UI state, window layout, local cache (thumbnails, recent files) | SQLite in `%APPDATA%\Kernel` on the main PC |
| Library files (images, audio), world backups | Pluto storage, e.g. `D:\Kernel\Library`, `D:\Kernel\Backups` |
| Claude session transcripts | The Agent SDK's own store on the home node |
| Anthropic key, node pairing tokens, module secrets | Windows Credential Manager on the node that uses them |

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
modules/roblox/
modules/ai-media/
modules/files/
docs/                  this plan, module authoring guide
```

## 13. Roadmap

Rough estimates for one person working focused.

| Phase | Deliverable | Rough time |
|---|---|---|
| **0 · Test run** | Tauri shell; node daemon + pairing over Tailscale; module SDK "hello world"; **test that the Agent SDK runs from bundled Node on Windows** | 1–2 wk |
| **1 · Buttons** | **Minecraft**, **PC monitor** and **Roblox** modules with full UI: tiles, console, tables, buttons, palette commands. Activity log. **Useful with no AI.** | 3–4 wk |
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
- **Pluto does double duty (home node + Roblox AFK):** Roblox takes some GPU memory, so the
  GPU queue gives Forge less headroom while it's running. Windows Update must not restart
  Pluto on its own: set active hours and schedule restarts.
- **Keeping only one Minecraft backup** means a corruption that goes unnoticed for over a
  week can't be undone. Mitigated by verify-before-delete. Keeping two is one setting away.

## 15. Open questions

- None blocking. (Minecraft: settled, it runs in WSL Ubuntu on Pluto.)

### Resolved

- Pluto stays on Windows (Roblox needs it), runs 24/7, and is the home node.
- Both PCs have a GTX 1080 Ti. Speech-to-text runs on the main PC, TTS (Kokoro) on Pluto's CPU.
- Claude access uses an API key with a $10/month cap. Subscription login isn't allowed for SDK apps.
- iPhone, so web push needs Add to Home Screen.
- Name: **Kernel**, wake word "Hey Kernel", tsundere catgirl persona.
