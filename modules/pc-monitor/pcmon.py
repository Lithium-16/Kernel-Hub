"""PC monitor: live stats with psutil (and NVML for NVIDIA GPUs), power actions, Wake-on-LAN."""

from __future__ import annotations

import asyncio
import re
import socket
import subprocess
import sys
import time
from collections.abc import Callable
from typing import Any

import psutil

from kernel_sdk import ActionError

MAC = re.compile(r"^[0-9a-f]{12}$")
Runner = Callable[[list[str]], tuple[int, str]]


def run_command(argv: list[str]) -> tuple[int, str]:
    flags = getattr(subprocess, "CREATE_NO_WINDOW", 0)
    p = subprocess.run(argv, capture_output=True, text=True, creationflags=flags, check=False)
    return p.returncode, (p.stdout + p.stderr).strip()


def parse_mac(text: str) -> bytes:
    """Accepts AA:BB:CC:DD:EE:FF, AA-BB-..., AABB.CCDD.EEFF or plain hex."""
    hex_digits = re.sub(r"[:\-.\s]", "", text.strip().lower())
    if not MAC.match(hex_digits):
        raise ActionError("invalid_params", f"'{text}' is not a MAC address")
    return bytes.fromhex(hex_digits)


def parse_targets(entries: list[str]) -> dict[str, bytes]:
    """`["pluto=AA:BB:CC:DD:EE:FF"]` -> `{"pluto": b"..."}`. Bad entries are a settings error."""
    targets: dict[str, bytes] = {}
    for entry in entries:
        name, sep, mac = str(entry).partition("=")
        if not sep or not name.strip():
            raise ValueError(f"wake_targets entry '{entry}' should look like name=AA:BB:CC:DD:EE:FF")
        targets[name.strip().lower()] = parse_mac(mac)
    return targets


def magic_packet(mac: bytes) -> bytes:
    return b"\xff" * 6 + mac * 16


def send_magic_packet(mac: bytes, broadcast: str, port: int = 9) -> None:
    with socket.socket(socket.AF_INET, socket.SOCK_DGRAM) as s:
        s.setsockopt(socket.SOL_SOCKET, socket.SO_BROADCAST, 1)
        s.sendto(magic_packet(mac), (broadcast, port))


def power_commands(platform: str, delay_s: int) -> dict[str, list[str]]:
    if platform == "win32":
        note = f"Kernel: this PC will go down in {delay_s} seconds. Cancel it from Kernel."
        return {
            # Sleeps, or hibernates if hibernation is enabled (Windows decides).
            "sleep": ["rundll32.exe", "powrprof.dll,SetSuspendState", "0,1,0"],
            "restart": ["shutdown", "/r", "/t", str(delay_s), "/c", note],
            "shutdown": ["shutdown", "/s", "/t", str(delay_s), "/c", note],
            "cancel": ["shutdown", "/a"],
        }
    minutes = f"+{max(1, round(delay_s / 60))}"
    return {
        "sleep": ["systemctl", "suspend"],
        "restart": ["shutdown", "-r", minutes],
        "shutdown": ["shutdown", "-h", minutes],
        "cancel": ["shutdown", "-c"],
    }


class Gpu:
    """First NVIDIA GPU through NVML, or nothing when there isn't one."""

    def __init__(self, nvml: Any = None) -> None:
        self.handle = None
        self.nvml = nvml
        try:
            if self.nvml is None:
                import pynvml

                self.nvml = pynvml
            self.nvml.nvmlInit()
            if self.nvml.nvmlDeviceGetCount() > 0:
                self.handle = self.nvml.nvmlDeviceGetHandleByIndex(0)
        except Exception:
            self.handle = None

    def sample(self) -> dict[str, Any]:
        if self.handle is None:
            return {}
        n = self.nvml
        try:
            util = n.nvmlDeviceGetUtilizationRates(self.handle)
            mem = n.nvmlDeviceGetMemoryInfo(self.handle)
            temp = n.nvmlDeviceGetTemperature(self.handle, n.NVML_TEMPERATURE_GPU)
            name = n.nvmlDeviceGetName(self.handle)
        except Exception:
            return {}
        return {
            "gpu": name.decode() if isinstance(name, bytes) else str(name),
            "gpu_pct": int(util.gpu),
            "gpu_temp_c": int(temp),
            "vram_used_mb": mem.used // (1024 * 1024),
            "vram_total_mb": mem.total // (1024 * 1024),
        }


