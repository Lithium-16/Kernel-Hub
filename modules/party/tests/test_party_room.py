import asyncio
import json
import random
from pathlib import Path

import pytest
from aiohttp.test_utils import TestClient, TestServer
from kernel_sdk import ActionError
from kernel_sdk.manifest import load_manifest

from partygames import Invalid
from partyroom import CODE_LETTERS, Party, PartyServer, Room

HERE = Path(__file__).resolve().parent.parent
CONTENT = [HERE / "content"]


class Clock:
    def __init__(self):
        self.t = 1000.0

    def __call__(self):
        return self.t


def room(**settings):
    events = []
    r = Room(settings, CONTENT, emit=lambda kind, msg, **kw: events.append(kind), clock=Clock(), rng=random.Random(5))
    return r, events


def connect(r, name):
    seat = r.join(r.code, name, None)
    seat.sockets.add(object())  # stands in for an open WebSocket
    r.connections_changed()
    return seat


def test_manifest():
    m = load_manifest(HERE / "module.toml")
    assert m.id == "party" and m.action("share.start").ai == "never"
    assert m.action("server.start").ai == "safe" and m.action("room.new").ai == "confirm"
    others = [load_manifest(HERE.parent / g / "module.toml") for g in ("flowrace", "openfork")]
    # Shareable alongside the other games: its own local port and https port.
    assert m.settings["port"] not in {o.settings["port"] for o in others}
    assert m.settings["share_port"] not in {o.settings["share_port"] for o in others}


def test_join_rules():
    r, events = room(max_players=3)
    assert len(r.code) == 4 and set(r.code) <= set(CODE_LETTERS)
    with pytest.raises(Invalid, match="no room"):
        r.join("ZZZZ" if r.code != "ZZZZ" else "BBBB", "Ann", None)
    with pytest.raises(Invalid, match="name"):
        r.join(r.code.lower(), "   ", None)
    ann = r.join(r.code.lower(), " Ann ", None)
    assert ann.name == "Ann" and events == ["player.joined"]
    with pytest.raises(Invalid, match="already called"):
        r.join(r.code, "ann", None)
    assert r.join(r.code, "whatever", ann.token) is ann  # reconnecting keeps the seat
    r.join(r.code, "Bo", None)
    r.join(r.code, "Cy" * 20, None)
    assert any(s.name == ("Cy" * 20)[:16] for s in r.seats.values())
    with pytest.raises(Invalid, match="full"):
        r.join(r.code, "Dee", None)
    r.kick("bo")
    with pytest.raises(Invalid, match="removed"):
        r.join(r.code, "Bo", next(t for t in r.banned))
    with pytest.raises(Invalid, match="Nobody"):
        r.kick("Zed")


def test_vip_picks_and_starts_and_results_add_up():
    r, events = room()
    a, b = connect(r, "Ann"), connect(r, "Bo")
    assert r.vip == a.pid
    with pytest.raises(Invalid, match="VIP"):
        r.handle(b.pid, {"type": "start", "game": "bluff"})
    with pytest.raises(Invalid, match="at least 3"):
        r.handle(a.pid, {"type": "start", "game": "quip"})
    r.handle(a.pid, {"type": "choose", "game": "bluff"})
    assert r.choice == "bluff"
    r.handle(a.pid, {"type": "start"})
    assert r.state == "playing" and r.game.key == "bluff" and "game.started" in events
    late = connect(r, "Late")
    assert r.player_state(late.pid)["view"] == {"game": "bluff", "phase": "next_game"}
    with pytest.raises(Invalid, match="next game"):
        r.handle(late.pid, {"type": "lie", "text": "x"})
    with pytest.raises(Invalid, match="already"):
        r.start("quip")
    r.handle(a.pid, {"type": "lie", "text": "rabbits"})
    while r.state == "playing":
        r.skip()
    assert r.state == "results" and r.games_played == 1 and "game.finished" in events
    res = r.results
    assert res["title"] == "Bluff Buffet" and {s["name"] for s in res["standings"]} == {"Ann", "Bo"}
    assert r.player_state(b.pid)["view"] is res
    r.handle(a.pid, {"type": "lobby"})
    assert r.state == "lobby"
    snap = r.snapshot()
    assert snap["online"] == 3 and snap["games_played"] == 1 and snap["room"] == "lobby"


