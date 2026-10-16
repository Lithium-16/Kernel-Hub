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
from petinstall import ExeSource
from petrelay import Relay
from petstore import Store

CLEANUP_S = 3600


class PetLink(TailscaleShare):
    def __init__(self, settings: dict[str, Any], data_dir: Path, *, emit=None, log: logging.Logger | None = None, run=run_command) -> None:
        super().__init__("PetLink", settings, int(settings.get("port", 8098)), emit=emit, log=log, run=run,
                         path=str(settings.get("share_path", "/petlink")))
        self.store = Store(data_dir / "petlink.db")
        self.exe_source = ExeSource(data_dir, override=str(settings.get("exe_override", "")))
        self.relay = Relay(self.store, data_dir / "files", log=self.log, exe_source=self.exe_source.get,
                           invite_builder=self._invite)
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

    def _invite(self, token: str) -> str:
        blob = base64.urlsafe_b64encode(json.dumps({"relay_url": self._relay_url(), "token": token}).encode()).decode().rstrip("=")
        return f"petlink:{blob}"

    def _issue(self, user_id: str, token: str) -> dict[str, Any]:
        invite = self._invite(token)
        download = f"{self.link.rstrip('/')}/install/{self.store.add_install_code(user_id)}"
        message = (
            f"Send them this one-time download link (valid 7 days, works once):\n{download}\n\n"
            f"Fallback if the link doesn't work, they paste this invite into PetLink:\n{invite}\n\n"
            "Note: once the download link is used, that invite stops working (the downloaded file carries a fresh one)."
        )
        return {"user": user_id, "download": download, "invite": invite, "message": message}

    def _find(self, name: str):
        user = next((u for u in self.store.users() if u.name.lower() == name.strip().lower()), None)
        if user is None:
            raise ActionError("invalid_params", f"no friend called {name!r}")
        return user

    async def add_user(self, name: str, bow_hue: int = 0) -> dict[str, Any]:
        name = name.strip()
        if not name or any(u.name.lower() == name.lower() for u in self.store.users()):
            raise ActionError("invalid_params", "pick a name that isn't empty or taken")
        self._relay_url()  # not shared yet? fail before creating the user
        user, token = self.store.add_user(name, bow_hue)
        self.emit("user.added", f"{name} was added to PetLink")
        return self._issue(user.id, token)

    async def relink(self, name: str) -> dict[str, Any]:
        user = self._find(name)
        self._relay_url()
        out = self._issue(user.id, self.store.rotate_token(user.id))
        ws = self.relay.sockets.get(user.id)
        if ws is not None:
            await ws.close(code=4401)  # the old token is dead; drop the old connection
        return out

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
        pending = self.store.pending_codes()
        return {
            "state": self.state,
            "sharing": self.sharing,
            "link": self.link,
            "users": [{"id": u.id, "name": u.name, "online": u.id in online, "peek": u.peek, "install_codes": pending.get(u.id, 0)} for u in self.store.users()],
            "files_mb": round(self.store.files_size() / 2**20, 1),
        }
