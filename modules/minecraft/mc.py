"""Talks to a Minecraft server that systemd runs on a Linux box (on Pluto: WSL Ubuntu).

Every operation is a small bash script from `scripts/`, piped to `bash -s` on the Linux
side over stdin. Nothing goes through cmd.exe quoting, and values are passed as
shell-quoted variables at the top of the script.

The server side provides `systemctl`, `mc-cmd` (send a console command), `mc-ping` (status
ping as JSON) and `logs/latest.log`.
"""

from __future__ import annotations

import asyncio
import contextlib
import json
import logging
import re
import shlex
import subprocess
from collections.abc import AsyncIterator
from dataclasses import dataclass
from datetime import UTC, datetime
from pathlib import Path
from typing import Any

from kernel_sdk import ActionError

SCRIPTS = Path(__file__).resolve().parent / "scripts"
PLAYER = re.compile(r"^[A-Za-z0-9_]{3,16}$")
SAFE_WORD = re.compile(r"^[A-Za-z0-9._@-]+$")
FORMATTING = re.compile(r"§.")
TRANSPORTS = ("wsl", "ssh", "direct")
# Stops wsl.exe/ssh from flashing a console window when kerneld runs without one.
NO_WINDOW = getattr(subprocess, "CREATE_NO_WINDOW", 0)

UNIT_STATES = {
    "activating": "starting",
    "deactivating": "stopping",
    "inactive": "stopped",
    "failed": "crashed",
}


def check_settings(s: dict[str, Any]) -> None:
    if s["transport"] not in TRANSPORTS:
        raise ValueError(f"transport must be one of {', '.join(TRANSPORTS)}")
    for key in ("distro", "ssh_host", "service"):
        if not SAFE_WORD.match(s[key]):
            raise ValueError(f"{key} has characters that are not allowed: {s[key]!r}")
    for key in ("server_dir", "backup_dir"):
        if not s[key].startswith("/"):
            raise ValueError(f"{key} must be an absolute Linux path")
    if s["keep_backups"] < 1:
        raise ValueError("keep_backups must be at least 1")
    for name in s["extra_services"]:
        if not SAFE_WORD.match(name):
            raise ValueError(f"extra_services has an invalid name: {name!r}")


def linux_argv(s: dict[str, Any], *cmd: str) -> list[str]:
    """The command line that runs `cmd` as root on the Linux side."""
    if s["transport"] == "wsl":
        return ["wsl.exe", "-d", s["distro"], "-u", "root", "--", *cmd]
    if s["transport"] == "ssh":
        # The remote shell is cmd.exe on Pluto, so only fixed, validated words go here.
        remote = " ".join(["wsl", "-d", s["distro"], "-u", "root", "--", *cmd])
        return ["ssh", "-o", "BatchMode=yes", s["ssh_host"], remote]
    return list(cmd)


@dataclass
class Result:
    code: int
    out: str
    err: str


class Shell:
    """Runs bash scripts through a fixed command line (for example `wsl.exe ... bash -s`)."""

    def __init__(self, argv: list[str], env: dict[str, str] | None = None) -> None:
        self.argv = argv
        self.env = env

    async def run(self, script: str, timeout: float) -> Result:
        try:
            proc = await asyncio.create_subprocess_exec(
                *self.argv,
                stdin=asyncio.subprocess.PIPE,
                stdout=asyncio.subprocess.PIPE,
                stderr=asyncio.subprocess.PIPE,
                env=self.env,
                creationflags=NO_WINDOW,
            )
        except OSError as e:
            raise ActionError("offline", f"could not run {self.argv[0]}: {e}") from None
        try:
            out, err = await asyncio.wait_for(proc.communicate(script.encode()), timeout)
        except TimeoutError:
            raise ActionError("timeout", f"no answer from the server within {timeout:g}s") from None
        finally:
            if proc.returncode is None:
                proc.kill()
        return Result(proc.returncode or 0, out.decode("utf-8", "replace"), err.decode("utf-8", "replace"))


def one_line(text: str, what: str, max_len: int) -> str:
    text = text.strip()
    if not text:
        raise ActionError("invalid_params", f"{what} is empty")
    if any(ch < " " or ch == "\x7f" for ch in text):
        raise ActionError("invalid_params", f"{what} must be a single line of plain text")
    if len(text) > max_len:
        raise ActionError("invalid_params", f"{what} is longer than {max_len} characters")
    return text


def check_player(name: str) -> str:
    if not PLAYER.match(name):
        raise ActionError("invalid_params", f"'{name}' is not a valid Minecraft player name")
    return name


def key_values(out: str) -> list[tuple[str, str]]:
    pairs = []
    for line in out.splitlines():
        key, sep, value = line.partition("=")
        if sep:
            pairs.append((key.strip(), value.strip()))
    return pairs


def chat_text(value: Any) -> str:
    """Flatten a chat component (string, dict with text/extra, or list) to plain text."""
    if isinstance(value, str):
        return FORMATTING.sub("", value)
    if isinstance(value, list):
        return "".join(chat_text(v) for v in value)
    if isinstance(value, dict):
        return chat_text(value.get("text", "")) + chat_text(value.get("extra", []))
    return ""


