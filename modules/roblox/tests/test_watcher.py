import importlib.util
import os
import time
from pathlib import Path

import pytest

from kernel_sdk import ActionError
from watcher import LogState, LogTail, Markers, Watcher, apply_line, line_time

JOB = "0f8fad5b-d9cb-469f-a165-70867728950e"
LINES = {
    "join": f"2026-09-25T01:00:00.000Z,1.0,1a2b,6 [FLog::Output] ! Joining game '{JOB}' place 606849621 at 10.0.0.5",
    "joined": "2026-09-25T01:00:05.000Z,6.0,1a2b,6 [FLog::Network] Replicator created: 1234",
    "disconnect": "2026-09-25T01:20:05.000Z,1206.0,1a2b,6 [FLog::Network] Time to disconnect replication data: 0.01",
    "left": "2026-09-25T01:20:06.000Z,1207.0,1a2b,6 [FLog::SingleSurfaceApp] leaveUGCGameInternal",
    "noise": "2026-09-25T01:10:00.000Z,600.0,1a2b,6 [FLog::Graphics] frame took 16ms",
}
MARKERS = Markers(
    joining=["[FLog::Output] ! Joining game"],
    joined=["[FLog::Network] Replicator created:"],
    disconnected=["[FLog::Network] Time to disconnect replication data:"],
    left=["[FLog::SingleSurfaceApp] leaveUGCGameInternal"],
)


def at(stamp: str) -> float:
    return line_time(stamp + ",x")


class FakeProc:
    next_pid = 4000

    def __init__(self, started: float, rss_mb: int = 900, dies_on_terminate: bool = True):
        self.started, self.rss = started, rss_mb * 1024 * 1024
        self.running, self.dies = True, dies_on_terminate
        self.calls = []
        FakeProc.next_pid += 1
        self.pid = FakeProc.next_pid

    def nice(self, value):
        self.calls.append(("nice", value))

    def cpu_affinity(self, cores):
        self.calls.append(("affinity", cores))

    def memory_info(self):
        return type("M", (), {"rss": self.rss})()

    def create_time(self):
        return self.started

    def is_running(self):
        return self.running

    def terminate(self):
        self.calls.append("terminate")
        if self.dies:
            self.running = False

    def kill(self):
        self.calls.append("kill")
        self.running = False


class FakeWindows:
    def __init__(self, efficiency_ok=True):
        self.calls = []
        self.efficiency_ok = efficiency_ok

    def windows(self, pid):
        return [pid * 10]

    def hide(self, hwnd):
        self.calls.append(("hide", hwnd))

    def show(self, hwnd):
        self.calls.append(("show", hwnd))

    def efficiency(self, pid, on):
        self.calls.append(("efficiency", pid, on))
        return self.efficiency_ok


def settings(tmp_path, **over):
    base = {
        "low_power": False,
        "fps_cap": 30,
        "hide_window": True,
        "priority": "below_normal",
        "efficiency_mode": True,
        "cpu_cores": 0,
        "place_id": 0,
        "auto_rejoin": False,
        "rejoin_cooldown_min": 10,
        "logs_dir": str(tmp_path),
        "process": "RobloxPlayerBeta.exe",
        "poll_s": 3.0,
        "joining": MARKERS.joining,
        "joined": MARKERS.joined,
        "disconnected": MARKERS.disconnected,
        "left": MARKERS.left,
    }
    return {**base, **over}


def write_log(folder: Path, name: str, *keys: str) -> Path:
    p = folder / name
    with p.open("a", encoding="utf-8") as f:
        for k in keys:
            f.write(LINES[k] + "\n")
    return p


def test_the_log_state_machine():
    s = LogState()
    apply_line(s, LINES["join"], MARKERS)
    assert (s.state, s.place_id, s.job_id) == ("joining", 606849621, JOB)
    apply_line(s, LINES["noise"], MARKERS)
    apply_line(s, LINES["joined"], MARKERS)
    assert s.state == "in_game" and s.since == at("2026-09-25T01:00:05.000Z")
    assert apply_line(s, LINES["disconnect"], MARKERS) is True
    assert (s.state, s.disconnects) == ("disconnected", 1)


def test_a_normal_leave_is_not_a_disconnect():
    s = LogState()
    for k in ["join", "joined", "disconnect", "left"]:
        apply_line(s, LINES[k], MARKERS)
    assert (s.state, s.disconnects) == ("menu", 0)
    assert s.last_event == "left the game"


def test_tail_follows_the_newest_player_log(tmp_path):
    write_log(tmp_path, "old_Player_1.log", "join")
    os.utime(tmp_path / "old_Player_1.log", (time.time() - 100, time.time() - 100))
    (tmp_path / "Studio_2.log").write_text(LINES["disconnect"] + "\n")
    tail = LogTail(tmp_path)
    new = write_log(tmp_path, "new_Player_2.log", "join")
    restarted, lines = tail.read()
    assert restarted and lines == [LINES["join"]]
    # A half-written line waits for its end.
    with new.open("a") as f:
        f.write(LINES["joined"][:30])
    assert tail.read() == (False, [])
    with new.open("a") as f:
        f.write(LINES["joined"][30:] + "\n")
    assert tail.read() == (False, [LINES["joined"]])


