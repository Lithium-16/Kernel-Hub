"""Module runtime: exposes manifest actions as MCP tools and status as an MCP resource."""

from __future__ import annotations

import asyncio
import inspect
import json
import logging
import os
import re
import sys
from collections import deque
from collections.abc import Awaitable, Callable
from dataclasses import dataclass
from pathlib import Path
from typing import Any

from mcp import types
from mcp.server import Server
from mcp.server.stdio import stdio_server

from .manifest import Action, Manifest, Param, load_manifest, load_settings

STATUS_URI = "kernel://status"
EVENTS_URI = "kernel://events"
EVENT_LEVELS = ("info", "warn", "error")
EVENT_KIND = re.compile(r"^[a-z][a-z0-9_]*(\.[a-z][a-z0-9_]*)*$")

ERROR_CODES = frozenset(
    {"invalid_params", "module_failed", "timeout", "busy", "not_permitted", "disabled", "offline", "internal"}
)

log = logging.getLogger("kernel_sdk")

Handler = Callable[..., Any | Awaitable[Any]]


class ActionError(Exception):
    """Raise from an action handler to return a typed error to Kernel."""

    def __init__(self, code: str, message: str) -> None:
        if code not in ERROR_CODES:
            raise ValueError(f"unknown error code '{code}'")
        super().__init__(message)
        self.code = code
        self.message = message


@dataclass
class ActionContext:
    module: Manifest
    action: Action
    log: logging.Logger


def _check_param(p: Param, value: Any) -> Any:
    ok = {
        "int": isinstance(value, int) and not isinstance(value, bool),
        "float": isinstance(value, (int, float)) and not isinstance(value, bool),
        "string": isinstance(value, str),
        "bool": isinstance(value, bool),
        "enum": isinstance(value, str) and value in p.options,
    }[p.type]
    if not ok:
        expected = f"one of {list(p.options)}" if p.type == "enum" else p.type
        raise ActionError("invalid_params", f"'{p.name}' must be {expected}")
    if p.type in ("int", "float"):
        if p.min is not None and value < p.min:
            raise ActionError("invalid_params", f"'{p.name}' must be at least {p.min}")
        if p.max is not None and value > p.max:
            raise ActionError("invalid_params", f"'{p.name}' must be at most {p.max}")
        if p.type == "float":
            value = float(value)
    return value


def resolve_params(action: Action, given: dict[str, Any]) -> dict[str, Any]:
    known = {p.name for p in action.params}
    unknown = sorted(set(given) - known)
    if unknown:
        raise ActionError("invalid_params", f"unknown parameter(s): {', '.join(unknown)}")
    out: dict[str, Any] = {}
    for p in action.params:
        if p.name in given:
            out[p.name] = _check_param(p, given[p.name])
        elif p.has_default:
            out[p.name] = p.default
        else:
            raise ActionError("invalid_params", f"missing required parameter '{p.name}'")
    return out


def _find_manifest() -> Path:
    env = os.environ.get("KERNEL_MODULE_DIR")
    candidates = [Path(env)] if env else []
    main = sys.modules.get("__main__")
    if main is not None and getattr(main, "__file__", None):
        candidates.append(Path(main.__file__).resolve().parent)
    candidates.append(Path.cwd())
    for d in candidates:
        if (d / "module.toml").is_file():
            return d / "module.toml"
    raise FileNotFoundError("module.toml not found (set KERNEL_MODULE_DIR)")


