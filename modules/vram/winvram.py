"""VRAM per process.

On Windows, NVML can't see per-process memory (the WDDM driver owns it), so this reads the
same counter Task Manager shows: `\\GPU Process Memory(*)\\Dedicated Usage`, through PDH.
Elsewhere it asks NVML.
"""

from __future__ import annotations

import re
import sys
from collections.abc import Callable
from typing import Any

INSTANCE_PID = re.compile(r"^pid_(\d+)_")
COUNTER = "\\GPU Process Memory(*)\\Dedicated Usage"
PDH_FMT_LARGE = 0x00000400
PDH_MORE_DATA = 0x800007D2


def parse_instances(items: list[tuple[str, int]]) -> dict[int, int]:
    """Counter instances look like `pid_1234_luid_0x..._phys_0`; one process may have several."""
    out: dict[int, int] = {}
    for name, value in items:
        m = INSTANCE_PID.match(name)
        if m and value > 0:
            pid = int(m.group(1))
            out[pid] = out.get(pid, 0) + int(value)
    return out


class Pdh:
    """The few PDH calls needed, through ctypes (Windows only)."""

    def __init__(self) -> None:
        import ctypes
        from ctypes import wintypes

        self.ctypes = ctypes
        pdh = ctypes.WinDLL("pdh")
        self.pdh = pdh

        class Value(ctypes.Union):
            _fields_ = [("longValue", ctypes.c_long), ("doubleValue", ctypes.c_double), ("largeValue", ctypes.c_longlong)]

        class FmtValue(ctypes.Structure):
            _fields_ = [("CStatus", wintypes.DWORD), ("value", Value)]

        class Item(ctypes.Structure):
            _fields_ = [("szName", wintypes.LPWSTR), ("FmtValue", FmtValue)]

        self.Item = Item
        pdh.PdhOpenQueryW.argtypes = [wintypes.LPCWSTR, ctypes.c_void_p, ctypes.POINTER(wintypes.HANDLE)]
        pdh.PdhOpenQueryW.restype = wintypes.DWORD
        pdh.PdhAddEnglishCounterW.argtypes = [
            wintypes.HANDLE,
            wintypes.LPCWSTR,
            ctypes.c_void_p,
            ctypes.POINTER(wintypes.HANDLE),
        ]
        pdh.PdhAddEnglishCounterW.restype = wintypes.DWORD
        pdh.PdhCollectQueryData.argtypes = [wintypes.HANDLE]
        pdh.PdhCollectQueryData.restype = wintypes.DWORD
        pdh.PdhGetFormattedCounterArrayW.argtypes = [
            wintypes.HANDLE,
            wintypes.DWORD,
            ctypes.POINTER(wintypes.DWORD),
            ctypes.POINTER(wintypes.DWORD),
            ctypes.c_void_p,
        ]
        pdh.PdhGetFormattedCounterArrayW.restype = wintypes.DWORD

        self.query, self.counter = wintypes.HANDLE(), wintypes.HANDLE()
        if pdh.PdhOpenQueryW(None, None, ctypes.byref(self.query)) != 0:
            raise OSError("PdhOpenQuery failed")
        if pdh.PdhAddEnglishCounterW(self.query, COUNTER, None, ctypes.byref(self.counter)) != 0:
            raise OSError("this Windows has no GPU Process Memory counters")
        self.wintypes = wintypes

    def __call__(self) -> dict[int, int]:
        ct, wt = self.ctypes, self.wintypes
        if self.pdh.PdhCollectQueryData(self.query) != 0:
            return {}
        size, count = wt.DWORD(0), wt.DWORD(0)
        status = self.pdh.PdhGetFormattedCounterArrayW(
            self.counter, PDH_FMT_LARGE, ct.byref(size), ct.byref(count), None
        )
        if status != PDH_MORE_DATA or not size.value:
            return {}
        buf = ct.create_string_buffer(size.value)
        status = self.pdh.PdhGetFormattedCounterArrayW(
            self.counter, PDH_FMT_LARGE, ct.byref(size), ct.byref(count), buf
        )
        if status != 0:
            return {}
        items = ct.cast(buf, ct.POINTER(self.Item))
        pairs = []
        for i in range(count.value):
            item = items[i]
            if item.FmtValue.CStatus == 0 and item.szName:
                pairs.append((item.szName, item.FmtValue.value.largeValue))
        return parse_instances(pairs)


def nvml_reader(nvml: Any, handle: Any) -> Callable[[], dict[int, int]]:
    def read() -> dict[int, int]:
        out: dict[int, int] = {}
        if nvml is None or handle is None:
            return out
        for fn in ("nvmlDeviceGetComputeRunningProcesses", "nvmlDeviceGetGraphicsRunningProcesses"):
            try:
                procs = getattr(nvml, fn)(handle)
            except Exception:
                continue
            for p in procs:
                used = getattr(p, "usedGpuMemory", None)
                if isinstance(used, int) and 0 < used < 2**60:
                    out[p.pid] = max(out.get(p.pid, 0), used)
        return out

    return read


def reader(nvml: Any, handle: Any) -> Callable[[], dict[int, int]]:
    if sys.platform == "win32":
        try:
            return Pdh()
        except Exception:  # no counters (old Windows, no GPU driver): fall back to NVML
            pass
    return nvml_reader(nvml, handle)
