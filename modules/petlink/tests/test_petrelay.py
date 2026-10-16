import asyncio
import json
from pathlib import Path

import pytest
from aiohttp import WSMsgType
from aiohttp.test_utils import TestClient, TestServer
from petrelay import Relay
from petstore import Store

FIXTURES = Path(__file__).resolve().parent.parent / "fixtures"


def fx(name: str) -> dict:
    return json.loads((FIXTURES / f"{name}.json").read_text())


@pytest.fixture
async def env(tmp_path):
    store = Store(tmp_path / "db.sqlite")
    a, ta = store.add_user("Ann", 0)
    b, tb = store.add_user("Bob", 210)
    relay = Relay(store, tmp_path / "files", per_minute=30)
    client = TestClient(TestServer(relay.app()))
    await client.start_server()
    yield {"store": store, "relay": relay, "client": client, "a": a, "b": b, "ta": ta, "tb": tb}
    await client.close()


async def recv(ws, kind):
    while True:
        msg = json.loads((await asyncio.wait_for(ws.receive(), 5)).data)
        if msg["kind"] == kind:
            return msg


async def login(client, token, path="/ws"):
    ws = await client.ws_connect(path)
    await ws.send_json({**fx("hello"), "token": token})
    return ws, await recv(ws, "welcome")


def msg(name, **over):
    return {**fx(name), **over}


async def test_bad_token_is_closed_with_4401(env):
    ws = await env["client"].ws_connect("/ws")
    await ws.send_json({**fx("hello"), "token": "wrong"})
    await ws.receive()
    assert ws.close_code == 4401


async def test_welcome_lists_friends_and_presence_is_broadcast(env):
    wa, welcome = await login(env["client"], env["ta"])
    assert welcome["me"]["id"] == env["a"].id
    assert welcome["friends"] == [{"id": env["b"].id, "name": "Bob", "bow_hue": 210, "peek": False, "online": False}]
    wb, _ = await login(env["client"], env["tb"], "/petlink/ws")  # the Funnel path works too
    seen = await recv(wa, "presence")
    assert (seen["from"], seen["state"]) == (env["b"].id, "online")
    await wb.close()
    seen = await recv(wa, "presence")
    assert seen["state"] == "offline"


async def test_mail_is_queued_while_offline_and_from_cannot_be_spoofed(env):
    a, b = env["a"], env["b"]
    wa, _ = await login(env["client"], env["ta"])
    await wa.send_json(msg("mail", **{"from": "u_fake", "to": b.id, "files": []}))
    wb, _ = await login(env["client"], env["tb"])
    got = await recv(wb, "mail")
    assert got["from"] == a.id and got["text"] == "hi"
    assert (await recv(wa, "delivered"))["mail_id"] == "m_1"
    # The same mail_id again (a client retrying after a restart) isn't delivered twice.
    await wa.send_json(msg("mail", to=b.id, files=[]))
    await wa.send_json(msg("touch", to=b.id))
    while (got := json.loads((await asyncio.wait_for(wb.receive(), 5)).data))["kind"] != "touch":
        assert got["kind"] != "mail", "the repeated mail_id was delivered again"
    assert got["what"] == "yeet"
    assert env["store"].pending_mail(b.id) == []
    assert (await recv(wa, "delivered"))["mail_id"] == "m_1"  # the re-send is acknowledged again


async def test_mail_with_someone_elses_file_is_refused(env):
    wa, _ = await login(env["client"], env["ta"])
    await wa.send_json(msg("mail", to=env["b"].id))  # f_abcdefgh was never uploaded
    err = await recv(wa, "error")
    assert err["code"] == "bad_file"
    assert env["store"].pending_mail(env["b"].id) == []


async def test_peek_needs_the_targets_toggle(env):
    a, b = env["a"], env["b"]
    wa, _ = await login(env["client"], env["ta"])
    wb, _ = await login(env["client"], env["tb"])
    await wa.send_json(msg("peek.request", to=b.id))
    res = await recv(wa, "peek.result")
    assert res["denied"] is True and res["from"] == b.id
    await wb.send_json(msg("peek.allow", on=True))
    assert (await recv(wa, "peek.allow"))["on"] is True
    await wa.send_json(msg("peek.request", to=b.id))
    assert (await recv(wb, "peek.request"))["from"] == a.id


