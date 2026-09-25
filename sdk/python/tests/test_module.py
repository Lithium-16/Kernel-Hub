import json

import pytest
from mcp import Client

from kernel_sdk import STATUS_URI, ActionError, Module, parse_manifest

MANIFEST = parse_manifest(
    {
        "id": "demo",
        "name": "Demo",
        "icon": "box",
        "version": "1.0.0",
        "runtime": "python",
        "entry": "main.py",
        "actions": [
            {
                "id": "greet.say",
                "label": "Greet",
                "ai": "safe",
                "params": {
                    "name": {"type": "string", "default": "world"},
                    "times": {"type": "int", "default": 1, "min": 1, "max": 3},
                    "tone": {"type": "enum", "options": ["warm", "cold"], "default": "warm"},
                },
            },
            {"id": "boom.now", "label": "Boom", "ai": "never"},
            {"id": "slow.wait", "label": "Slow", "ai": "safe", "timeout_s": 0.05},
            {"id": "typed.fail", "label": "Typed", "ai": "safe"},
        ],
    }
)


def make_module() -> Module:
    mod = Module(MANIFEST)
    counter = {"n": 0}

    @mod.status
    def status():
        return {"greetings": counter["n"]}

    @mod.action("greet.say")
    async def greet(ctx, name, times, tone):
        counter["n"] += 1
        return {"message": " ".join([f"Hello, {name}!"] * times), "tone": tone}

    @mod.action("boom.now")
    def boom(ctx):
        raise RuntimeError("kaboom")

    @mod.action("slow.wait")
    async def slow(ctx):
        import asyncio

        await asyncio.sleep(1)

    @mod.action("typed.fail")
    def typed(ctx):
        raise ActionError("busy", "try later")

    return mod


def test_rejects_handler_for_undeclared_action():
    mod = Module(MANIFEST)
    with pytest.raises(KeyError):
        mod.action("not.declared")


def test_validate_requires_all_handlers():
    mod = Module(MANIFEST)
    with pytest.raises(RuntimeError, match="no handler"):
        mod.server()


async def test_invoke_applies_defaults_and_validates():
    mod = make_module()
    assert await mod.invoke("greet.say", {}) == {"message": "Hello, world!", "tone": "warm"}
    assert (await mod.invoke("greet.say", {"name": "Pluto", "times": 2}))["message"] == "Hello, Pluto! Hello, Pluto!"
    for bad, match in [
        ({"times": 9}, "at most"),
        ({"times": True}, "must be int"),
        ({"tone": "loud"}, "one of"),
        ({"extra": 1}, "unknown parameter"),
    ]:
        with pytest.raises(ActionError, match=match) as e:
            await mod.invoke("greet.say", bad)
        assert e.value.code == "invalid_params"


async def test_invoke_maps_failures_to_error_codes():
    mod = make_module()
    with pytest.raises(ActionError) as e:
        await mod.invoke("boom.now", {})
    assert e.value.code == "module_failed"
    with pytest.raises(ActionError) as e:
        await mod.invoke("slow.wait", {})
    assert e.value.code == "timeout"
    with pytest.raises(ActionError) as e:
        await mod.invoke("typed.fail", {})
    assert (e.value.code, e.value.message) == ("busy", "try later")


async def test_over_mcp():
    mod = make_module()
    async with Client(mod.server()) as client:
        tools = await client.list_tools()
        by_name = {t.name: t for t in tools.tools}
        assert set(by_name) == {"greet__say", "boom__now", "slow__wait", "typed__fail"}
        assert by_name["greet__say"].meta["kernel/ai"] == "safe"
        assert by_name["greet__say"].input_schema["properties"]["times"]["maximum"] == 3

        ok = await client.call_tool("greet__say", {"name": "Kernel"})
        assert not ok.is_error
        assert ok.structured_content == {"result": {"message": "Hello, Kernel!", "tone": "warm"}}

        bad = await client.call_tool("greet__say", {"times": 5})
        assert bad.is_error
        assert json.loads(bad.content[0].text)["code"] == "invalid_params"

        status = await client.read_resource(STATUS_URI)
        assert json.loads(status.contents[0].text) == {"greetings": 1}
