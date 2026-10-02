"""Flow Race: install the game from GitHub, keep its server up, and share it through Tailscale.

The game is downloaded into the module's data folder (versions/<commit>/), built with npm, and
run with Node using Kernel's host (kernel-host.ts, copied into the game's server/ folder), which
keeps ratings in a file and listens on 127.0.0.1 only. Friends reach it through Tailscale:

- "anyone with the link": Tailscale Funnel gives the PC a public https://<pc>.<tailnet>.ts.net
  address that forwards to the game, and only to the game. Nothing to install for friends.
- "my tailnet": Tailscale Serve, the same address but only for people on (or shared into) the
  tailnet.

If the game's code changes so the Kernel host no longer starts, it falls back to the game's own
server/main.ts (ratings then reset on restart) and says so on the page.
"""

from __future__ import annotations

import asyncio
import contextlib
import io
import json
import logging
import os
import re
import shutil
import subprocess
import sys
import tarfile
import time
import urllib.error
import urllib.request
from collections.abc import Callable
from dataclasses import dataclass
from pathlib import Path
from typing import Any

from kernel_sdk import ActionError

HERE = Path(__file__).resolve().parent
HOST_FILE = HERE / "kernel-host.ts"
MIN_NODE = (22, 18)
START_TIMEOUT_S = 90
FUNNEL_PORTS = (443, 8443, 10000)
WHO = {"anyone with the link": "funnel", "my tailnet": "serve"}


@dataclass
class Ran:
    code: int
    out: str


def run_command(cmd: list[str], cwd: Path | None = None, timeout: float = 600, env: dict[str, str] | None = None) -> Ran:
    """Runs a command to the end, without a window on Windows."""
    flags = subprocess.CREATE_NO_WINDOW if sys.platform == "win32" else 0
    try:
        p = subprocess.run(
            cmd,
            cwd=cwd,
            env=env,
            stdin=subprocess.DEVNULL,
            capture_output=True,
            timeout=timeout,
            creationflags=flags,
        )
    except FileNotFoundError:
        return Ran(127, f"{cmd[0]} not found")
    except subprocess.TimeoutExpired as e:
        out = (e.stdout or b"") + (e.stderr or b"")
        return Ran(124, out.decode(errors="replace") + f"\n(timed out after {timeout:g}s)")
    return Ran(p.returncode, (p.stdout + p.stderr).decode(errors="replace"))


def http_get(url: str, timeout: float = 5, headers: dict[str, str] | None = None) -> bytes:
    req = urllib.request.Request(url, headers={"User-Agent": "kernel-flowrace", **(headers or {})})
    with urllib.request.urlopen(req, timeout=timeout) as r:
        return r.read()


def node_version(text: str) -> tuple[int, int] | None:
    m = re.match(r"v?(\d+)\.(\d+)", text.strip())
    return (int(m[1]), int(m[2])) if m else None


def tail(text: str, lines: int = 12) -> str:
    return "\n".join(text.strip().splitlines()[-lines:])


def share_url(dns_name: str, port: int) -> str:
    host = dns_name.rstrip(".")
    return f"https://{host}" if port == 443 else f"https://{host}:{port}"


def sharing_from(serve_status: dict[str, Any], dns_name: str, https_port: int, game_port: int) -> str:
    """'public', 'tailnet' or 'off', from `tailscale serve status --json`."""
    key = f"{dns_name.rstrip('.')}:{https_port}"
    handlers = ((serve_status.get("Web") or {}).get(key) or {}).get("Handlers") or {}
    proxy = str((handlers.get("/") or {}).get("Proxy", ""))
    if not re.search(rf"(127\.0\.0\.1|localhost):{game_port}/?$", proxy):
        return "off"
    return "public" if (serve_status.get("AllowFunnel") or {}).get(key) else "tailnet"


