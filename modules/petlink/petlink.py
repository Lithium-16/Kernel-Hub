"""PetLink on Kernel's side: runs the relay, manages friends and shares it through Tailscale."""

import asyncio
import base64
import json
import logging
from pathlib import Path
from typing import Any

from aiohttp import web
from kernel_sdk import ActionError
from kernel_sdk.webgame import TailscaleShare, run_command
from petrelay import Relay
from petstore import Store

CLEANUP_S = 3600


class PetLink(TailscaleShare):
    def __init__(self, settings: dict[str, Any], data_dir: Path, *, emit=None, log: logging.Logger | None = None, run=run_command) -> None:
        super().__init__("PetLink", settings, int(settings.get("port", 8098)), emit=emit, log=log, run=run,
                         path=str(settings.get("share_path", "/petlink")))
        self.store = Store(data_dir / "petlink.db")
        self.relay = Relay(self.store, data_dir / "files", log=self.log)
        self.state = "stopped"

    async def run_forever(self) -> None:
        runner = web.AppRunner(self.relay.app())
        await runner.setup()
        await web.TCPSite(runner, "127.0.0.1", self.port).start()
        self.state = "running"
        await asyncio.to_thread(self.refresh_sharing)
        try:
            while True:
                removed = self.relay.cleanup()
                if removed:
                    self.log.info("removed %d expired files", removed)
                await asyncio.sleep(CLEANUP_S)
        finally:
            self.state = "stopped"
            await runner.cleanup()

    def _relay_url(self) -> str:
        if not self.link:
            raise ActionError("disabled", "Share PetLink first, so the invite has an address that works from outside.")
        return "wss://" + self.link.removeprefix("https://").rstrip("/") + "/ws"

    async def add_user(self, name: str, bow_hue: int = 0) -> dict[str, Any]:
        name = name.strip()
        if not name or any(u.name.lower() == name.lower() for u in self.store.users()):
            raise ActionError("invalid_params", "pick a name that isn't empty or taken")
        relay_url = self._relay_url()
        user, token = self.store.add_user(name, bow_hue)
        blob = base64.urlsafe_b64encode(json.dumps({"relay_url": relay_url, "token": token}).encode()).decode().rstrip("=")
        self.emit("user.added", f"{name} was added to PetLink")
        return {"user": user.id, "invite": f"petlink:{blob}"}

    async def remove_user(self, name: str) -> dict[str, Any]:
        user = next((u for u in self.store.users() if u.name.lower() == name.strip().lower()), None)
        if user is None:
            raise ActionError("invalid_params", f"no friend called {name!r}")
        self.store.remove_user(user.id)
        ws = self.relay.sockets.get(user.id)
        if ws is not None:
            await ws.close(code=4401)
        return {"removed": user.name}

    def snapshot(self) -> dict[str, Any]:
        online = set(self.relay.online())
        return {
            "state": self.state,
            "sharing": self.sharing,
            "link": self.link,
            "users": [{"id": u.id, "name": u.name, "online": u.id in online, "peek": u.peek} for u in self.store.users()],
            "files_mb": round(self.store.files_size() / 2**20, 1),
        }
