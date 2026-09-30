import importlib.util
from pathlib import Path

import pytest

from kernel_sdk import ActionError
from vram import MB, App, Manager, Offline
from winvram import parse_instances

GB = 1024 * MB

APPS = [
    {"name": "Roblox", "kind": "watch", "process": "RobloxPlayerBeta.exe", "priority": 100},
    {"name": "ComfyUI", "kind": "comfyui", "url": "http://127.0.0.1:8188", "priority": 50, "exclusive": True},
    {"name": "Ollama", "kind": "ollama", "url": "http://127.0.0.1:11434", "priority": 30},
    {"name": "LM Studio", "kind": "process", "process": ["LM Studio.exe"], "priority": 20, "stop_when_needed": True},
]


BASE = {
    "auto": True,
    "min_free_mb": 1024,
    "after_s": 15,
    "poll_s": 3.0,
    "llm_max_wait_s": 90,
    "image_max_wait_s": 600,
    "llm_reserve_mb": 6144,
    "proxy_host": "127.0.0.1",
    "retry_out_of_memory": True,
}


class FakeGpu:
    def __init__(self):
        self.used, self.total = 6 * GB, 11 * GB
        self.pids = {100: int(0.8 * GB), 300: 2 * GB}

    def totals(self):
        return self.used, self.total

    def per_process(self):
        return dict(self.pids)


class World:
    """Fake Ollama and ComfyUI APIs and the process list."""

    def __init__(self):
        self.ollama = [{"name": "qwen3:8b", "size_vram": int(5.2 * GB)}]
        self.comfy_jobs = 0
        self.comfy_vram = 1 * GB
        self.comfy_up = True
        self.calls = []
        self.stopped = []
        self.procs = {"robloxplayerbeta.exe": [100], "lm studio.exe": [300]}

    def fetch(self, method, url, body, timeout):
        self.calls.append((method, url.split("/", 3)[-1], body))
        if ":11434" in url:
            if url.endswith("/api/ps"):
                return {"models": self.ollama}
            if url.endswith("/api/generate"):
                self.ollama = [m for m in self.ollama if m["name"] != body["model"]]
                return {"done": True, "done_reason": "unload"}
        if ":8188" in url:
            if not self.comfy_up:
                raise Offline("refused")
            if url.endswith("/system_stats"):
                return {"devices": [{"torch_vram_total": self.comfy_vram}]}
            if url.endswith("/queue"):
                return {"queue_running": [[1, "p"]] * min(self.comfy_jobs, 1), "queue_pending": []}
            if url.endswith("/free"):
                self.comfy_vram = 0
                return {}
        raise AssertionError(url)

    def stop(self, pids):
        self.stopped += pids
        return len(pids)


def make(**over):
    world, gpu, events, now = World(), FakeGpu(), [], [0.0]
    settings = {**BASE, "apps": APPS, **over}
    m = Manager(
        settings,
        gpu=gpu,
        emit=lambda kind, message, level="info", **data: events.append((kind, level, message)),
        fetch=world.fetch,
        processes=lambda: world.procs,
        clock=lambda: now[0],
        stop_process=world.stop,
    )
    return m, world, gpu, events, now


def unloads(world):
    return [(u, b) for m, u, b in world.calls if m == "POST"]


def test_app_settings_are_checked():
    assert App.parse(APPS[3]).process == ("lm studio.exe",)
    for bad in (
        {"name": "X", "kind": "gpu"},
        {"name": "X", "kind": "process"},
        {"name": "X", "kind": "ollama", "url": "localhost"},
        {"name": "X", "kind": "watch", "process": "a.exe", "colour": "red"},
        {"kind": "watch", "process": "a.exe"},
    ):
        with pytest.raises(ValueError):
            App.parse(bad)
    with pytest.raises(ValueError, match="different"):
        make(apps=[APPS[0], APPS[0]])


async def test_status_shows_each_app():
    m, *_ = make()
    s = await m.refresh()
    rows = {r["app"]: r for r in s["apps"]}
    assert (s["vram_used_mb"], s["vram_total_mb"], s["vram_free_mb"], s["state"]) == (6144, 11264, 5120, "ok")
    assert rows["Roblox"]["vram_mb"] == 819 and rows["Roblox"]["state"] == "busy"
    assert rows["Ollama"] == {"app": "Ollama", "vram_mb": 5324, "state": "loaded", "priority": 30, "detail": "qwen3:8b"}
    assert rows["ComfyUI"]["state"] == "loaded"
    assert rows["LM Studio"]["state"] == "busy"
    assert [r["app"] for r in s["apps"]] == ["Roblox", "ComfyUI", "Ollama", "LM Studio", "Everything else"]


