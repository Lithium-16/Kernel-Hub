"""OpenFork: the shared game hosting (kernel_sdk.webgame) with OpenFork's Kernel host."""

from pathlib import Path
from typing import Any

from kernel_sdk.webgame import WebGame

HOST_FILE = Path(__file__).resolve().parent / "kernel-host.ts"


class OpenFork(WebGame):
    def __init__(self, settings: dict[str, Any], data_dir: Path, **kw: Any) -> None:
        super().__init__("OpenFork", HOST_FILE, settings, data_dir, **kw)