class FlowRace:
    def __init__(
        self,
        settings: dict[str, Any],
        data_dir: Path,
        emit: Callable[..., None] | None = None,
        log: logging.Logger | None = None,
        run: Callable[..., Ran] = run_command,
        get: Callable[..., bytes] = http_get,
    ) -> None:
        self.settings = settings
        self.data = data_dir
        self.emit = emit or (lambda *a, **k: None)
        self.log = log or logging.getLogger("flowrace")
        self.run, self.get = run, get
        self.port = int(settings.get("port", 8095))
        self.process: subprocess.Popen[bytes] | None = None
        self.mode = ""  # "kernel" or "standalone"
        self.want_running = bool(settings.get("autostart", True))
        self.state = "stopped"
        self.busy = ""  # what an action is doing right now
        self.error = ""
        self.restarts: list[float] = []
        self.retry_at = 0.0
        self.game: dict[str, Any] = {}
        self.matches_seen: int | None = None
        self.sharing = "unknown"
        self.link = ""
        self.latest = ""
        self._lock = asyncio.Lock()

    # -- files ----------------------------------------------------------------------------

    @property
    def versions(self) -> Path:
        return self.data / "versions"

    def current(self) -> Path | None:
        try:
            name = (self.data / "current.txt").read_text().strip()
        except OSError:
            return None
        path = self.versions / name
        return path if name and (path / "package.json").is_file() else None

    def version(self) -> str:
        cur = self.current()
        return cur.name[:7] if cur else ""

    # -- install and update ---------------------------------------------------------------

    def _github(self, path: str, accept: str) -> bytes:
        headers = {"Accept": accept}
        if self.settings.get("github_token"):
            headers["Authorization"] = f"Bearer {self.settings['github_token']}"
        return self.get(f"https://api.github.com/repos/{self.settings['repo']}/{path}", timeout=60, headers=headers)

    def latest_commit(self) -> str:
        try:
            sha = self._github(f"commits/{self.settings.get('ref', 'main')}", "application/vnd.github.sha")
        except (urllib.error.URLError, OSError) as e:
            raise ActionError("offline", f"couldn't reach GitHub ({e}); downloads need a network where github.com works") from None
        return sha.decode().strip()

    def _tool(self, key: str, windows_exe: str) -> str:
        """A configured program, found on PATH or in Node's default install folder.

        kerneld may have started before Node was installed, so its PATH may not have it yet.
        """
        configured = str(self.settings.get(key, key))
        found = shutil.which(configured)
        if found:
            return found
        default = Path(os.environ.get("ProgramFiles", r"C:\Program Files")) / "nodejs" / windows_exe
        return str(default) if sys.platform == "win32" and default.is_file() else configured

    def check_node(self) -> str:
        node = self._tool("node", "node.exe")
        r = self.run([node, "--version"], timeout=20)
        version = node_version(r.out) if r.code == 0 else None
        if version is None:
            raise ActionError(
                "disabled", "Node.js isn't installed. Install Node 22.18 or newer (winget install OpenJS.NodeJS.LTS)."
            )
        if version < MIN_NODE:
            raise ActionError("disabled", f"Flow Race needs Node 22.18 or newer; this PC has {r.out.strip()}")
        return r.out.strip()

    def _install_blocking(self, sha: str) -> Path:
        self.check_node()
        npm = self._tool("npm", "npm.cmd")
        target = self.versions / sha
        if (target / "public" / "app.js").is_file():
            return target
        staging = self.versions / f".{sha}.partial"
        shutil.rmtree(staging, ignore_errors=True)
        staging.mkdir(parents=True)
        try:
            raw = self._github(f"tarball/{sha}", "application/vnd.github+json")
            with tarfile.open(fileobj=io.BytesIO(raw), mode="r:gz") as tar:
                members = [m for m in tar.getmembers() if "/" in m.name]
                for m in members:
                    m.name = m.name.split("/", 1)[1]  # drop the "<owner>-<repo>-<sha>/" folder
                extract = {"filter": "data"} if hasattr(tarfile, "data_filter") else {}
                tar.extractall(staging, members=members, **extract)
            for cmd in ([npm, "ci", "--no-audit", "--no-fund"], [npm, "run", "build"]):
                r = self.run(cmd, cwd=staging, timeout=900)
                if r.code != 0:
                    raise ActionError("module_failed", f"`{' '.join(cmd[1:])}` failed:\n{tail(r.out)}")
            shutil.rmtree(target, ignore_errors=True)
            staging.rename(target)
        finally:
            shutil.rmtree(staging, ignore_errors=True)
        return target

    def _switch_to(self, version: Path) -> None:
        (self.data / "current.txt").write_text(version.name)
        keep = {version.name}
        for old in sorted(self.versions.iterdir(), key=lambda p: p.stat().st_mtime, reverse=True):
            if old.name in keep or old.name.startswith("."):
                continue
            if len(keep) < 2:  # the one before, to go back to by hand
                keep.add(old.name)
                continue
            shutil.rmtree(old, ignore_errors=True)

    async def install(self, force: bool = False) -> dict[str, Any]:
        """Downloads the latest commit, builds it and (re)starts the server on it."""
        if self.busy:
            raise ActionError("busy", f"already {self.busy}")
        online = int(self.game.get("online", 0) or 0)
        if self.process_alive() and online and not force:
            raise ActionError("busy", f"{online} playing right now; update with force to kick them")
        self.busy = "updating"
        try:
            sha = await asyncio.to_thread(self.latest_commit)
            before = self.version()
            if sha[:7] == before:
                self.latest = sha
                return {"version": before, "updated": False}
            target = await asyncio.to_thread(self._install_blocking, sha)
            await asyncio.to_thread(self._switch_to, target)
            self.latest = sha
        finally:
            self.busy = ""
        self.emit("game.updated", f"Flow Race updated to {sha[:7]}", version=sha[:7])
        if self.want_running:
            await self.restart()
        return {"version": sha[:7], "updated": True, "was": before}

    async def check_update(self) -> None:
        if not self.current():
            return
        try:
            sha = await asyncio.to_thread(self.latest_commit)
        except ActionError:
            return
        if sha[:7] != self.version() and sha != self.latest:
            self.emit("game.update_available", f"A new Flow Race version is on GitHub ({sha[:7]})", version=sha[:7])
        self.latest = sha

    # -- the server process ---------------------------------------------------------------

    def process_alive(self) -> bool:
        return self.process is not None and self.process.poll() is None

    def _kill_leftover(self) -> None:
        """A server left running by an earlier run of this module would hold the port."""
        pid_file = self.data / "server.pid"
        try:
            pid = int(pid_file.read_text())
        except (OSError, ValueError):
            return
        pid_file.unlink(missing_ok=True)
        try:
            import psutil

            p = psutil.Process(pid)
            if "node" in p.name().lower():
                p.kill()
                p.wait(5)
                self.log.info("stopped a leftover game server (pid %d)", pid)
        except Exception:
            pass

    def _spawn(self, game: Path, mode: str) -> subprocess.Popen[bytes]:
        node = self._tool("node", "node.exe")
        if mode == "kernel":
            shutil.copyfile(HOST_FILE, game / "server" / "kernel-host.ts")
            entry = "server/kernel-host.ts"
        else:
            entry = "server/main.ts"
        env = {
            **os.environ,
            "PORT": str(self.port),
            "HOST": "127.0.0.1",
            "DATA_DIR": str(self.data / "save"),
            "MAX_CONNECTIONS": str(self.settings.get("max_connections", 64)),
            "STOP_ON_STDIN_CLOSE": "1",
        }
        flags = subprocess.CREATE_NO_WINDOW if sys.platform == "win32" else 0
        with (self.data / "server.log").open("ab") as log:
            log.write(f"\n--- {time.strftime('%Y-%m-%d %H:%M:%S')} starting {entry} ---\n".encode())
            proc = subprocess.Popen(
                [node, entry],
                cwd=game,
                env=env,
                stdin=subprocess.PIPE,
                stdout=log,
                stderr=subprocess.STDOUT,
                creationflags=flags,
            )
        (self.data / "server.pid").write_text(str(proc.pid))
        return proc

    def healthy(self) -> bool:
        try:
            return self.get(f"http://127.0.0.1:{self.port}/health", timeout=2) == b"ok"
        except (urllib.error.URLError, OSError):
            return False

    async def _wait_healthy(self, proc: subprocess.Popen[bytes]) -> bool:
        deadline = time.monotonic() + START_TIMEOUT_S
        while time.monotonic() < deadline:
            if proc.poll() is not None:
                return False
            if await asyncio.to_thread(self.healthy):
                return True
            await asyncio.sleep(0.5)
        return False

    async def start(self, manual: bool = True) -> dict[str, Any]:
        async with self._lock:
            self.want_running = True
            if manual:
                self.restarts, self.retry_at = [], 0.0
            if self.process_alive():
                return {"already": "running", "url": self.local_url()}
            game = self.current()
            if game is None:
                raise ActionError("disabled", "Flow Race isn't installed yet: press Update game")
            await asyncio.to_thread(self._kill_leftover)
            self.state, self.error = "starting", ""
            for mode in ("kernel", "standalone"):
                proc = self._spawn(game, mode)
                if await self._wait_healthy(proc):
                    self.process, self.mode, self.state = proc, mode, "running"
                    if mode == "standalone":
                        self.error = "Kernel's host didn't start with this version, so ratings reset on restart"
                        self.emit("game.fallback", self.error, level="warn")
                    return {"started": True, "mode": mode, "url": self.local_url()}
                await asyncio.to_thread(self._stop_process, proc)
            self.state = "crashed"
            self.error = f"the game server didn't start; see {self.data / 'server.log'}"
            raise ActionError("module_failed", self.error)

    def _stop_process(self, proc: subprocess.Popen[bytes]) -> None:
        if proc.poll() is None:
            if proc.stdin is not None:
                with contextlib.suppress(OSError):
                    proc.stdin.close()  # the host saves and exits
            try:
                proc.wait(5)
            except subprocess.TimeoutExpired:
                proc.kill()
                proc.wait(5)
        (self.data / "server.pid").unlink(missing_ok=True)

    async def stop(self) -> dict[str, Any]:
        async with self._lock:
            self.want_running = False
            if self.process is not None:
                await asyncio.to_thread(self._stop_process, self.process)
            self.process, self.state, self.game = None, "stopped", {}
            return {"stopped": True}

    async def restart(self) -> dict[str, Any]:
        await self.stop()
        return await self.start()

    def local_url(self) -> str:
        return f"http://127.0.0.1:{self.port}"

    # -- Tailscale ------------------------------------------------------------------------

    def _tailscale(self) -> str:
        configured = str(self.settings.get("tailscale", "tailscale"))
        found = shutil.which(configured)
        if found:
            return found
        default = Path(r"C:\Program Files\Tailscale\tailscale.exe")
        return str(default) if default.is_file() else configured

    def _https_port(self) -> int:
        port = int(self.settings.get("share_port", 443))
        if port not in FUNNEL_PORTS:
            raise ActionError("invalid_params", f"share_port must be one of {FUNNEL_PORTS}")
        return port

    def dns_name(self) -> str:
        r = self.run([self._tailscale(), "status", "--json"], timeout=20)
        if r.code != 0:
            raise ActionError("offline", f"Tailscale isn't running: {tail(r.out, 3)}")
        try:
            return str(json.loads(r.out)["Self"]["DNSName"]).rstrip(".")
        except (ValueError, KeyError, TypeError):
            raise ActionError("module_failed", "couldn't read this PC's Tailscale name") from None

    def refresh_sharing(self) -> None:
        try:
            dns = self.dns_name()
            r = self.run([self._tailscale(), "serve", "status", "--json"], timeout=20)
            status = json.loads(r.out) if r.code == 0 and r.out.strip().startswith("{") else {}
            port = self._https_port()
        except (ActionError, ValueError):
            self.sharing, self.link = "unknown", ""
            return
        self.sharing = sharing_from(status, dns, port, self.port)
        self.link = share_url(dns, port) if self.sharing != "off" else ""

    async def share(self, who: str) -> dict[str, Any]:
        kind = WHO.get(who)
        if kind is None:
            raise ActionError("invalid_params", f"who must be one of: {', '.join(WHO)}")
        port = self._https_port()
        ts = self._tailscale()
        # Switch off the other kind first, so the two don't both claim the port.
        await asyncio.to_thread(self.run, [ts, "funnel" if kind == "serve" else "serve", f"--https={port}", "off"], timeout=30)
        cmd = [ts, kind, "--bg", "--yes", f"--https={port}", f"http://127.0.0.1:{self.port}"]
        r = await asyncio.to_thread(self.run, cmd, timeout=40)
        if r.code != 0 and "--yes" in r.out:  # an older Tailscale without --yes
            cmd.remove("--yes")
            r = await asyncio.to_thread(self.run, cmd, timeout=40)
        link = re.search(r"https://login\.tailscale\.com/\S+", r.out)
        if r.code != 0:
            if link:
                raise ActionError(
                    "disabled",
                    f"Tailscale needs your OK first: open {link[0]} (as the tailnet admin), "
                    "allow it, then press Share again.",
                )
            raise ActionError("module_failed", f"tailscale {kind} failed:\n{tail(r.out, 6)}")
        await asyncio.to_thread(self.refresh_sharing)
        if self.sharing == "off":
            raise ActionError("module_failed", f"Tailscale accepted it but isn't sharing the game:\n{tail(r.out, 6)}")
        who_text = "anyone with the link" if self.sharing == "public" else "people on your tailnet"
        self.emit("share.on", f"Flow Race is open to {who_text}: {self.link}", link=self.link)
        return {"link": self.link, "who": who_text}

    async def unshare(self) -> dict[str, Any]:
        port = self._https_port()
        ts = self._tailscale()
        await asyncio.to_thread(self.run, [ts, "funnel", f"--https={port}", "off"], timeout=30)
        await asyncio.to_thread(self.run, [ts, "serve", f"--https={port}", "off"], timeout=30)
        await asyncio.to_thread(self.refresh_sharing)
        self.emit("share.off", "Flow Race is no longer shared")
        return {"sharing": self.sharing}

    # -- watching -------------------------------------------------------------------------

    async def poll_game(self) -> None:
        if not self.process_alive() or self.mode != "kernel":
            self.game = {}
            return
        try:
            raw = await asyncio.to_thread(self.get, f"{self.local_url()}/kernel/status", 3)
            game = json.loads(raw)
        except (urllib.error.URLError, OSError, ValueError):
            return
        matches = int(game.get("matches", 0))
        last = game.get("last_match") or {}
        if self.matches_seen is not None and matches > self.matches_seen and last:
            players = ", ".join(last.get("players") or [])
            winner = last.get("winner")
            text = f"{winner} won a Flow Race ({players})" if winner else f"Flow Race match finished ({players})"
            self.emit("match.finished", text, winner=winner, players=last.get("players"))
        self.matches_seen = matches
        self.game = game

    def _retry_later(self) -> None:
        now = time.monotonic()
        self.restarts = [t for t in self.restarts if now - t < 600] + [now]
        if len(self.restarts) > 5:
            self.want_running = False
            self.error += ". It failed 5 times in 10 minutes, so it stays off until you press Start."
            self.emit("game.gave_up", "Flow Race keeps crashing; left off", level="error")
        self.retry_at = now + min(60, 2 ** len(self.restarts))

    async def watch_process(self) -> None:
        """Restarts a crashed server, backing off if it keeps crashing."""
        if not self.want_running or self.busy or self._lock.locked() or self.current() is None:
            return
        if self.process is not None:
            if self.process.poll() is None:
                return
            code = self.process.returncode
            self.process, self.state, self.game = None, "crashed", {}
            self.error = f"the game server stopped (exit code {code}); see {self.data / 'server.log'}"
            self.emit("game.crashed", f"Flow Race's server stopped (exit code {code}); restarting", level="warn")
            self._retry_later()
            return
        if time.monotonic() < self.retry_at:
            return
        try:
            await self.start(manual=False)
        except ActionError:
            self._retry_later()

    async def run_forever(self) -> None:
        """Background task: first install, keep it up, watch players and updates."""
        poll = float(self.settings.get("poll_s", 5.0))
        check_every = float(self.settings.get("update_check_h", 6)) * 3600
        last_check = last_share = 0.0
        if self.current() is None and self.want_running:
            try:
                await self.install()
            except ActionError as e:
                self.state, self.error = "not installed", e.message
        while True:
            await self.watch_process()
            await self.poll_game()
            now = time.monotonic()
            if now - last_share > 60:
                last_share = now
                await asyncio.to_thread(self.refresh_sharing)
            if check_every > 0 and now - last_check > check_every:
                last_check = now
                await self.check_update()
            await asyncio.sleep(poll)

    def snapshot(self) -> dict[str, Any]:
        state = self.busy or self.state
        if not self.current() and not self.busy:
            state = "not installed"
        game = self.game
        return {
            "state": state,
            "version": self.version(),
            "update_available": bool(self.latest and self.version() and self.latest[:7] != self.version()),
            "online": game.get("online", 0),
            "players": game.get("players"),
            "matches": game.get("matches"),
            "last_match": game.get("last_match"),
            "top": game.get("top", []),
            "sharing": self.sharing,
            "link": self.link,
            "local_url": self.local_url() if self.process_alive() else "",
            "saves_ratings": self.mode != "standalone",
            "error": self.error,
        }
