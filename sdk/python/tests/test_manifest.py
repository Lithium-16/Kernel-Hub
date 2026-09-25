from pathlib import Path

import pytest

from kernel_sdk import ManifestError, load_manifest, parse_manifest

HELLO = Path(__file__).resolve().parents[3] / "modules" / "hello" / "module.toml"


def base(**over):
    data = {"id": "demo", "name": "Demo", "icon": "box", "version": "1.0.0", "runtime": "python", "entry": "main.py"}
    data.update(over)
    return data


def test_loads_hello_module():
    m = load_manifest(HELLO)
    assert m.id == "hello"
    assert [a.id for a in m.actions] == ["greet.say", "counter.reset", "slow.wait", "debug.crash"]
    greet = m.action("greet.say")
    assert greet.tool_name == "greet__say"
    assert greet.input_schema() == {
        "type": "object",
        "properties": {"name": {"type": "string", "description": "Who to greet", "default": "world"}},
        "required": [],
        "additionalProperties": False,
    }
    assert m.action("slow.wait").input_schema()["required"] == ["seconds"]
    assert m.action("slow.wait").timeout_s == 1


@pytest.mark.parametrize(
    ("data", "message"),
    [
        (base(id="Bad Id"), "invalid id"),
        ({k: v for k, v in base().items() if k != "entry"}, "missing required field 'entry'"),
        (base(runtime="ruby"), "unknown runtime"),
        (base(actions=[{"id": "nodots", "label": "x"}]), "invalid action id"),
        (base(actions=[{"id": "a.b", "label": "x"}, {"id": "a.b", "label": "y"}]), "duplicate action id"),
        (base(actions=[{"id": "a.b", "label": "x", "ai": "yolo"}]), "ai must be"),
        (base(actions=[{"id": "a.b", "label": "x", "params": {"p": {"type": "enum"}}}]), "need 'options'"),
        (base(actions=[{"id": "a.b", "label": "x", "params": {"p": {"type": "date"}}}]), "unknown type"),
        (base(actions=[{"id": "a.b"}]), "missing required field 'label'"),
    ],
)
def test_rejects_invalid_manifests(data, message):
    with pytest.raises(ManifestError, match=message):
        parse_manifest(data)


def test_ai_tier_defaults_to_confirm():
    m = parse_manifest(base(actions=[{"id": "a.b", "label": "x"}]))
    assert m.actions[0].ai == "confirm"
