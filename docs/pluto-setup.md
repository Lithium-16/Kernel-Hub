# Setting up Pluto as the home node

About 10 minutes, once. After that, updates are a button in the app.

## What you need

- A merge to `main` since the node bundle existed, so CI has published a `node-build-N` release
  (check the repo's **Releases** page).
- A **read-only GitHub token**, only while the repo is private (public: press Enter when asked):
  <https://github.com/settings/personal-access-tokens/new>
  - Repository access: **only** `B0RE16/laughing-waffle`
  - Permissions: **Contents: Read-only**. Nothing else.
  - The installer asks for it and saves it in Pluto's `node.toml`, where the node uses it to
    download updates.
- Pluto logged in (it already is, for the Roblox AFK). The node runs in your user session so it
  can use WSL.

## Install

From your PC, in your clone of the repo (PowerShell):

```powershell
git pull
scp scripts\install-node.ps1 pluto:
ssh -t pluto powershell -ExecutionPolicy Bypass -File install-node.ps1
```

It asks for the token, then:

1. downloads the newest build and checks its SHA-256
2. installs it to `%LOCALAPPDATA%\Kernel\node\app`
3. makes a Python environment for modules with [uv](https://docs.astral.sh/uv/) (installs uv if needed)
4. writes `%LOCALAPPDATA%\Kernel\node\node.toml` with a new **node token**
5. registers the **Kernel node** scheduled task: starts at logon, and restarts kerneld within
   5 minutes if it ever stops
6. opens port 47800 in Windows Firewall for Tailscale only (not your home network)
7. starts the node and prints the address and token

## Connect the app

Install the desktop app (the `kernel-desktop-windows` artifact from a CI run), open **Settings**,
and enter the address and token the installer printed. The sidebar shows **Minecraft** and **Node**.

## Discord alerts

Kernel can post to a Discord channel when something happens: the Minecraft server crashes or
stops on its own, a backup fails, Roblox disconnects or closes, the GPU runs hot, a disk fills
up, a new build is out. In Discord: channel settings > **Integrations** > **Webhooks** >
**New Webhook** > **Copy Webhook URL**. Then add to `%LOCALAPPDATA%\Kernel\node\node.toml`:

```toml
[notify]
discord_webhook = "https://discord.com/api/webhooks/..."
min_level = "warn"              # info, warn or error
include = ["player.joined"]     # also send these, whatever their level ("minecraft.*" works too)
mute = []                       # never send these
```

Restart the node. Every event, sent or not, is in the app under **Activity > Events**.

## Scheduled actions

Also in node.toml, one `[[schedule]]` per job, in Pluto's local time:

```toml
[[schedule]]
name = "Nightly backup"
at = "daily 04:00"              # "sun 03:30", "mon,wed,fri 18:00", "weekdays 07:00", "every 30m"
module = "minecraft"
action = "world.backup"

[[schedule]]
name = "Weekly restart"
at = "mon 05:00"
module = "minecraft"
action = "server.stop"
params = { delay_min = 5 }
approved = true                 # needed for actions that normally ask first
```

Runs missed while Pluto was off are skipped. The **Node** page lists each schedule with its next
and last run, and a failed run is an event (so it can reach Discord).

## Automations

Also in node.toml: run an action when something happens, or when a status holds for a while.

```toml
[[on_event]]
name = "Free VRAM after a batch"
event = "comfyui.queue.finished"    # an event kind; "roblox.*" matches all of Roblox's
module = "vram"
action = "vram.free_idle"

[[when]]
name = "Stop an empty server"
status = "minecraft"                 # whose status to check
condition = "players_online == 0"    # ==, !=, <, <=, >, >= against a status value
for_min = 30
module = "minecraft"
action = "server.stop"
approved = true
```

A `when` automation runs once each time its condition starts holding, not over and over. The
Node page lists every automation with its last run and result; a failure is an event.

## Module settings

Easiest: open the module in the app and press **Settings**. It shows every setting with its
note, saves to this PC, and restarts the module. Tokens are never shown, and settings that start a
program (`start_command`) can only be changed on the PC itself.

By hand: per-machine settings go in `%LOCALAPPDATA%\Kernel\node\data\settings\<module>.toml`
(they survive updates). The defaults are in each module's `module.toml`. Restart the node (Node >
Restart node) after changing them.

**PC monitor, Wake-on-LAN.** On the PC that should *send* the wake-up (Pluto, to wake the main PC,
or the other way round), `data\settings\pc-monitor.toml`:

```toml
wake_targets = ["main-pc=AA:BB:CC:DD:EE:FF"]   # the sleeping PC's MAC: `getmac /v` on that PC
wake_broadcast = "192.168.1.255"               # your LAN's broadcast address
```

The PC being woken needs "Wake on Magic Packet" on in its network adapter's properties
(Advanced and Power Management tabs) and in the BIOS, and Windows **fast startup turned off**.

**ComfyUI.** Tell it where ComfyUI lives, in `data\settings\comfyui.toml`. For the portable build:

```toml
comfy_dir = "D:/ComfyUI_windows_portable"
start_command = ["python_embeded\\python.exe", "-s", "ComfyUI\\main.py", "--windows-standalone-build"]
```

(Leave `start_command` empty if you start ComfyUI yourself; everything else still works.)
To run workflows from Kernel, open each one in ComfyUI, use **Workflow > Export (API)**, and save
it in `kernel-workflows\` inside `comfy_dir`. The prompt goes into a `{{prompt}}` placeholder if the
workflow has one, otherwise into the text box wired to the sampler's positive input (same for
`{{negative}}`). Downloads take Hugging Face links and Civitai *download* links; set
`civitai_token` for Civitai files that need an account.

**VRAM.** Shares the GPU between Roblox, ComfyUI and Ollama out of the box: while ComfyUI has
jobs queued, Ollama's models are unloaded (ComfyUI is `exclusive`), and if free VRAM stays under
1 GB, the lowest-priority idle app is unloaded. Roblox is only watched. Add your other GPU apps
in `data\settings\vram.toml` (this replaces the whole list, so keep the three):

```toml
apps = [
  { name = "Roblox", kind = "watch", process = "RobloxPlayerBeta.exe", priority = 100 },
  { name = "ComfyUI", kind = "comfyui", url = "http://127.0.0.1:8188", priority = 50, exclusive = true },
  { name = "Ollama", kind = "ollama", url = "http://127.0.0.1:11434", priority = 30 },
  { name = "LM Studio", kind = "process", process = "LM Studio.exe", priority = 20, stop_when_needed = true },
]
```

Higher priority keeps its VRAM. A `process` app is closed to make room only with
`stop_when_needed = true`; without it, it's just measured. **Give the GPU to…** frees
everything else for one app; **Free idle VRAM** unloads whatever isn't busy.

**Taking turns.** So LLMs and image/video generation don't fight over the GPU, point your apps
at the VRAM module's proxies instead of the apps themselves:

| Instead of | Use | For |
|---|---|---|
| `http://127.0.0.1:11434` (Ollama) | `http://127.0.0.1:11435` | Open WebUI, scripts, anything that talks to Ollama |
| `http://127.0.0.1:8188` (ComfyUI) | `http://127.0.0.1:8189` | the ComfyUI page in your browser, and `url` in `comfyui.toml` |

Then: while ComfyUI is working, LLM requests wait, and new images join the queue, so images
around an LLM request run back to back and models swap once. A running LLM request makes new
image jobs wait for it (a few seconds). No LLM request waits more than `llm_max_wait_s` (90 s).
An image job that fails for lack of VRAM is queued again once, after making room. Apps that
still talk to Ollama or ComfyUI directly aren't held back, but the unloading above still covers
them. Other LLM servers (LM Studio, KoboldCpp) can take turns too: add them as
`kind = "llm"` with their `url` and a `proxy_port`.

**Roblox.** Works with no settings. Low-power AFK mode is on by default. `data\settings\roblox.toml`
options:

```toml
place_id = 606849621     # place to rejoin; 0 = the last place seen in the logs
auto_rejoin = true       # rejoin by itself after a disconnect (idle kick), at most every 10 min

low_power = true         # the whole low-power mode below
fps_cap = 30             # Roblox's own frame cap; lower values are tried, Roblox decides
hide_window = true       # hide the window once in game (Roblox > Show Roblox brings it back)
priority = "below_normal"  # normal, below_normal or idle
efficiency_mode = true   # Windows 11 Efficiency mode
cpu_cores = 0            # limit Roblox to this many cores; 0 = no limit
```

What low-power mode does, and doesn't:

- **Graphics:** lowest textures, quality level 1, no MSAA, gray sky, no grass, lighting voxelizer
  paused. Only flags on [Roblox's allowlist](https://devforum.roblox.com/t/allowlist-for-local-client-configuration-via-fast-flags/3966569),
  written into Roblox's `ClientAppSettings.json` next to any of your own. They apply **the next
  time Roblox starts**, and are re-added after Roblox updates itself.
- **Frame cap:** Roblox's own `FramerateCap` setting, written while Roblox is closed.
- **Once in game:** the window is hidden, priority lowered, Efficiency mode on. The Roblox
  status shows `tuning: applied`, or what Windows refused.
- **Not** a headless client: nothing is patched or injected into Roblox (that's what its
  anti-cheat bans for). Compare Pluto's GPU in the PC monitor with it on and off to see the savings.
- Joining from the browser is fine. For **Rejoin** to work, log in inside the Roblox app once.

It only watches the client and relaunches it: it never sends input to the game.

## Updates

**Node > Check for updates**, then **Install update**. The node downloads the new build, verifies
it, restarts on it in a few seconds, and puts the old version back if the new one fails to install.
It also checks on its own every 6 hours (`check_interval_h`). Set `auto_install = true` in
`node.toml` to install without the button.

The WSL keepalive drops for those few seconds while the node restarts. WSL waits a little
before shutting down, so it rides through, but keep the **WSL keepalive** scheduled task as a
backup anyway.

## Files

| Path (under `%LOCALAPPDATA%\Kernel\node`) | What |
|---|---|
| `app\` | the current build: `kerneld.exe`, modules, the SDK wheel. Replaced by updates |
| `app.previous\` | the build before the last update (for rollback) |
| `python\` | Python environment for modules |
| `node.toml` | config: node token, GitHub token, modules, update settings |
| `data\logs\` | `kerneld.log.*`, `update.log.*`, and `modules\<id>.log` |
| `data\settings\<module>.toml` | per-machine module settings (kept across updates) |
| `data\node.db` | activity log |

## Troubleshooting

- **App says "Can't reach the node":** is the task running (`Get-ScheduledTask 'Kernel node'`)?
  Is the firewall rule there? Are both PCs signed in to Tailscale? Use Pluto's Tailscale address
  (`100.x.x.x`): the node refuses home-network connections unless `allow_lan = true`.
- **Update check fails with 404:** the token can't read the repo; make a new one as above and
  put it in `[update] token` in `node.toml`.
- **Reinstall or repair:** run `install-node.ps1` again. It keeps `node.toml`.
