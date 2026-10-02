"""Flow Race: the shared game hosting (kernel_sdk.webgame) with Flow Race's Kernel host."""

from pathlib import Path
from typing import Any

from kernel_sdk.webgame import Ran, WebGame, http_get, node_version, run_command, share_url, sharing_from

HOST_FILE = Path(__file__).resolve().parent / "kernel-host.ts"

__all__ = ["FlowRace", "Ran", "http_get", "node_version", "run_command", "share_url", "sharing_from"]


class FlowRace(WebGame):
    def __init__(self, settings: dict[str, Any], data_dir: Path, **kw: Any) -> None:
        super().__init__("Flow Race", HOST_FILE, settings, data_dir, **kw)