def parse_ping(text: str) -> dict[str, Any]:
    """Read the server list ping. Accepts the JSON status response, with a text fallback."""
    start, end = text.find("{"), text.rfind("}")
    data: Any = None
    if 0 <= start < end:
        with contextlib.suppress(ValueError):
            data = json.loads(text[start : end + 1])
    if not isinstance(data, dict):
        info: dict[str, Any] = {"ping": text.strip()[:500]}
        if m := re.search(r"(\d+)\s*/\s*(\d+)", text):
            info["players_online"], info["players_max"] = int(m[1]), int(m[2])
        return info
    players = data.get("players") or {}
    version = data.get("version") or {}
    return {
        "players_online": players.get("online"),
        "players_max": players.get("max"),
        "players": [p["name"] for p in players.get("sample") or [] if isinstance(p, dict) and "name" in p],
        "version": version.get("name") if isinstance(version, dict) else None,
        "motd": chat_text(data.get("description", "")).strip() or None,
    }


def parse_status(out: str) -> dict[str, Any]:
    head, _, ping = out.partition("--ping--\n")
    kv = key_values(head)
    values = dict(kv)
    unit = values.get("state", "unknown") or "unknown"

    responded = bool(ping) and "--ping-failed--" not in ping
    if unit in ("active", "reloading"):
        state = "running" if responded else "starting"
    else:
        state = UNIT_STATES.get(unit, "unknown")

    status: dict[str, Any] = {
        "state": state,
        "unit_state": unit,
        "services": {k.removeprefix("service."): v for k, v in kv if k.startswith("service.")},
        "players_online": None,
        "players_max": None,
        "players": [],
        "version": None,
        "motd": None,
        "uptime_s": None,
        "memory_mb": None,
        "mods_count": int(values["mods"]) if values.get("mods", "").isdigit() else None,
    }
    if responded:
        status.update(parse_ping(ping))
    now, since = values.get("now", ""), values.get("since", "")
    if state in ("running", "starting") and now.isdigit() and since.isdigit():
        status["uptime_s"] = max(0, int(now) - int(since))
    if values.get("memory", "").isdigit():
        status["memory_mb"] = int(values["memory"]) // (1024 * 1024)

    backups = []
    for key, value in kv:
        if key != "backup":
            continue
        parts = value.split(" ", 2)
        if len(parts) == 3:
            mtime, size, name = parts
            at = datetime.fromtimestamp(float(mtime), UTC).isoformat(timespec="seconds").replace("+00:00", "Z")
            backups.append({"file": name, "bytes": int(size), "at": at})
    status["backups"] = backups
    status["last_backup"] = backups[0]["at"] if backups else None
    return status


