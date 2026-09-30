# Setting up Pluto as the home node

About 10 minutes, once. After that, updates are a button in the app.

## What you need

- A merge to `main` since the node bundle existed, so CI has published a `node-build-N` release
  (check the repo's **Releases** page).
- A **read-only GitHub token**, because the repo is private:
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
6. opens port 47800 in Windows Firewall for your LAN and Tailscale only
7. starts the node and prints the address and token

## Connect the app

Install the desktop app (the `kernel-desktop-windows` artifact from a CI run), open **Settings**,
and enter the address and token the installer printed. The sidebar shows **Minecraft** and **Node**.

## Module settings

Per-machine settings go in `%LOCALAPPDATA%\Kernel\node\data\settings\<module>.toml` (they
survive updates). The defaults are in each module's `module.toml`. Restart the node (Node >
Restart node) after changing them.

**PC monitor, Wake-on-LAN.** On the PC that should *send* the wake-up (Pluto, to wake the main PC,
or the other way round), `data\settings\pc-monitor.toml`:

```toml
wake_targets = ["main-pc=AA:BB:CC:DD:EE:FF"]   # the sleeping PC's MAC: `getmac /v` on that PC
wake_broadcast = "192.168.1.255"               # your LAN's broadcast address
```

The PC being woken needs "Wake on Magic Packet" on in its network adapter's properties
(Advanced and Power Management tabs) and in the BIOS, and Windows **fast startup turned off**.

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
  Is the firewall rule there? Try the Tailscale address.
- **Update check fails with 404:** the token can't read the repo; make a new one as above and
  put it in `[update] token` in `node.toml`.
- **Reinstall or repair:** run `install-node.ps1` again. It keeps `node.toml`.