class Monitor:
    def __init__(
        self,
        settings: dict[str, Any],
        runner: Runner = run_command,
        platform: str = sys.platform,
        gpu: Gpu | None = None,
    ) -> None:
        self.settings = settings
        self.targets = parse_targets(settings["wake_targets"])
        self.runner = runner
        self.commands = power_commands(platform, int(settings["power_delay_s"]))
        self.gpu = gpu if gpu is not None else Gpu()
        self.snapshot: dict[str, Any] = {}
        self.pending: str | None = None
        self._net: tuple[float, int, int] | None = None
        psutil.cpu_percent(interval=None)  # the first call only primes the counter

    # -- status ---------------------------------------------------------------------------

    def disks(self) -> list[dict[str, Any]]:
        wanted = [d.lower().rstrip("\\/") for d in self.settings["disks"]]
        out = []
        for part in psutil.disk_partitions(all=False):
            mount = part.mountpoint
            if wanted and mount.lower().rstrip("\\/") not in wanted:
                continue
            if not wanted and ("cdrom" in part.opts or part.fstype in ("", "squashfs", "tmpfs", "overlay")):
                continue
            try:
                u = psutil.disk_usage(mount)
            except OSError:
                continue
            out.append({"disk": mount, "used_pct": round(u.percent, 1), "free_bytes": u.free, "total_bytes": u.total})
        return out

    def sample(self) -> dict[str, Any]:
        mem = psutil.virtual_memory()
        net = psutil.net_io_counters()
        now = time.monotonic()
        down = up = None
        if self._net is not None and net is not None:
            t, rx, tx = self._net
            dt = max(now - t, 1e-3)
            down, up = int((net.bytes_recv - rx) / dt), int((net.bytes_sent - tx) / dt)
        if net is not None:
            self._net = (now, net.bytes_recv, net.bytes_sent)
        snap: dict[str, Any] = {
            "cpu_pct": round(psutil.cpu_percent(interval=None), 1),
            "cores": psutil.cpu_count(),
            "memory_used_mb": (mem.total - mem.available) // (1024 * 1024),
            "memory_total_mb": mem.total // (1024 * 1024),
            **self.gpu.sample(),
            "net_down_bps": down,
            "net_up_bps": up,
            "uptime_s": int(time.time() - psutil.boot_time()),
            "host": socket.gethostname(),
            "disks": self.disks(),
        }
        if self.pending:
            snap["power"] = self.pending
        self.snapshot = snap
        return snap

    async def poll(self) -> None:
        while True:
            self.sample()
            await asyncio.sleep(self.settings["sample_s"])

    # -- power ----------------------------------------------------------------------------

    def _run(self, which: str) -> str:
        code, out = self.runner(self.commands[which])
        if code != 0:
            raise ActionError("module_failed", out or f"{self.commands[which][0]} exited with {code}")
        return out

    async def sleep(self) -> dict[str, Any]:
        # Reply first: once the PC is asleep, nothing gets back to the app.
        async def later() -> None:
            await asyncio.sleep(2)
            await asyncio.to_thread(self._run, "sleep")

        asyncio.get_running_loop().create_task(later())
        return {"sleeping_in_s": 2}

    def schedule(self, which: str) -> dict[str, Any]:
        self._run(which)
        delay = int(self.settings["power_delay_s"])
        self.pending = f"{which} in {delay}s (scheduled {time.strftime('%H:%M:%S')})"
        return {"scheduled": which, "in_s": delay}

    def cancel(self) -> dict[str, Any]:
        code, out = self.runner(self.commands["cancel"])
        was = self.pending
        self.pending = None
        if code != 0 and not was:
            return {"cancelled": False, "message": "nothing was scheduled"}
        return {"cancelled": True, "was": was}

    # -- wake-on-lan ----------------------------------------------------------------------

    def wake(self, target: str) -> dict[str, Any]:
        key = target.strip().lower()
        if key in self.targets:
            mac = self.targets[key]
        else:
            try:
                mac = parse_mac(target)
            except ActionError:
                known = ", ".join(sorted(self.targets)) or "none set up (wake_targets)"
                raise ActionError("invalid_params", f"unknown PC '{target}'. Known: {known}") from None
        send_magic_packet(mac, self.settings["wake_broadcast"])
        return {"sent_to": ":".join(f"{b:02X}" for b in mac), "via": self.settings["wake_broadcast"]}