async def test_status_while_afk(tmp_path):
    write_log(tmp_path, "a_Player_1.log", "join", "joined")
    now = at("2026-09-25T02:00:05.000Z")
    proc = FakeProc(started=now - 7200)
    w = Watcher(roots=[], settings=settings(tmp_path), processes=lambda: [proc], launch=lambda uri: None, clock=lambda: now)
    s = await w.refresh()
    assert s["state"] == "in_game"
    assert s["session_s"] == 3600
    assert s["client_up_s"] == 7200
    assert s["memory_mb"] == 900
    assert s["place_id"] == 606849621
    assert s["last_event"] == "joined the game"


async def test_closed_when_the_process_is_gone(tmp_path):
    write_log(tmp_path, "a_Player_1.log", "join", "joined")
    w = Watcher(roots=[], settings=settings(tmp_path), processes=lambda: [], launch=lambda uri: None)
    s = await w.refresh()
    assert s["state"] == "closed" and "session_s" not in s


async def test_rejoin_uses_the_last_place(tmp_path):
    launched = []
    w = Watcher(roots=[], settings=settings(tmp_path), processes=lambda: [], launch=launched.append)
    with pytest.raises(ActionError, match="no place to rejoin"):
        w.rejoin()
    write_log(tmp_path, "a_Player_1.log", "join")
    await w.refresh()
    assert w.rejoin() == {"rejoining": 606849621}
    assert launched == ["roblox://experiences/start?placeId=606849621"]


async def test_place_id_setting_wins(tmp_path):
    launched = []
    w = Watcher(roots=[], settings=settings(tmp_path, place_id=920587237), processes=lambda: [], launch=launched.append)
    w.rejoin()
    assert launched == ["roblox://experiences/start?placeId=920587237"]


async def test_close_forces_a_hung_client(tmp_path):
    proc = FakeProc(started=0, dies_on_terminate=False)
    w = Watcher(roots=[], settings=settings(tmp_path), processes=lambda: [proc], launch=lambda uri: None)
    assert await w.close(timeout=0.3) == {"closed": 1}
    assert proc.calls == ["terminate", "kill"]


async def test_auto_rejoin_after_a_disconnect_with_cooldown(tmp_path):
    procs = [FakeProc(started=0)]
    launched = []
    clock = [at("2026-09-25T01:21:00.000Z")]
    events = []
    w = Watcher(
        roots=[],
        settings=settings(tmp_path, auto_rejoin=True),
        processes=lambda: procs,
        launch=launched.append,
        clock=lambda: clock[0],
        emit=lambda kind, message, level="info", **data: events.append((kind, level, message)),
    )
    log = write_log(tmp_path, "a_Player_1.log", "join", "joined")
    await w.refresh()
    assert launched == []
    with log.open("a") as f:
        f.write(LINES["disconnect"] + "\n")
    await w.refresh()
    assert launched == ["roblox://experiences/start?placeId=606849621"]
    assert procs[0].calls == ["terminate"]
    assert w.snapshot["last_event"] == "disconnected from the game"
    assert events == [
        ("game.joined", "info", "Joined the game"),
        ("game.disconnected", "warn", "Disconnected from the game; rejoining"),
    ]
    # A second disconnect inside the cooldown doesn't rejoin again.
    procs[:] = [FakeProc(started=0)]
    clock[0] += 60
    with log.open("a") as f:
        f.write(LINES["join"] + "\n" + LINES["joined"] + "\n" + LINES["disconnect"] + "\n")
    await w.refresh()
    assert len(launched) == 1
    assert events[-1] == ("game.disconnected", "warn", "Disconnected from the game; not rejoining (rejoined recently)")


async def test_reports_the_client_closing_unless_we_closed_it(tmp_path):
    procs = [FakeProc(started=0)]
    events = []
    w = Watcher(
        roots=[],
        settings=settings(tmp_path),
        processes=lambda: procs,
        launch=lambda uri: None,
        emit=lambda kind, message, level="info", **data: events.append(kind),
    )
    await w.refresh()
    procs.clear()
    await w.refresh()
    assert events == ["client.closed"]
    procs.append(FakeProc(started=0))
    await w.refresh()
    await w.close(timeout=0.1)
    procs.clear()
    await w.refresh()
    assert events == ["client.closed"]


def test_manifest_and_handlers(monkeypatch):
    module_dir = Path(__file__).resolve().parent.parent
    monkeypatch.setenv("KERNEL_MODULE_DIR", str(module_dir))
    spec = importlib.util.spec_from_file_location("roblox_main", module_dir / "main.py")
    main = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(main)
    main.mod.validate()
    assert {a.id: a.ai for a in main.mod.manifest.actions} == {
        "client.rejoin": "confirm",
        "client.relaunch": "confirm",
        "window.show": "safe",
        "window.hide": "safe",
        "client.close": "confirm",
    }
    assert main.mod.settings["low_power"] is True and main.mod.settings["fps_cap"] == 30
