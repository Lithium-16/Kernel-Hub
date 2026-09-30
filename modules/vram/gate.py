"""Taking turns on the GPU: LLM requests and image jobs, grouped to avoid swapping models.

Apps are reached through small local proxies (Ollama on 11435 instead of 11434, ComfyUI on
8189 instead of 8188) that hold a request until it's that group's turn:

- While ComfyUI is working, LLM requests wait. New image jobs go straight in, so images
  queued around an LLM request run back to back and the models swap once, not twice.
- While an LLM request is running, new image jobs wait for it (they're short).
- An LLM request never waits longer than `llm_max_wait_s`; after that it runs anyway, and
  image jobs wait for it instead.

Only requests that do work are held: POSTs to an LLM, POST /prompt to ComfyUI. Everything
else (model lists, the ComfyUI page and its websocket) passes straight through.
"""

from __future__ import annotations

import asyncio
import contextlib
import logging
import time
from collections.abc import AsyncIterator, Awaitable, Callable
from dataclasses import dataclass, field

HEAD_LIMIT = 64 * 1024
# How long an image job counts as "ComfyUI is busy" before the next status poll confirms it.
SUBMIT_GRACE_S = 6.0


@dataclass
class Waiter:
    since: float
    app: str = ""
    ready: asyncio.Event = field(default_factory=asyncio.Event)


class Turns:
    def __init__(
        self,
        llm_max_wait_s: float,
        image_max_wait_s: float,
        clock: Callable[[], float] = time.monotonic,
        before_llm: Callable[[], Awaitable[None]] | None = None,
        log: logging.Logger | None = None,
        grace_s: float = SUBMIT_GRACE_S,
    ) -> None:
        self.llm_max_wait_s, self.image_max_wait_s = llm_max_wait_s, image_max_wait_s
        self.grace_s = grace_s
        self.clock = clock
        self.before_llm = before_llm
        self.log = log or logging.getLogger("vram.turns")
        self.image_busy = False  # ComfyUI has jobs running or queued (from its status)
        self.submitted_at = -1e9  # last image job we let through
        self.inflight: dict[str, int] = {}  # LLM app -> requests running
        self.llm_waiting: list[Waiter] = []
        self.image_waiting: list[Waiter] = []
        self.overlaps = 0  # times an LLM ran during images because it waited too long
        self._waking = False

    # -- state ----------------------------------------------------------------------------

    @property
    def llm_running(self) -> int:
        return sum(self.inflight.values())

    def images_active(self) -> bool:
        return self.image_busy or self.clock() - self.submitted_at < self.grace_s

    def llm_overdue(self) -> bool:
        now = self.clock()
        return any(now - w.since >= self.llm_max_wait_s for w in self.llm_waiting)

    def set_image_busy(self, busy: bool) -> None:
        self.image_busy = busy
        self.wake()

    def status(self) -> dict[str, int | str]:
        turn = "llm" if self.llm_running else "images" if self.images_active() else "free"
        return {
            "turn": turn,
            "llm_running": self.llm_running,
            "llm_waiting": len(self.llm_waiting),
            "images_waiting": len(self.image_waiting),
        }

    # -- turns ----------------------------------------------------------------------------

    def wake(self) -> None:
        """Let through whoever may go now."""
        if self.llm_waiting and (not self.images_active() or self.llm_overdue()):
            if not self._waking:
                self._waking = True
                asyncio.get_running_loop().create_task(self._start_llm_turn())
        if self.image_waiting and not self.llm_running and not self.llm_overdue():
            if not self.llm_waiting or self.images_active():
                waiting, self.image_waiting = self.image_waiting, []
                self.submitted_at = self.clock()
                for w in waiting:
                    w.ready.set()

    async def _start_llm_turn(self) -> None:
        try:
            if not self.images_active() and self.before_llm is not None:
                await self.before_llm()
        except Exception:
            self.log.exception("making room for the LLM failed")
        finally:
            self._waking = False
            waiting, self.llm_waiting = self.llm_waiting, []
            for w in waiting:
                # Counted as running from here, so no image job slips in before it starts.
                self.inflight[w.app] = self.inflight.get(w.app, 0) + 1
                w.ready.set()

    async def _wait(self, queue: list[Waiter], limit: float, app: str = "") -> bool:
        """False when it waited the whole `limit` and goes anyway."""
        w = Waiter(self.clock(), app)
        queue.append(w)
        self.wake()
        try:
            await asyncio.wait_for(w.ready.wait(), timeout=limit)
            return True
        except TimeoutError:
            return w.ready.is_set()  # let through just as the wait ran out
        finally:
            if w in queue:
                queue.remove(w)

    @contextlib.asynccontextmanager
    async def llm(self, app: str) -> AsyncIterator[None]:
        granted = False
        if self.images_active() or self.llm_waiting:
            granted = await self._wait(self.llm_waiting, self.llm_max_wait_s, app)
            if not granted:
                self.overlaps += 1
                self.log.warning("an LLM request waited %gs; running it alongside images", self.llm_max_wait_s)
        if not granted:  # a granted turn was already counted
            self.inflight[app] = self.inflight.get(app, 0) + 1
        try:
            yield
        finally:
            self.inflight[app] -= 1
            self.wake()

    async def image(self) -> None:
        if self.llm_running or self.llm_overdue():
            await self._wait(self.image_waiting, self.image_max_wait_s)
        self.submitted_at = self.clock()

    async def tick(self) -> None:
        """Called on every status poll: catches overdue waiters."""
        self.wake()


