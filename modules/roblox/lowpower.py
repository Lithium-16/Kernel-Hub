"""Low-power AFK mode for the Roblox client, without touching Roblox's code.

- Graphics: only Fast Flags on Roblox's allowlist (anything else is ignored by the client since
  2025-09-29), written to `<version>\\ClientSettings\\ClientAppSettings.json` and merged with any
  flags already there. They take effect the next time Roblox starts.
- Frame rate: Roblox's own cap (`FramerateCap` in `GlobalBasicSettings_13.xml`, the same setting as
  its menu). Written only while Roblox is closed, because it rewrites that file when it exits.
- After joining: plain Windows window and process controls. Hide the window, lower the priority,
  Efficiency mode (EcoQoS), optionally fewer CPU cores. Nothing is injected into the client.
"""

from __future__ import annotations

import json
import os
import re
import sys
from pathlib import Path
from typing import Any, Protocol

EXE = "RobloxPlayerBeta.exe"

# From Roblox's Fast Flag allowlist (Rendering and Geometry). Values as strings, like Bloxstrap writes them.
LOW_POWER_FLAGS: dict[str, str] = {
    "DFFlagTextureQualityOverrideEnabled": "True",
    "DFIntTextureQualityOverride": "0",
    "DFIntDebugFRMQualityLevelOverride": "1",
    "FIntDebugForceMSAASamples": "1",
    "FFlagDebugSkyGray": "True",
    "DFFlagDebugPauseVoxelizer": "True",
    "FIntFRMMinGrassDistance": "0",
    "FIntFRMMaxGrassDistance": "0",
}

FRAMERATE_CAP = re.compile(r'(<int name="FramerateCap">)(-?\d+)(</int>)')


def version_roots() -> list[Path]:
    roots = []
    if local := os.environ.get("LOCALAPPDATA"):
        roots.append(Path(local) / "Roblox" / "Versions")
    for env in ("ProgramFiles(x86)", "ProgramFiles"):
        if base := os.environ.get(env):
            roots.append(Path(base) / "Roblox" / "Versions")
    return roots


def newest_version_dir(roots: list[Path]) -> Path | None:
    """The installed client folder Roblox will start from: the newest one with the player exe."""
    found = []
    for root in roots:
        try:
            found += [d for d in root.iterdir() if (d / EXE).is_file()]
        except OSError:
            continue
    return max(found, key=lambda d: (d / EXE).stat().st_mtime, default=None)


def apply_flags(version_dir: Path, enabled: bool) -> bool:
    """Add (or, when disabled, remove) our flags, keeping everyone else's. True if the file changed."""
    path = version_dir / "ClientSettings" / "ClientAppSettings.json"
    try:
        current = json.loads(path.read_text(encoding="utf-8")) if path.is_file() else {}
        if not isinstance(current, dict):
            current = {}
    except (OSError, ValueError):
        current = {}
    wanted = dict(current)
    for key, value in LOW_POWER_FLAGS.items():
        if enabled:
            wanted[key] = value
        elif wanted.get(key) == value:
            del wanted[key]
    if wanted == current and (path.is_file() or not wanted):
        return False
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(wanted, indent=2) + "\n", encoding="utf-8")
    return True


def global_settings_path() -> Path:
    return Path(os.environ.get("LOCALAPPDATA", str(Path.home()))) / "Roblox" / "GlobalBasicSettings_13.xml"


def set_framerate_cap(path: Path, cap: int) -> str:
    """Returns what happened: "set", "unchanged", or why it couldn't."""
    try:
        text = path.read_text(encoding="utf-8")
    except OSError:
        return "Roblox hasn't saved its settings yet (start it once)"
    match = FRAMERATE_CAP.search(text)
    if not match:
        return "no frame rate setting in Roblox's settings file"
    if match[2] == str(cap):
        return "unchanged"
    path.write_text(FRAMERATE_CAP.sub(rf"\g<1>{cap}\g<3>", text, count=1), encoding="utf-8")
    return "set"


class WindowControl(Protocol):
    def windows(self, pid: int) -> list[int]: ...
    def hide(self, hwnd: int) -> None: ...
    def show(self, hwnd: int) -> None: ...
    def efficiency(self, pid: int, on: bool) -> bool: ...


