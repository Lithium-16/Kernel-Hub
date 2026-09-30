"""Shares one GPU between apps: measures each app's VRAM and unloads them by priority.

Ollama and ComfyUI are measured and unloaded through their own APIs. Other apps are measured
by process (Windows reports dedicated VRAM per process; see winvram.py) and are only ever
closed when their `stop_when_needed` says so. `watch` apps (Roblox) are never touched.
"""

from __future__ import annotations

import asyncio
import contextlib
import json
import logging
import time
import urllib.error
import urllib.parse
import urllib.request
from collections.abc import AsyncIterator, Callable
from dataclasses import dataclass
from datetime import datetime
from typing import Any

from gate import Gate, Request, Turns
from kernel_sdk import ActionError

KINDS = ("ollama", "llm", "comfyui", "process", "watch")
# Which turn an app's work belongs to (see gate.py).
LLM_KINDS = ("ollama", "llm")
OOM = ("out of memory", "outofmemory", "allocation on device")
MB = 1024 * 1024
# A process app counts as busy (holding the GPU for real work) above this much VRAM.
BUSY_MB = 512
# Don't unload the same app again within this long (it may be reloading on purpose).
COOLDOWN_S = 30.0

Emit = Callable[..., None]
Fetch = Callable[[str, str, Any, float], Any]


class Offline(Exception):
    pass


def http_json(method: str, url: str, body: Any = None, timeout: float = 5) -> Any:
    data = json.dumps(body).encode() if body is not None else None
    req = urllib.request.Request(url, data=data, method=method)
    if data is not None:
        req.add_header("Content-Type", "application/json")
    try:
        with urllib.request.urlopen(req, timeout=timeout) as r:
            raw = r.read()
    except urllib.error.HTTPError as e:
        raise ActionError("module_failed", f"{url} answered {e.code}") from None
    except (urllib.error.URLError, OSError) as e:
        raise Offline(str(e)) from None
    # Ollama streams by default; the last line is the summary.
    text = raw.decode("utf-8", "replace").strip()
    return json.loads(text.splitlines()[-1]) if text else None


@dataclass
class App:
    name: str
    kind: str
    priority: int
    url: str = ""
    process: tuple[str, ...] = ()
    exclusive: bool = False
    stop_when_needed: bool = False
    proxy_port: int = 0

    @classmethod
    def parse(cls, raw: Any) -> App:
        if not isinstance(raw, dict):
            raise ValueError("each app in 'apps' must be a table like { name = ..., kind = ... }")
        name, kind = str(raw.get("name", "")).strip(), raw.get("kind")
        if not name:
            raise ValueError("every app needs a name")
        if kind not in KINDS:
            raise ValueError(f"app '{name}': kind must be one of {', '.join(KINDS)}")
        process = raw.get("process", ())
        process = (process,) if isinstance(process, str) else tuple(process)
        if kind in ("process", "watch") and not process:
            raise ValueError(f"app '{name}': a {kind} app needs process = \"name.exe\"")
        url = str(raw.get("url", "")).rstrip("/")
        if kind in ("ollama", "llm", "comfyui") and not url.startswith("http://"):
            raise ValueError(f"app '{name}': needs url = \"http://127.0.0.1:port\"")
        proxy_port = int(raw.get("proxy_port", 0))
        if proxy_port and kind not in ("ollama", "llm", "comfyui"):
            raise ValueError(f"app '{name}': only ollama, llm and comfyui apps can have a proxy_port")
        if not 0 <= proxy_port < 65536:
            raise ValueError(f"app '{name}': proxy_port must be a port number")
        unknown = set(raw) - {
            "name",
            "kind",
            "priority",
            "url",
            "process",
            "exclusive",
            "stop_when_needed",
            "proxy_port",
        }
        if unknown:
            raise ValueError(f"app '{name}': unknown key(s) {', '.join(sorted(unknown))}")
        return cls(
            name=name,
            kind=kind,
            priority=int(raw.get("priority", 0)),
            url=url,
            process=tuple(p.lower() for p in process),
            exclusive=bool(raw.get("exclusive", False)),
            stop_when_needed=bool(raw.get("stop_when_needed", False)),
            proxy_port=proxy_port,
        )

    @property
    def freeable(self) -> bool:
        return self.kind in ("ollama", "comfyui") or (self.kind in ("process", "llm") and self.stop_when_needed)

    @property
    def upstream(self) -> tuple[str, int]:
        u = urllib.parse.urlparse(self.url)
        return u.hostname or "127.0.0.1", u.port or 80