# -- the proxy -----------------------------------------------------------------------------


@dataclass
class Request:
    method: str
    path: str
    head: bytes  # request line and headers, as received
    upgrade: bool


async def read_head(reader: asyncio.StreamReader) -> Request | None:
    try:
        raw = await reader.readuntil(b"\r\n\r\n")
    except (asyncio.IncompleteReadError, asyncio.LimitOverrunError, ConnectionError):
        return None
    if len(raw) > HEAD_LIMIT:
        return None
    first, *lines = raw.decode("latin-1").split("\r\n")
    parts = first.split(" ")
    if len(parts) < 3:
        return None
    headers = {k.strip().lower(): v.strip() for k, _, v in (h.partition(":") for h in lines if h)}
    upgrade = "upgrade" in headers.get("connection", "").lower()
    return Request(parts[0].upper(), parts[1], raw, upgrade)


def close_after(head: bytes) -> bytes:
    """One request per connection, so every request passes the gate."""
    first, *lines = head.decode("latin-1").rstrip("\r\n").split("\r\n")
    kept = [h for h in lines if h.split(":", 1)[0].strip().lower() not in ("connection", "keep-alive")]
    return "\r\n".join([first, *kept, "Connection: close", "", ""]).encode("latin-1")


async def pipe(reader: asyncio.StreamReader, writer: asyncio.StreamWriter) -> None:
    try:
        while chunk := await reader.read(65536):
            writer.write(chunk)
            await writer.drain()
    except (ConnectionError, OSError):
        pass
    finally:
        with contextlib.suppress(Exception):
            if writer.can_write_eof():
                writer.write_eof()


class Gate:
    """A local proxy in front of one app. `hold(request)` returns a context to run inside."""

    def __init__(
        self,
        name: str,
        upstream: tuple[str, int],
        hold: Callable[[Request], contextlib.AbstractAsyncContextManager[None] | None],
        log: logging.Logger | None = None,
    ) -> None:
        self.name, self.upstream, self.hold = name, upstream, hold
        self.log = log or logging.getLogger("vram.gate")
        self.server: asyncio.Server | None = None

    async def start(self, host: str, port: int) -> None:
        self.server = await asyncio.start_server(self.handle, host, port, limit=HEAD_LIMIT)

    async def handle(self, reader: asyncio.StreamReader, writer: asyncio.StreamWriter) -> None:
        try:
            req = await read_head(reader)
            if req is None:
                return
            ctx = None if req.upgrade else self.hold(req)
            async with ctx if ctx is not None else contextlib.nullcontext():
                await self.forward(req, reader, writer)
        except Exception:
            self.log.exception("%s proxy: request failed", self.name)
        finally:
            with contextlib.suppress(Exception):
                writer.close()
                await writer.wait_closed()

    async def forward(self, req: Request, reader: asyncio.StreamReader, writer: asyncio.StreamWriter) -> None:
        try:
            up_reader, up_writer = await asyncio.open_connection(*self.upstream)
        except OSError:
            body = f"{self.name} isn't running".encode()
            writer.write(
                b"HTTP/1.1 502 Bad Gateway\r\nContent-Type: text/plain\r\nConnection: close\r\n"
                + f"Content-Length: {len(body)}\r\n\r\n".encode()
                + body
            )
            await writer.drain()
            return
        up_writer.write(req.head if req.upgrade else close_after(req.head))
        await up_writer.drain()
        # Both ways at once: request bodies of any encoding, streamed replies, websockets.
        upload = asyncio.create_task(pipe(reader, up_writer))
        await pipe(up_reader, writer)
        upload.cancel()
        with contextlib.suppress(Exception):
            up_writer.close()