class Win32:
    """The few user32/kernel32 calls needed, through ctypes (Windows only)."""

    SW_HIDE, SW_SHOW, SW_RESTORE = 0, 5, 9
    GW_OWNER = 4
    PROCESS_SET_INFORMATION = 0x0200
    PROCESS_POWER_THROTTLING = 4  # PROCESS_INFORMATION_CLASS.ProcessPowerThrottling
    THROTTLE_EXECUTION_SPEED = 0x1

    def __init__(self) -> None:
        import ctypes
        from ctypes import wintypes

        self.ctypes, self.wintypes = ctypes, wintypes
        self.user32 = ctypes.WinDLL("user32", use_last_error=True)
        self.kernel32 = ctypes.WinDLL("kernel32", use_last_error=True)
        self.enum_proc = ctypes.WINFUNCTYPE(wintypes.BOOL, wintypes.HWND, wintypes.LPARAM)
        u, k = self.user32, self.kernel32
        u.EnumWindows.argtypes = [self.enum_proc, wintypes.LPARAM]
        u.GetWindowThreadProcessId.argtypes = [wintypes.HWND, ctypes.POINTER(wintypes.DWORD)]
        u.GetWindow.argtypes = [wintypes.HWND, wintypes.UINT]
        u.GetWindow.restype = wintypes.HWND
        u.GetWindowTextLengthW.argtypes = [wintypes.HWND]
        u.ShowWindow.argtypes = [wintypes.HWND, ctypes.c_int]
        u.SetForegroundWindow.argtypes = [wintypes.HWND]
        k.OpenProcess.argtypes = [wintypes.DWORD, wintypes.BOOL, wintypes.DWORD]
        k.OpenProcess.restype = wintypes.HANDLE
        k.SetProcessInformation.argtypes = [wintypes.HANDLE, ctypes.c_int, ctypes.c_void_p, wintypes.DWORD]
        k.CloseHandle.argtypes = [wintypes.HANDLE]

        class ThrottleState(ctypes.Structure):
            _fields_ = [("Version", wintypes.ULONG), ("ControlMask", wintypes.ULONG), ("StateMask", wintypes.ULONG)]

        self.ThrottleState = ThrottleState

    def windows(self, pid: int) -> list[int]:
        """Top-level, titled windows of `pid` (visible or hidden)."""
        found: list[int] = []
        wt = self.wintypes

        def each(hwnd: Any, _lparam: Any) -> bool:
            owner = wt.DWORD()
            self.user32.GetWindowThreadProcessId(hwnd, self.ctypes.byref(owner))
            if (
                owner.value == pid
                and not self.user32.GetWindow(hwnd, self.GW_OWNER)
                and self.user32.GetWindowTextLengthW(hwnd) > 0
            ):
                found.append(int(hwnd))
            return True

        self.user32.EnumWindows(self.enum_proc(each), 0)
        return found

    def hide(self, hwnd: int) -> None:
        self.user32.ShowWindow(hwnd, self.SW_HIDE)

    def show(self, hwnd: int) -> None:
        self.user32.ShowWindow(hwnd, self.SW_SHOW)
        self.user32.ShowWindow(hwnd, self.SW_RESTORE)
        self.user32.SetForegroundWindow(hwnd)

    def efficiency(self, pid: int, on: bool) -> bool:
        """Windows 11 Efficiency mode's EcoQoS half (the priority half is set separately)."""
        handle = self.kernel32.OpenProcess(self.PROCESS_SET_INFORMATION, False, pid)
        if not handle:
            return False
        try:
            state = self.ThrottleState(1, self.THROTTLE_EXECUTION_SPEED, self.THROTTLE_EXECUTION_SPEED if on else 0)
            return bool(
                self.kernel32.SetProcessInformation(
                    handle, self.PROCESS_POWER_THROTTLING, self.ctypes.byref(state), self.ctypes.sizeof(state)
                )
            )
        finally:
            self.kernel32.CloseHandle(handle)


def window_control() -> WindowControl | None:
    return Win32() if sys.platform == "win32" else None
