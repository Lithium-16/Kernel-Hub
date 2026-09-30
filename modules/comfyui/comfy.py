"""ComfyUI through its HTTP API: status, queue, saved workflows, models and downloads.

Workflows are the files ComfyUI's "Export (API)" writes: a map of node id to
`{"class_type", "inputs"}`. Nothing is sent to ComfyUI that it couldn't get from its own UI.
"""

from __future__ import annotations

import asyncio
import json
import logging
import random
import re
import subprocess
import sys
import time
import urllib.error
import urllib.parse
import urllib.request
from collections.abc import Callable
from pathlib import Path
from typing import Any

from kernel_sdk import ActionError

MODEL_EXTENSIONS = (".safetensors", ".ckpt", ".pt", ".pth", ".bin", ".gguf", ".sft", ".onnx")
FOLDER = re.compile(r"^[A-Za-z0-9_\-]{1,64}$")
FILENAME = re.compile(r"^[^\\/:*?\"<>|\x00-\x1f]{1,200}$")
SEED_INPUTS = ("seed", "noise_seed")
PLACEHOLDER = "{{prompt}}"
NEGATIVE_PLACEHOLDER = "{{negative}}"
MAX_SEED = 2**53 - 1  # what ComfyUI's UI allows
# The download hosts tokens are sent to; everything else gets no credentials.
TOKEN_HOSTS = {"civitai.com": "civitai_token", "huggingface.co": "hf_token"}

Emit = Callable[..., None]
Fetch = Callable[[str, str, Any, float], Any]


class Offline(Exception):
    pass


def http_json(method: str, url: str, body: Any = None, timeout: float = 10) -> Any:
    """One JSON request. Raises Offline when ComfyUI isn't there, ActionError when it says no."""
    data = json.dumps(body).encode() if body is not None else None
    req = urllib.request.Request(url, data=data, method=method)
    if data is not None:
        req.add_header("Content-Type", "application/json")
    try:
        with urllib.request.urlopen(req, timeout=timeout) as r:
            raw = r.read()
    except urllib.error.HTTPError as e:
        text = e.read().decode("utf-8", "replace")
        try:
            return {"_status": e.code, **json.loads(text)}
        except ValueError:
            raise ActionError("module_failed", f"ComfyUI answered {e.code}: {text[:200]}") from None
    except (urllib.error.URLError, OSError) as e:
        raise Offline(str(e)) from None
    return json.loads(raw) if raw.strip() else None


# -- workflows -----------------------------------------------------------------------------


def load_workflow(path: Path) -> dict[str, Any]:
    try:
        data = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, ValueError) as e:
        raise ActionError("invalid_params", f"can't read {path.name}: {e}") from None
    if isinstance(data, dict) and "nodes" in data and "links" in data:
        raise ActionError(
            "invalid_params",
            f"{path.name} is a UI workflow. In ComfyUI, use Workflow > Export (API) and save that instead",
        )
    if not isinstance(data, dict) or not all(isinstance(n, dict) and "class_type" in n for n in data.values()):
        raise ActionError("invalid_params", f"{path.name} is not an API-format workflow")
    return data


def _replace_placeholder(graph: dict[str, Any], placeholder: str, text: str) -> bool:
    found = False
    for node in graph.values():
        for key, value in (node.get("inputs") or {}).items():
            if isinstance(value, str) and placeholder in value:
                node["inputs"][key] = value.replace(placeholder, text)
                found = True
    return found


def _sampler_text_node(graph: dict[str, Any], which: str) -> dict[str, Any] | None:
    """The text-encode node wired into a sampler's `positive` or `negative` input."""
    for node in graph.values():
        link = (node.get("inputs") or {}).get(which)
        if isinstance(link, list) and link and str(link[0]) in graph:
            source = graph[str(link[0])]
            if isinstance((source.get("inputs") or {}).get("text"), str):
                return source
    return None


