"""Loading and validating `module.toml` manifests."""

from __future__ import annotations

import re
import tomllib
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any, Literal

MODULE_ID = re.compile(r"^[a-z][a-z0-9-]{1,31}$")
ACTION_ID = re.compile(r"^[a-z][a-z0-9_]*(\.[a-z][a-z0-9_]*)+$")
PARAM_NAME = re.compile(r"^[a-z][a-z0-9_]*$")

ParamType = Literal["int", "float", "string", "bool", "enum"]
AiTier = Literal["safe", "confirm", "never"]

_JSON_TYPES = {"int": "integer", "float": "number", "string": "string", "bool": "boolean", "enum": "string"}


class ManifestError(ValueError):
    """The manifest is missing fields or has invalid values."""


@dataclass(frozen=True)
class Param:
    name: str
    type: ParamType
    description: str | None = None
    default: Any = None
    has_default: bool = False
    min: float | None = None
    max: float | None = None
    options: tuple[str, ...] = ()

    def json_schema(self) -> dict[str, Any]:
        schema: dict[str, Any] = {"type": _JSON_TYPES[self.type]}
        if self.description:
            schema["description"] = self.description
        if self.has_default:
            schema["default"] = self.default
        if self.min is not None:
            schema["minimum"] = self.min
        if self.max is not None:
            schema["maximum"] = self.max
        if self.type == "enum":
            schema["enum"] = list(self.options)
        return schema


@dataclass(frozen=True)
class Action:
    id: str
    label: str
    ai: AiTier
    icon: str | None = None
    description: str | None = None
    params: tuple[Param, ...] = ()
    timeout_s: float = 60.0
    quiet: bool = False

    @property
    def tool_name(self) -> str:
        """MCP tool name: dots become double underscores (`server.start` -> `server__start`)."""
        return self.id.replace(".", "__")

    def input_schema(self) -> dict[str, Any]:
        return {
            "type": "object",
            "properties": {p.name: p.json_schema() for p in self.params},
            "required": [p.name for p in self.params if not p.has_default],
            "additionalProperties": False,
        }


@dataclass(frozen=True)
class Manifest:
    id: str
    name: str
    icon: str
    version: str
    runtime: str
    entry: str
    actions: tuple[Action, ...] = field(default=())
    settings: dict[str, Any] = field(default_factory=dict)
    root: Path | None = None

    def action(self, action_id: str) -> Action | None:
        return next((a for a in self.actions if a.id == action_id), None)

    def action_for_tool(self, tool_name: str) -> Action | None:
        return next((a for a in self.actions if a.tool_name == tool_name), None)


def _req(table: dict[str, Any], key: str, where: str) -> Any:
    if key not in table:
        raise ManifestError(f"{where}: missing required field '{key}'")
    return table[key]


def _parse_param(name: str, raw: Any, where: str) -> Param:
    if not PARAM_NAME.match(name):
        raise ManifestError(f"{where}: invalid parameter name '{name}'")
    if not isinstance(raw, dict):
        raise ManifestError(f"{where}.{name}: must be a table")
    ptype = _req(raw, "type", f"{where}.{name}")
    if ptype not in _JSON_TYPES:
        raise ManifestError(f"{where}.{name}: unknown type '{ptype}'")
    options = tuple(raw.get("options", ()))
    if ptype == "enum" and not options:
        raise ManifestError(f"{where}.{name}: enum parameters need 'options'")
    return Param(
        name=name,
        type=ptype,
        description=raw.get("description"),
        default=raw.get("default"),
        has_default="default" in raw,
        min=raw.get("min"),
        max=raw.get("max"),
        options=options,
    )


def parse_manifest(data: dict[str, Any], root: Path | None = None) -> Manifest:
    module_id = _req(data, "id", "module")
    if not isinstance(module_id, str) or not MODULE_ID.match(module_id):
        raise ManifestError(f"module: invalid id '{module_id}'")
    for key in ("name", "icon", "version", "runtime", "entry"):
        _req(data, key, "module")
    if data["runtime"] not in ("python", "node"):
        raise ManifestError(f"module: unknown runtime '{data['runtime']}'")

    actions: list[Action] = []
    seen: set[str] = set()
    for i, raw in enumerate(data.get("actions", [])):
        where = f"actions[{i}]"
        action_id = _req(raw, "id", where)
        if not ACTION_ID.match(action_id):
            raise ManifestError(f"{where}: invalid action id '{action_id}' (expected noun.verb)")
        if action_id in seen:
            raise ManifestError(f"{where}: duplicate action id '{action_id}'")
        seen.add(action_id)
        ai = raw.get("ai", "confirm")
        if ai not in ("safe", "confirm", "never"):
            raise ManifestError(f"{where}: ai must be safe, confirm or never")
        params = tuple(_parse_param(n, p, f"{where}.params") for n, p in raw.get("params", {}).items())
        quiet = raw.get("quiet", False)
        if not isinstance(quiet, bool):
            raise ManifestError(f"{where}: quiet must be true or false")
        if quiet and ai != "safe":
            raise ManifestError(f"{where}: only safe actions can be quiet (everything else is always logged)")
        actions.append(
            Action(
                id=action_id,
                label=_req(raw, "label", where),
                ai=ai,
                icon=raw.get("icon"),
                description=raw.get("description"),
                params=params,
                timeout_s=float(raw.get("timeout_s", 60)),
                quiet=quiet,
            )
        )

    settings = data.get("settings", {})
    if not isinstance(settings, dict):
        raise ManifestError("settings: must be a table")
    for key, value in settings.items():
        if not PARAM_NAME.match(key):
            raise ManifestError(f"settings: invalid name '{key}'")
        if not isinstance(value, (str, int, float, bool, list)):
            raise ManifestError(f"settings.{key}: must be a string, number, bool or list")

    return Manifest(
        id=module_id,
        name=data["name"],
        icon=data["icon"],
        version=str(data["version"]),
        runtime=data["runtime"],
        entry=data["entry"],
        actions=tuple(actions),
        settings=dict(settings),
        root=root,
    )


def load_manifest(path: Path) -> Manifest:
    with path.open("rb") as f:
        return parse_manifest(tomllib.load(f), root=path.parent)


LOCAL_SETTINGS = "settings.local.toml"


def _same_kind(default: Any, value: Any) -> bool:
    if isinstance(default, bool) or isinstance(value, bool):
        return isinstance(default, bool) and isinstance(value, bool)
    if isinstance(default, float):
        return isinstance(value, (int, float))
    return type(default) is type(value)


def load_settings(manifest: Manifest) -> dict[str, Any]:
    """The manifest's `[settings]` defaults, overridden by `settings.local.toml` next to it.

    The local file is per machine (not committed). It may only set keys the manifest
    declares, with the same type, so a typo fails loudly instead of being ignored.
    """
    settings = dict(manifest.settings)
    if manifest.root is None:
        return settings
    path = manifest.root / LOCAL_SETTINGS
    if not path.is_file():
        return settings
    with path.open("rb") as f:
        local = tomllib.load(f)
    for key, value in local.items():
        if key not in settings:
            raise ManifestError(f"{LOCAL_SETTINGS}: unknown setting '{key}'")
        if not _same_kind(settings[key], value):
            raise ManifestError(f"{LOCAL_SETTINGS}: '{key}' must be {type(settings[key]).__name__}")
        settings[key] = float(value) if isinstance(settings[key], float) else value
    return settings
