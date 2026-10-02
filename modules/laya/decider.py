"""Asks LAYA which of the assistant's tools a message is about.

LAYA answers typed questions about a piece of text in one forward pass. It chooses among all
the options at once when they fit its token budget; past that, it narrows them by embedding
similarity (`shortlist_choice`) first and chooses among the closest few.

The node's assistant sends the tool list with every call (name -> one line about it) and gets
back the tools ranked, how sure LAYA is of the first one, and the probability that the message
needs several steps.

Measured zero-shot on Kernel-style requests it ranks well but is overconfident (wrong answers at
95%+), so the assistant only ever uses it to read a status early or to shorten the tool list,
never to press a button.
"""

from __future__ import annotations

import asyncio
import importlib
import importlib.util
import json
import logging
import os
import shutil
import subprocess
import sys
import threading
import time
from collections.abc import Callable
from pathlib import Path
from typing import Any, Protocol

from kernel_sdk import ActionError

TOOL_QUESTION = "Which button or status check does `message` ask for?"
HARD_QUESTION = (
    "Does `message` need several different steps, planning or general knowledge, rather than "
    "one status check or one button?"
)
# The question and its options share this many of the model's 512 tokens; a chat message
# rarely needs more than the rest (a longer one is cut, which is fine for picking a tool).
HEAD_TOKENS = 448


class Engine(Protocol):
    name: str

    def shortlist(self, state: dict[str, str], options: dict[str, str], k: int) -> list[str]: ...

    def predict(self, state: dict[str, str], questions: dict[str, Any]) -> dict[str, Any]: ...


class LayaEngine:
    """The real thing: one checkpoint, loaded once."""

    def __init__(self, model: str, device: str, token: str) -> None:
        from laya import Router, cached_embed_fn, embed_fn_from_agent, shortlist_choice

        self.name = model or "english"
        self.router = Router(device=device or None, token=token or None, max_loaded=1)
        agent = self.router.load(self.name)
        self.embed = cached_embed_fn(embed_fn_from_agent(agent))
        self._shortlist = shortlist_choice

    def shortlist(self, state: dict[str, str], options: dict[str, str], k: int) -> list[str]:
        return list(self._shortlist(state, options, self.embed, k=k, instructions=TOOL_QUESTION))

    def predict(self, state: dict[str, str], questions: dict[str, Any]) -> dict[str, Any]:
        return self.router.predict(state, questions, model=self.name, head_max_len=HEAD_TOKENS)

    def close(self) -> None:
        self.router.unload()


def installed() -> bool:
    importlib.invalidate_caches()
    return importlib.util.find_spec("laya") is not None


def install_command(python: str = sys.executable) -> list[str]:
    """uv if Kernel's installer put it on this PC, pip otherwise."""
    uv = shutil.which("uv") or next(
        (str(p) for p in (Path.home() / ".local" / "bin" / n for n in ("uv.exe", "uv")) if p.is_file()),
        None,
    )
    if uv:
        return [uv, "pip", "install", "--python", python, "laya"]
    return [python, "-m", "pip", "install", "laya"]


def ranked(result: dict[str, Any], picked: list[str], rest: list[str]) -> tuple[list[str], float]:
    """LAYA's choices by probability, then the shortlist's remainder; and the top's confidence."""
    answer = result["answers"]["tool"]
    probs = answer.get("probabilities") or {}
    order = sorted(picked, key=lambda k: -float(probs.get(k, 0.0)))
    if answer.get("choice") in picked:  # the argmax first, even on equal probabilities
        order.remove(answer["choice"])
        order.insert(0, answer["choice"])
    return order + [k for k in rest if k not in order], float(answer.get("answer_confidence", 0.0))