def prepare(graph: dict[str, Any], prompt: str, negative: str, seed: int) -> dict[str, Any]:
    """A copy of the workflow with the prompt, negative prompt and seed filled in."""
    g = json.loads(json.dumps(graph))
    for text, placeholder, which in ((prompt, PLACEHOLDER, "positive"), (negative, NEGATIVE_PLACEHOLDER, "negative")):
        if not text:
            _replace_placeholder(g, placeholder, "")
            continue
        if not _replace_placeholder(g, placeholder, text):
            node = _sampler_text_node(g, which)
            if node is None:
                raise ActionError(
                    "invalid_params",
                    f"this workflow has no {placeholder} and no sampler {which} text box to put the prompt in",
                )
            node["inputs"]["text"] = text
    for node in g.values():
        inputs = node.get("inputs") or {}
        for key in SEED_INPUTS:
            if isinstance(inputs.get(key), int):
                inputs[key] = seed
    return g


# -- history -------------------------------------------------------------------------------


def summarize(prompt_id: str, entry: dict[str, Any]) -> dict[str, Any]:
    """One finished job from /history: ok or not, how long, which images."""
    status = entry.get("status") or {}
    messages = {m[0]: m[1] for m in status.get("messages") or [] if isinstance(m, list) and len(m) == 2}
    started = (messages.get("execution_start") or {}).get("timestamp")
    ended = (messages.get("execution_success") or messages.get("execution_error") or {}).get("timestamp")
    images = []
    for out in (entry.get("outputs") or {}).values():
        for img in out.get("images") or []:
            if img.get("type") == "output" and img.get("filename"):
                images.append(f"{img.get('subfolder') + '/' if img.get('subfolder') else ''}{img['filename']}")
    error = messages.get("execution_error")
    ok = status.get("status_str") == "success" or (status.get("completed") and not error)
    return {
        "id": prompt_id,
        "ok": bool(ok),
        "seconds": round((ended - started) / 1000, 1) if started and ended else None,
        "images": images,
        "error": f"{error.get('node_type', '?')}: {error.get('exception_message', '').strip()}" if error else None,
        "at": ended,
    }


# -- models --------------------------------------------------------------------------------


def check_folder(folder: str) -> str:
    if not FOLDER.match(folder):
        raise ActionError("invalid_params", f"'{folder}' is not a model folder name")
    return folder


def filename_from(url: str, headers: Any) -> str:
    disposition = headers.get("Content-Disposition") or ""
    m = re.search(r"filename\*=UTF-8''([^;]+)", disposition) or re.search(r'filename="?([^";]+)"?', disposition)
    if m:
        return urllib.parse.unquote(m.group(1)).strip()
    return urllib.parse.unquote(Path(urllib.parse.urlparse(url).path).name)


def normalize_url(url: str) -> str:
    """Hugging Face page links (`/blob/`) point at HTML; `/resolve/` is the file."""
    parsed = urllib.parse.urlparse(url.strip())
    if parsed.scheme != "https" or not parsed.hostname:
        raise ActionError("invalid_params", "downloads need an https:// link")
    if parsed.hostname.endswith("huggingface.co") and "/blob/" in parsed.path:
        parsed = parsed._replace(path=parsed.path.replace("/blob/", "/resolve/", 1))
    if parsed.hostname.endswith("civitai.com") and parsed.path.startswith("/models/"):
        raise ActionError(
            "invalid_params",
            "that's a Civitai page; use the file's download link (civitai.com/api/download/models/...)",
        )
    return urllib.parse.urlunparse(parsed)


class Download:
    def __init__(self, url: str, folder: str) -> None:
        self.url, self.folder = url, folder
        self.file = ""
        self.done = 0
        self.total: int | None = None
        self.state = "starting"
        self.started = time.monotonic()

    def row(self) -> dict[str, Any]:
        pct = round(100 * self.done / self.total) if self.total else None
        return {"file": self.file or self.url, "folder": self.folder, "state": self.state, "done_pct": pct}