def test_vip_passes_on_when_they_disconnect():
    r, _ = room()
    a, b = connect(r, "Ann"), connect(r, "Bo")
    a.sockets.clear()
    assert r.vip == b.pid
    assert r.room_view()["players"][0]["connected"] is False


def test_new_room_starts_over():
    r, _ = room()
    connect(r, "Ann")
    old = r.code
    r.rng = random.Random(99)
    r.new_room()
    assert r.code != old and not r.seats and r.state == "lobby"


# -- over real WebSockets -----------------------------------------------------------------


async def client_for(r):
    server = PartyServer(r, "k" * 24, 0)
    client = TestClient(TestServer(server.app()))
    await client.start_server()
    return server, client


async def recv(ws, kind="state", until=None):
    while True:
        msg = json.loads((await asyncio.wait_for(ws.receive(), 5)).data)
        if msg["type"] == kind and (until is None or until(msg)):
            return msg


async def test_pages_and_headers():
    r, _ = room()
    server, client = await client_for(r)
    try:
        resp = await client.get("/health")
        assert await resp.text() == "ok"
        resp = await client.get("/")
        assert resp.status == 200 and "play.js" in await resp.text()
        assert "script-src 'self'" in resp.headers["Content-Security-Policy"]
        assert (await client.get("/host")).status == 403
        assert (await client.get("/host?key=wrong")).status == 403
        assert (await client.get("/host?key=" + "k" * 24)).status == 200
        assert (await client.get("/static/party.css")).status == 200
        assert (await client.get("/static/secret.py")).status == 404
        assert (await client.get("/static/..%2Fpartyroom.py")).status == 404
        with pytest.raises(Exception):
            await client.ws_connect("/ws?role=host&key=wrong")
    finally:
        await client.close()


async def test_a_whole_quip_clash_over_websockets():
    r, events = room()
    server, client = await client_for(r)
    try:
        host = await client.ws_connect("/ws?role=host&key=" + "k" * 24)
        first = await recv(host)
        assert first["room"]["code"] == r.code and first["view"] is None and "link" in first
        phones = {}
        for name in ("Ann", "Bo", "Cy"):
            ws = await client.ws_connect("/ws")
            await recv(ws, "hello")
            await ws.send_json({"type": "join", "code": r.code.lower(), "name": name})
            joined = await recv(ws, "joined")
            assert joined["pid"] and joined["token"]
            phones[name] = (ws, joined["pid"], joined["token"])
        ann = phones["Ann"][0]
        await ann.send_json({"type": "start", "game": "quip"})
        state = await recv(ann, until=lambda m: m["room"]["state"] == "playing")
        assert state["view"]["phase"] == "write" and state["view"]["todo"]["number"] == 1
        # Wrong-phase messages come back as a readable error.
        await ann.send_json({"type": "vote", "choice": 0})
        assert "big screen" in (await recv(ann, "error"))["message"]
        for name, (ws, pid, _) in phones.items():
            for _ in range(2):
                await ws.send_json({"type": "answer", "text": f"{name} is funny"})
        await asyncio.sleep(0.2)
        if r.game.phase == "assemble":  # a pick clashed with a piece someone just used
            await ann.send_json({"type": "skip"})
        state = await recv(ann, until=lambda m: m["view"]["phase"] == "vote")
        hv = await recv(host, until=lambda m: m["view"] and m["view"]["phase"] == "vote")
        assert len(hv["view"]["matchup"]["answers"]) == 2 and "authors" not in hv["view"]["matchup"]
        # A phone that drops and reconnects with its token gets its seat back.
        bo_ws, bo_pid, bo_token = phones["Bo"]
        await bo_ws.close()
        bo2 = await client.ws_connect("/ws")
        await bo2.send_json({"type": "join", "code": r.code, "token": bo_token})
        assert (await recv(bo2, "joined"))["pid"] == bo_pid
        phones["Bo"] = (bo2, bo_pid, bo_token)
        while r.state == "playing":
            await asyncio.sleep(0.11)  # stay under the per-phone message limit
            await ann.send_json({"type": "skip"})
            await recv(ann)
        res = await recv(host, until=lambda m: m["room"]["state"] == "results")
        assert res["view"]["title"] == "Quip Clash" and len(res["view"]["standings"]) == 3
        assert events.count("player.joined") == 3 and "game.finished" in events
        for ws, _, _ in phones.values():
            await ws.close()
        await host.close()
    finally:
        await client.close()


