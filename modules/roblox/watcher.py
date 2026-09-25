"""Roblox AFK watcher: the client process, plus its log for joins, disconnects and leaves.

Watching and relaunching only: it never sends input to the game.
"""

from __future__ import annotations

import asyncio
import os
import re
import sys
import time
from collections.abc import Callable
from dataclasses import dataclass, field
from datetime import datetime
from pathlib import Path
from typing import Any

import psutil

from kernel_sdk import ActionError

JOINING = re.compile(r"! Joining game '([0-9a-f\-]{36})' place ([0-9]+) at ([0-9.]+)")


def line_time(line: str) -> float:
    """Roblox log lines start with an ISO timestamp (`2026-09-25T01:02:03.456Z,...`)."""
    stamp = line.split(",", 1)[0].split(" ", 1)[0]
    try:
        return datetime.fromisoformat(stamp.replace("Z", "+00:00")).timestamp()
    except ValueError:
        return time.time()


@dataclass
class LogState:
    """What the log says about the current session."""

    state: str = "menu"  # menu / joining / in_game / disconnected
    place_id: int | None = None
    job_id: str | None = None
    since: float | None = None
    last_event: str | None = None
    last_event_at: float | None = None
    disconnects: int = 0

    def event(self, text: str, at: float) -> None:
        self.last_event, self.last_event_at = text, at


@dataclass
class Markers:
    joining: list[str]
    joined: list[str]
    disconnected: list[str]
    left: list[str]


def apply_line(s: LogState, line: str, m: Markers) -> bool:
    """Update `s` from one log line. Returns True when the line was a disconnect."""
    if any(k in line for k in m.joining):
        at = line_time(line)
        match = JOINING.search(line)
        if match:
            s.job_id, s.place_id = match[1], int(match[2])
        s.state, s.since = "joining", at
        s.event(f"joining place {s.place_id}" if s.place_id else "joining a game", at)
    elif any(k in line for k in m.joined):
        at = line_time(line)
        s.state, s.since = "in_game", at
        s.event("joined the game", at)
    elif any(k in line for k in m.disconnected):
        # Leaving normally also logs this, right before the "left" marker.
        at = line_time(line)
        s.state = "disconnected"
        s.disconnects += 1
        s.event("disconnected from the game", at)
        return True
    elif any(k in line for k in m.left):
        at = line_time(line)
        if s.state == "disconnected":
            s.disconnects -= 1  # it was a normal leave after all
        s.state, s.since = "menu", at
        s.event("left the game", at)
    return False


@dataclass
class LogTail:
    """Follows the newest Player log in a folder, line by line."""

    folder: Path
    path: Path | None = None
    offset: int = 0
    partial: str = field(default="")

    def newest(self) -> Path | None:
        try:
            logs = [p for p in self.folder.iterdir() if p.suffix == ".log" and "player" in p.name.lower()]
        except OSError:
            return None
        return max(logs, key=lambda p: p.stat().st_mtime, default=None)

    def read(self) -> tuple[bool, list[str]]:
        """New complete lines. The flag is True when a new log file started (a new client)."""
        newest = self.newest()
        restarted = False
        if newest != self.path:
            self.path, self.offset, self.partial, restarted = newest, 0, "", True
        if self.path is None:
            return restarted, []
        try:
            with self.path.open("rb") as f:
                f.seek(self.offset)
                data = f.read()
        except OSError:
            return restarted, []
        self.offset += len(data)
        text = self.partial + data.decode("utf-8", "replace")
        lines = text.split("\n")
        self.partial = lines.pop()
        return restarted, [line.rstrip("\r") for line in lines if line.strip()]


def _alive(p: Any) -> bool:
    try:
        return bool(p.is_running())
    except psutil.Error:
        return False


def default_logs_dir() -> Path:
    return Path(os.environ.get("LOCALAPPDATA", str(Path.home()))) / "Roblox" / "logs"


def launch_uri(uri: str) -> None:
    if sys.platform != "win32":
        raise ActionError("module_failed", "launching Roblox only works on Windows")
    os.startfile(uri)  # type: ignore[attr-defined]


