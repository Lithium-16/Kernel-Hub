# Setting up Pluto as the home node

About 10 minutes, once. After that, updates are a button in the app.

## What you need

- A merge to `main` since the node bundle existed, so CI has published a `node-build-N` release
  (check the repo's **Releases** page).
- A **read-only GitHub token**, only while the repo is private (public: press Enter when asked):
  <https://github.com/settings/personal-access-tokens/new>
  - Repository access: **only** `Lithium-16/Kernel-Hub`
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

The app updates itself: **Settings > App updates > Check for updates** (it also looks on start
and shows "Update available" in the sidebar). It downloads the installer from the newest
release, checks its SHA-256, installs quietly and reopens. The first install is still by hand.

## The assistant

Kernel's assistant (the **Assistant** page in the app) answers with a model on Pluto, for free,
and can press your modules' buttons. It only runs actions that are safe for it; anything that
normally asks first shows you an **Approve** button, and button-only actions are never offered.

1. Install [Ollama](https://ollama.com/download) on Pluto, then in PowerShell:
   `ollama pull qwen3:8b` (about 5 GB). Any Ollama model with tool support works; set
   `model` in `[assistant]` in node.toml to switch.
2. That's it: the assistant talks to Ollama through the VRAM module's turn-taking proxy, so it
   waits for ComfyUI jobs instead of fighting them for the GPU.
3. Optional, Claude as the fallback when the local model fails or for hard questions (the
   **Claude** button on the page): make an API key at <https://console.anthropic.com>, then in
   node.toml:

   ```toml
   [assistant]
   anthropic_api_key = "sk-ant-..."
   monthly_budget_usd = 3.0     # it stops using Claude for the month at this
   ```

   The Node page shows this month's spend. Everything the assistant runs is in Activity, as the
   assistant.

### LAYA (optional): quicker answers to "is X up?"

[LAYA](https://pypi.org/project/laya/) is a small decision model (Apache-2.0) the assistant can
ask before the chat model. When it's sure you're asking how a module is doing, the status is read
straight away, so the reply takes one model round instead of two. It never presses buttons
(it's sometimes confidently wrong about those). Replies it helped show "LAYA: …" under them.

1. Turn **LAYA** on in **Settings > Node updates and modules**.
2. It installs the laya package and PyTorch by itself (a few hundred MB, from PyPI), then loads
   the model, which downloads ~1.7 GB from Hugging Face the first time. The **Install LAYA** and
   **Load model** buttons do the same by hand. Both need a network where PyPI and
   huggingface.co work.
3. It runs on the CPU by default (~2 GB of RAM). Be honest with yourself about the trade: on a
   CPU it adds roughly 1-3 s to *every* message and saves a model round only on status
   questions. With `device = "cuda"` in `data\settings\laya.toml` it takes tens of
   milliseconds, but needs a CUDA build of PyTorch that still supports the 1080 Ti (like
   ComfyUI's) and ~1-2 GB of VRAM. Try it; if chat feels slower, press **Unload model** or
   remove the module.

Knobs in `[assistant]` (defaults shown):

```toml
laya = true             # use it when the module is running
laya_fast = 0.9         # how sure it must be to read a status early; above 1 turns that off
laya_tools = 0          # e.g. 12: the local model only sees LAYA's top 12 buttons (faster, but a
                        # wrong guess hides the right one)
laya_claude = 0.0       # e.g. 0.8: requests that look like several steps go to Claude first
laya_timeout_ms = 3000  # slower than this and it's skipped for 10 minutes
```

The thresholds are guesses from a small test; watch the "LAYA: …" notes and adjust.

## Flow Race: a game server your friends can join

The **Flow Race** module keeps [Flow Race](https://github.com/john-doe16/FlowRace) running on
Pluto all the time and gives you a link to send friends. They open it in a browser and play;
they don't install anything.

1. **Node.js 22.18+** is needed. Without it, the module downloads the current LTS for itself
   (a zip from nodejs.org, checksum checked, kept in its data folder; no admin prompt).
2. Turn **Flow Race** on in **Settings > Node updates and modules**. On first start it
   downloads the game from GitHub, runs `npm ci` and builds it (a minute or two, on a network
   where github.com and npm work), then starts the server. It restarts it if it crashes, and
   after reboots.
3. On the **Flow Race** page, press **Share** → *anyone with the link*. The first time,
   Tailscale shows a link to switch Funnel on for your tailnet: open it, allow it, press
   **Share** again. The page then shows the link, like `https://pluto.tailXXXX.ts.net`. Send
   that to your friends.

What sharing does and doesn't open:

- **Anyone with the link** uses [Tailscale Funnel](https://tailscale.com/kb/1223/funnel): that
  one address forwards to the game only (Kernel, Minecraft's console, ComfyUI and the rest stay
  Tailscale-only). The game listens on 127.0.0.1, so nothing is opened on your router or home
  network. It is still a server on the public internet: anyone who finds the link can play,
  and you're trusting the game's code with that. **Stop sharing** takes the link down at once.
- **My tailnet** is the same link but only for devices on your tailnet (friends you've
  [shared Pluto with](https://tailscale.com/kb/1084/sharing) need Tailscale installed).

Players, ratings and the leaderboard are saved in `data\module-data\flowrace\save` and
survive restarts. New commits install by themselves when nobody's playing (see "Updates for
both games" below); **Update game** does it right away, and waits while people are playing
unless you tick *force*. If a future version of the game changes in a way Kernel's host
doesn't understand, it falls back to the game's own server and the page says ratings won't be
saved until that's fixed.

Wins show up as events ("Pluto won a Flow Race"), so you can send them to Discord like any
other event (`flowrace.match.*` in `[notify] include`).

## OpenFork: the strategy game, same idea

The **OpenFork** module does for [OpenFork](https://github.com/Lithium-16/OpenFork) what Flow Race's
does: downloads it, builds it, keeps it running, and shares it through Tailscale. Turn it on in
**Settings > Node updates and modules**, then press **Share** on its page. Its link uses port
8443 (`https://pluto.tailXXXX.ts.net:8443`), so both games can be shared at the same time.

Guest identities and match history are saved, so
an update or restart doesn't forget who is who; games in progress do end when the server
restarts.

### Updates for both games

Both check GitHub every 30 minutes (`update_check_h`). With `auto_update` on (the default) a new
commit is downloaded, built and switched to by itself, but only when nobody is connected, so a
game in progress is never cut off. **Update game** does it right away. If a commit fails to
build, the old version keeps running and you get an event; it tries again on the next commit.

Both follow the repository's default branch (`main`). To try a branch first, put its name in
`ref` (the gear on the game's page); once it's merged and deleted, the module goes back to the
default branch by itself and tells you. A renamed repository or GitHub username is followed too:
the module switches to the new name and tells you, so you can save it in `repo`.

## Party Games: Jackbox-style games on your big screen

The **Party Games** module hosts party games on Pluto: the host screen goes on your TV, monitor
or Discord stream, and friends play on their phones. Nothing to download or build; it's part of
Kernel.

1. Turn **Party Games** on in **Settings > Node updates and modules**.
2. Its page shows a **host link**: open it on the big screen (it has a key, so only you can
   open it; don't send it to anyone).
3. Press **Share** (same choices as Flow Race, see "What sharing does and doesn't open"
   above) and send friends the link. It uses port 10000
   (`https://pluto.tailXXXX.ts.net:10000`), so all three games can be shared at the same time.
   They open it on their phone, type their name and the room code from the big screen. The
   first one in is the VIP: they pick the game and press start.

The games (3 to 8 players; Bluff Buffet works with 2):

- **Quip Clash**: answer silly prompts, then everyone votes between two answers.
- **Bluff Buffet**: write a believable lie for a weird true fact, then find the truth.
- **Shirt Showdown**: draw designs, write slogans, make shirts and battle them.

To try a game alone, move the mouse on the host screen and press **Add bot** (or ask the
assistant to add bots). Bots answer, lie, draw and vote by themselves; with three bots the host
screen's **Start** plays a whole game on its own. They stay out of the hall of fame, and
**Remove bots** clears them. Friends don't need Kernel to host either: on the share link's join page, **Host a game on
this screen** opens a room of their own, with its own code and big screen, while yours keeps
going. Up to `max_rooms` (4) can be open at once, a room closes after 30 minutes with nobody in
it (or with **Close this room**), and every room records into the same hall of fame. Turn
`open_hosting` off to allow only your room. Friends on a laptop can join with the same link: on a big window the
page spreads out, with the question on the left and a bigger drawing pad.

It remembers who played: a profile per name (a new phone asks for the profile's 4-digit PIN),
monthly seasons with a champion, a hall of fame, greatest hits and badges, shown between games.
Only people in the room see it. It's saved in `data\module-data\party\party.db`; **Rename
player**, **Set player PIN**, **Forget player** and **Remove greatest hit** fix things up.

Your own prompts and facts (inside jokes welcome; `{player}` becomes a friend in the room) go in
`data\module-data\party\content\` as `quips.txt`, `facts.txt`, `doodle_ideas.txt` or
`slogan_ideas.txt`, same format as the built-in ones in `modules\party\content`. Press
**Reload prompts** after editing.

Games, winners, badges and season champions are events (`party.*` in `[notify] include` sends
them to Discord). Put a webhook in the module's `discord_webhook` setting to also get a recap of
the night when the room closes (or press **Post recap**).

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

Everything is in the app under **Settings > Node updates and modules**; you don't need to be at
Pluto:

- **Update now** installs the newest build. The node downloads it, verifies it, restarts on it
  in a few seconds, and puts the old version back if the new one fails to install. The sidebar
  says **Update Pluto** when a build is waiting.
- **Install updates automatically**: Pluto checks GitHub every 6 hours (`check_interval_h`) and
  installs new builds by itself. Same as `auto_install = true` in node.toml; the switch wins.
- **Modules**: a switch for every module in the build. New modules arrive switched off; turn
  one on and Kernel restarts with it (a few seconds). Each one sets itself up the first time:
  Flow Race downloads the game and, if needed, its own Node.js; LAYA installs its package.
  The switches win over `enabled_modules` in node.toml. They're kept in
  `data\node-prefs.json`, which you can delete to go back to node.toml.

These switches are buttons only: the assistant can't flip them.

### From any browser: the Kernel page

Pluto also serves a small page at **http://pluto:47800** (its Tailscale name or 100.x address,
same port as the app), so you can do all of this from your phone or any PC on your tailnet
without the app. Sign in with the token from node.toml once; the browser remembers it.

- **Update everything**: updates each game that has a new commit, then Kernel itself (with all
  its modules); the page reconnects when Pluto is back.
- Kernel's build, **Check now**, **Update now** and the auto-update switch.
- **Games**: version, players online, **Update game**, **Share** / **Stop sharing**, and once
  shared, **Open** and **Copy link** for the address to send friends.
- **Modules**: the on/off switches.
- **Desktop app**: a link to the newest installer. The app can't be updated from Pluto (it runs
  on your PC), but with **Install app updates automatically** on in its Settings it updates
  itself.

Like the app, the page only answers on Tailscale and this PC; it has no password of its own
besides the node token.

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

- **"GitHub's hourly limit for this network is used up"** (older builds said just
  "GitHub answered 403"): GitHub's API allows 60 anonymous requests an hour per home IP,
  shared by every PC and app on the network. Kernel then reads the newest release straight
  from github.com instead, which has no such limit, so updates keep working. Older builds
  don't: wait for the time it gives (at most an hour), or rerun `install-node.ps1` once. A
  read-only token in `[update] token` raises the limit to 5000 an hour.

- **App says "Can't reach the node":** is the task running (`Get-ScheduledTask 'Kernel node'`)?
  Is the firewall rule there? Are both PCs signed in to Tailscale? Use Pluto's Tailscale address
  (`100.x.x.x`): the node refuses home-network connections unless `allow_lan = true`.
- **Update check fails with 404:** the token can't read the repo; make a new one as above and
  put it in `[update] token` in `node.toml`.
- **Reinstall or repair:** run `install-node.ps1` again. It keeps `node.toml`.