async def test_party_on_kernels_side(tmp_path):
    events = []
    p = Party({"port": 0, "max_players": 8}, tmp_path, emit=lambda kind, msg, **kw: events.append(kind),
              run=lambda *a, **k: None)
    key = (tmp_path / "host_key.txt").read_text()
    assert len(key) >= 16 and p.host_url().endswith(key)
    assert Party({"port": 0}, tmp_path).server.host_key == key  # kept across restarts
    assert p.snapshot()["state"] == "stopped" and p.snapshot()["host_url"] == ""
    p.room.join(p.room.code, "Ann", None)
    out = await p.kick("ann")
    assert out == {"kicked": "Ann"}
    with pytest.raises(ActionError):
        await p.kick("nobody")
    with pytest.raises(ActionError):
        await p.end_game()
    old = p.room.code
    p.room.rng = random.Random(7)
    await p.new_room()
    assert p.room.code != old and events[-1] == "room.opened"
    p.link = "https://pluto.example.ts.net:10000"
    assert p.join_link() == f"https://pluto.example.ts.net:10000/?code={p.room.code}"
    assert (tmp_path / "content").is_dir()


async def test_shirt_showdown_with_drawings_over_websockets():
    r, events = room()
    server, client = await client_for(r)
    try:
        phones = []
        for name in ("Ann", "Bo", "Cy"):
            ws = await client.ws_connect("/ws")
            await ws.send_json({"type": "join", "code": r.code, "name": name})
            phones.append((ws, (await recv(ws, "joined"))["pid"]))
        ann = phones[0][0]
        await ann.send_json({"type": "start", "game": "shirt"})
        await recv(ann, until=lambda m: m["room"]["state"] == "playing")
        # A big but allowed drawing (about 30 KB of JSON) gets through.
        big = [{"c": 2, "w": 1, "p": [k % 400 for k in range(1000)]} for _ in range(7)]
        for ws, _ in phones:
            await asyncio.sleep(0.05)
            await ws.send_json({"type": "drawing", "strokes": big})
            await ws.send_json({"type": "drawing", "strokes": [{"c": 9, "w": 0, "p": [1, 1]}]})
            assert "didn't come through" in (await recv(ws, "error"))["message"]
            await ws.send_json({"type": "done"})
        await recv(ann, until=lambda m: m["view"]["phase"] == "slogan")
        for k, (ws, _) in enumerate(phones):
            await asyncio.sleep(0.05)
            await ws.send_json({"type": "slogan", "text": f"Slogan number {k}"})
            await ws.send_json({"type": "done"})
        state = await recv(ann, until=lambda m: m["view"]["phase"] == "assemble")
        assert state["view"]["drawings"] and state["view"]["slogans"]
        for i, (ws, _) in enumerate(phones):
            v = state["view"] if i == 0 else (await recv(ws, until=lambda m: m["view"]["phase"] == "assemble"))["view"]
            await ws.send_json({"type": "shirt", "drawing": v["drawings"][0]["id"] if v["drawings"] else None,
                                "slogan": v["slogans"][0]["id"] if v["slogans"] else None, "color": 3})
        await asyncio.sleep(0.2)
        if r.game.phase == "assemble":  # a pick clashed with a piece someone just used
            await ann.send_json({"type": "skip"})
        state = await recv(ann, until=lambda m: m["view"]["phase"] == "vote")
        assert len(state["view"]["shirts"]) == 2 and state["view"]["shirts"][0]["strokes"]
        while r.state == "playing":
            await asyncio.sleep(0.11)
            await ann.send_json({"type": "skip"})
            await recv(ann)
        assert r.results["title"] == "Shirt Showdown" and "game.finished" in events
        for ws, _ in phones:
            await ws.close()
    finally:
        await client.close()