@dataclass
class Usage:
    """One app, right now."""

    mb: int = 0
    state: str = "off"  # off / idle / loaded / busy
    detail: str = ""
    models: tuple[str, ...] = ()  # Ollama models loaded
    pids: tuple[int, ...] = ()


class Gpu:
    """Total and used VRAM of the first NVIDIA GPU (NVML), and VRAM per process."""

    def __init__(self, nvml: Any = None, per_process: Callable[[], dict[int, int]] | None = None) -> None:
        self.nvml, self.handle = nvml, None
        try:
            if self.nvml is None:
                import pynvml

                self.nvml = pynvml
            self.nvml.nvmlInit()
            self.handle = self.nvml.nvmlDeviceGetHandleByIndex(0)
        except Exception:
            self.handle = None
        self._per_process = per_process

    def totals(self) -> tuple[int, int] | None:
        """(used, total) in bytes."""
        if self.handle is None:
            return None
        try:
            m = self.nvml.nvmlDeviceGetMemoryInfo(self.handle)
        except Exception:
            return None
        return int(m.used), int(m.total)

    def per_process(self) -> dict[int, int]:
        if self._per_process is None:
            from winvram import reader

            self._per_process = reader(self.nvml, self.handle)
        try:
            return self._per_process()
        except Exception:
            return {}


def processes_by_name() -> dict[str, list[int]]:
    import psutil

    out: dict[str, list[int]] = {}
    for p in psutil.process_iter(["name"]):
        name = (p.info.get("name") or "").lower()
        if name:
            out.setdefault(name, []).append(p.pid)
    return out


def gb(mb: int) -> str:
    return f"{mb / 1024:.1f} GB"


