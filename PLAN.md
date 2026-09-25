# Kernel — implementation plan

This is the build plan for the system described in [ARCHITECTURE.md](ARCHITECTURE.md).
The architecture doc says *what* and *why*. This doc says *how*, *in what order*, and
*how we know each piece is done*.

Conventions: **MUST** means required for the phase to count as done. **Later** means
explicitly deferred. Estimates assume one person working focused, and they're rough.

---

## Contents

1. [Scope of v1](#1-scope-of-v1)
2. [Repository, tooling and CI](#2-repository-tooling-and-ci)
3. [Node daemon](#3-node-daemon)
4. [Kernel protocol](#4-kernel-protocol)
5. [Module system](#5-module-system)
6. [Kernel services (home node)](#6-kernel-services-home-node)
7. [Desktop app](#7-desktop-app)
8. [Phone web app](#8-phone-web-app)
9. [Data model](#9-data-model)
10. [Module specs](#10-module-specs)
11. [Security](#11-security)
12. [Testing](#12-testing)
13. [Packaging, updates and releases](#13-packaging-updates-and-releases)
14. [Phases, tasks and acceptance criteria](#14-phases-tasks-and-acceptance-criteria)
15. [Risks, open questions, decision log](#15-risks-open-questions-decision-log)

---

## 1. Scope of v1

**v1 is done when**, for a week of daily use:

- I can start, stop, restart and back up the Minecraft server, and use its console, from the
  desktop, the palette, the phone, and by asking the assistant.
- I can see every PC's health and sleep, wake, restart or shut them down, with confirmations.
- I get a phone alert within a minute if the Roblox client on Pluto crashes or disconnects,
  and I can relaunch it from the phone.
- The assistant can operate every module, asks before any `confirm` action, and every action
  appears in the activity log with who started it.
- The crash restart, stop-when-empty, weekly backup, disk alert and Roblox alert automations run
  unattended, including while the main PC is off.
- I can generate images on Pluto from chat or a button, and they land in the library on Pluto.
- The wake word works well enough that I leave it on.
- Installing on a fresh machine takes under 10 minutes following the onboarding.

**Out of v1:** multiple users, sharing modules, video/music, a sandbox for the assistant's
web browsing (web access stays `confirm`), code signing, macOS/Linux.

---

## 2. Repository, tooling and CI

### Layout

```
.
├─ ARCHITECTURE.md  PLAN.md  README.md
├─ Cargo.toml                  # Rust workspace
├─ package.json  pnpm-workspace.yaml
├─ apps/
│  ├─ desktop/                 # Tauri 2 app
│  │  ├─ src-tauri/            # Rust: windows, tray, palette hotkey, voice capture, local node client
│  │  └─ src/                  # React UI
│  └─ phone/                   # PWA (Vite + React), built into the home node's static assets
├─ crates/
│  ├─ node/                    # node daemon binary (kerneld.exe)
│  ├─ kernel-services/            # home-node services, linked into kerneld (feature "home")
│  ├─ protocol/                # generated Rust types + hand-written helpers
│  └─ common/                  # logging, paths, keyring, config
├─ packages/
│  ├─ protocol/                # zod schemas = source of truth; emits JSON Schema
│  ├─ assistant/               # Node process: Claude Agent SDK runtime
│  ├─ ui/                      # shared React components (desktop + phone)
│  └─ sdk-ts/                  # TypeScript module SDK
├─ sdk/python/kernel_sdk/         # Python module SDK (published locally as a wheel)
├─ modules/
│  ├─ minecraft/  pc-monitor/  roblox/  ai-media/  files/
├─ tools/
│  ├─ fake-node/               # test double for the desktop app
└─ .github/workflows/
```

### Toolchain (pinned)

| Tool | Version policy | Notes |
|---|---|---|
| Rust | stable, pinned in `rust-toolchain.toml` | `clippy -D warnings`, `rustfmt` |
| Node | 22 LTS, pinned in `.nvmrc` | also the runtime bundled with the assistant |
| pnpm | pinned via `packageManager` | workspaces |
| Python | 3.12, managed with `uv` | modules bundle their own venv |
| TypeScript | strict mode, `noUncheckedIndexedAccess` | Biome for lint and format |

### Code generation

`packages/protocol` (zod) → `pnpm gen:schema` → `schema/*.json` → `cargo run -p protocol-gen`
→ `crates/protocol/src/generated.rs` (via `typify`), plus Python pydantic models via
`datamodel-code-generator`. The generated files are committed, and CI fails if regenerating changes them.

> **Phase 0 status:** the `typify` step is deferred. With 9 message types, the Rust types
> in `crates/protocol` are written by hand, and the contract test (`crates/protocol/tests/fixtures.rs`)
> round-trips every zod fixture through them. A zod change that the Rust side doesn't follow
> fails CI. Switch to generation when the protocol grows past about 20 types (phase 2, with `chat.*`).
> Python modules don't need protocol models yet, because they only talk MCP.

### CI (GitHub Actions)

| Workflow | Runner | Jobs |
|---|---|---|
| `ci.yml` (push/PR) | `windows-latest` | Rust fmt/clippy/test · pnpm lint/typecheck/test · pytest for SDK + modules · codegen drift check · contract tests |
| `ci.yml` | `ubuntu-latest` | the same Rust/TS/Python unit tests (fast feedback, catches Windows-only assumptions) |
| `release.yml` (tag `v*`) | `windows-latest` | build desktop installer, node installer, module bundles, then sign the updater manifest and publish a GitHub Release |

---

## 3. Node daemon

Binary: `kerneld.exe` (Rust, tokio). One per machine. On Pluto it's built with the
`home` feature, which adds the Kernel services.

### Responsibilities

- Pairing and authentication with other nodes and the desktop app.
- Discovering, starting, supervising, updating and removing modules.
- Collecting module status and events and relaying them to subscribers.
- Running actions (checking permissions, recording them in the activity log).
- Running node-local automations (§6.3).
- Writing logs and producing diagnostics bundles.
- A tray icon: status, open logs, restart node, pause all automations.

### Running on Windows

- Runs **per user**, not as a Windows service, because modules need the user's session
  (desktop notifications, the GPU context for Forge, hypervisor CLI tools) and a tray icon.
- Autostart via `HKCU\...\Run`. On Pluto, Windows auto-login plus "never sleep" are documented
  setup steps (the Minecraft module's WSL keepalive only runs while kerneld does).
- One copy per user, enforced with a named mutex.

### Folders

```
%LOCALAPPDATA%\Kernel\node\
  config.toml          # node id, name, listen addrs, allowed folders, flags
  node.db              # SQLite (node-local tables, §9)
  logs\                # rotating, 10 × 10 MB
  modules\<id>\        # installed module code (read-only at runtime)
  data\<id>\           # per-module writable data
  runtimes\python312\  # bundled Python, shared by Python modules
```

The module source-of-truth folder is `D:\Kernel\modules` (configurable). Installing copies
it into `modules\<id>\` and builds a venv, so editing the source never breaks a running module.

### Supervising modules

- Each module is a child process, spawned with a clean environment (only `KERNEL_*`
  variables plus `PATH`, `SystemRoot` and `TEMP`) and its working directory set to `data\<id>\`.
- **Health:** the module must answer an MCP `ping` every 10s. Three misses → kill and restart.
- **Restart backoff:** 1s, 2s, 4s … up to 60s. More than 5 crashes in 5 minutes → state
  `failed` plus an alert, and no more automatic restarts until one is requested from the UI.
- Module stdout/stderr go to `logs\modules\<id>.log`.
- Graceful stop: an MCP `shutdown` notification, 10s grace, then kill.

### Networking

- Listens on `0.0.0.0:47800`. **The app** checks the source address against RFC1918 ranges
  and Tailscale `100.64.0.0/10` and rejects everything else. Belt and braces.
- The installer adds a Windows Firewall rule: TCP 47800, profile **Private** plus the
  Tailscale adapter only.
- TLS with a self-signed certificate per node. Peers pin each other's certificate fingerprint
  at pairing time (§4.2).

---

## 4. Kernel protocol

Everything runs over **one WSS connection** per peer pair (desktop↔node, node↔home node),
using JSON messages validated with the zod-generated schemas. MCP is used *inside* a node
(node↔module over stdio), not on the network.

### 4.1 Envelope

```jsonc
{ "v": 1, "id": "01J…", "type": "action.invoke", "ts": "2026-09-24T20:11:02Z", "body": { … } }
```

- `v`: protocol major version. A mismatch closes the connection with code 4001 and the
  UI shows "update required".
- Requests carry an `id`, and responses carry `re: <id>`.

### 4.2 Pairing

1. The new node's tray shows **Pair…**, which displays a 6-digit code and its certificate fingerprint.
2. In the desktop app, go to **Settings → Nodes → Pair a new node**, and enter the node's address and the code.
3. The desktop app connects over TLS, and both sides confirm the code with SPAKE2 (`spake2`
   crate), so the code is never sent in the clear.
4. After a successful exchange, each side stores the other's certificate fingerprint and a
   long-lived token in Credential Manager (`Kernel/<peer-id>`).
5. The home node is paired first. After that, the home node introduces new nodes to every
   existing client.

Unpairing deletes those records on both sides and closes live connections.

### 4.3 Message types (v1)

| Type | Direction | Purpose |
|---|---|---|
| `hello` / `welcome` | client → node | authenticate, exchange versions and capabilities |
| `catalog.get` → `catalog` | client → node | modules, manifests, actions, views, current status |
| `status.subscribe` / `status.update` | ↔ | streaming status for modules (≤ 4 Hz per module) |
| `event` | node → client | module events (`minecraft.server.crashed`, …) |
| `action.invoke` → `action.result` | client → node | run an action with params, the calling actor, and an optional approval id |
| `action.progress` | node → client | long-running actions (backup at 40%…) |
| `approval.request` / `approval.decide` / `approval.resolved` | ↔ | the approval flow (§6.2) |
| `activity.append` / `activity.query` | ↔ | activity log |
| `automation.*` | ↔ | create, update, enable, run now, list runs |
| `logs.tail` | client → node | live log view for a module or node |
| `diag.bundle` | client → node | produce a diagnostics zip |
| `chat.*` | desktop/phone → home | assistant conversation (§6.1) |
| `voice.*` | desktop → home | voice session streaming (§7.6) |

### 4.4 Errors

`action.result` with `ok:false` and an `error.code` of:

- `offline`
- `disabled`
- `not_permitted`
- `needs_approval`
- `invalid_params`
- `module_failed`
- `timeout`
- `busy`
- `internal`

Every error also carries a human-readable `message`.
The UI maps each code to one line of copy plus a suggested next step.

---

## 5. Module system

### 5.1 Manifest (`module.toml`)

| Field | Required | Meaning |
|---|---|---|
| `id` | yes | `[a-z][a-z0-9-]{1,31}`, unique per node |
| `name`, `icon`, `version` | yes | display name, Lucide icon name, semver |
| `runtime` | yes | `python` or `node` |
| `entry` | yes | entry file |
| `requires` | no | e.g. `platform = "windows"`, `commands = ["VBoxManage"]` |
| `[settings]` | no | typed settings with defaults, rendered as a form. Fields marked `secret = true` go to Credential Manager |
| `[status]` | no | status fields (typed), plus `sidebar` and `tiles` templates |
| `[[actions]]` | no | see below |
| `[[events]]` | no | `id`, `payload` schema, `description` |
| `[[views]]` | no | layout of UI blocks (§5.3) |

**Actions:**

| Field | Meaning |
|---|---|
| `id` | `noun.verb` (`server.start`) |
| `label`, `icon`, `description` | used on the button, in the palette, and as the tool description for the assistant |
| `params` | typed parameters (`int`, `float`, `string`, `enum`, `bool`, `path`) with constraints |
| `ai` | `safe`, `confirm` or `never` |
| `confirm_when` | expression that forces a confirm **for humans too** (e.g. `players > 0`) |
| `enabled_when` | expression over status. When false, the button is disabled, the tool is hidden and the reason is shown |
| `long_running` | enables `action.progress` and a cancel button |
| `timeout_s` | default 60 |
| `quiet` | `true` keeps a read-only, frequently polled action (like `console.tail`) out of the activity log. Only allowed on `safe` actions, so anything that needs a confirm is always logged |

Expressions use a tiny, side-effect-free language (`cel-interpreter` crate: comparisons,
`&&`, `||`, and field access on status only).

### 5.2 Module SDK

**Python**

```python
from kernel_sdk import Module, action, event, status

mod = Module("minecraft")

@mod.status(every=2.0)
async def current():
    s = await rcon.query()
    return {"state": s.state, "players": s.online, "max_players": s.max, "tps": s.tps}

@mod.action("server.start")
async def start(ctx):
    await vm_guest.run_server()
    await ctx.wait_until(lambda st: st["state"] == "running", timeout=180)
    return {"ok": True}

@mod.action("server.stop")
async def stop(ctx, delay_min: int = 0):
    if delay_min:
        await rcon.say(f"Server stopping in {delay_min} min")
        await ctx.sleep(delay_min * 60)
    await rcon.command("stop")

mod.run()
```

**TypeScript** has the same shape (`defineModule`, `action`, `status`) with zod params.

The SDK handles:

- validating the manifest against the code at start-up (missing handlers fail fast)
- MCP transport, pings and shutdown
- structured logging
- settings and secret access (`ctx.settings`, `ctx.secret("rcon_password")`)
- emitting events and progress (`ctx.emit`, `ctx.progress`)

### 5.3 UI blocks (v1 set)

| Block | Binds to | Used by |
|---|---|---|
| `toolbar` | actions (ordered) | all |
| `tiles` | status fields (+ optional meter) | Minecraft, PC monitor |
| `metrics` | per-device groups of meters | PC monitor |
| `console:<source>` | log/stream + command action | Minecraft (screen paste + log) |
| `table:<field>` | array status field, with row actions | players, backups |
| `list:<field>` | array status field | backups, alerts |
| `form:<action>` | action params | "generate image", settings |
| `gallery:<field>` | media items | AI media library |
| `note` | static text (markdown subset) | warnings like "Restore is button-only" |

The desktop app and the phone draw these blocks with the same `packages/ui` components.
There's no module-supplied UI code in v1 (a sandboxed iframe block comes **later**).

### 5.4 Lifecycle

| Stage | What happens |
|---|---|
| install | copy folder, validate manifest, create the venv or `pnpm install --prod`, register in `node.db` |
| enable / disable | start or stop the process. Disabled modules keep their settings |
| update | the source folder's version changed → the UI offers an update → stop, reinstall, start. Rolls back if start-up fails |
| remove | stop, delete code, keep `data\<id>\` unless "delete data" is checked |

---

## 6. Kernel services (home node)

### 6.1 Assistant runtime

- A Node process (`packages/assistant`), supervised by the home node like a module (but not
  a module), talking to the node over a local socket.
- Uses the **Claude Agent SDK** with:
  - **tools:** one in-process MCP server built by the home node from the live catalog. Tool
    name `<module>__<action>` with dots replaced by underscores, the description from the
    manifest, and a JSON Schema input built from `params`.
  - **built-in tools:** Claude Code's file, shell and web tools are **not** enabled in v1.
    The assistant acts only through module tools. Web search/fetch is a `confirm` tool.
  - **permissions:** the SDK's `canUseTool` callback checks the action's tier plus
    `confirm_when`. A `confirm` result → create an approval (§6.2) and wait.
  - **context:** a compact status summary of all online modules, refreshed each turn, sent as a
    system message.
  - **model:** the Settings → Assistant choice. Effort defaults to `medium` for chat.
    Short routine turns ("start the server") go to a cheaper model, and longer conversations
    use the main one.
  - **persona:** a system prompt that makes it **Kernel**, the tsundere catgirl, with its
    text editable in Settings → Assistant. Hard rule in the prompt, and enforced by the UI:
    approval cards, errors and descriptions of what an action will do are generated from the
    manifest and never styled by the persona.
- **Account:** an Anthropic **API key**, entered once during onboarding and stored on the home
  node. Per the Agent SDK docs, apps built on the SDK can't use claude.ai subscription login
  or rate limits without Anthropic's approval, so this isn't optional.
- A local brain (Ollama) is an optional **later** backend behind the same tool list, labeled
  experimental.
- **Conversations:** the SDK owns transcripts. `chats` (§9) stores id, title, timestamps
  and the SDK session id for resume.
- **Save as automation:** takes the chat's successful tool calls as steps, has the model
  suggest which values should become inputs, and opens the automation editor pre-filled.

### 6.2 Approvals

State machine: `pending → approved | denied | expired | cancelled`.

- Created by the assistant, an automation step marked `confirm`, or a human action hitting
  `confirm_when`.
- Fanned out to: the desktop (inline in chat, plus a toast if the chat isn't open), the phone
  (top of the home screen), and a Windows notification with Approve/Deny buttons.
- The first decision wins. Expires after 10 minutes (setting) → treated as denied.
- Every step is recorded in `approvals` and the activity log.

### 6.3 Automation engine

**Definition format** (stored as JSON in `automations`, edited in the UI):

```jsonc
{
  "name": "Stop when empty",
  "trigger": { "type": "condition", "module": "minecraft", "when": "players == 0", "for": "15m" },
  "steps": [
    { "type": "action", "module": "minecraft", "action": "server.stop", "params": {} }
  ],
  "policy": { "concurrency": "skip", "missed": "skip", "retries": 0 }
}
```

- **Triggers:**
  - `schedule` (cron plus time zone)
  - `event` (module event id plus an optional filter)
  - `condition` (expression over status, plus an optional `for` duration)
  - `voice` (phrase, home node only)
- **Steps:**
  - `action`
  - `wait` (duration, or `until` an expression with a timeout)
  - `notify`
  - `assistant` (a prompt, with its output available to later steps)
  - `if` (expression, then/else)
- **Where it runs:** the home node computes this when the automation is saved. If every
  trigger and step touches one node's modules (and there's no `assistant` step), it's
  deployed to that node. Otherwise it stays on the home node. The UI shows "Runs on …".
- **Policies:**
  - `concurrency`: skip, queue or parallel
  - `missed` (for schedules when the node was off): skip or run once
  - `retries` with backoff
- Every run is written to `automation_runs`, with its step results and durations.

### 6.4 Claude API proxy

- A local HTTP server on `127.0.0.1` only, which the assistant process uses as `ANTHROPIC_BASE_URL`.
- The assistant gets a random bearer token per process start. The proxy swaps it for the
  real key from Credential Manager.
- Streams responses through unchanged, so prompt caching works.
- Records `usage` (input, output, cache read/write tokens) per request into `usage`, with
  the cost from a price table that ships with the app and can be edited.
- **Budget:** a monthly cap from settings (default **$10**). At 80% → notification. At 100% → requests get a
  402, the assistant tells the user, and automations with `assistant` steps pause.
- **Concurrency:** 4 requests in flight. Retries 429 and 529 errors with jittered backoff,
  respecting `retry-after`.

### 6.5 Notifications

- Channels: desktop toast (through the desktop app, or through the node when the app is
  closed), the phone (web push via the PWA), and the activity feed.
- Per-category settings: approvals, alerts, automation failures, automation successes (off by default).

---

## 7. Desktop app

### 7.1 Windows and navigation

| Window | Notes |
|---|---|
| Main | a frameless custom title bar that keeps the Windows snap layouts. Sidebar + content |
| Palette | always-on-top, Alt+Space global hotkey, closes when it loses focus |
| Settings | a separate small window (960×640) |
| Toasts | native Windows notifications (`tauri-plugin-notification`) |

**Screens** (matching the design canvas):

- Assistant
- Automations (list + detail + editor)
- a Module screen per module (built from the manifest's views)
- PC monitor
- Activity log
- Settings (General, Assistant, Modules, Nodes, Permissions, Shortcuts)
- Onboarding

### 7.2 State and data

- **Rust side:** keeps connections to the home node and the local node, merges their catalogs,
  and exposes typed Tauri commands (`invoke`, `approve`, `subscribe`) plus events
  (`status`, `activity`, `approval`).
- **UI side:**
  - TanStack Query for request/response data
  - a small Zustand store for live status, fed by Tauri events
  - TanStack Router for screens (flat route list, no nesting)
- **Degraded mode:** if the home node is unreachable, the app connects directly to the other
  paired nodes. The assistant, automations editing and the library show "Home node offline",
  while module buttons keep working.

### 7.3 Design system

The canvas design is implemented as tokens:

- colors: `--bg`, `--side`, `--raise`, `--line`, text-1/2/3, accent, add, del, warn
- square corners everywhere
- Geist and Geist Mono
- Lucide icons at 1.5px stroke with square caps and miter joins

Components live in `packages/ui`:

- `Sidebar`, `Row`, `Kbd`, `Btn`, `ActionBar`, `Panel`, `Tile`, `Meter`, `Console`, `Table`,
  `Approval`, `Toggle`, `Picker`, `Composer`, `Feed`
- plus the §5.3 blocks built from them

Accessibility requirements: everything reachable by keyboard, visible focus rings, contrast
≥ 4.5:1 for text, and `aria-label` on icon-only buttons.

### 7.4 Keyboard

| Keys | Action |
|---|---|
| Alt+Space | palette (global) |
| Ctrl+K | palette (in app) |
| Ctrl+Space (hold) | push-to-talk |
| Ctrl+Enter / Esc | approve / deny the focused approval |
| Ctrl+L | activity log |
| Ctrl+, | settings |
| Ctrl+1…9 | jump to sidebar item |

Module actions can declare a suggested shortcut. Conflicts are resolved in Settings → Shortcuts.

### 7.5 Tray

- Menu: Open Kernel, Palette, Mic on/off, Pause automations, and Quit.
- Closing the main window hides it to the tray.

### 7.6 Voice

**Pipeline:**

```
mic (WASAPI, 16 kHz mono) → Silero VAD → openWakeWord ("Hey Kernel")
  → on trigger: faster-whisper on the MAIN PC's 1080 Ti (int8, `small.en`)
  → text sent to the home node as a chat turn
  → assistant reply → Kokoro TTS (Pluto CPU) → audio frames back → play
```

- **Latency budget** (from end of speech to first audio), ≤ 2.0 s:
  - VAD end-of-speech: 300 ms
  - STT: 500 ms
  - first assistant token: 700 ms
  - TTS first chunk: 300 ms
  - network: 200 ms
- **Barge-in:** if speech is detected while TTS is playing, stop playback and start a new turn.
- **Privacy:** before the wake word, audio never leaves the process, and a tray indicator
  shows the mic state. A setting lets you require push-to-talk only.
- **Fallback:** if Pluto is unreachable, voice is disabled and the tray shows why.
- **Wake word model:** "Hey Kernel" isn't a stock openWakeWord phrase, so it's trained with
  openWakeWord's synthetic-speech training notebook (a one-off, about an hour on the 1080 Ti),
  then tuned with ~50 real recordings of my voice plus an hour of background audio (Roblox
  and game sound included) to cut false triggers. The model file ships with the desktop app.
- **Running STT locally** on the main PC keeps the round trip off the network and leaves
  Pluto's GPU for Forge. Pluto's `speech.transcribe` is still available for other uses (e.g.
  transcribing files).

### 7.7 Onboarding (first run)

1. Welcome, and choose "This is my main PC".
2. Pair the home node (Pluto): the code-entry screen.
3. Enter the Anthropic API key. It's stored on the home node, never on the main PC.
4. Pair the other nodes, or skip.
5. Enable modules. Per-module setup forms appear (Minecraft shows its WSL settings with Pluto's defaults filled in).
6. Permissions: review the defaults.
7. Voice: mic test, wake word test, or skip.

---

## 8. Phone web app

- A Vite + React PWA sharing `packages/ui`, served by the home node at `/m/`.
- Reached through **Tailscale Serve** (HTTPS with a real certificate on the tailnet domain).
  Not reachable any other way.
- **Device login:** Desktop → Settings → Nodes → "Add phone" shows a QR code with a one-time
  code. The phone exchanges it for a device token, stored in the PWA's storage. Device tokens
  can be revoked in Settings.
- **Screens:**
  - Home: pending approvals, module cards with big action buttons, node status, recent activity
  - a Module detail screen built from the same views, with the console and tables simplified for small screens
  - Assistant chat
  - Activity
- Web push notifications (VAPID) for approvals and alerts.
- **iPhone:** web push only works once the PWA has been added to the Home Screen (iOS 16.4+),
  and push permission can only be requested after a tap. The device-login flow ends with an
  "Add to Home Screen, then tap Enable notifications" step with screenshots, and the home
  screen shows a banner until notifications are on.
- Touch targets ≥ 44 px, and it works in portrait at 360 px width.

---

## 9. Data model

### Home node (`kernel.db`)

| Table | Key columns |
|---|---|
| `nodes` | id, name, role, address, cert_fp, last_seen, version |
| `modules` | node_id, module_id, version, enabled, state, manifest_json |
| `activity` | id, ts, actor (`user`/`assistant`/`automation`/`phone`), actor_ref, node_id, module_id, action, params_json, result (`ok`/`error`/`denied`), error_code, duration_ms, approval_id |
| `approvals` | id, created_ts, requested_by, node_id, module_id, action, params_json, reason, state, decided_by, decided_ts |
| `automations` | id, name, definition_json, placement_node, enabled, created_from_chat |
| `automation_runs` | id, automation_id, node_id, started_ts, finished_ts, status, steps_json |
| `chats` | id, title, created_ts, updated_ts, sdk_session_id, model |
| `usage` | ts, request_id, model, in_tokens, out_tokens, cache_read, cache_write, cost_usd, chat_id |
| `library_items` | id, kind (`image`/`audio`), path, prompt, params_json, model, seed, source (chat/automation/button), created_ts, tags |
| `alerts` | id, ts, node_id, module_id, severity, text, resolved_ts |
| `devices` | id, name, token_hash, created_ts, last_seen (phones) |
| `settings` | key, value_json |

`library_items` also has an FTS5 index over the prompt and tags.

### Every node (`node.db`)

`module_settings`, `local_automations` (deployed copies), `local_runs` (synced up to the home
node when it's reachable), `status_cache`.

### Migrations

Uses `refinery` (Rust) with numbered SQL files, run on start-up. The database is backed up
to `*.db.bak` before each migration.

### Retention

- `activity`: 180 days
- `usage`: kept forever (it's small)
- `automation_runs`: 90 days
- logs: rotation limits

---

## 10. Module specs

### 10.1 Minecraft (`modules/minecraft`, runs on Pluto)

**The real setup** (settled, see D16): NeoForge 21.1.251 / MC 1.21.1 with about 30 mods in
`/srv/minecraft`, inside **WSL Ubuntu on Pluto**, run by systemd as the `minecraft` user
inside a detached `screen` session (`screen -DmS mc`, `SCREENDIR=/run/screen-mc`). My own helpers
(`mc-cmd`, `mc-ping`, `mc-console`) stay for manual use; the module doesn't call them (below). Other units: `playit` (the playit.gg tunnel that
is the only public way in, so no port forward and no exposed home IP) and `mc-notify` (Discord).
The old Forge 1.20.1 install stays in `/srv/minecraft-1.20.1` as a backup.

**How the module reaches it:** kerneld on Pluto runs the module on Windows, and the module pipes
small bash scripts (`modules/minecraft/scripts/*.sh`) to `wsl.exe -d Ubuntu -u root -- bash -s`
over stdin. Values go in as shell-quoted variables, never through cmd.exe quoting. There's no RCON
and no node inside Linux. Transports `ssh` (from another PC, `ssh pluto "wsl ... bash -s"`) and
`direct` (already on Linux, used by tests) exist too.

**WSL keepalive:** WSL shuts its VM down when no session is attached, which kills the server
(`vmIdleTimeout` doesn't help). The module holds `wsl -d Ubuntu -u root -- sleep infinity` open for
as long as it runs and restarts it if it ends. Status shows `keepalive`. Until kerneld runs as a
service at boot, keep `MC-KEEPALIVE.bat` as well.

**Settings** (`module.toml` defaults, per-machine `settings.local.toml`): `transport`, `distro`,
`ssh_host`, `server_dir`, `service`, `extra_services`, `backup_dir` (`/srv/minecraft-backups`),
`keep_backups` (1), `keep_wsl_alive`, `refresh_s`, `start_timeout_s`.

**Status** (refreshed every `refresh_s` by one script run, cached between polls):
`state` (`running`/`starting`/`stopping`/`stopped`/`crashed`/`unknown`; "running" means systemd
says active **and** the server answers pings), `players[]` (the ping's sample), `players_online`,
`players_max`, `version`, `motd`, `uptime_s`, `memory_mb` (the unit's cgroup), `mods_count`,
`services` (playit, mc-notify), `backups[]`, `last_backup`, `keepalive`. TPS is left out for now:
asking for it writes to the console every poll.

**Actions:**

| Action | Tier | Notes |
|---|---|---|
| `server.start` | safe | `systemctl start`, then waits until pings answer (`start_timeout_s`) |
| `server.stop` | confirm | optional `delay_min` with a chat warning each minute, then `systemctl stop` |
| `server.restart` | confirm | `systemctl restart`, then waits for pings |
| `server.command` | confirm | one console line, pasted into the screen session; returns what the server logged after it |
| `server.say` | safe | chat broadcast |
| `console.tail` | safe | last N lines of `logs/latest.log` |
| `player.kick`, `player.op`, `player.deop` | confirm | names checked against `[A-Za-z0-9_]{3,16}` |
| `whitelist.add` / `whitelist.remove` / `whitelist.list` | confirm / confirm / safe | |
| `world.backup` | safe | `save-off` → `save-all flush` → waits for "Saved the game" → `tar.gz` of the world → `save-on` (always, via a trap) → **verify** (the archive reads and `level.dat` is valid gzip) → only then delete backups beyond `keep_backups`. Checks free space first. A failed verify keeps the old backup |
| `mods.list` | safe | jars in `mods/` |
| `service.restart` | confirm | `playit` or `mc-notify` |

Start, stop, restart and backup never overlap (the second one gets `busy`). Still to come:
`world.restore` (never, for the assistant), `confirm_when players > 0`, events (`server.crashed`,
`player.joined`, `backup.failed`, ...) and the weekly backup automation (phase 3; until then run
it by hand or from a timer).

**Console input:** the module pastes text into the screen session with `readbuf` + `paste`,
never `screen -X stuff` (which `mc-cmd` uses). `stuff` interprets `^M`, `\015` and `$VARS` in its
argument, so `say hi^Mop someone` would run **two** commands, and `server.say` is `safe` for the
assistant. Checked against real screen 4.9 (`tests/test_real_screen.py`). **Status** comes from
the module's own server list ping on localhost (a few lines of Python in `scripts/lib.sh`), so it
doesn't depend on `mc-ping`'s output format. Each script is wrapped in `{ }` with stdin detached,
because it arrives on bash's stdin.

**Testing:** `tests/test_scripts.py` runs the real scripts against a fake server folder, fake
`systemctl`, `screen` and `runuser`, and a TCP server that answers the status ping. `probe.sh` is a read-only check of the real server's
assumptions. From PowerShell, in the repo root:
`cmd /c 'ssh pluto "wsl -d Ubuntu -u root -- bash -s" < modules\minecraft\probe.sh'`
(PowerShell has no `<`, and piping with `Get-Content` can add CRLFs that break bash).

### 10.2 VM power (dropped)

Not needed: the server runs in WSL on Pluto, and the Minecraft module keeps WSL alive itself (§10.1).

### 10.3 PC monitor (`modules/pc-monitor`, every node)

- **Status:**
  - CPU % (and per core)
  - RAM used/total
  - disks used/total
  - GPU % / VRAM / temperature via NVML (`pynvml`; works on the 1080 Ti)
  - CPU temperature via LibreHardwareMonitor's WMI provider when installed (optional, otherwise hidden)
  - network throughput
  - uptime
- **Actions:**
  - `power.sleep` (confirm)
  - `power.restart` (confirm, 60s countdown, cancellable)
  - `power.shutdown` (confirm, same)
  - `power.wake` (safe, sends a Wake-on-LAN magic packet to a *peer's* MAC address)
- **Wake-on-LAN** needs "Wake on Magic Packet" enabled in Pluto's NIC settings and BIOS, and
  Windows fast startup turned off. This is a documented setup step, and the module has a
  "Test WoL" button.
- **Events:** `threshold.crossed` (disk > X%, GPU temp > Y°C, both configurable).

### 10.4 AI media (`modules/ai-media`, runs on Pluto)

- **Forge** (A1111-compatible API, launched with `--api`, optionally managed by the module):
  - `image.generate` → `/sdapi/v1/txt2img`
  - `image.edit` → `/sdapi/v1/img2img` (including inpainting masks)
  - `image.upscale` → `/sdapi/v1/extra-single-image`
  - model and LoRA lists → `/sdapi/v1/sd-models`, `/sdapi/v1/loras`
  - progress → `/sdapi/v1/progress`
- **TTS:** `speech.say` with Kokoro (CPU) by default, Piper as a light fallback, and XTTS
  (GPU) optional for voice cloning.
- **STT:** `speech.transcribe` with faster-whisper `small` or `medium` (int8 on the GPU,
  falling back to the CPU). It's also used by the voice pipeline.
- **GPU queue:** one GPU job at a time. Priority: voice > interactive image > automations.
  The queue shows in status and on the module screen.
- **Library:** every output is written to `D:\Kernel\Library\YYYY\MM\` with a JSON sidecar and
  inserted into `library_items`. Thumbnails are generated at 256 px.
- **Pinned versions:** a PyTorch and CUDA combination that supports compute capability 6.1
  (Pascal). It's recorded in `modules/ai-media/requirements.lock` and checked at start-up.

### 10.5 Roblox (`modules/roblox`, runs on Pluto)

Watches the Roblox client I leave AFK on Pluto. **Watching and relaunching only:** it never
sends input to the game or automates gameplay.

- **Settings:** `place_id` (optional, for rejoin), `alert_on_disconnect` (on), `auto_relaunch` (off).
- **Status:**
  - `state` (`closed`/`running`/`disconnected`)
  - `session_s`
  - `memory_mb`
  - `gpu_mem_mb`
  - `last_event` (text + time)
  - `place_name` if known
- **How it knows:**
  - the process list for `RobloxPlayerBeta.exe` (running, crashed, exited)
  - tailing the newest file in `%LOCALAPPDATA%\Roblox\logs\` for disconnect/kick lines
    (the patterns are kept in a config file, since Roblox changes its log format)
  - Roblox's own error window when present
- **Actions:**
  - `client.relaunch` (confirm): close the client if it's hung, then start it again
  - `client.rejoin` (confirm, needs `place_id`): open `roblox://experiences/start?placeId=<id>`
  - `client.close` (confirm)
- **Events:** `client.crashed`, `client.disconnected`, `client.closed`.
- **GPU note:** Roblox's VRAM use is reported to the AI media GPU queue, so Forge jobs size
  their batches to what's actually free.

### 10.6 Files (`modules/files`, runs on Pluto and the main PC)

- **Allowed roots** come from settings, and every path is resolved and checked against the
  roots, rejecting symlink escapes and `..`.
- **Actions:**
  - `files.list`, `files.read` (up to 10 MB), `files.search` (all safe)
  - `files.write`, `files.move`, `files.delete` (confirm)
  - `files.transfer` (between nodes, streamed in chunks, safe when the destination is empty)
  - `backup.folder` (copies a folder to a target with versioned snapshots, safe)
- **Backups:** the "second copy" job from ARCHITECTURE §8 is a `backup.folder` automation,
  off by default, and the UI nags until it's configured.

---

## 11. Security

**Threat model:**

| Threat | Mitigation |
|---|---|
| Something on the LAN or internet talks to a node | Firewall rule for Private/Tailscale only · source address check in the app · TLS with pinned certificates · per-peer tokens |
| A stolen phone | Revocable device tokens · `confirm` approvals still need an unlocked phone · approvals expire |
| Prompt injection (web pages, chat messages, player names in logs) | The assistant only has module tools · anything from the web can't trigger a `confirm` action without a human · `never` tier · status text is marked as data in prompts |
| The assistant reading secrets | The API key only exists in the proxy · module secrets only in that module's process · the assistant process gets a scrubbed environment |
| A buggy or rogue module | Its own process with a scrubbed environment and its own data folder · no network ports opened by modules (they reach out, nothing reaches in) · modules are local, trusted code (no marketplace) |
| A compromised Minecraft server (a bad mod, say) | It runs in WSL as the `minecraft` user, and the only public way in is the playit.gg tunnel. **WSL is weaker isolation than a real VM:** by default Linux can read Windows drives (`/mnt/c`) and start Windows programs. Recommended hardening in `/etc/wsl.conf`: `[automount] enabled=false` and `[interop] enabled=false` (Kernel doesn't need either; it calls *into* WSL, not out). Nothing in WSL holds a Kernel token |
| Mistaken destructive actions | `confirm_when` also applies to humans · restore is `never` · the activity log records who did what |

**Secrets inventory:** Anthropic key (home node), peer tokens (each node), device tokens
(stored hashed on the home node), and the Discord webhook, which stays where it is (`/etc/mc-notify/webhook`, root only).
Kernel's own secrets live in Credential Manager.

---

## 12. Testing

| Level | What | Tools |
|---|---|---|
| Unit | Rust crates, SDKs, modules (Minecraft scripts against fakes, backup logic, path checks, expression evaluation) | `cargo test`, `vitest`, `pytest` |
| Contract | zod ⇄ Rust ⇄ pydantic round-trip of every message type, using golden JSON fixtures | CI job |
| Module harness | `kernel-sdk test` runs a module against a fake node, asserts manifest ↔ handlers, and runs recorded scenarios | SDK |
| Minecraft | the real bash scripts against a fake server folder with fake `systemctl`/`screen`/`runuser` and a fake ping server, covering status, start, stop, commands and backup paths (`modules/minecraft/tests`) | pytest |
| Integration | A real node plus fake modules plus a test desktop client: pairing, approvals, automations placement, offline/reconnect | Rust integration tests |
| UI | component tests plus key flows (approve in chat, Minecraft start/stop, sleep confirm) against `tools/fake-node` | Playwright (web build of the UI) |
| Assistant | recorded-response tests for tool selection and approval behavior; a small eval set of 30 requests ("start the server", "is Pluto hot?") with expected tool calls | vitest + recorded responses |
| Manual | a release checklist on real hardware: WoL, sleep/wake, WSL keepalive, voice latency, fresh install | `docs/release-checklist.md` |

Coverage targets: 80% lines for `crates/node`, the SDKs, and the `minecraft` and `files`
modules. None for UI code, where behavior tests matter more.

---

## 13. Packaging, updates and releases

| Artifact | Format | Contents |
|---|---|---|
| Desktop app | Tauri NSIS installer, per-user | app + WebView2 bootstrapper + the local node |
| Node | NSIS installer, per-user | `kerneld.exe` + the bundled Python runtime + the firewall rule. On Pluto it also includes the assistant (bundled Node 22 + `packages/assistant`) and the phone PWA |
| Linux VM node | tarball + install script | `kerneld` (musl build) + a systemd user unit |
| Modules | folder zips attached to the release | copied into `D:\Kernel\modules` |

- **Updates:** the Tauri updater for the desktop app, and the node checks the same GitHub
  Releases feed. Both verify the updater signature (the key is kept offline). Nodes update
  one at a time, home node last. Protocol changes stay backward compatible within a major version.
- **Versioning:** one version for the whole repo (`vX.Y.Z`) and a changelog generated from
  Conventional Commits.
- **Unsigned for now:** document the SmartScreen "More info → Run anyway" step, and move to
  Azure Trusted Signing before sharing any further.

---

## 14. Phases, tasks and acceptance criteria

### Phase 0: test run (1–2 weeks)

- [x] Repo scaffold per §2 (Cargo + pnpm workspaces, Biome, uv), CI on Windows and Linux runners
- [x] Tauri window with the design tokens, sidebar, and a palette window on Alt+Space
  (`apps/desktop`). Module screens are generated from the catalog: action buttons (inputs go in
  a More menu, anything above `safe` needs a second click), status tiles, lists and tables, and
  a live console for modules with `console.tail` + `server.command`. Activity and Settings
  screens. The installer is a CI artifact. Node address and token live in local storage until
  pairing
- [~] `kerneld` skeleton: config, logs, WebSocket listener with token auth and a LAN/tailnet
  address filter, module supervisor, activity log in SQLite. **Still to do:** tray, TLS,
  SPAKE2 pairing between two machines over Tailscale
- [x] Python SDK "hello" module: status fields, four actions (one per tier, plus a timeout
  case), supervised with restart and backoff, covered by `crates/node/tests/e2e.rs`
- [ ] **Agent SDK test:** a bundled Node 22 runs `packages/assistant` on Windows, calls one module tool through the proxy, and the key isn't visible from inside the assistant process (checked by test)
- [ ] Decision recorded: the Tauri + Node process split holds (or switch to Electron)

**Accept when:** clicking a button in the desktop app on the main PC runs the hello action
on Pluto, the result appears in the activity log, and the assistant can call it from a
command-line harness.

### Phase 1: buttons (3–4 weeks)

- [ ] Protocol v1 message types from §4.3 except `chat.*`, `voice.*` and `automation.*`
- [ ] Module manifest parser and validator, UI blocks `toolbar`, `tiles`, `metrics`, `console`, `table`, `list`, `note`
- [~] **Minecraft** module (§10.1): actions, status, WSL keepalive and verified backups done and tested against fakes. **Still to do:** a run against the real server (`probe.sh` first), events, `confirm_when`
- [x] ~~**VM power** module~~ dropped: the server is in WSL on Pluto (D16)
- [ ] **PC monitor** module on all three nodes, including WoL and power actions with confirms
- [ ] **Roblox** module on Pluto (§10.5): status, crash/disconnect detection, relaunch
- [ ] Activity log screen, and a basic `approvals` flow for human `confirm_when`
- [ ] Degraded mode when the home node is offline
- [ ] Onboarding steps 1, 2, 4, 5

**Accept when:** for 3 days, the Minecraft server and the PCs are managed only through Kernel
buttons, with no crashes that lose state, and Sleep on Pluto → Wake from the main PC works.

### Phase 2: assistant (3 weeks)

- [ ] `packages/assistant` with the Agent SDK, the tool catalog built from manifests, a status summary in context
- [ ] API proxy with usage metering, budget cap and concurrency limit
- [ ] Full approval flow (§6.2) in chat, toasts and notifications
- [ ] Assistant screen as designed: chat, tool-call rows, the approval card, and the live and activity panels
- [ ] Palette "Ask the assistant" mode
- [ ] Settings → Assistant and Permissions tabs
- [ ] 30-request assistant eval set passing ≥ 90% on the right tool and arguments

**Accept when:** "start the server and tell me when it's up" and "stop it in an hour and
back up after" work end to end, with approval, the activity log shows who did what, and
the monthly cost is visible.

### Phase 3: automations + phone (3 weeks)

- [ ] Automation engine (§6.3) with node placement, the five starter automations (Minecraft crash restart, stop when empty, weekly backup, disk alert, Roblox alert), and a runs history
- [ ] Automations screen (list, detail, simple step editor) and "save chat as automation"
- [ ] Node-local execution and syncing runs back to the home node
- [ ] Phone PWA (§8) with device login, home, module screens, approvals, chat and web push
- [ ] Notifications settings

**Accept when:** with the main PC **off**, a forced Minecraft crash is restarted and I get
a phone notification, the weekly backup runs and replaces last week's, a Roblox disconnect
reaches my iPhone, and I can approve a stop from the phone.

### Phase 4: AI media + storage (3–4 weeks)

- [ ] **Files** module on Pluto and the main PC, including transfers and folder backup
- [ ] Minecraft backups moved to Pluto through Files, with `world.restore` enabled
- [ ] **AI media** module: Forge generate/edit/upscale, the GPU queue, Kokoro TTS, faster-whisper STT
- [ ] Library screen (gallery, detail, search, "more like this"), stored on Pluto with a main-PC thumbnail cache
- [ ] Assistant returns images inline in chat
- [ ] Second-copy backup automation, and nagging until it's configured

**Accept when:** "make a 16:9 wallpaper of a snowy pixel-art village" produces an image
in chat and in the library on Pluto, and a world restore from a Pluto backup works.

### Phase 5: voice + polish (3 weeks)

- [ ] Voice pipeline (§7.6): push-to-talk first, then the wake word, TTS replies, barge-in
- [ ] Train and tune the "Hey Kernel" wake word model. Target: under 1 false trigger per day with Roblox audio playing
- [ ] Latency measured and logged. p50 ≤ 2.0 s, p90 ≤ 3.0 s on the home network
- [ ] Onboarding step 7, a mic indicator in the tray and sidebar
- [ ] Installers (§13), the updater, export diagnostics, release checklist
- [ ] Optional local brain (Ollama) behind a setting, labeled experimental
- [ ] Accessibility pass (keyboard-only walkthrough of every screen)

**Accept when:** the v1 definition in §1 holds for a full week.

### After v1

Candidates, not commitments:

- a sandboxed module iframe block
- a Discord bridge module
- Home Assistant module
- a WSL2 sandbox for browsing tasks
- a low-power always-on box (Raspberry Pi) as a fallback home node and WoL relay
- code signing

---

## 15. Risks, open questions, decision log

### Risks

| Risk | Likelihood | Impact | Mitigation |
|---|---|---|---|
| Agent SDK packaging on Windows | medium | high | Phase 0 test. Fallback: Electron, or calling the Claude API directly with our own tool loop |
| Pluto restarting for Windows Update breaks the home node and the Roblox session | medium | medium | Pluto runs 24/7 (D13). Set active hours, pause auto-restart, and schedule update restarts for when I'm not AFK. Degraded mode on the main PC |
| 1080 Ti support dropped by PyTorch/CUDA | medium | medium | Pinned lockfile. CPU fallback for TTS and STT |
| WSL shuts down with no session attached, killing the server | high | high | The Minecraft module holds a session open. Keep `MC-KEEPALIVE.bat` until kerneld starts at boot |
| Voice false triggers or latency | medium | medium | Push-to-talk first, tunable wake threshold, measured latency |
| Scope creep | high | high | Phases gated on acceptance criteria. "After v1" list for new ideas |

### Open questions

Resolved: Minecraft runs in WSL Ubuntu on Pluto under systemd (D16) · Pluto runs 24/7 (D13) · TTS voice is Kokoro (D14) · backups (D15) · main PC GPU is a 1080 Ti (D14).

### Decision log

| # | Decision | Why |
|---|---|---|
| D1 | Windows-first, single user, unsigned | the audience is me |
| D2 | Tauri 2 + Rust node + Node assistant process | small footprint and Rust experience. Agent SDK is Node-only. Revisit after phase 0 |
| D3 | Pluto is the home node | needs to be always on for the phone, automations and the assistant |
| D4 | Modules are processes speaking MCP + a manifest | isolation, any language (Python/TS), assistant tools for free |
| D5 | Buttons = palette = phone = AI tools = automation steps (one action path) | "use it without AI", consistency, one permission check |
| D6 | Three AI permission tiers + `confirm_when` for humans too | safety without nagging |
| D7 | Assistant has module tools only in v1 (no shell/file/web built-ins) | limits the damage from prompt injection |
| D8 | UI blocks, no module UI code, in v1 | consistent look, no untrusted UI code |
| D9 | Automations run on the owning node when possible | keep working when the main PC or Pluto are off |
| D10 | Square, Raycast-style design, Geist, Lucide icons | per the design canvas |
| D11 | Name **Kernel**, wake word "Hey Kernel", tsundere catgirl persona | my choice. Persona never styles safety text |
| D12 | Anthropic API key, $10/month default cap | the Agent SDK can't use subscription login without approval |
| D13 | Pluto stays Windows and runs 24/7 | Roblox AFK needs Windows. Being on 24/7 is exactly what the home node needs |
| D14 | STT on the main PC's 1080 Ti, TTS (Kokoro) on Pluto's CPU | lowest voice latency, and keeps Pluto's GPU free for Forge |
| D15 | Weekly Minecraft backup, keep 1, verify before deleting the old one | my choice. Verification removes the "zero good backups" window |
| D16 | Minecraft module drives WSL on Pluto through `wsl.exe` + bash scripts; no VM node, no RCON, no VM power module | that's how the server already runs (systemd + screen, playit.gg). One fewer node and secret |