# -- the hall of fame in the room ---------------------------------------------------------


def stored_room(tmp_path):
    from partystore import Store

    events = []
    store = Store(tmp_path / "party.db")
    r = Room({}, CONTENT, emit=lambda kind, msg, **kw: events.append((kind, msg)), clock=Clock(),
             rng=random.Random(5), store=store)
    return r, store, events


def test_profiles_on_join_with_pins_and_devices(tmp_path):
    r, store, _ = stored_room(tmp_path)
    ann = r.join(r.code, "ann", None, None, "1234")
    assert ann.profile and ann.device and ann.name == "ann"
    r.kick("ann")
    r.banned.clear()
    # A new phone (no device token) needs Ann's PIN.
    with pytest.raises(Invalid) as e:
        r.join(r.code, "ANN", None)
    assert e.value.code == "pin_needed"
    with pytest.raises(Invalid) as e:
        r.join(r.code, "ANN", None, None, "9999")
    assert e.value.code == "pin_wrong" and "left" in str(e.value)
    again = r.join(r.code, "ANN", None, None, "1234")
    assert again.profile == ann.profile and again.name == "ann"  # the profile's own spelling
    # The same profile joining again (another phone) gets the same seat.
    assert r.join(r.code, "ann", None, again.device) is again
    bo = connect(r, "Bo")
    r.set_pin(bo.pid, "2468")
    with pytest.raises(Invalid, match="4 digits"):
        r.set_pin(bo.pid, "12")
    assert r.player_state(bo.pid)["profile"]["pin"] is True


def test_finished_games_go_into_the_hall_of_fame(tmp_path):
    r, store, events = stored_room(tmp_path)
    a, b = connect(r, "Ann"), connect(r, "Bo")
    r.start("bluff")
    g = r.game
    g.handle(a.pid, {"type": "lie", "text": "rabbits"}, 0)
    g.handle(b.pid, {"type": "lie", "text": "koalas"}, 0)
    g.skip(0)
    truth = next(i for i, o in enumerate(g.bluff.options) if o.truth)
    rabbits = next(i for i, o in enumerate(g.bluff.options) if o.text == "rabbits")
    g.handle(a.pid, {"type": "pick", "choice": truth}, 0)
    g.handle(b.pid, {"type": "pick", "choice": rabbits}, 0)
    while r.state == "playing":
        r.skip()
    res = r.results
    assert {x["badge"] for x in res["badges"]} >= {"first_win"}
    assert any(k == "player.badge" and "First Win" in m for k, m in events)
    fame = r.host_state()["fame"]
    assert fame["board"][0]["name"] == "Ann" and fame["games"] == 1
    assert r.room_view()["players"][0]["champ"] is True
    card = r.player_state(a.pid)["profile"]
    assert card["wins"] == 1 and card["rivals"] == [{"name": "Bo", "won": 1, "lost": 0}]
    snap = r.snapshot()
    assert snap["season_top"][0]["name"] == "Ann" and snap["champion"]["name"] == "Ann"