async def test_a_busy_exclusive_app_clears_the_ones_below_it():
    m, world, gpu, events, now = make()
    world.comfy_jobs = 1
    gpu.pids.pop(300)  # LM Studio idle
    await m.refresh()
    await m.balance()
    assert unloads(world) == [("api/generate", {"model": "qwen3:8b", "keep_alive": 0})]
    assert events == [("vram.freed", "info", "Unloaded Ollama (qwen3:8b), 5.2 GB, to make room for ComfyUI")]
    assert world.stopped == []  # LM Studio held no VRAM
    # Reloaded straight away: left alone until the cooldown passes.
    world.ollama = [{"name": "qwen3:8b", "size_vram": 5 * GB}]
    await m.refresh()
    await m.balance()
    assert len(unloads(world)) == 1
    now[0] = 31
    await m.refresh()
    await m.balance()
    assert len(unloads(world)) == 2


async def test_low_vram_unloads_the_lowest_idle_app_after_a_while():
    m, world, gpu, events, now = make()
    gpu.used = int(10.5 * GB)
    gpu.pids.pop(300)
    world.procs.pop("lm studio.exe")
    await m.refresh()
    await m.balance()
    assert unloads(world) == []  # not for long enough yet
    now[0] = 16
    await m.refresh()
    await m.balance()
    assert unloads(world) == [("api/generate", {"model": "qwen3:8b", "keep_alive": 0})]
    now[0] = 40
    await m.refresh()
    await m.balance()
    assert unloads(world)[-1] == ("free", {"unload_models": True, "free_memory": True})
    now[0] = 80
    await m.refresh()
    await m.balance()
    assert events[-1][:2] == ("vram.low", "warn")
    await m.balance()
    assert [e[0] for e in events].count("vram.low") == 1


async def test_busy_comfyui_is_never_interrupted():
    m, world, *_ = make()
    world.comfy_jobs = 1
    r = await m.focus("ollama")
    assert "ComfyUI" not in r["freed_mb"]
    assert world.stopped == [300]  # LM Studio is marked stop_when_needed
    assert ("free", {"unload_models": True, "free_memory": True}) not in unloads(world)


async def test_manual_actions():
    m, world, *_ = make(auto=False)
    await m.refresh()
    await m.balance()
    assert unloads(world) == []  # auto off: buttons only
    r = await m.free_idle()
    assert set(r["freed_mb"]) == {"Ollama", "ComfyUI"}
    assert world.stopped == []  # free_idle never closes apps
    with pytest.raises(ActionError) as e:
        await m.free_one("roblox")
    assert e.value.code == "not_permitted"
    assert (await m.free_one("Ollama"))["why"] == "it has nothing loaded"
    with pytest.raises(ActionError, match="Known: Roblox"):
        await m.free_one("Steam")
    assert m.set_auto(True) == {"auto": True}


async def test_offline_apps_are_off():
    m, world, *_ = make()
    world.comfy_up = False
    s = await m.refresh()
    assert {r["app"]: r["state"] for r in s["apps"]}["ComfyUI"] == "off"


def test_windows_counter_instances():
    items = [
        ("pid_1234_luid_0x00000000_0x0000C2F1_phys_0", 3 * GB),
        ("pid_1234_luid_0x00000000_0x0000C2F1_phys_1", 1 * GB),
        ("pid_88_luid_0x00000000_0x0000C2F1_phys_0", 0),
        ("_Total", 9 * GB),
    ]
    assert parse_instances(items) == {1234: 4 * GB}


def test_manifest_and_handlers(monkeypatch):
    module_dir = Path(__file__).resolve().parent.parent
    monkeypatch.setenv("KERNEL_MODULE_DIR", str(module_dir))
    spec = importlib.util.spec_from_file_location("vram_main", module_dir / "main.py")
    main = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(main)
    main.mod.validate()
    assert [a.name for a in main.manager.apps] == ["Roblox", "ComfyUI", "Ollama"]
    assert {a.id: a.ai for a in main.mod.manifest.actions}["vram.focus"] == "confirm"