class Server:
    def __init__(self, settings: dict[str, Any], shell: Shell | None = None, log: logging.Logger | None = None):
        check_settings(settings)
        self.settings = settings
        self.shell = shell or Shell(linux_argv(settings, "bash", "-s"))
        self.log = log or logging.getLogger("minecraft")
        self.save_timeout_s = 120
        self.snapshot: dict[str, Any] = {"state": "unknown"}
        self.keepalive_up = False
        self._lock = asyncio.Lock()
        self._vars = {
            "MC_DIR": settings["server_dir"],
            "MC_SERVICE": settings["service"],
            "BACKUP_DIR": settings["backup_dir"],
            "SERVICES": " ".join(settings["extra_services"]),
            "KEEP": settings["keep_backups"],
        }

    # -- plumbing -------------------------------------------------------------------------

    def script(self, name: str, **variables: Any) -> str:
        lines = ["set -euo pipefail"]
        lines += [f"{k}={shlex.quote(str(v))}" for k, v in {**self._vars, **variables}.items()]
        lines.append((SCRIPTS / f"{name}.sh").read_text(encoding="utf-8"))
        return "\n".join(lines) + "\n"

    async def run(self, name: str, timeout: float = 30, **variables: Any) -> str:
        r = await self.shell.run(self.script(name, **variables), timeout)
        if r.code != 0:
            errors = [v for k, v in key_values(r.out) if k == "error"]
            lines = [line for line in r.err.splitlines() if line.strip()]
            message = errors[-1] if errors else lines[-1] if lines else f"{name} exited with code {r.code}"
            raise ActionError("module_failed", message)
        return r.out

    @contextlib.asynccontextmanager
    async def exclusive(self) -> AsyncIterator[None]:
        """Start, stop, restart and backup never overlap."""
        if self._lock.locked():
            raise ActionError("busy", "another server operation is still running")
        async with self._lock:
            yield

    # -- status ---------------------------------------------------------------------------

    async def refresh(self) -> dict[str, Any]:
        try:
            self.snapshot = parse_status(await self.run("status", timeout=20))
        except ActionError as e:
            self.snapshot = {"state": "unknown", "error": e.message}
        return self.snapshot

    def status(self) -> dict[str, Any]:
        keepalive = self.keepalive_up if self.keeps_wsl_alive else None
        return {**self.snapshot, "keepalive": keepalive}

    @property
    def keeps_wsl_alive(self) -> bool:
        return bool(self.settings["keep_wsl_alive"]) and self.settings["transport"] != "direct"

    async def poll(self) -> None:
        while True:
            await self.refresh()
            await asyncio.sleep(self.settings["refresh_s"])

    async def keepalive(self) -> None:
        """Hold a WSL session open for as long as the module runs, restarting it if it ends."""
        if not self.keeps_wsl_alive:
            return
        argv = linux_argv(self.settings, "sleep", "infinity")
        while True:
            try:
                proc = await asyncio.create_subprocess_exec(
                    *argv,
                    stdin=asyncio.subprocess.DEVNULL,
                    stdout=asyncio.subprocess.DEVNULL,
                    stderr=asyncio.subprocess.DEVNULL,
                    creationflags=NO_WINDOW,
                )
            except OSError as e:
                self.log.warning("cannot start the WSL keepalive (%s); retrying in 60s", e)
                await asyncio.sleep(60)
                continue
            self.keepalive_up = True
            try:
                await proc.wait()
            finally:
                self.keepalive_up = False
                if proc.returncode is None:
                    proc.kill()
            self.log.warning("WSL keepalive exited with code %s; restarting in 5s", proc.returncode)
            await asyncio.sleep(5)

    async def wait_ready(self, timeout: float) -> dict[str, Any]:
        loop = asyncio.get_running_loop()
        deadline = loop.time() + timeout
        while True:
            snap = await self.refresh()
            if snap["state"] == "running":
                return snap
            if snap["state"] in ("crashed", "stopped"):
                raise ActionError("module_failed", f"the server {snap['state']} while starting; check console.tail")
            if loop.time() > deadline:
                raise ActionError("timeout", f"the server did not answer pings within {timeout:g}s")
            await asyncio.sleep(3)

    # -- lifecycle ------------------------------------------------------------------------

    async def systemctl(self, verb: str, unit: str, timeout: float) -> None:
        await self.run("service", timeout=timeout, VERB=verb, UNIT=unit)

    async def start(self) -> dict[str, Any]:
        async with self.exclusive():
            await self.systemctl("start", self.settings["service"], timeout=60)
            return await self.wait_ready(self.settings["start_timeout_s"])

    async def stop(self, delay_min: int = 0) -> dict[str, Any]:
        async with self.exclusive():
            if delay_min and (await self.refresh())["state"] == "running":
                for left in range(delay_min, 0, -1):
                    unit = "minute" if left == 1 else "minutes"
                    await self.command(f"say Server stopping in {left} {unit}", wait_s=0)
                    await asyncio.sleep(60)
            await self.systemctl("stop", self.settings["service"], timeout=240)
            return await self.refresh()

    async def restart(self) -> dict[str, Any]:
        async with self.exclusive():
            await self.systemctl("restart", self.settings["service"], timeout=300)
            return await self.wait_ready(self.settings["start_timeout_s"])

    async def restart_service(self, name: str) -> dict[str, Any]:
        if name not in self.settings["extra_services"]:
            raise ActionError("invalid_params", f"'{name}' is not one of the configured services")
        await self.systemctl("restart", name, timeout=60)
        return {"service": name, "state": (await self.refresh()).get("services", {}).get(name)}

    # -- console --------------------------------------------------------------------------

    async def command(self, command: str, wait_s: float = 1.0) -> dict[str, Any]:
        command = one_line(command, "the command", 256).lstrip("/")
        out = await self.run("command", timeout=30, CMD=command, WAIT_S=wait_s)
        _, _, rest = out.partition("--out--\n")
        direct, _, logged = rest.partition("--log--\n")
        lines = [line for line in (direct + logged).splitlines() if line.strip()]
        return {"command": command, "output": lines}

    async def say(self, message: str) -> dict[str, Any]:
        return await self.command(f"say {one_line(message, 'the message', 200)}")

    async def tail(self, lines: int) -> dict[str, Any]:
        return {"lines": (await self.run("tail", LINES=lines)).splitlines()}

    async def whitelist(self) -> dict[str, Any]:
        entries = json.loads(await self.run("read", FILE="whitelist.json") or "[]")
        return {"players": sorted((e.get("name", "") for e in entries), key=str.lower)}

    async def mods(self) -> dict[str, Any]:
        mods = []
        for line in (await self.run("mods")).splitlines():
            name, _, size = line.partition("\t")
            if name:
                mods.append({"file": name, "bytes": int(size) if size.isdigit() else None})
        return {"count": len(mods), "mods": mods}

    # -- backups --------------------------------------------------------------------------

    async def backup(self) -> dict[str, Any]:
        async with self.exclusive():
            out = await self.run("backup", timeout=1700, SAVE_TIMEOUT_S=self.save_timeout_s)
        kv = key_values(out)
        values = dict(kv)
        await self.refresh()
        return {
            "file": values.get("file"),
            "bytes": int(values["bytes"]) if values.get("bytes", "").isdigit() else None,
            "deleted": [v for k, v in kv if k == "deleted"],
        }
