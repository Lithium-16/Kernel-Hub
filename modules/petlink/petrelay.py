"""PetLink's relay: one WebSocket per user, routing by `to`, queued mail and a small file store."""

import asyncio
import json
import logging
import re
import secrets
import struct
import time
from collections import deque
from collections.abc import Awaitable, Callable
from pathlib import Path
from typing import Any

from aiohttp import WSCloseCode, WSMsgType, web
from petstore import Store, User

PROTOCOL = 1
FILE_ID = re.compile(r"^[A-Za-z0-9_-]{8,64}$")
ROUTED = {"mail", "mail.read", "peek.request", "peek.result", "visit.start", "visit.end", "touch"}
CLOSE_AUTH, CLOSE_REPLACED, CLOSE_UPGRADE = 4401, 4409, 4426
CHUNK = 256 * 1024


def ulid() -> str:
    # ponytail: random id, not a real time-ordered ULID; nothing sorts by it.
    return "01" + secrets.token_hex(12).upper()


class Relay:
    def __init__(
        self,
        store: Store,
        files_dir: Path,
        *,
        clock: Callable[[], float] = time.time,
        log: logging.Logger | None = None,
        max_file: int = 100 * 2**20,
        hourly_upload: int = 200 * 2**20,
        store_limit: int = 2 * 2**30,
        per_minute: int = 30,
        install_per_minute: int = 20,
        exe_source: Callable[[], Awaitable[Path | None]] | None = None,
    ) -> None:
        self.store, self.files_dir, self.clock = store, files_dir, clock
        self.log = log or logging.getLogger("petlink")
        self.max_file, self.hourly_upload, self.store_limit, self.per_minute = max_file, hourly_upload, store_limit, per_minute
        files_dir.mkdir(parents=True, exist_ok=True)
        self.install_per_minute, self.exe_source = install_per_minute, exe_source
        self.installs: deque[float] = deque()
        self.sockets: dict[str, web.WebSocketResponse] = {}
        self.presence: dict[str, str] = {}
        self.visits: dict[str, str] = {}  # visitor -> host
        self.sent: dict[str, deque[float]] = {}
        self.uploaded: dict[str, deque[tuple[float, int]]] = {}

    def app(self) -> web.Application:
        app = web.Application(client_max_size=self.max_file + 1)
        for prefix in ("", "/petlink"):
            app.router.add_get(f"{prefix}/ws", self.ws)
            app.router.add_put(f"{prefix}/files/{{id}}", self.put_file)
            app.router.add_get(f"{prefix}/files/{{id}}", self.get_file)
            app.router.add_get(f"{prefix}/install/{{code}}", self.install)
        return app

    def online(self) -> list[str]:
        return list(self.sockets)

    # -- sending ---------------------------------------------------------------------------

    def _envelope(self, kind: str, to: str, sender: str = "", **body: Any) -> dict[str, Any]:
        return {"v": PROTOCOL, "id": ulid(), "kind": kind, "from": sender, "to": to, **body}

    async def _send(self, user_id: str, message: dict[str, Any]) -> bool:
        ws = self.sockets.get(user_id)
        if ws is None or ws.closed:
            return False
        try:
            await ws.send_json(message)
            return True
        except (ConnectionResetError, RuntimeError):
            return False

    async def _error(self, user_id: str, code: str, text: str, ref: str = "") -> None:
        await self._send(user_id, self._envelope("error", user_id, code=code, message=text, ref=ref))

    async def _broadcast(self, sender: str, message: dict[str, Any]) -> None:
        for user in self.store.users():
            if user.id != sender:
                await self._send(user.id, {**message, "to": user.id})

    async def _flush_mail(self, user_id: str) -> None:
        for m in self.store.pending_mail(user_id):
            if not await self._send(user_id, m):
                return
            self.store.mail_delivered(m["mail_id"])
            await self._send(m["from"], self._envelope("delivered", m["from"], mail_id=m["mail_id"]))

    # -- the socket ------------------------------------------------------------------------

    def _welcome(self, me: User) -> dict[str, Any]:
        friends = [
            {"id": u.id, "name": u.name, "bow_hue": u.bow_hue, "peek": u.peek, "online": u.id in self.sockets}
            for u in self.store.users()
            if u.id != me.id
        ]
        return self._envelope(
            "welcome", me.id, me={"id": me.id, "name": me.name, "bow_hue": me.bow_hue, "peek": me.peek}, friends=friends
        )

    async def ws(self, request: web.Request) -> web.WebSocketResponse:
        ws = web.WebSocketResponse(heartbeat=30, max_msg_size=1 << 20)
        await ws.prepare(request)
        try:
            first = await asyncio.wait_for(ws.receive_json(), 10)
        except (TimeoutError, ValueError, TypeError):
            await ws.close(code=CLOSE_AUTH)
            return ws
        hello = isinstance(first, dict) and first.get("kind") == "hello"
        user = self.store.auth(str(first.get("token", ""))) if hello else None
        if user is None:
            await ws.close(code=CLOSE_AUTH)
            return ws
        if first.get("v") != PROTOCOL:
            await ws.send_json(self._envelope("error", user.id, code="upgrade", message="please update PetLink", ref=""))
            await ws.close(code=CLOSE_UPGRADE)
            return ws
        old = self.sockets.get(user.id)
        self.sockets[user.id] = ws
        if old is not None:
            await old.close(code=CLOSE_REPLACED)
        self.presence[user.id] = "online"
        await ws.send_json(self._welcome(user))
        await self._broadcast(user.id, self._envelope("presence", "", user.id, state="online"))
        await self._flush_mail(user.id)
        try:
            async for raw in ws:
                if raw.type != WSMsgType.TEXT:
                    continue
                try:
                    data = json.loads(raw.data)
                    if not isinstance(data, dict):
                        raise ValueError
                except ValueError:
                    await self._error(user.id, "bad_json", "not a JSON object")
                    continue
                await self._handle(user, data)
        finally:
            if self.sockets.get(user.id) is ws:  # not replaced by a newer login
                await self._gone(user.id)
        return ws

    async def _gone(self, user_id: str) -> None:
        del self.sockets[user_id]
        self.presence[user_id] = "offline"
        for visitor, host in list(self.visits.items()):
            if user_id in (visitor, host):
                del self.visits[visitor]
                other = host if visitor == user_id else visitor
                await self._send(other, self._envelope("visit.end", other, user_id))
        await self._broadcast(user_id, self._envelope("presence", "", user_id, state="offline"))

    def _limited(self, user_id: str) -> bool:
        now = self.clock()
        q = self.sent.setdefault(user_id, deque())
        while q and q[0] < now - 60:
            q.popleft()
        q.append(now)
        return len(q) > self.per_minute

    async def _handle(self, user: User, data: dict[str, Any]) -> None:
        if self._limited(user.id):
            await self._error(user.id, "rate_limited", "slow down", str(data.get("id", "")))
            return
        data["from"] = user.id
        data["v"] = PROTOCOL
        kind, ref = data.get("kind"), str(data.get("id", ""))
        if kind == "presence" and data.get("state") in ("online", "away"):
            self.presence[user.id] = data["state"]
            await self._broadcast(user.id, data)
            return
        if kind == "peek.allow":
            self.store.set_peek(user.id, bool(data.get("on")))
            await self._broadcast(user.id, {**data, "on": bool(data.get("on"))})
            return
        if kind not in ROUTED:
            await self._error(user.id, "bad_kind", f"unknown kind {kind!r}", ref)
            return
        to = str(data.get("to", ""))
        if to == user.id or self.store.user(to) is None:
            await self._error(user.id, "bad_target", "no such friend", ref)
            return
        if kind == "mail":
            await self._mail(user, to, data, ref)
            return
        if kind == "peek.request" and not self.store.user(to).peek:
            await self._send(user.id, self._envelope("peek.result", user.id, to, peek_id=data.get("peek_id"), file_id="", denied=True))
            return
        if kind == "visit.end":
            self.visits.pop(user.id, None)
            if self.visits.get(to) == user.id:
                del self.visits[to]
        if not await self._send(to, data):
            await self._error(user.id, "offline", "your friend is offline", ref)
        elif kind == "visit.start":
            self.visits[user.id] = to

    async def _mail(self, user: User, to: str, data: dict[str, Any], ref: str) -> None:
        mail_id = str(data.get("mail_id", ""))
        files = data.get("files") or []
        if not mail_id or not isinstance(files, list) or not all(isinstance(f, dict) for f in files):
            await self._error(user.id, "bad_mail", "mail needs a mail_id", ref)
            return
        for f in files:
            row = self.store.file(str(f.get("file_id", "")))
            if row is None or row.owner != user.id or row.to != to:
                await self._error(user.id, "bad_file", "a file in this mail wasn't uploaded for this friend", ref)
                return
        if self.store.queue_mail(mail_id, user.id, to, data):
            await self._flush_mail(to)
        elif self.store.mail_state(mail_id) == "delivered":
            # A re-send after the sender missed the first `delivered` (it was offline then).
            await self._send(user.id, self._envelope("delivered", user.id, mail_id=mail_id))

    # -- files ------------------------------------------------------------------------------

    def _bearer(self, request: web.Request) -> User | None:
        auth = request.headers.get("Authorization", "")
        return self.store.auth(auth[7:]) if auth.startswith("Bearer ") else None

    def _quota_left(self, user_id: str) -> int:
        now = self.clock()
        q = self.uploaded.setdefault(user_id, deque())
        while q and q[0][0] < now - 3600:
            q.popleft()
        return self.hourly_upload - sum(n for _, n in q)

    async def put_file(self, request: web.Request) -> web.Response:
        user = self._bearer(request)
        if user is None:
            return web.Response(status=401)
        file_id, to, name = request.match_info["id"], request.query.get("to", ""), request.query.get("name", "")
        if not FILE_ID.match(file_id) or not name or self.store.user(to) is None or to == user.id:
            return web.Response(status=400, text="bad file id, name or recipient")
        if self.store.file(file_id) is not None:
            return web.Response(status=409)
        size = request.content_length
        if size is None or size > self.max_file:
            return web.Response(status=413)
        if size > self._quota_left(user.id):
            return web.Response(status=429)
        if self.store.files_size() + size > self.store_limit:
            return web.Response(status=507)
        path = self.files_dir / file_id
        written = 0
        try:
            with path.open("wb") as out:
                async for chunk in request.content.iter_chunked(CHUNK):
                    written += len(chunk)
                    if written > self.max_file:
                        out.close()
                        path.unlink(missing_ok=True)
                        return web.Response(status=413)
                    out.write(chunk)
        except BaseException:  # the client dropped mid-PUT: no orphan file
            path.unlink(missing_ok=True)
            raise
        self.store.add_file(file_id, user.id, to, name[:200], written, path)
        self.uploaded[user.id].append((self.clock(), written))
        return web.Response(status=201)

    async def _stream_file(self, request: web.Request, path: Path) -> web.StreamResponse:
        resp = web.StreamResponse(headers={"Content-Type": "application/octet-stream"})
        resp.content_length = path.stat().st_size
        await resp.prepare(request)
        with path.open("rb") as f:
            while chunk := f.read(CHUNK):
                await resp.write(chunk)
        await resp.write_eof()
        return resp

    async def get_file(self, request: web.Request) -> web.StreamResponse:
        user = self._bearer(request)
        if user is None:
            return web.Response(status=401)
        row = self.store.file(request.match_info["id"])
        if row is None or not row.path.exists():
            return web.Response(status=404)
        if row.to != user.id:
            return web.Response(status=403)
        try:
            resp = await self._stream_file(request, row.path)
        except (ConnectionResetError, asyncio.CancelledError):
            self.log.info("download of %s interrupted; keeping it", row.id)
            raise web.HTTPInternalServerError() from None
        self.store.drop_file(row.id)
        row.path.unlink(missing_ok=True)
        return resp

    # -- one-time installer download ---------------------------------------------------------

    async def install(self, request: web.Request) -> web.Response:
        # Behind Funnel every request comes from 127.0.0.1, so the limit is global, not per address.
        now = self.clock()
        while self.installs and self.installs[0] < now - 60:
            self.installs.popleft()
        self.installs.append(now)
        if len(self.installs) > self.install_per_minute:
            return web.Response(status=429, text="Too many tries, wait a minute.")
        gone = web.Response(status=404, text="This download link is used up or expired. Ask for a new one.")
        code = request.match_info["code"]
        if self.store.take_install_code(code, consume=False) is None:
            return gone
        exe = await self.exe_source() if self.exe_source else None
        if exe is None:
            return web.Response(status=503, text="PetLink isn't published yet")  # the code stays valid
        taken = self.store.take_install_code(code)
        if taken is None:  # lost a race with another request
            return gone
        invite = taken[1].encode()
        body = await asyncio.to_thread(exe.read_bytes) + invite + struct.pack("<I", len(invite)) + b"PLINVITE"
        return web.Response(
            body=body,
            content_type="application/octet-stream",
            headers={"Content-Disposition": 'attachment; filename="PetLink.exe"'},
        )

    def cleanup(self) -> int:
        paths = self.store.expire(self.clock())
        for p in paths:
            p.unlink(missing_ok=True)
        return len(paths)