# -- the module ----------------------------------------------------------------------------


class Comfy:
    def __init__(
        self,
        settings: dict[str, Any],
        emit: Emit | None = None,
        fetch: Fetch = http_json,
        log: logging.Logger | None = None,
    ) -> None:
        self.settings = settings
        self.url = str(settings["url"]).rstrip("/")
        if not self.url.startswith(("http://", "https://")):
            raise ValueError("url must start with http:// or https://")
        self.emit: Emit = emit or (lambda *args, **kwargs: None)
        self.fetch = fetch
        self.log = log or logging.getLogger("comfyui")
        base = Path(settings["comfy_dir"]) if settings["comfy_dir"] else None
        self.comfy_dir = base
        self.models_dir = Path(settings["models_dir"]) if settings["models_dir"] else self._default_models(base)
        self.workflows_dir = (
            Path(settings["workflows_dir"]) if settings["workflows_dir"] else base / "kernel-workflows" if base else None
        )
        self.snapshot: dict[str, Any] = {"state": "offline"}
        self.names: dict[str, str] = {}  # prompt id -> workflow name, for jobs we queued
        self.seen: set[str] | None = None  # finished prompt ids already reported
        self.recent: list[dict[str, Any]] = []
        self.batch = 0  # jobs finished since the queue was last empty
        self.downloads: list[Download] = []
        self.stopping = False
        self.process: subprocess.Popen[bytes] | None = None

    @staticmethod
    def _default_models(base: Path | None) -> Path | None:
        if base is None:
            return None
        portable = base / "ComfyUI" / "models"
        return portable if portable.is_dir() else base / "models"

    async def call(self, method: str, path: str, body: Any = None, timeout: float = 10) -> Any:
        return await asyncio.to_thread(self.fetch, method, self.url + path, body, timeout)

    async def api(self, method: str, path: str, body: Any = None, timeout: float = 10) -> Any:
        """Like `call`, but an offline ComfyUI is an action error."""
        try:
            return await self.call(method, path, body, timeout)
        except Offline:
            raise ActionError("offline", f"ComfyUI isn't running at {self.url}") from None

    # -- status ---------------------------------------------------------------------------

    async def refresh(self) -> dict[str, Any]:
        was = self.snapshot.get("state")
        try:
            stats = await self.call("GET", "/system_stats")
            queue = await self.call("GET", "/queue")
            history = await self.call("GET", "/history?max_items=32")
        except (Offline, ActionError, ValueError):
            snap: dict[str, Any] = {"state": "offline"}
            if was in ("idle", "working") and not self.stopping:
                self.emit("server.stopped", "ComfyUI stopped", level="warn")
            self.stopping = False  # we stopped it ourselves, and now it's gone
            self.snapshot = self._with_extras(snap)
            return self.snapshot
        running = queue.get("queue_running") or []
        pending = queue.get("queue_pending") or []
        snap = {
            "state": "working" if running else "idle",
            "queue_running": len(running),
            "queue_pending": len(pending),
        }
        if running:
            pid = str(running[0][1]) if len(running[0]) > 1 else ""
            snap["current"] = self.names.get(pid, "a job from ComfyUI")
        devices = stats.get("devices") or []
        if devices:
            d = devices[0]
            snap["gpu"] = str(d.get("name", "")).split(" : ")[0].removeprefix("cuda:0 ").strip() or None
            if d.get("vram_total"):
                snap["vram_used_mb"] = (d["vram_total"] - d.get("vram_free", 0)) // (1024 * 1024)
                snap["vram_total_mb"] = d["vram_total"] // (1024 * 1024)
        system = stats.get("system") or {}
        if system.get("comfyui_version"):
            snap["version"] = system["comfyui_version"]
        if was in ("offline", "stopping", "starting") and was is not None and self.seen is not None:
            self.emit("server.started", "ComfyUI is up")
        self._report(history or {}, busy=bool(running or pending))
        if self.recent:
            last = self.recent[0]
            snap["last_job"] = f"{'done' if last['ok'] else 'failed'}: {last['name']}" + (
                f" in {last['seconds']:g}s" if last["seconds"] else ""
            )
        self.snapshot = self._with_extras(snap)
        return self.snapshot

    def _with_extras(self, snap: dict[str, Any]) -> dict[str, Any]:
        if self.downloads:
            snap["downloads"] = [d.row() for d in self.downloads[-5:]]
        return snap

    def _report(self, history: dict[str, Any], busy: bool) -> None:
        """Events for jobs that finished since the last look."""
        ids = list(history)
        if self.seen is None:  # first look: what's already there isn't news
            self.seen = set(ids)
            self.recent = [self._named(summarize(i, history[i])) for i in reversed(ids[-10:])]
            return
        for pid in ids:
            if pid in self.seen:
                continue
            self.seen.add(pid)
            job = self._named(summarize(pid, history[pid]))
            self.recent = [job, *self.recent][:50]
            self.batch += 1
            if job["ok"]:
                count = len(job["images"])
                took = f" in {job['seconds']:g}s" if job["seconds"] else ""
                self.emit(
                    "job.done",
                    f"{job['name']} finished{took} ({count} image{'s' if count != 1 else ''})",
                    images=job["images"],
                    seconds=job["seconds"],
                )
            else:
                self.emit("job.failed", f"{job['name']} failed: {job['error'] or 'unknown error'}", level="warn")
        if not busy and self.batch:
            if self.batch > 1:
                self.emit("queue.finished", f"Queue finished: {self.batch} jobs", jobs=self.batch)
            self.batch = 0

    def _named(self, job: dict[str, Any]) -> dict[str, Any]:
        job["name"] = self.names.pop(job["id"], None) or "A job from ComfyUI"
        return job

    async def poll(self) -> None:
        while True:
            await self.refresh()
            await asyncio.sleep(float(self.settings["poll_s"]))

    # -- queue ----------------------------------------------------------------------------

    def workflow_files(self) -> list[Path]:
        if self.workflows_dir is None:
            raise ActionError("invalid_params", "set workflows_dir (or comfy_dir) in this module's settings")
        if not self.workflows_dir.is_dir():
            return []
        return sorted(
            (p for p in self.workflows_dir.iterdir() if p.suffix.lower() == ".json"), key=lambda p: p.stem.lower()
        )

    def list_workflows(self) -> dict[str, Any]:
        return {"folder": str(self.workflows_dir), "workflows": [p.stem for p in self.workflow_files()]}

    def find_workflow(self, name: str) -> Path:
        files = self.workflow_files()
        wanted = name.strip().lower().removesuffix(".json")
        for p in files:
            if p.stem.lower() == wanted:
                return p
        known = ", ".join(p.stem for p in files) or f"none yet (save them in {self.workflows_dir})"
        raise ActionError("invalid_params", f"no workflow '{name}'. Known: {known}")

    async def run_workflow(self, workflow: str, prompt: str, negative: str, seed: int, count: int) -> dict[str, Any]:
        path = self.find_workflow(workflow)
        graph = load_workflow(path)
        queued = []
        for i in range(count):
            s = random.randint(0, MAX_SEED) if seed < 0 else seed + i
            body = {"prompt": prepare(graph, prompt, negative, s), "client_id": "kernel"}
            r = await self.api("POST", "/prompt", body)
            if not isinstance(r, dict) or "prompt_id" not in r:
                errors = r.get("node_errors") if isinstance(r, dict) else None
                detail = (r or {}).get("error", {}).get("message") if isinstance(r, dict) else None
                message = detail or "ComfyUI refused the workflow"
                if errors:
                    first = next(iter(errors.values()))
                    reasons = "; ".join(e.get("message", "") for e in first.get("errors", []))
                    message += f" ({first.get('class_type', '?')}: {reasons})"
                raise ActionError("invalid_params", message)
            self.names[r["prompt_id"]] = path.stem
            queued.append({"id": r["prompt_id"], "seed": s})
        return {"workflow": path.stem, "queued": queued}

    async def interrupt(self) -> dict[str, Any]:
        await self.api("POST", "/interrupt", {})
        return {"interrupted": True}

    async def clear(self) -> dict[str, Any]:
        queue = await self.api("GET", "/queue")
        pending = len(queue.get("queue_pending") or [])
        await self.api("POST", "/queue", {"clear": True})
        return {"removed": pending}

    async def free(self) -> dict[str, Any]:
        await self.api("POST", "/free", {"unload_models": True, "free_memory": True})
        return {"freed": True}

    def history(self, limit: int) -> dict[str, Any]:
        return {"jobs": self.recent[:limit]}

    # -- models ---------------------------------------------------------------------------

    async def list_models(self, folder: str) -> dict[str, Any]:
        folder = check_folder(folder)
        if self.models_dir is not None and (self.models_dir / folder).is_dir():
            root = self.models_dir / folder
            models = []
            for p in sorted(root.rglob("*"), key=lambda p: str(p).lower()):
                if p.is_file() and p.suffix.lower() in MODEL_EXTENSIONS:
                    models.append({"file": p.relative_to(root).as_posix(), "bytes": p.stat().st_size})
            return {"folder": folder, "count": len(models), "models": models}
        names = await self.api("GET", f"/models/{folder}")
        if not isinstance(names, list):
            raise ActionError("invalid_params", f"ComfyUI has no model folder '{folder}'")
        return {"folder": folder, "count": len(names), "models": [{"file": n} for n in names]}

    def start_download(self, url: str, folder: str, filename: str) -> dict[str, Any]:
        folder = check_folder(folder)
        url = normalize_url(url)
        if self.models_dir is None:
            raise ActionError("invalid_params", "set models_dir (or comfy_dir) in this module's settings")
        target = self.models_dir / folder
        if not target.is_dir():
            raise ActionError("invalid_params", f"there's no model folder '{folder}' in {self.models_dir}")
        if filename:
            self._check_filename(filename)
        if sum(d.state in ("starting", "downloading") for d in self.downloads) >= 2:
            raise ActionError("busy", "two downloads are already running")
        d = Download(url, folder)
        d.file = filename
        self.downloads = [x for x in self.downloads if x.state in ("starting", "downloading")][-4:] + [d]
        asyncio.get_running_loop().create_task(self._download(d, target))
        return {"downloading": filename or url, "to": str(target)}

    @staticmethod
    def _check_filename(name: str) -> None:
        if not FILENAME.match(name) or name in (".", "..") or not name.lower().endswith(MODEL_EXTENSIONS):
            raise ActionError(
                "invalid_params", f"'{name}' should be a plain file name ending in {', '.join(MODEL_EXTENSIONS)}"
            )

    async def _download(self, d: Download, target: Path) -> None:
        try:
            await asyncio.to_thread(self._fetch_file, d, target)
        except Exception as e:  # reported as an event; the action already returned
            d.state = "failed"
            message = e.message if isinstance(e, ActionError) else str(e)
            self.emit("download.failed", f"Download of {d.file or d.url} failed: {message}", level="warn")
            return
        d.state = "done"
        mb = f" ({d.done / 1e6:,.0f} MB)" if d.done else ""
        self.emit("download.done", f"Downloaded {d.file} to {d.folder}{mb}", file=d.file, folder=d.folder)

    def _fetch_file(self, d: Download, target: Path) -> None:
        req = urllib.request.Request(d.url, headers={"User-Agent": "kernel-comfyui/0.1"})
        host = urllib.parse.urlparse(d.url).hostname or ""
        for domain, key in TOKEN_HOSTS.items():
            if (host == domain or host.endswith("." + domain)) and self.settings.get(key):
                req.add_header("Authorization", f"Bearer {self.settings[key]}")
        try:
            resp = urllib.request.urlopen(req, timeout=30)
        except urllib.error.HTTPError as e:
            hint = " (needs a token? set civitai_token or hf_token)" if e.code in (401, 403) else ""
            raise ActionError("module_failed", f"the server answered {e.code}{hint}") from None
        with resp:
            name = d.file or filename_from(resp.geturl(), resp.headers)
            self._check_filename(name)
            final = target / name
            if final.exists():
                raise ActionError("invalid_params", f"{name} is already in {d.folder}")
            d.file = name
            d.total = int(resp.headers.get("Content-Length") or 0) or None
            d.state = "downloading"
            part = final.with_name(final.name + ".part")
            try:
                with part.open("wb") as f:
                    while chunk := resp.read(1024 * 1024):
                        f.write(chunk)
                        d.done += len(chunk)
                if d.total and d.done != d.total:
                    raise ActionError("module_failed", f"got {d.done} of {d.total} bytes")
                part.replace(final)
            finally:
                part.unlink(missing_ok=True)

    # -- the ComfyUI process --------------------------------------------------------------

    async def start(self) -> dict[str, Any]:
        command = list(self.settings["start_command"])
        if not command or self.comfy_dir is None:
            raise ActionError("invalid_params", "set comfy_dir and start_command in this module's settings")
        if (await self.refresh())["state"] != "offline":
            return {"already": "running"}
        log_path = self.comfy_dir / "kernel-comfyui.log"
        flags = 0
        if sys.platform == "win32":
            flags = subprocess.CREATE_NO_WINDOW | subprocess.CREATE_NEW_PROCESS_GROUP
        with log_path.open("ab") as log:
            try:
                self.process = subprocess.Popen(
                    command,
                    cwd=self.comfy_dir,
                    stdin=subprocess.DEVNULL,
                    stdout=log,
                    stderr=subprocess.STDOUT,
                    creationflags=flags,
                )
            except OSError as e:
                raise ActionError("module_failed", f"couldn't start ComfyUI: {e}") from None
        self.snapshot = {**self.snapshot, "state": "starting"}
        deadline = time.monotonic() + 120
        while time.monotonic() < deadline:
            await asyncio.sleep(2)
            if self.process.poll() is not None:
                raise ActionError("module_failed", f"ComfyUI exited with code {self.process.returncode}; see {log_path}")
            if (await self.refresh())["state"] != "offline":
                return {"started": True, "log": str(log_path)}
        raise ActionError("timeout", f"ComfyUI didn't answer within 2 minutes; see {log_path}")

    async def stop(self) -> dict[str, Any]:
        import psutil

        port = urllib.parse.urlparse(self.url).port or 80
        pids = set()
        if self.process is not None and self.process.poll() is None:
            pids.add(self.process.pid)
        try:
            for c in psutil.net_connections(kind="tcp"):
                if c.status == psutil.CONN_LISTEN and c.laddr and c.laddr.port == port and c.pid:
                    pids.add(c.pid)
        except psutil.AccessDenied:
            pass
        procs = []
        for pid in pids:
            try:
                p = psutil.Process(pid)
                procs += [p, *p.children(recursive=True)]
            except psutil.Error:
                continue
        if not procs:
            return {"stopped": 0, "message": "ComfyUI wasn't running"}
        self.stopping = True
        for p in procs:
            try:
                p.terminate()
            except psutil.Error:
                pass
        _, alive = await asyncio.to_thread(psutil.wait_procs, procs, 10)
        for p in alive:
            try:
                p.kill()
            except psutil.Error:
                pass
        return {"stopped": len(procs)}
