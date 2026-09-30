import json
import os
import time
from pathlib import Path

import pytest

from kernel_sdk import ActionError
from lowpower import LOW_POWER_FLAGS, apply_flags, newest_version_dir, set_framerate_cap
from test_watcher import LINES, FakeProc, FakeWindows, settings, write_log
from watcher import PRIORITIES, Watcher

SETTINGS_XML = """<roblox version="4">
<Item class="UserGameSettings">
<Properties>
<int name="FramerateCap">-1</int>
<bool name="Fullscreen">false</bool>
</Properties>
</Item>
</roblox>
"""


def install(root: Path, name: str, age_s: float) -> Path:
    d = root / name
    d.mkdir(parents=True)
    exe = d / "RobloxPlayerBeta.exe"
    exe.write_bytes(b"MZ")
    t = time.time() - age_s
    os.utime(exe, (t, t))
    return d


def test_finds_the_newest_installed_client(tmp_path):
    install(tmp_path / "Versions", "version-old", 1000)
    new = install(tmp_path / "Versions", "version-new", 10)
    (tmp_path / "Versions" / "version-studio").mkdir()
    assert newest_version_dir([tmp_path / "Versions", tmp_path / "missing"]) == new
    assert newest_version_dir([tmp_path / "missing"]) is None


def test_flags_merge_with_the_users_own(tmp_path):
    d = install(tmp_path, "version-a", 0)
    cfg = d / "ClientSettings" / "ClientAppSettings.json"
    cfg.parent.mkdir()
    cfg.write_text(json.dumps({"FFlagDebugGraphicsPreferVulkan": "True"}))
    assert apply_flags(d, True) is True
    data = json.loads(cfg.read_text())
    assert data["FFlagDebugGraphicsPreferVulkan"] == "True"
    assert all(data[k] == v for k, v in LOW_POWER_FLAGS.items())
    assert apply_flags(d, True) is False  # already there
    assert apply_flags(d, False) is True
    assert json.loads(cfg.read_text()) == {"FFlagDebugGraphicsPreferVulkan": "True"}


def test_turning_off_without_a_file_writes_nothing(tmp_path):
    d = install(tmp_path, "version-a", 0)
    assert apply_flags(d, False) is False
    assert not (d / "ClientSettings").exists()


@pytest.mark.parametrize("cap", [1, 30])
def test_framerate_cap(tmp_path, cap):
    xml = tmp_path / "GlobalBasicSettings_13.xml"
    xml.write_text(SETTINGS_XML)
    assert set_framerate_cap(xml, cap) == "set"
    assert f'<int name="FramerateCap">{cap}</int>' in xml.read_text()
    assert '<bool name="Fullscreen">false</bool>' in xml.read_text()
    assert set_framerate_cap(xml, cap) == "unchanged"


def test_framerate_cap_needs_roblox_to_have_saved_settings(tmp_path):
    assert "start it once" in set_framerate_cap(tmp_path / "nope.xml", 30)
    other = tmp_path / "other.xml"
    other.write_text("<roblox/>")
    assert "no frame rate" in set_framerate_cap(other, 30)


def low_power_watcher(tmp_path, procs, win=None, **over):
    versions = tmp_path / "Versions"
    install(versions, "version-a", 0)
    xml = tmp_path / "GlobalBasicSettings_13.xml"
    xml.write_text(SETTINGS_XML)
    logs = tmp_path / "logs"
    logs.mkdir()
    w = Watcher(
        settings(logs, low_power=True, **over),
        processes=lambda: procs,
        launch=lambda uri: None,
        windows=win,
        roots=[versions],
        global_settings=xml,
    )
    return w, logs, versions / "version-a", xml


async def test_prepares_the_next_launch_while_closed(tmp_path):
    w, logs, version, xml = low_power_watcher(tmp_path, [], FakeWindows(), fps_cap=1)
    s = await w.refresh()
    assert (s["flags"], s["fps_cap"], s["window"]) == ("applied", "set", "closed")
    assert '<int name="FramerateCap">1</int>' in xml.read_text()
    assert json.loads((version / "ClientSettings" / "ClientAppSettings.json").read_text()) == LOW_POWER_FLAGS


async def test_tunes_the_client_once_it_joins(tmp_path):
    procs = [FakeProc(started=0)]
    win = FakeWindows()
    w, logs, *_ = low_power_watcher(tmp_path, procs, win)
    log = write_log(logs, "a_Player_1.log", "join")
    await w.refresh()
    assert win.calls == [] and procs[0].calls == []  # still joining
    with log.open("a") as f:
        f.write(LINES["joined"] + "\n")
    s = await w.refresh()
    pid = procs[0].pid
    assert win.calls == [("efficiency", pid, True), ("hide", pid * 10)]
    assert procs[0].calls == [("nice", PRIORITIES["below_normal"])]
    assert (s["window"], s["tuning"]) == ("hidden", "applied")
    await w.refresh()
    assert len(win.calls) == 2  # once per client process

    assert w.set_window(True) == {"window": "shown"}
    assert win.calls[-1] == ("show", pid * 10)
    assert w.set_window(False) == {"window": "hidden"}


async def test_a_new_client_gets_tuned_again(tmp_path):
    procs = [FakeProc(started=0)]
    win = FakeWindows()
    w, logs, *_ = low_power_watcher(tmp_path, procs, win, cpu_cores=2, priority="idle")
    write_log(logs, "a_Player_1.log", "join", "joined")
    await w.refresh()
    procs[:] = []
    await w.refresh()  # closed: flags re-checked, tuning reset
    procs[:] = [FakeProc(started=0)]
    await w.refresh()
    assert ("hide", procs[0].pid * 10) in win.calls
    assert procs[0].calls == [("nice", PRIORITIES["idle"]), ("affinity", [0, 1])]


async def test_reports_what_windows_refused(tmp_path):
    procs = [FakeProc(started=0)]
    w, logs, *_ = low_power_watcher(tmp_path, procs, FakeWindows(efficiency_ok=False))
    write_log(logs, "a_Player_1.log", "join", "joined")
    assert (await w.refresh())["tuning"] == "efficiency mode refused"


async def test_off_means_hands_off(tmp_path):
    procs = [FakeProc(started=0)]
    win = FakeWindows()
    w, logs, version, xml = low_power_watcher(tmp_path, [], win)
    w.settings["low_power"] = False
    s = await w.refresh()
    assert s["flags"] == "off" and "fps_cap" not in s
    assert '<int name="FramerateCap">-1</int>' in xml.read_text()
    w.processes = lambda: procs
    write_log(logs, "a_Player_1.log", "join", "joined")
    await w.refresh()
    assert win.calls == [] and procs[0].calls == []


async def test_window_control_needs_windows(tmp_path):
    procs = [FakeProc(started=0)]
    w, logs, *_ = low_power_watcher(tmp_path, procs, None)
    w.win = None
    write_log(logs, "a_Player_1.log", "join", "joined")
    assert (await w.refresh())["window"] == "window control needs Windows"
    with pytest.raises(ActionError, match="only works on Windows"):
        w.set_window(True)


def test_rejects_a_bad_priority(tmp_path):
    with pytest.raises(ValueError, match="priority"):
        Watcher(settings(tmp_path, priority="realtime"), processes=lambda: [], roots=[])