class Decider:
    def __init__(
        self,
        settings: dict[str, Any],
        emit: Callable[..., None] | None = None,
        log: logging.Logger | None = None,
        engine_factory: Callable[[dict[str, Any]], Engine] | None = None,
        is_installed: Callable[[], bool] = installed,
    ) -> None:
        self.settings = settings
        self.emit = emit or (lambda *a, **k: None)
        self.log = log or logging.getLogger("laya")
        self.engine_factory = engine_factory or (
            lambda s: LayaEngine(str(s.get("model", "english")), str(s.get("device", "cpu")), str(s.get("hf_token", "")))
        )
        self.is_installed = is_installed
        self.engine: Engine | None = None
        self.state = "idle"
        self.error = ""
        self.installing = False
        self.decisions = 0
        self.total_ms = 0.0
        self.last_ms: float | None = None
        self._lock = threading.Lock()
        self._loading: asyncio.Task[None] | None = None

    # -- the model ------------------------------------------------------------------------

    async def start(self) -> None:
        """Background task: install the package if it's missing (auto_install), then load the
        model if asked to."""
        if not self.is_installed():
            if not self.settings.get("auto_install", True):
                return
            try:
                await self.install()  # loads the model afterwards when preload is on
            except ActionError as e:
                self.state, self.error = "failed", e.message
            return
        if self.settings.get("preload", True):
            await self.load()

    def _load_blocking(self) -> None:
        with self._lock:
            if self.engine is not None:
                return
            if self.settings.get("hf_token"):
                os.environ.setdefault("HF_TOKEN", str(self.settings["hf_token"]))
            started = time.monotonic()
            self.engine = self.engine_factory(self.settings)
            self.log.info("LAYA %s loaded in %.1fs", self.engine.name, time.monotonic() - started)

    async def _load(self) -> None:
        self.state, self.error = "loading", ""
        try:
            await asyncio.to_thread(self._load_blocking)
        except Exception as e:  # a missing package, no network for the download, out of RAM
            self.state, self.error = "failed", f"{type(e).__name__}: {e}"
            self.log.exception("loading LAYA failed")
            self.emit("laya.failed", f"LAYA couldn't load: {self.error}", level="warn")
            return
        self.state = "ready"
        self.emit("laya.ready", f"LAYA ({self.engine.name if self.engine else '?'}) is ready")

    async def load(self) -> dict[str, Any]:
        if self.engine is not None:
            return {"state": "ready"}
        if not self.is_installed():
            raise ActionError("disabled", "LAYA isn't installed yet. Press Install LAYA on its page.")
        if self._loading is None or self._loading.done():
            self._loading = asyncio.ensure_future(self._load())
        await self._loading
        if self.engine is None:
            raise ActionError("module_failed", self.error or "the model didn't load")
        return {"state": "ready"}

    def unload(self) -> dict[str, Any]:
        with self._lock:
            engine, self.engine = self.engine, None
        close = getattr(engine, "close", None)
        if close is not None:
            close()
        self.state = "idle"
        return {"state": "idle"}

    # -- deciding -------------------------------------------------------------------------

    def _ask(self, engine: Engine, state: dict[str, str], options: dict[str, str], picked: list[str]) -> dict[str, Any]:
        questions = {
            "tool": {"type": "choice", "instructions": TOOL_QUESTION, "criteria": {k: options[k] for k in picked}},
            "hard": {"type": "noul", "instructions": HARD_QUESTION},
        }
        return engine.predict(state, questions)

    def _decide_blocking(
        self, engine: Engine, text: str, context: str, options: dict[str, str], keep: int, pick: int
    ) -> dict[str, Any]:
        state = {"message": text}
        if context:
            state["before"] = context
        started = time.monotonic()
        everything = list(options)
        try:
            # Choosing among every option at once is what LAYA does best (on a 16-request test
            # set: the right tool first 11 times, in the top four 15 times).
            picked, short = everything, everything
            result = self._ask(engine, state, options, picked)
        except ValueError as e:
            if "option markers" not in str(e):
                raise
            # Too many to fit its token budget: narrow by similarity first (much weaker), then
            # choose among the closest few, fewer again if those don't fit either.
            short = engine.shortlist(state, options, min(keep, len(options) - 1))
            pick = max(2, min(pick, len(short)))
            while True:
                picked = short[:pick]
                try:
                    result = self._ask(engine, state, options, picked)
                    break
                except ValueError as e:
                    if pick <= 2 or "option markers" not in str(e):
                        raise
                    pick = max(2, pick // 2)
        order, confidence = ranked(result, picked, short)
        return {
            "ranked": order[:keep],
            "top": order[0],
            "confidence": round(confidence, 4),
            "hard": float(result["answers"]["hard"]["noul"]),
            "shortlisted": short is not everything,
            "model": str((result.get("routing") or {}).get("model", engine.name)),
            "ms": round((time.monotonic() - started) * 1000, 1),
        }

    async def decide(self, text: str, context: str, options: str, keep: int, pick: int) -> dict[str, Any]:
        try:
            parsed = json.loads(options)
        except json.JSONDecodeError as e:
            raise ActionError("invalid_params", f"options isn't JSON: {e}") from None
        if not isinstance(parsed, dict) or not parsed:
            raise ActionError("invalid_params", "options must be a non-empty object of name -> description")
        opts = {str(k): str(v) for k, v in parsed.items()}
        engine = self.engine
        if engine is None:
            if not self.is_installed():
                raise ActionError("disabled", "LAYA isn't installed")
            if self.state != "failed" and (self._loading is None or self._loading.done()):
                self._loading = asyncio.ensure_future(self._load())  # ready for the next message
            raise ActionError("busy", "LAYA's model isn't loaded yet")
        if len(opts) == 1:
            only = next(iter(opts))
            return {"ranked": [only], "top": only, "confidence": 1.0, "hard": 0.0, "shortlisted": False, "model": engine.name, "ms": 0.0}
        out = await asyncio.to_thread(self._decide_blocking, engine, text, context, opts, keep, pick)
        self.decisions += 1
        self.total_ms += out["ms"]
        self.last_ms = out["ms"]
        return out

    # -- setup ----------------------------------------------------------------------------

    async def install(self) -> dict[str, Any]:
        if self.installing:
            raise ActionError("busy", "already installing")
        self.installing = True
        cmd = install_command()
        self.log.info("installing: %s", " ".join(cmd))
        try:
            flags = subprocess.CREATE_NO_WINDOW if sys.platform == "win32" else 0
            proc = await asyncio.create_subprocess_exec(
                *cmd,
                stdin=subprocess.DEVNULL,
                stdout=subprocess.PIPE,
                stderr=subprocess.STDOUT,
                creationflags=flags,
            )
            out, _ = await proc.communicate()
        finally:
            self.installing = False
        tail = out.decode(errors="replace").strip().splitlines()[-15:]
        if proc.returncode != 0:
            self.emit("laya.install_failed", "Installing LAYA failed", level="error")
            raise ActionError("module_failed", "installing failed:\n" + "\n".join(tail))
        self.emit("laya.installed", "LAYA is installed")
        if self.settings.get("preload", True):
            self._loading = asyncio.ensure_future(self._load())
        return {"installed": True, "output": "\n".join(tail)}

    def snapshot(self) -> dict[str, Any]:
        is_installed = self.is_installed()
        state = self.state
        if self.installing:
            state = "installing"
        elif not is_installed and self.engine is None:
            state = "not installed"
        return {
            "state": state,
            "model": self.engine.name if self.engine else str(self.settings.get("model", "english")),
            "device": str(self.settings.get("device", "cpu")),
            "decisions": self.decisions,
            "last_ms": None if self.last_ms is None else round(self.last_ms),
            "avg_ms": round(self.total_ms / self.decisions) if self.decisions else None,
            "error": self.error,
        }