async def test_recap_and_kernel_actions(tmp_path):
    class Gone:
        closed = True  # a socket that has already closed: sends are skipped

    p = Party({"port": 0, "discord_webhook": ""}, tmp_path, run=lambda *a, **k: None)
    a = p.room.join(p.room.code, "Ann", None)
    a.sockets.add(Gone())
    b = p.room.join(p.room.code, "Bo", None)
    b.sockets.add(Gone())
    assert p.recap() == ""
    p.room.start("bluff")
    while p.room.state == "playing":
        p.room.skip()
    text = p.recap()
    assert "1 game (Bluff Buffet)" in text and "1. " in text
    with pytest.raises(ActionError, match="discord_webhook"):
        await p.send_recap_now()
    posted = []

    class Resp:
        def __enter__(self):
            return self

        def __exit__(self, *a):
            return False

    import urllib.request

    real = urllib.request.urlopen
    urllib.request.urlopen = lambda req, timeout: posted.append(json.loads(req.data)) or Resp()
    try:
        p.settings["discord_webhook"] = "https://discord.example/api/webhooks/1/x"
        assert (await p.send_recap_now()) == {"sent": True}
        assert posted[0]["content"].startswith("**Party Night recap**")
        assert not await p.send_recap()  # the same night isn't posted twice
    finally:
        urllib.request.urlopen = real
    out = await p.rename("bo", "Bobby")
    assert out["to"] == "Bobby" and b.name == "Bobby"
    await p.set_pin("Bobby", "1357")
    with pytest.raises(ActionError):
        await p.set_pin("Bobby", "abc")
    with pytest.raises(ActionError):
        await p.remove_hit(12345)
    await p.forget("Bobby")
    assert b.profile is None and p.store.profile_by_name("Bobby") is None
    counts = await p.reload_content()
    assert counts["quips"] >= 90 and counts["facts"] >= 35


def play_out(r, limit=4000):
    """Runs the clock until the game ends, the bots doing all the work."""
    for _ in range(limit):
        if r.state != "playing":
            return
        r.clock.t += 0.5
        r.tick()
    raise AssertionError(f"stuck in {r.game.phase}")


@pytest.mark.parametrize("game", ["quip", "bluff", "shirt"])
def test_bots_play_a_whole_game_by_themselves(game):
    r, events = room()
    with pytest.raises(Invalid, match="at least"):
        r.start("quip")
    bots = [r.add_bot() for _ in range(3)]
    assert len({b.name for b in bots}) == 3 and all(p["bot"] for p in r.room_view()["players"])
    assert r.vip is None  # a bot never runs the room; the host screen starts the game
    r.host_command({"type": "start", "game": game})
    refused = []
    real = r.game.handle

    def handle(pid, msg, now):
        try:
            real(pid, msg, now)
        except Invalid as e:
            refused.append((r.game.phase, msg.get("type"), str(e)))
            raise

    r.game.handle = handle
    start = r.clock.t
    play_out(r)
    assert refused == []  # every bot move is one the game accepts
    assert r.state == "results" and r.results["game"] == game
    assert sum(s["score"] for s in r.results["standings"]) > 0
    if game == "quip":
        # bots answer and vote without waiting for the timers to run out
        assert r.clock.t - start < 3 * 90 + 60
    assert "player.joined" not in events


def test_a_phone_plays_with_bots_and_stays_vip():
    r, _ = room()
    bot = r.add_bot()
    ann = connect(r, "Ann")
    r.add_bot()
    assert r.vip == ann.pid
    r.handle(ann.pid, {"type": "start", "game": "quip"})
    with pytest.raises(Invalid, match="between games"):
        r.add_bot()
    with pytest.raises(Invalid, match="between games"):
        r.remove_bots()
    g = r.game
    for _ in range(40):  # the bots answer; Ann is the one everyone waits for
        r.clock.t += 0.5
        r.tick()
    assert g.phase == "write" and g.waiting_on() == {ann.pid}
    r.host_command({"type": "kick", "pid": bot.pid})  # kicking a bot doesn't ban anyone
    assert not r.banned
    r.end_game()
    assert r.remove_bots() == 1 and list(r.seats) == [ann.pid]


def test_bots_fill_the_room_up_to_max_players():
    r, _ = room(max_players=3)
    connect(r, "Ann")
    r.add_bot()
    r.add_bot()
    with pytest.raises(Invalid, match="full"):
        r.add_bot()


def test_bots_stay_out_of_the_hall_of_fame(tmp_path):
    r, store, _ = stored_room(tmp_path)
    connect(r, "Ann")
    for _ in range(2):
        r.add_bot()
    r.start("quip")
    play_out(r)
    names = {row["name"] for row in r.host_state()["fame"]["board"]}
    assert names <= {"Ann"}
    assert all(h["name"] == "Ann" for h in r.host_state()["fame"]["hits"])