class Watcher:
    def __init__(
        self,
        settings: dict[str, Any],
        processes: Callable[[], list[Any]] | None = None,
        launch: Callable[[str], None] = launch_uri,
        clock: Callable[[], float] = time.time,
    ) -> None:
        self.settings = settings
        self.markers = Markers(settings["joining"], settings["joined"], settings["disconnected"], settings["left"])
        folder = Path(settings["logs_dir"]) if settings["logs_dir"] else default_logs_dir()
        self.tail = LogTail(folder)
        self.log = LogState()
        self.processes = processes or self._find_processes
        self.launch = launch
        self.clock = clock
        self.last_rejoin: float | None = None
        self.snapshot: dict[str, Any] = {"state": "closed"}

    def _find_processes(self) -> list[psutil.Process]:
        name = self.settings["process"].lower()
        found = []
        for p in psutil.process_iter(["name"]):
            if (p.info.get("name") or "").lower() == name:
                found.append(p)
        return found

    # -- status ---------------------------------------------------------------------------

    async def refresh(self) -> dict[str, Any]:
        restarted, lines = self.tail.read()
        if restarted:
            self.log = LogState()
        disconnected_now = False
        for line in lines:
            disconnected_now = apply_line(self.log, line, self.markers) or disconnected_now

        procs = self.processes()
        now = self.clock()
        snap: dict[str, Any] = {"state": "closed"}
        if procs:
            p = procs[0]
            snap["state"] = self.log.state
            try:
                snap["memory_mb"] = p.memory_info().rss // (1024 * 1024)
                snap["client_up_s"] = int(now - p.create_time())
            except (psutil.Error, AttributeError):
                pass
        if procs and self.log.since and self.log.state in ("in_game", "joining"):
            snap["session_s"] = max(0, int(now - self.log.since))
        snap["place_id"] = self.settings["place_id"] or self.log.place_id
        snap["disconnects"] = self.log.disconnects
        if self.log.last_event:
            snap["last_event"] = self.log.last_event
            snap["last_event_at"] = datetime.fromtimestamp(self.log.last_event_at or now).astimezone().isoformat(
                timespec="seconds"
            )
        snap["auto_rejoin"] = bool(self.settings["auto_rejoin"])
        self.snapshot = snap
        if disconnected_now:
            await self.maybe_auto_rejoin()
        return snap

    async def poll(self) -> None:
        while True:
            await self.refresh()
            await asyncio.sleep(self.settings["poll_s"])

    # -- actions --------------------------------------------------------------------------

    def place(self) -> int:
        place = self.settings["place_id"] or self.log.place_id
        if not place:
            raise ActionError("invalid_params", "no place to rejoin yet: join one once, or set place_id")
        return int(place)

    def rejoin(self) -> dict[str, Any]:
        place = self.place()
        self.launch(f"roblox://experiences/start?placeId={place}")
        self.last_rejoin = self.clock()
        return {"rejoining": place}

    async def close(self, timeout: float = 5.0) -> dict[str, Any]:
        """Ask the client to exit, then force it after `timeout` seconds."""
        procs = self.processes()
        for p in procs:
            try:
                p.terminate()
            except psutil.Error:
                pass
        deadline = time.monotonic() + timeout
        while any(_alive(p) for p in procs) and time.monotonic() < deadline:
            await asyncio.sleep(0.25)
        for p in procs:
            if _alive(p):
                try:
                    p.kill()
                except psutil.Error:
                    pass
        return {"closed": len(procs)}

    async def relaunch(self) -> dict[str, Any]:
        place = self.place()  # check before closing anything
        closed = (await self.close())["closed"]
        self.launch(f"roblox://experiences/start?placeId={place}")
        self.last_rejoin = self.clock()
        return {"closed": closed, "rejoining": place}

    async def maybe_auto_rejoin(self) -> None:
        if not self.settings["auto_rejoin"]:
            return
        cooldown = float(self.settings["rejoin_cooldown_min"]) * 60
        if self.last_rejoin is not None and self.clock() - self.last_rejoin < cooldown:
            return
        try:
            await self.relaunch()
            self.log.event("disconnected; rejoining automatically", self.clock())
        except ActionError as e:
            self.log.event(f"disconnected; couldn't rejoin: {e.message}", self.clock())
