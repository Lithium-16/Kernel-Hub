import hashlib
import json
import struct

from aiohttp.test_utils import TestClient, TestServer
from petinstall import ExeSource
from petrelay import Relay
from petstore import Store

EXE = b"MZ-fake-exe"
REL = [
    {"tag_name": "node-build-3", "draft": False, "prerelease": False, "assets": [
        {"name": "petlink-windows-x64.exe", "browser_download_url": "u/exe3"},
        {"name": "petlink-windows-x64.exe.sha256", "browser_download_url": "u/sha3"}]},
    {"tag_name": "node-build-9", "draft": False, "prerelease": False, "assets": []},  # no petlink asset
    {"tag_name": "node-build-8", "draft": True, "prerelease": False, "assets": [
        {"name": "petlink-windows-x64.exe", "browser_download_url": "u/exe8"},
        {"name": "petlink-windows-x64.exe.sha256", "browser_download_url": "u/sha8"}]},
    {"tag_name": "v1", "draft": False, "prerelease": False, "assets": []},
]


def fake_fetch(calls, sha=None):
    def fetch(url):
        calls.append(url)
        if "releases" in url:
            return json.dumps(REL).encode()
        if url == "u/sha3":
            return ((sha or hashlib.sha256(EXE).hexdigest()) + "  petlink-windows-x64.exe\n").encode()
        if url == "u/exe3":
            return EXE
        raise AssertionError(url)
    return fetch


async def test_picks_verifies_caches_and_refreshes_hourly(tmp_path):
    calls, now = [], [1000.0]
    src = ExeSource(tmp_path, fake_fetch(calls), clock=lambda: now[0])
    path = await src.get()
    assert path.read_bytes() == EXE
    n = len(calls)
    assert await src.get() == path and len(calls) == n  # fresh: no network
    now[0] += 3601
    assert await src.get() == path and len(calls) == n + 1  # re-checked the list only, same build
    assert ExeSource(tmp_path, fake_fetch([]), clock=lambda: now[0] + 99999).cached().read_bytes() == EXE  # survives restart


async def test_bad_sha_is_refused_and_none_published(tmp_path):
    assert await ExeSource(tmp_path, fake_fetch([], sha="0" * 64)).get() is None
    def boom(url):
        raise OSError("offline")
    assert await ExeSource(tmp_path / "x", boom).get() is None


async def test_exe_override(tmp_path):
    f = tmp_path / "my.exe"
    f.write_bytes(b"local")
    calls = []
    assert await ExeSource(tmp_path, fake_fetch(calls), override=str(f)).get() == f and calls == []


# -- the relay route --------------------------------------------------------------------------


async def _client(tmp_path, exe, clock=None, **kw):
    store = Store(tmp_path / "db", **({"clock": clock} if clock else {}))
    async def provider():
        return exe
    relay = Relay(store, tmp_path / "files", exe_source=provider, invite_builder=lambda t: "petlink:" + t, **kw)
    c = TestClient(TestServer(relay.app()))
    await c.start_server()
    return store, relay, c


async def test_install_link_serves_exe_with_trailer_once(tmp_path):
    exe = tmp_path / "p.exe"
    exe.write_bytes(EXE)
    store, relay, c = await _client(tmp_path, exe)
    user, old = store.add_user("Ann", 0)
    code = store.add_install_code(user.id)
    r = await c.get(f"/petlink/install/{code}")
    body = await r.read()
    assert r.status == 200 and 'filename="PetLink.exe"' in r.headers["Content-Disposition"]
    assert body.startswith(EXE) and body.endswith(b"PLINVITE")
    n = struct.unpack("<I", body[-12:-8])[0]
    invite = body[-12 - n:-12].decode()
    assert body[len(EXE):-12 - n] == b"" and invite.startswith("petlink:")
    assert store.auth(old) is None and store.auth(invite[8:]).id == user.id  # the download rotated the token
    assert (await c.get(f"/install/{code}")).status == 404
    assert (await c.get("/install/nope")).status == 404
    await c.close()


async def test_expired_code_404_and_unpublished_503_keeps_code(tmp_path):
    t = [0.0]
    store, relay, c = await _client(tmp_path, None, clock=lambda: t[0])
    u, tok = store.add_user("Ann", 0)
    code = store.add_install_code(u.id)
    assert (await c.get(f"/install/{code}")).status == 503
    assert (await c.get(f"/install/{code}")).status == 503  # not burned
    assert store.auth(tok).id == u.id and store.pending_codes() == {u.id: 1}  # token untouched
    t[0] = 8 * 86400
    assert (await c.get(f"/install/{code}")).status == 404
    await c.close()


async def test_install_rate_limit(tmp_path):
    store, relay, c = await _client(tmp_path, None, install_per_minute=3)
    codes = [(await c.get("/install/x")).status for _ in range(4)]
    assert codes == [404, 404, 404, 429]
    await c.close()