async def test_visit_ends_when_the_host_drops(env):
    a, b = env["a"], env["b"]
    wa, _ = await login(env["client"], env["ta"])
    wb, _ = await login(env["client"], env["tb"])
    await wa.send_json(msg("visit.start", to=b.id))
    await recv(wb, "visit.start")
    await wb.close()
    end = await recv(wa, "visit.end")
    assert end["from"] == b.id


async def test_second_login_replaces_the_first_without_going_offline(env):
    wa1, _ = await login(env["client"], env["ta"])
    wb, _ = await login(env["client"], env["tb"])
    wa2, _ = await login(env["client"], env["ta"])
    while (await asyncio.wait_for(wa1.receive(), 5)).type not in (WSMsgType.CLOSE, WSMsgType.CLOSED):
        pass  # skip the presence messages it got before being replaced
    assert wa1.close_code == 4409
    await wa2.send_json(msg("touch", to=env["b"].id))
    await recv(wb, "touch")
    assert env["a"].id in env["relay"].online()


async def test_rate_limit(env):
    env["relay"].per_minute = 3
    wa, _ = await login(env["client"], env["ta"])
    for _ in range(4):
        await wa.send_json(msg("touch", to=env["b"].id))
    codes = [(await recv(wa, "error"))["code"] for _ in range(4)]  # Bob is offline, so the first 3 are "offline"
    assert codes[-1] == "rate_limited"


async def test_files_upload_download_once_and_only_for_the_recipient(env):
    c, a, b = env["client"], env["a"], env["b"]
    auth = lambda t: {"Authorization": f"Bearer {t}"}  # noqa: E731
    r = await c.put(f"/files/f_abcdefgh?to={b.id}&name=cat.png", data=b"meow", headers=auth(env["ta"]))
    assert r.status == 201
    assert (await c.get("/files/f_abcdefgh", headers=auth(env["ta"]))).status == 403  # sender can't
    assert (await c.get("/files/f_abcdefgh")).status == 401
    r = await c.get("/petlink/files/f_abcdefgh", headers=auth(env["tb"]))
    assert r.status == 200 and await r.read() == b"meow"
    assert (await c.get("/files/f_abcdefgh", headers=auth(env["tb"]))).status == 404  # gone after download
    # A mail that references a file the sender uploaded is accepted.
    await c.put(f"/files/f_abcdefgi?to={b.id}&name=x.txt", data=b"x", headers=auth(env["ta"]))
    wa, _ = await login(c, env["ta"])
    await wa.send_json(msg("mail", to=b.id, mail_id="m_2", files=[{"file_id": "f_abcdefgi", "name": "x.txt", "size": 1, "mime": "text/plain"}]))
    wb, _ = await login(c, env["tb"])
    assert (await recv(wb, "mail"))["mail_id"] == "m_2"


async def test_upload_limits(env):
    c, b = env["client"], env["b"]
    env["relay"].max_file = 10
    h = {"Authorization": f"Bearer {env['ta']}"}
    assert (await c.put(f"/files/f_toolarge1?to={b.id}&name=a", data=b"x" * 11, headers=h)).status == 413
    assert (await c.put(f"/files/bad id?to={b.id}&name=a", data=b"x", headers=h)).status == 400
    assert (await c.put(f"/files/f_nobody001?to=u_nobody&name=a", data=b"x", headers=h)).status == 400
    env["relay"].hourly_upload = 5
    env["relay"].max_file = 100
    assert (await c.put(f"/files/f_quota0001?to={b.id}&name=a", data=b"x" * 6, headers=h)).status == 429


async def test_interrupted_download_keeps_the_file(env, monkeypatch):
    c, b = env["client"], env["b"]
    await c.put(f"/files/f_keepme001?to={b.id}&name=a", data=b"x" * 200_000, headers={"Authorization": f"Bearer {env['ta']}"})
    row = env["store"].file("f_keepme001")
    original = env["relay"]._stream_file

    async def broken(request, path):
        raise ConnectionResetError

    monkeypatch.setattr(env["relay"], "_stream_file", broken)
    r = await c.get("/files/f_keepme001", headers={"Authorization": f"Bearer {env['tb']}"})
    assert r.status == 500
    assert env["store"].file("f_keepme001") is not None and row.path.exists()
    monkeypatch.setattr(env["relay"], "_stream_file", original)
    r = await c.get("/files/f_keepme001", headers={"Authorization": f"Bearer {env['tb']}"})
    assert r.status == 200 and len(await r.read()) == 200_000
