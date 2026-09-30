import asyncio
import importlib.util
import socket
from pathlib import Path
from types import SimpleNamespace

import pytest

from kernel_sdk import ActionError
from pcmon import Gpu, Monitor, magic_packet, parse_mac, parse_targets, power_commands

MAC = "AA:BB:CC:DD:EE:0F"


def settings(**over):
    base = {
        "wake_targets": [f"Pluto={MAC}"],
        "wake_broadcast": "127.0.0.1",
        "sample_s": 2.0,
        "power_delay_s": 60,
        "disks": [],
    }
    return {**base, **over}


class Recorder:
    def __init__(self, code=0):
        self.calls = []
        self.code = code

    def __call__(self, argv):
        self.calls.append(argv)
        return self.code, ""


class FakeNvml:
    NVML_TEMPERATURE_GPU = 0

    def nvmlInit(self):
        pass

    def nvmlDeviceGetCount(self):
        return 1

    def nvmlDeviceGetHandleByIndex(self, i):
        return "gpu0"

    def nvmlDeviceGetUtilizationRates(self, h):
        return SimpleNamespace(gpu=37)

    def nvmlDeviceGetMemoryInfo(self, h):
        return SimpleNamespace(used=3 * 1024**3, total=11 * 1024**3)

    def nvmlDeviceGetTemperature(self, h, kind):
        return 64

    def nvmlDeviceGetName(self, h):
        return b"NVIDIA GeForce GTX 1080 Ti"


class NoNvml(FakeNvml):
    def nvmlInit(self):
        raise RuntimeError("NVML library not found")


def test_macs_and_targets():
    assert parse_mac("aa-bb-cc-dd-ee-0f") == bytes.fromhex("aabbccddee0f")
    assert parse_mac("AABB.CCDD.EE0F") == bytes.fromhex("aabbccddee0f")
    for bad in ["", "AA:BB:CC", "GG:BB:CC:DD:EE:FF", "AA:BB:CC:DD:EE:FF:00"]:
        with pytest.raises(ActionError):
            parse_mac(bad)
    assert parse_targets([f" Pluto = {MAC}"]) == {"pluto": bytes.fromhex("aabbccddee0f")}
    with pytest.raises(ValueError, match="name=AA"):
        parse_targets(["just-a-mac"])


def test_magic_packet_shape():
    mac = bytes.fromhex("aabbccddee0f")
    p = magic_packet(mac)
    assert len(p) == 102 and p[:6] == b"\xff" * 6 and p[6:] == mac * 16


def test_wake_sends_a_real_packet():
    with socket.socket(socket.AF_INET, socket.SOCK_DGRAM) as rx:
        rx.bind(("127.0.0.1", 0))
        rx.settimeout(2)
        port = rx.getsockname()[1]
        m = Monitor(settings(), runner=Recorder(), gpu=Gpu(NoNvml()))
        import pcmon

        original = pcmon.send_magic_packet
        pcmon.send_magic_packet = lambda mac, bcast: original(mac, bcast, port)
        try:
            r = m.wake("PLUTO")
            data, _ = rx.recvfrom(200)
        finally:
            pcmon.send_magic_packet = original
    assert data == magic_packet(bytes.fromhex("aabbccddee0f"))
    assert r["sent_to"] == MAC


def test_wake_rejects_unknown_names():
    m = Monitor(settings(), runner=Recorder(), gpu=Gpu(NoNvml()))
    with pytest.raises(ActionError, match="Known: pluto"):
        m.wake("mars")


@pytest.mark.parametrize(
    ("platform", "restart", "cancel"),
    [
        ("win32", ["shutdown", "/r", "/t", "60"], ["shutdown", "/a"]),
        ("linux", ["shutdown", "-r", "+1"], ["shutdown", "-c"]),
    ],
)
def test_power_commands(platform, restart, cancel):
    cmds = power_commands(platform, 60)
    assert cmds["restart"][: len(restart)] == restart
    assert cmds["cancel"] == cancel


def test_restart_can_be_cancelled():
    rec = Recorder()
    m = Monitor(settings(), runner=rec, platform="win32", gpu=Gpu(NoNvml()))
    assert m.schedule("restart") == {"scheduled": "restart", "in_s": 60}
    assert m.sample()["power"].startswith("restart in 60s")
    assert m.cancel()["cancelled"] is True
    assert "power" not in m.sample()
    assert [c[:2] for c in rec.calls] == [["shutdown", "/r"], ["shutdown", "/a"]]


def test_failed_power_command_is_an_error():
    m = Monitor(settings(), runner=Recorder(code=1), platform="linux", gpu=Gpu(NoNvml()))
    with pytest.raises(ActionError) as e:
        m.schedule("shutdown")
    assert e.value.code == "module_failed"
    assert m.cancel() == {"cancelled": False, "message": "nothing was scheduled"}


async def test_sleep_replies_before_sleeping():
    rec = Recorder()
    m = Monitor(settings(), runner=rec, platform="win32", gpu=Gpu(NoNvml()))
    assert await m.sleep() == {"sleeping_in_s": 2}
    assert rec.calls == []
    await asyncio.sleep(2.3)
    assert rec.calls == [["rundll32.exe", "powrprof.dll,SetSuspendState", "0,1,0"]]


def test_sample_with_a_gpu():
    m = Monitor(settings(), runner=Recorder(), gpu=Gpu(FakeNvml()))
    s = m.sample()
    assert s["gpu"] == "NVIDIA GeForce GTX 1080 Ti"
    assert (s["gpu_pct"], s["gpu_temp_c"], s["vram_used_mb"], s["vram_total_mb"]) == (37, 64, 3072, 11264)
    assert 0 <= s["cpu_pct"] <= 100
    assert s["memory_total_mb"] >= s["memory_used_mb"] > 0
    assert s["uptime_s"] > 0
    assert s["net_down_bps"] is None  # needs two samples
    assert isinstance(m.sample()["net_down_bps"], int)
    assert all({"disk", "used_pct", "free_bytes", "total_bytes"} <= set(d) for d in s["disks"])


def test_no_gpu_means_no_gpu_fields():
    s = Monitor(settings(), runner=Recorder(), gpu=Gpu(NoNvml())).sample()
    assert not any(k.startswith(("gpu", "vram")) for k in s)


def test_disk_filter():
    m = Monitor(settings(disks=["/"]), runner=Recorder(), gpu=Gpu(NoNvml()))
    assert [d["disk"] for d in m.disks()] in ([], ["/"])


def test_manifest_and_handlers(monkeypatch):
    # Loaded by path: every module has a main.py, so `import main` would get whichever came first.
    module_dir = Path(__file__).resolve().parent.parent
    monkeypatch.setenv("KERNEL_MODULE_DIR", str(module_dir))
    spec = importlib.util.spec_from_file_location("pc_monitor_main", module_dir / "main.py")
    main = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(main)
    main.mod.validate()
    tiers = {a.id: a.ai for a in main.mod.manifest.actions}
    assert tiers == {
        "power.sleep": "confirm",
        "power.restart": "confirm",
        "power.shutdown": "confirm",
        "power.cancel": "safe",
        "power.wake": "safe",
    }