class Manager:
    def __init__(
        self,
        settings: dict[str, Any],
        gpu: Gpu | None = None,
        emit: Emit | None = None,
        fetch: Fetch = http_json,
        processes: Callable[[], dict[str, list[int]]] = processes_by_name,
        clock: Callable[[], float] = time.monotonic,
        stop_process: Callable[[list[int]], int] | None = None,
        log: logging.Logger | None = None,
    ) -> None:
        self.settings = settings
        self.apps = sorted((App.parse(a) for a in settings["apps"]), key=lambda a: -a.priority)
        names = [a.name.lower() for a in self.apps]
        if len(set(names)) != len(names):
            raise ValueError("app names must be different")
        self.auto = bool(settings["auto"])
        self.gpu = gpu if gpu is not None else Gpu()
        self.emit: Emit = emit or (lambda *args, **kwargs: None)
        self.fetch, self.processes, self.clock = fetch, processes, clock
        self.stop_process = stop_process or terminate
        self.log = log or logging.getLogger("vram")
        self.usage: dict[str, Usage] = {}
        self.free_mb: int | None = None
        self.low_since: float | None = None
        self.warned_low = False
        self.last_freed: dict[str, float] = {}
        self.last_action: tuple[str, float] | None = None
        self.snapshot: dict[str, Any] = {}
        self.turns = Turns(
            float(settings["llm_max_wait_s"]),
            float(settings["image_max_wait_s"]),
            before_llm=self.make_room_for_llm,
            log=self.log,
        )
        self.gates: list[Gate] = []
        self.history_seen: set[str] | None = None
        self.retried: set[str] = set()  # jobs we queued again, never retried twice
        self.retries = 0

    def app(self, name: str) -> App:
        for a in self.apps:
            if a.name.lower() == name.strip().lower():
                return a
        raise ActionError("invalid_params", f"no app '{name}'. Known: {', '.join(a.name for a in self.apps)}")

    # -- measuring ------------------------------------------------------------------------

    async def call(self, method: str, url: str, body: Any = None, timeout: float = 5) -> Any:
        return await asyncio.to_thread(self.fetch, method, url, body, timeout)

    async def measure(self, app: App, per_pid: dict[int, int], by_name: dict[str, list[int]]) -> Usage:
        pids = tuple(pid for name in app.process for pid in by_name.get(name, []))
        by_process = sum(per_pid.get(pid, 0) for pid in pids) // MB
        running = self.turns.inflight.get(app.name, 0)
        if app.kind == "ollama":
            try:
                ps = await self.call("GET", f"{app.url}/api/ps")
            except (Offline, ActionError, ValueError):
                return Usage()
            models = [m for m in (ps or {}).get("models") or [] if isinstance(m, dict)]
            mb = sum(int(m.get("size_vram") or 0) for m in models) // MB
            names = tuple(str(m.get("name") or m.get("model")) for m in models)
            state = "busy" if running else "loaded" if models else "idle"
            return Usage(mb=mb, state=state, detail=", ".join(names), models=names)
        if app.kind == "llm":
            if not pids and not running:
                return Usage()
            state = "busy" if running else "loaded" if by_process >= 256 else "idle"
            return Usage(mb=by_process, state=state, pids=pids)
        if app.kind == "comfyui":
            try:
                stats = await self.call("GET", f"{app.url}/system_stats")
                queue = await self.call("GET", f"{app.url}/queue")
            except (Offline, ActionError, ValueError):
                return Usage()
            devices = (stats or {}).get("devices") or [{}]
            mb = int(devices[0].get("torch_vram_total") or 0) // MB or by_process
            jobs = len(queue.get("queue_running") or []) + len(queue.get("queue_pending") or [])
            state = "busy" if jobs else "loaded" if mb >= 256 else "idle"
            return Usage(mb=mb, state=state, detail=f"{jobs} job{'s' if jobs != 1 else ''}" if jobs else "")
        if not pids:
            return Usage()
        state = "busy" if by_process >= BUSY_MB else "idle"
        return Usage(mb=by_process, state=state, pids=pids)

    async def refresh(self) -> dict[str, Any]:
        per_pid = await asyncio.to_thread(self.gpu.per_process)
        by_name = await asyncio.to_thread(self.processes)
        results = await asyncio.gather(*(self.measure(a, per_pid, by_name) for a in self.apps))
        self.usage = {a.name: u for a, u in zip(self.apps, results, strict=True)}
        self.turns.set_image_busy(any(self.usage[a.name].state == "busy" for a in self.apps if a.kind == "comfyui"))
        totals = self.gpu.totals()
        snap: dict[str, Any] = {"auto": self.auto}
        if totals:
            used, total = totals[0] // MB, totals[1] // MB
            self.free_mb = total - used
            snap.update({"vram_used_mb": used, "vram_total_mb": total, "vram_free_mb": self.free_mb})
            floor = int(self.settings["min_free_mb"])
            snap["state"] = "tight" if floor and self.free_mb < floor else "ok"
        else:
            self.free_mb = None
            snap["state"] = "unknown"
        rows = [
            {"app": a.name, "vram_mb": u.mb, "state": u.state, "priority": a.priority, "detail": u.detail}
            for a, u in ((a, self.usage[a.name]) for a in self.apps)
        ]
        if totals:
            other = totals[0] // MB - sum(r["vram_mb"] for r in rows)
            rows.append({"app": "Everything else", "vram_mb": max(0, other), "state": "", "priority": None, "detail": ""})
        snap["apps"] = rows
        if self.gates:
            snap.update(self.turns.status())
        if self.retries:
            snap["retried_jobs"] = self.retries
        if self.last_action:
            text, at = self.last_action
            snap["last_action"] = text
            snap["last_action_at"] = datetime.fromtimestamp(at).astimezone().isoformat(timespec="seconds")
        self.snapshot = snap
        return snap

    # -- freeing --------------------------------------------------------------------------

    async def free(self, app: App, why: str, stop: bool | None = None) -> int:
        """Unload one app. Returns the VRAM it held (MB), 0 if nothing was done."""
        u = self.usage.get(app.name, Usage())
        if u.state == "off" or not app.freeable and not stop:
            return 0
        if u.state == "busy" and app.kind in LLM_KINDS:
            return 0  # never pull a model out from under a request
        held = u.mb
        try:
            if app.kind == "ollama":
                if not u.models:
                    return 0
                for model in u.models:
                    await self.call("POST", f"{app.url}/api/generate", {"model": model, "keep_alive": 0}, 30)
            elif app.kind == "comfyui":
                if u.state == "busy" or held < 256:
                    return 0
                await self.call("POST", f"{app.url}/free", {"unload_models": True, "free_memory": True}, 30)
            elif app.kind in ("process", "llm") and (app.stop_when_needed if stop is None else stop) and u.pids:
                await asyncio.to_thread(self.stop_process, list(u.pids))
            else:
                return 0
        except (Offline, ActionError) as e:
            self.log.warning("couldn't free %s: %s", app.name, e)
            return 0
        self.last_freed[app.name] = self.clock()
        verb = "Closed" if app.kind == "process" else "Unloaded"
        what = f"{app.name} ({u.detail})" if app.kind == "ollama" and u.detail else app.name
        text = f"{verb} {what}, {gb(held)}, {why}"
        self.last_action = (text, time.time())
        self.emit("vram.freed", text, app=app.name, mb=held)
        return held

    def _cooling(self, app: App) -> bool:
        at = self.last_freed.get(app.name)
        return at is not None and self.clock() - at < COOLDOWN_S

    async def balance(self) -> None:
        """The automatic part: exclusive apps first, then the free-VRAM floor."""
        if not self.auto:
            return
        for boss in self.apps:
            if not boss.exclusive or self.usage.get(boss.name, Usage()).state != "busy":
                continue
            for app in self.apps:
                if app.priority < boss.priority and not self._cooling(app):
                    u = self.usage.get(app.name, Usage())
                    if u.mb > 0 and u.state != "busy":
                        await self.free(app, f"to make room for {boss.name}")
        floor = int(self.settings["min_free_mb"])
        if not floor or self.free_mb is None:
            return
        if self.free_mb >= floor:
            self.low_since, self.warned_low = None, False
            return
        now = self.clock()
        self.low_since = self.low_since if self.low_since is not None else now
        if now - self.low_since < float(self.settings["after_s"]):
            return
        for app in reversed(self.apps):  # lowest priority first
            u = self.usage.get(app.name, Usage())
            if app.freeable and u.mb > 0 and u.state != "busy" and not self._cooling(app):
                if await self.free(app, f"only {gb(self.free_mb)} VRAM was free"):
                    self.low_since = now
                    return
        if not self.warned_low:
            self.warned_low = True
            self.emit(
                "vram.low",
                f"Only {gb(self.free_mb)} VRAM free, and nothing idle to unload",
                level="warn",
                free_mb=self.free_mb,
            )

    async def poll(self) -> None:
        await self.start_gates()
        while True:
            await self.refresh()
            await self.balance()
            if self.settings["retry_out_of_memory"]:
                await self.retry_failed()
            await self.turns.tick()
            # Someone is waiting for a turn: look again soon, so the turn changes on time.
            waiting = self.turns.llm_waiting or self.turns.image_waiting
            await asyncio.sleep(1.0 if waiting else float(self.settings["poll_s"]))

    # -- turns ----------------------------------------------------------------------------

    async def start_gates(self) -> None:
        host = str(self.settings["proxy_host"])
        for app in self.apps:
            if not app.proxy_port:
                continue
            gate = Gate(app.name, app.upstream, self._hold_for(app), log=self.log)
            try:
                await gate.start(host, app.proxy_port)
            except OSError as e:
                self.log.error("can't listen on %s:%s for %s: %s", host, app.proxy_port, app.name, e)
                self.emit(
                    "gate.failed",
                    f"Couldn't open port {app.proxy_port} for {app.name}: {e}",
                    level="warn",
                )
                continue
            self.gates.append(gate)
            self.log.info("%s takes turns through %s:%s", app.name, host, app.proxy_port)

    def _hold_for(self, app: App) -> Callable[[Request], Any]:
        if app.kind in LLM_KINDS:
            # Only requests that make the model work; lists and versions pass straight through.
            return lambda req: self.turns.llm(app.name) if req.method == "POST" else None

        @contextlib.asynccontextmanager
        async def image_turn() -> AsyncIterator[None]:
            await self.turns.image()
            yield

        return lambda req: image_turn() if req.method == "POST" and req.path.split("?")[0] == "/prompt" else None

    async def make_room_for_llm(self) -> None:
        """Before the LLM's turn: unload an idle ComfyUI if VRAM is short."""
        await self.refresh()
        need = int(self.settings["llm_reserve_mb"])
        if self.free_mb is None or self.free_mb >= need:
            return
        for app in self.apps:
            if app.kind == "comfyui" and self.usage[app.name].state != "busy":
                await self.free(app, "for the LLM's turn")

    # -- retries --------------------------------------------------------------------------

    async def retry_failed(self) -> None:
        """Queue a job again, once, if it failed for lack of VRAM."""
        for app in self.apps:
            if app.kind != "comfyui" or self.usage.get(app.name, Usage()).state == "off":
                continue
            try:
                history = await self.call("GET", f"{app.url}/history?max_items=16")
            except (Offline, ActionError, ValueError):
                continue
            if not isinstance(history, dict):
                continue
            if self.history_seen is None:
                self.history_seen = set(history)
                continue
            for pid, entry in history.items():
                if pid in self.history_seen:
                    continue
                self.history_seen.add(pid)
                if pid not in self.retried and out_of_memory(entry):
                    await self.retry(app, pid, entry)

    async def retry(self, app: App, pid: str, entry: dict[str, Any]) -> None:
        prompt = entry.get("prompt") or []
        if len(prompt) < 3 or not isinstance(prompt[2], dict):
            return
        extra = prompt[3] if len(prompt) > 3 and isinstance(prompt[3], dict) else {}
        # Make room first: every idle app below ComfyUI, and any idle LLM.
        for other in self.apps:
            u = self.usage.get(other.name, Usage())
            if other is not app and u.state != "busy" and (other.priority < app.priority or other.kind in LLM_KINDS):
                await self.free(other, f"to retry a {app.name} job that ran out of VRAM")
        body = {"prompt": prompt[2], "extra_data": extra, "client_id": extra.get("client_id") or "kernel"}
        try:
            r = await self.call("POST", f"{app.url}/prompt", body, 30)
        except (Offline, ActionError) as e:
            self.log.warning("couldn't queue the job again: %s", e)
            return
        if isinstance(r, dict) and r.get("prompt_id"):
            self.retried.add(str(r["prompt_id"]))
            self.retries += 1
            self.emit(
                "job.retried",
                f"A {app.name} job ran out of VRAM; made room and queued it again",
                failed=pid,
                retry=r["prompt_id"],
            )

    # -- actions --------------------------------------------------------------------------

    async def free_idle(self) -> dict[str, Any]:
        await self.refresh()
        freed = {}
        for app in self.apps:
            if app.kind in ("ollama", "comfyui") and self.usage[app.name].state != "busy":
                if mb := await self.free(app, "on request"):
                    freed[app.name] = mb
        await self.refresh()
        return {"freed_mb": freed, "free_mb": self.free_mb}

    async def focus(self, name: str) -> dict[str, Any]:
        target = self.app(name)
        await self.refresh()
        freed = {}
        for app in self.apps:
            if app is target or app.kind == "watch":
                continue
            if mb := await self.free(app, f"to give the GPU to {target.name}"):
                freed[app.name] = mb
        await self.refresh()
        return {"for": target.name, "freed_mb": freed, "free_mb": self.free_mb}

    async def free_one(self, name: str) -> dict[str, Any]:
        app = self.app(name)
        if app.kind == "watch":
            raise ActionError("not_permitted", f"{app.name} is only watched; Kernel never unloads it")
        await self.refresh()
        mb = await self.free(app, "on request")
        if not mb:
            state = self.usage[app.name].state
            reason = {
                "off": "it isn't running",
                "busy": "it's busy",
            }.get(state, "it has nothing loaded" if app.freeable else "it isn't marked stop_when_needed")
            return {"freed_mb": 0, "why": reason}
        await self.refresh()
        return {"freed_mb": mb, "free_mb": self.free_mb}

    def set_auto(self, on: bool) -> dict[str, Any]:
        self.auto = on
        self.snapshot["auto"] = on
        return {"auto": on}


def out_of_memory(entry: dict[str, Any]) -> bool:
    status = entry.get("status") or {}
    if status.get("status_str") != "error":
        return False
    for message in status.get("messages") or []:
        if isinstance(message, list) and len(message) == 2 and message[0] == "execution_error":
            text = f"{message[1].get('exception_type', '')} {message[1].get('exception_message', '')}".lower()
            return any(k in text for k in OOM)
    return False


def terminate(pids: list[int]) -> int:
    import psutil

    procs = []
    for pid in pids:
        try:
            procs.append(psutil.Process(pid))
        except psutil.Error:
            continue
    for p in procs:
        try:
            p.terminate()
        except psutil.Error:
            pass
    _, alive = psutil.wait_procs(procs, timeout=10)
    for p in alive:
        try:
            p.kill()
        except psutil.Error:
            pass
    return len(procs)
