import asyncio
import json

import pytest
from decider import Decider, install_command
from kernel_sdk import ActionError

OPTIONS = {
    "minecraft__server_start": "Minecraft: Start server",
    "minecraft__server_stop": "Minecraft: Stop server",
    "status:minecraft": "Minecraft: read its status",
    "comfyui__workflow_run": "ComfyUI: Run workflow",
    "vram__memory_free": "VRAM: Free up VRAM",
}


class FakeEngine:
    """Shortlists by shared words; prefers `favourite` with the given probability."""

    name = "english"

    def __init__(self, favourite="status:minecraft", p=0.9, fits=99):
        self.favourite, self.p, self.fits = favourite, p, fits
        self.calls = []

    def shortlist(self, state, options, k):
        words = set(state["message"].lower().split())
        return sorted(options, key=lambda o: -len(words & set(options[o].lower().split())))[:k]

    def predict(self, state, questions):
        crit = questions["tool"]["criteria"]
        self.calls.append((state, list(crit)))
        if len(crit) > self.fits:
            raise ValueError("question 'tool': only 3 of its 5 option markers fit in max_len=512")
        rest = (1 - self.p) / max(1, len(crit) - 1)
        probs = {k: (self.p if k == self.favourite else rest) for k in crit}
        choice = max(probs, key=probs.get)
        return {
            "answers": {
                "tool": {"choice": choice, "probabilities": probs, "answer_confidence": probs[choice]},
                "hard": {"noul": 0.2},
            },
            "routing": {"model": "english"},
        }


def decider(engine=None, installed=True):
    return Decider({"preload": True}, engine_factory=lambda s: engine or FakeEngine(), is_installed=lambda: installed)


async def test_ranks_the_tools_for_a_message():
    d = decider(FakeEngine(favourite="status:minecraft", p=0.93))
    await d.load()
    out = await d.decide("is the minecraft server up?", "", json.dumps(OPTIONS), keep=4, pick=3)
    assert out["top"] == "status:minecraft" and out["confidence"] == pytest.approx(0.93)
    assert len(out["ranked"]) == 4 and out["ranked"][0] == "status:minecraft"
    assert out["hard"] == 0.2 and not out["shortlisted"]
    assert d.engine.calls[0][1] == list(OPTIONS)  # chose among all of them at once
    assert d.snapshot()["decisions"] == 1 and d.snapshot()["state"] == "ready"


async def test_uses_fewer_options_when_they_dont_fit():
    engine = FakeEngine(favourite="minecraft__server_start", fits=3)
    d = decider(engine)
    await d.load()
    out = await d.decide("start minecraft server", "the one before", json.dumps(OPTIONS), keep=4, pick=4)
    # all 5 don't fit; the 4 closest don't either; 2 do.
    assert [len(c) for _, c in engine.calls] == [5, 4, 2]
    assert out["shortlisted"]
    assert out["top"] == "minecraft__server_start"
    assert engine.calls[0][0] == {"message": "start minecraft server", "before": "the one before"}


async def test_not_ready_answers_quickly_and_starts_loading():
    loaded = asyncio.Event()

    def slow(settings):
        loaded.set()
        return FakeEngine()

    d = Decider({"preload": False}, engine_factory=slow, is_installed=lambda: True)
    with pytest.raises(ActionError) as e:
        await d.decide("hi", "", '{"a": "A", "b": "B"}', 4, 2)
    assert e.value.code == "busy"
    await asyncio.wait_for(loaded.wait(), 2)
    await asyncio.sleep(0.05)
    assert d.state == "ready"


async def test_not_installed():
    d = decider(installed=False)
    await d.start()  # doesn't try to load
    assert d.snapshot()["state"] == "not installed"
    with pytest.raises(ActionError) as e:
        await d.decide("hi", "", '{"a": "A", "b": "B"}', 4, 2)
    assert e.value.code == "disabled"


async def test_a_failed_load_is_reported():
    events = []

    def broken(settings):
        raise OSError("no network")

    d = Decider({}, emit=lambda kind, msg, **k: events.append(kind), engine_factory=broken, is_installed=lambda: True)
    with pytest.raises(ActionError):
        await d.load()
    assert d.snapshot()["state"] == "failed" and "no network" in d.snapshot()["error"]
    assert events == ["laya.failed"]


async def test_bad_options():
    d = decider()
    await d.load()
    with pytest.raises(ActionError):
        await d.decide("hi", "", "not json", 4, 2)
    with pytest.raises(ActionError):
        await d.decide("hi", "", "{}", 4, 2)


def test_install_command_uses_the_given_python():
    cmd = install_command("C:/Kernel/python/Scripts/python.exe")
    assert cmd[-1] == "laya" and "C:/Kernel/python/Scripts/python.exe" in cmd


def test_manifest_matches_the_handlers():
    from pathlib import Path

    from kernel_sdk.manifest import load_manifest

    m = load_manifest(Path(__file__).resolve().parent.parent / "module.toml")
    assert [a.id for a in m.actions] == ["tools.rank", "model.load", "model.unload", "setup.install"]
    assert m.action("tools.rank").quiet
