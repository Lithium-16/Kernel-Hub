import importlib.util
from pathlib import Path

from kernel_sdk import Module


def test_manifest_and_handlers(monkeypatch):
    # Loaded by path: every module has a main.py, so `import main` would get whichever came first.
    module_dir = Path(__file__).resolve().parent.parent
    monkeypatch.setenv("KERNEL_MODULE_DIR", str(module_dir))
    spec = importlib.util.spec_from_file_location("minecraft_main", module_dir / "main.py")
    main = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(main)
    mod: Module = main.mod
    mod.validate()
    assert mod.settings["server_dir"] == "/srv/minecraft"
    assert {a.id for a in mod.manifest.actions} >= {"server.start", "server.stop", "world.backup"}
    tiers = {a.id: a.ai for a in mod.manifest.actions}
    assert tiers["server.command"] == "confirm"
    assert tiers["player.op"] == "confirm"
    assert tiers["world.backup"] == "safe"
