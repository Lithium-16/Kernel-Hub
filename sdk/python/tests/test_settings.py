import asyncio

import pytest

from kernel_sdk import ManifestError, Module, load_manifest, parse_manifest

TOML = """
id = "demo"
name = "Demo"
icon = "box"
version = "1.0.0"
runtime = "python"
entry = "main.py"

[settings]
distro = "Ubuntu"
refresh_s = 10.0
keep = 1
enabled = true
"""


def write_module(tmp_path, local=None):
    (tmp_path / "module.toml").write_text(TOML)
    if local is not None:
        (tmp_path / "settings.local.toml").write_text(local)
    return load_manifest(tmp_path / "module.toml")


def test_defaults_come_from_the_manifest(tmp_path):
    mod = Module(write_module(tmp_path))
    assert mod.settings == {"distro": "Ubuntu", "refresh_s": 10.0, "keep": 1, "enabled": True}


def test_kernel_settings_file_wins(tmp_path, monkeypatch):
    elsewhere = tmp_path / "data" / "demo.toml"
    elsewhere.parent.mkdir()
    elsewhere.write_text('distro = "Arch"\n')
    monkeypatch.setenv("KERNEL_SETTINGS_FILE", str(elsewhere))
    mod = Module(write_module(tmp_path, 'distro = "Debian"\n'))
    assert mod.settings["distro"] == "Arch"


def test_missing_kernel_settings_file_falls_back(tmp_path, monkeypatch):
    monkeypatch.setenv("KERNEL_SETTINGS_FILE", str(tmp_path / "nope.toml"))
    mod = Module(write_module(tmp_path, 'distro = "Debian"\n'))
    assert mod.settings["distro"] == "Debian"


def test_local_file_overrides_defaults(tmp_path):
    mod = Module(write_module(tmp_path, 'distro = "Debian"\nrefresh_s = 5\n'))
    assert mod.settings["distro"] == "Debian"
    assert mod.settings["refresh_s"] == 5.0
    assert isinstance(mod.settings["refresh_s"], float)


@pytest.mark.parametrize(
    ("local", "message"),
    [
        ("distro_name = 'x'", "unknown setting"),
        ("keep = '2'", "must be int"),
        ("enabled = 1", "must be bool"),
        ("keep = true", "must be int"),
    ],
)
def test_local_file_is_checked(tmp_path, local, message):
    with pytest.raises(ManifestError, match=message):
        Module(write_module(tmp_path, local))


def test_settings_must_be_a_table_of_values():
    base = {"id": "demo", "name": "D", "icon": "b", "version": "1", "runtime": "python", "entry": "m.py"}
    with pytest.raises(ManifestError, match="must be a table"):
        parse_manifest({**base, "settings": "x"})
    with pytest.raises(ManifestError, match="invalid name"):
        parse_manifest({**base, "settings": {"Bad-Name": 1}})


async def test_background_tasks_restart_after_errors(monkeypatch):
    mod = Module(parse_manifest({"id": "demo", "name": "D", "icon": "b", "version": "1", "runtime": "python", "entry": "m.py"}))
    runs = []
    real_sleep = asyncio.sleep

    async def fast_sleep(seconds):
        await real_sleep(0)

    monkeypatch.setattr(asyncio, "sleep", fast_sleep)

    @mod.background
    async def flaky():
        runs.append(1)
        if len(runs) < 3:
            raise RuntimeError("boom")

    await asyncio.wait_for(mod._keep_running(flaky), 1)
    assert len(runs) == 3
