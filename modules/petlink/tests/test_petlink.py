import base64
import json
from pathlib import Path

from kernel_sdk.manifest import load_manifest
from petlink import PetLink

HERE = Path(__file__).resolve().parent.parent


def test_manifest():
    m = load_manifest(HERE / "module.toml")
    assert m.id == "petlink" and m.settings["port"] == 8098
    assert m.settings["share_port"] == 10000 and m.settings["share_path"] == "/petlink"
    assert m.action("user.add").ai == "never" and m.action("user.remove").ai == "never"
    assert m.action("share.start").ai == "never"
    others = [load_manifest(HERE.parent / g / "module.toml") for g in ("flowrace", "openfork", "party")]
    assert m.settings["port"] not in {o.settings["port"] for o in others}


async def test_add_user_gives_a_working_invite(tmp_path):
    p = PetLink({"port": 0, "share_port": 10000, "share_path": "/petlink"}, tmp_path, run=lambda *a, **k: None)
    p.link = "https://pluto.example.ts.net:10000/petlink"
    out = await p.add_user("Friend", 210)
    assert out["invite"].startswith("petlink:")
    data = json.loads(base64.urlsafe_b64decode(out["invite"][8:] + "=="))
    assert data["relay_url"] == "wss://pluto.example.ts.net:10000/petlink/ws"
    assert p.store.auth(data["token"]).name == "Friend"
    assert [u["name"] for u in p.snapshot()["users"]] == ["Friend"]
    await p.remove_user("friend")
    assert p.snapshot()["users"] == []


async def test_add_user_returns_download_link_and_relink_rotates(tmp_path):
    p = PetLink({"port": 0, "share_port": 10000, "share_path": "/petlink"}, tmp_path, run=lambda *a, **k: None)
    p.link = "https://pluto.example.ts.net:10000/petlink"
    out = await p.add_user("Friend", 210)
    assert out["download"].startswith("https://pluto.example.ts.net:10000/petlink/install/")
    assert out["message"].index(out["download"]) < out["message"].index(out["invite"])
    assert p.snapshot()["users"][0]["install_codes"] == 1
    assert "install/" not in str(p.snapshot()).replace(p.link + "/install", "") and out["download"].rsplit("/", 1)[1] not in str(p.snapshot())
    old = json.loads(base64.urlsafe_b64decode(out["invite"][8:] + "=="))["token"]
    again = await p.relink("friend")
    new = json.loads(base64.urlsafe_b64decode(again["invite"][8:] + "=="))["token"]
    assert p.store.auth(old) is None and p.store.auth(new).name == "Friend"
    assert again["user"] == out["user"] and again["download"] != out["download"]
    assert p.snapshot()["users"][0]["install_codes"] == 1


async def test_exe_override_setting_is_used(tmp_path):
    f = tmp_path / "x.exe"
    f.write_bytes(b"x")
    p = PetLink({"port": 0, "exe_override": str(f)}, tmp_path, run=lambda *a, **k: None)
    assert await p.exe_source.get() == f


def test_manifest_has_relink_and_exe_override():
    m = load_manifest(HERE / "module.toml")
    assert m.action("user.relink").ai == "never" and m.settings["exe_override"] == ""
