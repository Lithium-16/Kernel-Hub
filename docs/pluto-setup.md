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