class Module:
    """A Kernel module. Register handlers with `@mod.action(...)` and `@mod.status`, then `mod.run()`."""

    def __init__(self, manifest: Manifest | Path | str | None = None) -> None:
        if isinstance(manifest, Manifest):
            self.manifest = manifest
        else:
            self.manifest = load_manifest(Path(manifest) if manifest else _find_manifest())
        self._handlers: dict[str, Handler] = {}
        self._status: Handler | None = None
        self._background: list[Callable[[], Awaitable[None]]] = []
        # Events wait here until the node reads them (every couple of seconds).
        self._events: deque[dict[str, Any]] = deque(maxlen=200)
        self.settings = load_settings(self.manifest)
        self.log = logging.getLogger(f"kernel.{self.manifest.id}")

    @property
    def data_dir(self) -> Path:
        """A folder for the module's own files (downloads, saves), kept across Kernel updates.

        Set by the node; when run by hand, `data/` next to module.toml. Created on first use.
        """
        env = os.environ.get("KERNEL_DATA_DIR")
        path = Path(env) if env else (self.manifest.root or Path.cwd()) / "data"
        path.mkdir(parents=True, exist_ok=True)
        return path

    def action(self, action_id: str) -> Callable[[Handler], Handler]:
        spec = self.manifest.action(action_id)
        if spec is None:
            raise KeyError(f"action '{action_id}' is not declared in module.toml")

        def register(fn: Handler) -> Handler:
            self._handlers[action_id] = fn
            return fn

        return register

    def status(self, fn: Handler) -> Handler:
        self._status = fn
        return fn

    def emit(self, kind: str, message: str, level: str = "info", **data: Any) -> None:
        """Report something that happened, like `mod.emit("player.joined", "Steve joined", player="Steve")`.

        The node keeps it, shows it in the app and can send it to Discord. `level` is info, warn
        or error; `kind` is a short dotted name. Extra keyword arguments must be JSON values.
        """
        if not EVENT_KIND.match(kind):
            raise ValueError(f"event kind '{kind}' must look like 'server.crashed'")
        if level not in EVENT_LEVELS:
            raise ValueError(f"event level must be one of {EVENT_LEVELS}")
        json.dumps(data)
        self.log.info("event %s: %s", kind, message)
        self._events.append({"kind": kind, "level": level, "message": message, "data": data})

    def take_events(self) -> list[dict[str, Any]]:
        """Events not yet read by the node; reading clears them."""
        events = list(self._events)
        self._events.clear()
        return events

    def background(self, fn: Callable[[], Awaitable[None]]) -> Callable[[], Awaitable[None]]:
        """Run a coroutine for the life of the module. It is restarted if it raises."""
        self._background.append(fn)
        return fn

    async def _keep_running(self, fn: Callable[[], Awaitable[None]]) -> None:
        while True:
            try:
                await fn()
                return
            except asyncio.CancelledError:
                raise
            except Exception:
                self.log.exception("background task %s failed; restarting in 5s", fn.__name__)
                await asyncio.sleep(5)

    def validate(self) -> None:
        missing = [a.id for a in self.manifest.actions if a.id not in self._handlers]
        if missing:
            raise RuntimeError(f"no handler for declared action(s): {', '.join(missing)}")

    async def _call(self, fn: Handler, *args: Any, **kwargs: Any) -> Any:
        result = fn(*args, **kwargs)
        if inspect.isawaitable(result):
            result = await result
        return result

    async def invoke(self, action_id: str, params: dict[str, Any]) -> Any:
        """Validate params and run a handler. Raises ActionError on failure."""
        spec = self.manifest.action(action_id)
        if spec is None or action_id not in self._handlers:
            raise ActionError("invalid_params", f"unknown action '{action_id}'")
        args = resolve_params(spec, params)
        ctx = ActionContext(module=self.manifest, action=spec, log=self.log)
        try:
            return await asyncio.wait_for(self._call(self._handlers[action_id], ctx, **args), spec.timeout_s)
        except TimeoutError:
            raise ActionError("timeout", f"'{action_id}' took longer than {spec.timeout_s:g}s") from None
        except ActionError:
            raise
        except Exception as e:
            self.log.exception("action %s failed", action_id)
            raise ActionError("module_failed", f"{type(e).__name__}: {e}") from e

    async def current_status(self) -> dict[str, Any]:
        if self._status is None:
            return {}
        value = await self._call(self._status)
        if not isinstance(value, dict):
            raise TypeError("status provider must return a dict")
        return value

    def server(self) -> Server[Any]:
        """Build the MCP server. Tool names are action ids with dots replaced by `__`."""
        self.validate()
        m = self.manifest

        async def list_tools(ctx: Any, params: Any) -> types.ListToolsResult:
            return types.ListToolsResult(
                tools=[
                    types.Tool(
                        name=a.tool_name,
                        title=a.label,
                        description=a.description or a.label,
                        input_schema=a.input_schema(),
                        meta={"kernel/action": a.id, "kernel/ai": a.ai, "kernel/quiet": a.quiet},
                    )
                    for a in m.actions
                ]
            )

        async def call_tool(ctx: Any, params: types.CallToolRequestParams) -> types.CallToolResult:
            action = m.action_for_tool(params.name)
            try:
                if action is None:
                    raise ActionError("invalid_params", f"unknown tool '{params.name}'")
                result = await self.invoke(action.id, dict(params.arguments or {}))
            except ActionError as e:
                body = {"code": e.code, "message": e.message}
                return types.CallToolResult(
                    content=[types.TextContent(type="text", text=json.dumps(body))],
                    structured_content={"error": body},
                    is_error=True,
                )
            return types.CallToolResult(
                content=[types.TextContent(type="text", text=json.dumps(result))],
                structured_content={"result": result},
            )

        async def list_resources(ctx: Any, params: Any) -> types.ListResourcesResult:
            return types.ListResourcesResult(
                resources=[
                    types.Resource(name="status", uri=STATUS_URI, mime_type="application/json"),
                    types.Resource(name="events", uri=EVENTS_URI, mime_type="application/json"),
                ]
            )

        async def read_resource(ctx: Any, params: types.ReadResourceRequestParams) -> types.ReadResourceResult:
            uri = str(params.uri)
            if uri == STATUS_URI:
                value: Any = await self.current_status()
            elif uri == EVENTS_URI:
                value = self.take_events()
            else:
                raise ValueError(f"unknown resource {params.uri}")
            return types.ReadResourceResult(
                contents=[types.TextResourceContents(uri=uri, mime_type="application/json", text=json.dumps(value))]
            )

        return Server(
            m.id,
            version=m.version,
            on_list_tools=list_tools,
            on_call_tool=call_tool,
            on_list_resources=list_resources,
            on_read_resource=read_resource,
        )

    async def run_async(self) -> None:
        server = self.server()
        async with stdio_server() as (read, write):
            tasks = [asyncio.create_task(self._keep_running(fn)) for fn in self._background]
            try:
                await server.run(read, write, server.create_initialization_options())
            finally:
                for t in tasks:
                    t.cancel()
                await asyncio.gather(*tasks, return_exceptions=True)

    def run(self) -> None:
        logging.basicConfig(
            level=os.environ.get("KERNEL_LOG_LEVEL", "INFO"),
            stream=sys.stderr,
            format="%(asctime)s %(levelname)s %(name)s: %(message)s",
        )
        asyncio.run(self.run_async())
