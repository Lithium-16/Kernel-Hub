import importlib

from kernel_sdk import Module


def test_manifest_and_handlers(monkeypatch):
    from conftest import MODULE_DIR

    monkeypatch.setenv("KERNEL_MODULE_DIR", str(MODULE_DIR))
    main = importlib.import_module("main")
    mod: Module = main.mod
    mod.validate()
    assert mod.settings["server_dir"] == "/srv/minecraft"
    assert {a.id for a in mod.manifest.actions} >= {"server.start", "server.stop", "world.backup"}
    tiers = {a.id: a.ai for a in mod.manifest.actions}
    assert tiers["server.command"] == "confirm"
    assert tiers["player.op"] == "confirm"
    assert tiers["world.backup"] == "safe"
