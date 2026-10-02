from pathlib import Path

from kernel_sdk.manifest import load_manifest
from openfork import HOST_FILE, OpenFork

HERE = Path(__file__).resolve().parent.parent


def test_manifest_and_host():
    m = load_manifest(HERE / "module.toml")
    assert m.action("share.start").ai == "never" and m.action("game.update").ai == "confirm"
    assert m.settings["repo"] == "B0RE16/OpenFork" and m.settings["auto_update"] is True
    flowrace = load_manifest(HERE.parent / "flowrace" / "module.toml")
    # Both games can be shared at once: different local ports and share links.
    assert m.settings["port"] != flowrace.settings["port"]
    assert m.settings["share_port"] != flowrace.settings["share_port"]
    assert HOST_FILE.is_file() and "/kernel/status" in HOST_FILE.read_text()


def test_follows_the_configured_branch(tmp_path):
    asked = []

    def get(url, timeout=5, headers=None):
        asked.append(url)
        return b"f" * 40

    g = OpenFork({"repo": "B0RE16/OpenFork", "ref": "claude/brave-goldberg-jcd77j"}, tmp_path, get=get)
    assert g.latest_commit() == "f" * 40
    assert asked == ["https://api.github.com/repos/B0RE16/OpenFork/commits/claude/brave-goldberg-jcd77j"]
    assert g.title == "OpenFork" and g.snapshot()["state"] == "not installed"

