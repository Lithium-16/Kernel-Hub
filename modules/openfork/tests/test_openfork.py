import email.message
import json
import urllib.error
from pathlib import Path

import pytest
from kernel_sdk import ActionError

from kernel_sdk.manifest import load_manifest
from openfork import HOST_FILE, OpenFork

HERE = Path(__file__).resolve().parent.parent


def test_manifest_and_host():
    m = load_manifest(HERE / "module.toml")
    assert m.action("share.start").ai == "never" and m.action("game.update").ai == "confirm"
    assert m.settings["repo"] == "Lithium-16/OpenFork" and m.settings["auto_update"] is True
    flowrace = load_manifest(HERE.parent / "flowrace" / "module.toml")
    # The default branch, never a feature branch that goes stale once merged.
    assert m.settings["ref"] == "" and flowrace.settings["ref"] == ""
    # Both games can be shared at once: different local ports and share links.
    assert m.settings["port"] != flowrace.settings["port"]
    assert m.settings["share_port"] != flowrace.settings["share_port"]
    assert HOST_FILE.is_file() and "/kernel/status" in HOST_FILE.read_text()


def test_follows_the_configured_branch(tmp_path):
    asked = []

    def get(url, timeout=5, headers=None):
        asked.append(url)
        return b"f" * 40

    g = OpenFork({"repo": "Lithium-16/OpenFork", "ref": "some-branch"}, tmp_path, get=get)
    assert g.latest_commit() == "f" * 40
    assert asked[-1] == "https://api.github.com/repos/Lithium-16/OpenFork/commits/some-branch"
    assert g.title == "OpenFork" and g.snapshot()["state"] == "not installed"


def test_follows_the_default_branch(tmp_path):
    asked = []

    def get(url, timeout=5, headers=None):
        asked.append(url)
        return b"a" * 40

    g = OpenFork({"repo": "Lithium-16/OpenFork", "ref": ""}, tmp_path, get=get)
    assert g.latest_commit() == "a" * 40
    assert asked[-1] == "https://api.github.com/repos/Lithium-16/OpenFork/commits/HEAD"


def gone(url):
    return urllib.error.HTTPError(url, 404, "Not Found", email.message.Message(), None)


def test_a_deleted_branch_falls_back_to_the_default_branch(tmp_path):
    events = []

    def get(url, timeout=5, headers=None):
        if url.endswith("/commits/old-branch"):
            raise gone(url)
        return b"b" * 40

    g = OpenFork({"repo": "Lithium-16/OpenFork", "ref": "old-branch"}, tmp_path, get=get, emit=lambda kind, msg, **kw: events.append(kind))
    assert g.latest_commit() == "b" * 40
    assert g.settings["ref"] == "" and events == ["game.branch_gone"]


def test_a_missing_repository_is_not_mistaken_for_a_deleted_branch(tmp_path):
    def get(url, timeout=5, headers=None):
        raise gone(url)

    g = OpenFork({"repo": "Nobody/Nothing", "ref": "main"}, tmp_path, get=get)
    with pytest.raises(ActionError):
        g.latest_commit()
    assert g.settings["ref"] == "main"


def test_a_renamed_repository_is_followed(tmp_path):
    asked, events = [], []

    def get(url, timeout=5, headers=None):
        asked.append(url)
        if url == "https://api.github.com/repos/B0RE16/OpenFork":
            return json.dumps({"full_name": "Lithium-16/OpenFork"}).encode()
        return b"c" * 40

    g = OpenFork({"repo": "B0RE16/OpenFork", "ref": ""}, tmp_path, get=get, emit=lambda kind, msg, **kw: events.append(kind))
    assert g.latest_commit() == "c" * 40
    assert g.settings["repo"] == "Lithium-16/OpenFork" and events == ["game.repo_moved"]
    assert asked[-1] == "https://api.github.com/repos/Lithium-16/OpenFork/commits/HEAD"
    g.latest_commit()
    assert asked.count("https://api.github.com/repos/Lithium-16/OpenFork") == 0, "checked once per start"


def test_a_merged_branch_follows_the_default_branch(tmp_path):
    events = []
    status = {"v": "behind"}

    def get(url, timeout=5, headers=None):
        if url == "https://api.github.com/repos/Lithium-16/OpenFork":
            return json.dumps({"full_name": "Lithium-16/OpenFork", "default_branch": "main"}).encode()
        if "/compare/main...old-branch" in url:
            return json.dumps({"status": status["v"]}).encode()
        if url.endswith("/commits/old-branch"):
            return b"o" * 40
        if url.endswith("/commits/HEAD"):
            return b"m" * 40
        raise AssertionError(url)

    g = OpenFork({"repo": "Lithium-16/OpenFork", "ref": "old-branch"}, tmp_path, get=get, emit=lambda kind, msg, **kw: events.append(kind))
    assert g.latest_commit() == "m" * 40, "merged: the default branch"
    assert g.latest_commit() == "m" * 40 and events == ["game.branch_merged"], "said once"
    assert g.settings["ref"] == "old-branch", "kept, so new commits on it are followed again"
    status["v"] = "ahead"
    assert g.latest_commit() == "o" * 40, "new work on the branch: follow it"
