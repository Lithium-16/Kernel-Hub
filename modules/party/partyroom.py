"""Party Games: the room and its web server.

One room at a time, with a four-letter code. The big screen opens /host?key=… (the key is only
in Kernel's status, so a share link can't drive the room), and players open / on their phones,
type the code and a name, and play over one WebSocket each. The first player in is the VIP, who
picks the game and starts it. The server listens on 127.0.0.1 only; friends reach it through
Tailscale (TailscaleShare, the same sharing Flow Race and OpenFork use).
"""

from __future__ import annotations

import asyncio
import contextlib
import hmac
import json
import logging
import random
import secrets
import time
import urllib.error
import urllib.request
from collections.abc import Callable
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any

from aiohttp import WSMsgType, web
from kernel_sdk import ActionError
from kernel_sdk.webgame import TailscaleShare, run_command
from partygames import GAMES, Content, Game, Invalid, clean
from partystore import PinNeeded, PinWrong, Store

HERE = Path(__file__).resolve().parent
WEB = HERE / "web"
CODE_LETTERS = "BCDFGHJKLMNPQRSTVWXZ"  # no vowels: codes never spell words
COLORS = 8
MAX_NAME = 16
# Test players the host can add from the host screen (or Kernel), so a game can be tried alone.
BOT_NAMES = ["Botley", "Robo Rita", "Unit 7", "Chip", "Sprocket", "Gizmo", "Bleep", "Widget"]
TICK_S = 0.25
STATIC = {
    "party.css": "text/css",
    "play.js": "text/javascript",
    "host.js": "text/javascript",
    "draw.js": "text/javascript",
    "scenes.js": "text/javascript",
}
# The host page for a room that has closed (or a wrong link): a way back, never a dead end.
GONE_PAGE = """<!doctype html><html lang="en"><head><meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1"><title>Party Night</title>
<style>body{margin:0;min-height:100vh;display:grid;place-items:center;background:#150e24;color:#f4efff;
font:18px/1.5 system-ui,sans-serif;text-align:center;padding:24px}a{display:inline-block;margin-top:18px;
padding:14px 26px;border-radius:16px;background:#ffb100;color:#150e24;font-weight:800;text-decoration:none}</style>
</head><body><main><h1>This room has closed</h1><p>The link is old, or the host closed the room.</p>
<a href="/">Go to the join page</a></main></body></html>"""
CSP = (
    "default-src 'self'; img-src 'self' data:; style-src 'self' 'unsafe-inline' https://fonts.googleapis.com; "
    "font-src https://fonts.gstatic.com; script-src 'self'; connect-src 'self' ws: wss:; "
    "frame-ancestors 'none'; base-uri 'none'; form-action 'none'"
)


@dataclass
class Seat:
    pid: str
    name: str
    token: str
    color: int
    profile: int | None = None  # in the hall of fame
    device: str = ""  # the phone's profile token, so it signs in by itself next time
    sockets: set[web.WebSocketResponse] = field(default_factory=set)
    bot: bool = False  # a test player the host added; plays by itself

    @property
    def connected(self) -> bool:
        return self.bot or bool(self.sockets)


class Room:
    """Who's here, what's being played, and what every screen should show. No networking."""

    def __init__(
        self,
        settings: dict[str, Any],
        content_dirs: list[Path],
        emit: Callable[..., None] | None = None,
        clock: Callable[[], float] = time.monotonic,
        rng: random.Random | None = None,
        store: Store | None = None,
    ) -> None:
        self.settings = settings
        self.store = store
        self._fame: dict[str, Any] | None = None
        self._cards: dict[int, dict[str, Any] | None] = {}
        self._seen = -1  # the store version the caches above were read at
        self.content_dirs = content_dirs
        self.emit = emit or (lambda *a, **k: None)
        self.clock = clock
        self.rng = rng or random.Random()
        self.used: dict[str, set[Any]] = {}
        self.games_played = 0
        self.guest = False  # opened by someone on the share link (not Kernel's own room)
        self.host_key = ""
        self.new_room()

    # -- the room -------------------------------------------------------------------------

    def new_room(self) -> None:
        self.code = "".join(self.rng.choice(CODE_LETTERS) for _ in range(4))
        self.seats: dict[str, Seat] = {}
        self.game: Game | None = None
        self.state = "lobby"  # lobby, playing or results
        self.choice = "quip"
        self.results: dict[str, Any] | None = None
        self.night: dict[str, int] = {}  # points tonight, across games
        self.night_wins: dict[str, int] = {}
        self.night_hits: list[dict[str, Any]] = []
        self.night_games: list[str] = []
        self.banned: set[str] = set()  # tokens of kicked players
        self.bot_due: dict[str, float] = {}  # when each bot makes its next move
        self.content = Content.load(*self.content_dirs)

    @property
    def max_players(self) -> int:
        return max(2, min(int(self.settings.get("max_players", 8)), 8))

    def name_of(self, pid: str) -> str:
        seat = self.seats.get(pid)
        return seat.name if seat else "?"

    @property
    def vip(self) -> str | None:
        """The first player in who's still connected picks the game (never a bot)."""
        return next((p for p, s in self.seats.items() if s.connected and not s.bot), None)

    def join(self, code: Any, name: Any, token: Any, device: Any = None, pin: Any = None) -> Seat:
        if not isinstance(code, str) or code.strip().upper() != self.code:
            raise Invalid("There's no room with that code. Check the big screen.")
        if isinstance(token, str) and token in self.banned:
            raise Invalid("You were removed from this room.")
        if isinstance(token, str):
            seat = next((s for s in self.seats.values() if hmac.compare_digest(s.token, token)), None)
            if seat is not None:
                return seat  # coming back (reconnect, refresh, new tab)
        name = clean(name, MAX_NAME)
        if not name:
            raise Invalid("Type a name first.")
        taken = next((s for s in self.seats.values() if s.name.lower() == name.lower()), None)
        if taken is not None and (self.store is None or taken.profile is None):
            raise Invalid(f"Someone here is already called {name}. Pick another name.")
        if taken is None and len(self.seats) >= self.max_players:
            raise Invalid(f"The room is full ({self.max_players} players).")
        profile, device_token = None, ""
        if self.store is not None:
            try:
                profile, name, device_token = self.store.sign_in(name, device, pin or None)
            except PinNeeded as e:
                raise Invalid(f"{e} has a PIN. Type it to play as {e} on this phone.", "pin_needed", str(e)) from None
            except PinWrong as e:
                raise Invalid(str(e), code="pin_wrong") from None
            except ValueError as e:
                raise Invalid(str(e)) from None
            here = next((s for s in self.seats.values() if s.profile == profile), None)
            if here is not None:
                here.device = device_token
                return here  # the same person on another phone (or after clearing the browser)
            self._cards.pop(profile, None)
            self._fame = None
        used = {s.color for s in self.seats.values()}
        color = next((c for c in range(COLORS) if c not in used), 0)
        seat = Seat(secrets.token_hex(4), name, secrets.token_urlsafe(18), color, profile, device_token)
        self.seats[seat.pid] = seat
        self.night.setdefault(seat.pid, 0)
        self.emit("player.joined", f"{name} joined the party (room {self.code})", player=name)
        return seat

    def kick(self, name: str) -> Seat:
        seat = next((s for s in self.seats.values() if s.name.lower() == name.strip().lower()), None)
        if seat is None:
            raise Invalid(f"Nobody here is called {name}.")
        del self.seats[seat.pid]
        if not seat.bot:
            self.banned.add(seat.token)
        self.connections_changed()
        return seat

    def leave(self, pid: str) -> None:
        """A player pressed Leave: gone from the room, or (mid-game) away until it ends."""
        seat = self.seats.get(pid)
        if seat is None:
            return
        if self.game is not None and pid in self.game.pids:
            seat.sockets.clear()
        else:
            del self.seats[pid]
        self.connections_changed()

    def add_bot(self) -> Seat:
        """A test player that answers and votes by itself (between games only)."""
        if self.state == "playing":
            raise Invalid("Add bots between games.")
        if len(self.seats) >= self.max_players:
            raise Invalid(f"The room is full ({self.max_players} players).")
        taken = {s.name.lower() for s in self.seats.values()}
        name = next((n for n in BOT_NAMES if n.lower() not in taken), None)
        k = 2
        while name is None:
            name = next((f"{n} {k}" for n in BOT_NAMES if f"{n} {k}".lower() not in taken), None)
            k += 1
        used = {s.color for s in self.seats.values()}
        color = next((c for c in range(COLORS) if c not in used), 0)
        seat = Seat(secrets.token_hex(4), name, secrets.token_urlsafe(18), color, bot=True)
        self.seats[seat.pid] = seat
        self.night.setdefault(seat.pid, 0)
        return seat

    def remove_bots(self) -> int:
        if self.state == "playing":
            raise Invalid("Remove bots between games.")
        bots = [p for p, s in self.seats.items() if s.bot]
        for p in bots:
            del self.seats[p]
        return len(bots)

    def _bots_play(self, now: float) -> bool:
        """Bots the game is waiting on make their move a few seconds after the phase starts."""
        g = self.game
        assert g is not None
        waiting = g.waiting_on() or set()
        bots = {p for p in g.pids if p in self.seats and self.seats[p].bot and p in waiting}
        for p in list(self.bot_due):
            if p not in bots:
                del self.bot_due[p]
        moved = False
        for p in bots:
            if p not in self.bot_due:
                quick = g.phase in ("vote", "final_vote", "pick")
                self.bot_due[p] = now + self.rng.uniform(1.5, 4) if quick else now + self.rng.uniform(2, 7)
            elif now >= self.bot_due[p]:
                del self.bot_due[p]
                msg = g.bot_move(p)
                if msg is not None:
                    try:
                        g.handle(p, msg, now)
                        moved = True
                    except Invalid:
                        pass
        return moved

    def connections_changed(self) -> None:
        if self.game is not None:
            self.game.set_active({p for p, s in self.seats.items() if s.connected})

    # -- playing --------------------------------------------------------------------------

    def start(self, key: Any) -> None:
        cls = GAMES.get(key) if isinstance(key, str) else None
        if cls is None:
            raise Invalid("Pick a game first.")
        if self.state == "playing":
            raise Invalid("A game is already on.")
        pids = [p for p, s in self.seats.items() if s.connected]
        if len(pids) < cls.min_players:
            raise Invalid(f"{cls.title} needs at least {cls.min_players} players.")
        names = {p: s.name for p, s in self.seats.items()}
        scale = float(self.settings.get("timer_scale", 1.0))
        used = self.used.setdefault(key, set())
        now = self.clock()
        options: dict[str, Any] = {"used": used}
        if cls.key == "bluff":
            options["questions"] = int(self.settings.get("bluff_questions", 5))
        elif cls.key == "shirt":
            options["rounds"] = int(self.settings.get("shirt_rounds", 2))
        elif cls.key == "drama":
            options["rounds"] = int(self.settings.get("drama_rounds", 1))
        self.game = cls(pids, self.content, self.rng, now, scale, names, **options)
        self.choice, self.state, self.results = key, "playing", None
        self.emit("game.started", f"{cls.title} started with {', '.join(names[p] for p in pids)}", game=cls.title)

    def end_game(self) -> None:
        self.game, self.state = None, "lobby"

    def to_lobby(self) -> None:
        if self.state == "playing":
            raise Invalid("Finish the game first.")
        self.state, self.results = "lobby", None

    def _finish(self) -> None:
        g = self.game
        assert g is not None
        standings = [s | {"name": self.name_of(s["pid"])} for s in g.standings()]
        for s in standings:
            self.night[s["pid"]] = self.night.get(s["pid"], 0) + s["score"]
        hits = [h | {"name": self.name_of(h["pid"])} for h in g.hits]
        winner = standings[0] if standings and standings[0]["score"] > 0 else None
        self.results = {"game": g.key, "title": g.title, "standings": standings, "hits": hits,
                        "winner": winner["pid"] if winner else None}
        self.game, self.state = None, "results"
        self.games_played += 1
        self.night_games.append(g.title)
        self.night_hits += hits
        if winner:
            self.night_wins[winner["pid"]] = self.night_wins.get(winner["pid"], 0) + 1
        self.results["badges"] = self._remember(g, standings, hits)
        who = f"{winner['name']} won {g.title}" if winner else f"{g.title} finished"
        scores = ", ".join(f"{s['name']} {s['score']:,}" for s in standings)
        self.emit("game.finished", f"{who} ({scores})", game=g.title,
                  winner=winner["name"] if winner else None, scores={s["name"]: s["score"] for s in standings})

    def _remember(self, g: Game, standings: list[dict[str, Any]], hits: list[dict[str, Any]]) -> list[dict[str, Any]]:
        """Saves the game to the hall of fame; returns the badges earned (with the seat they
        belong to) and announces them, and a new season champion, as events."""
        if self.store is None:
            return []
        bots = {p for p, s in self.seats.items() if s.bot}
        standings = [s for s in standings if s["pid"] not in bots]
        hits = [h for h in hits if h["pid"] not in bots]
        prof = {p: s.profile for p, s in self.seats.items() if s.profile is not None}
        seat_of = {v: k for k, v in prof.items()}
        out = self.store.record(
            g.key,
            g.title,
            [(prof.get(s["pid"]), s["score"]) for s in standings],
            [h | {"profile_id": prof.get(h["pid"])} for h in hits],
            [(prof[w], prof[lo], k) for w, lo, k in g.duels if w in prof and lo in prof],
            [(prof[p], b) for p, b in g.feats if p in prof],
            {prof[p]: n for p, n in self.night_wins.items() if p in prof},
        )
        self._fame = None
        self._cards.clear()
        badges = []
        for b in out["badges"]:
            pid = seat_of.get(b["profile_id"])
            name = self.name_of(pid) if pid else (self.store.profile(b["profile_id"]) or {}).get("name", "?")
            badges.append(b | {"pid": pid, "name": name})
            self.emit("player.badge", f"{name} earned the {b['title']} badge: {b['about']}", player=name, badge=b["title"])
        champ = out["champion"]
        if champ:
            label = time.strftime("%B %Y", time.strptime(champ["season"] + "-01", "%Y-%m-%d"))
            self.emit("season.champion", f"{champ['name']} is the {label} champion with {champ['points']:,} points",
                      player=champ["name"], season=champ["season"], points=champ["points"])
        return badges

    def _fresh(self) -> None:
        """Other rooms share the store: drop cached views when anything changed in it."""
        if self.store is not None and self.store.version != self._seen:
            self._seen = self.store.version
            self._fame = None
            self._cards.clear()

    def fame(self) -> dict[str, Any] | None:
        if self.store is None:
            return None
        self._fresh()
        if self._fame is None:
            self._fame = self.store.fame()
        return self._fame

    def card(self, pid: str) -> dict[str, Any] | None:
        seat = self.seats.get(pid)
        if self.store is None or seat is None or seat.profile is None:
            return None
        self._fresh()
        if seat.profile not in self._cards:
            self._cards[seat.profile] = self.store.profile(seat.profile)
        return self._cards[seat.profile]

    def set_pin(self, pid: str, pin: Any) -> None:
        seat = self.seats.get(pid)
        if self.store is None or seat is None or seat.profile is None:
            raise Invalid("Profiles are off.")
        try:
            self.store.set_pin(seat.profile, pin)
        except ValueError as e:
            raise Invalid(str(e)) from None
        self._cards.pop(seat.profile, None)

    def tick(self) -> bool:
        if self.game is None:
            return False
        moved = self._bots_play(self.clock())
        changed = self.game.tick(self.clock()) or moved
        if self.game.done:
            self._finish()
            return True
        return changed

    def skip(self) -> None:
        if self.game is None:
            raise Invalid("No game is on.")
        self.game.skip(self.clock())
        if self.game.done:
            self._finish()

    def handle(self, pid: str, msg: dict[str, Any]) -> None:
        """A message from a player who has joined."""
        kind = msg.get("type")
        is_vip = pid == self.vip
        if kind == "set_pin":
            self.set_pin(pid, msg.get("pin"))
        elif kind == "choose" and is_vip:
            if msg.get("game") not in GAMES:
                raise Invalid("Pick a game first.")
            self.choice = msg["game"]
        elif kind == "start" and is_vip:
            self.start(msg.get("game", self.choice))
        elif kind == "skip" and is_vip:
            self.skip()
        elif kind == "lobby" and is_vip:
            self.to_lobby()
        elif kind in ("choose", "start", "skip", "lobby"):
            raise Invalid("Only the VIP can do that.")
        elif self.game is not None and pid in self.game.pids:
            self.game.handle(pid, msg, self.clock())
            if self.game.tick(self.clock()) and self.game.done:
                self._finish()
        else:
            raise Invalid("Hang on: you're in for the next game.")

    def host_command(self, msg: dict[str, Any]) -> None:
        kind = msg.get("type")
        if kind == "choose":
            if msg.get("game") not in GAMES:
                raise Invalid("Pick a game first.")
            self.choice = msg["game"]
        elif kind == "start":
            self.start(msg.get("game", self.choice))
        elif kind == "skip":
            self.skip()
        elif kind == "lobby":
            self.to_lobby()
        elif kind == "kick" and isinstance(msg.get("pid"), str) and msg["pid"] in self.seats:
            self.kick(self.seats[msg["pid"]].name)
        elif kind == "end":
            self.end_game()
        elif kind == "add_bot":
            self.add_bot()
        elif kind == "remove_bots":
            self.remove_bots()
        else:
            raise Invalid("Unknown command.")

    # -- what the screens show ------------------------------------------------------------

    def room_view(self) -> dict[str, Any]:
        scores = self.game.scores if self.game else {}
        champ = ((self.fame() or {}).get("champion") or {}).get("id")
        return {
            "code": self.code,
            "state": self.state,
            "choice": self.choice,
            "vip": self.vip,
            "guest": self.guest,
            "max_players": self.max_players,
            "players": [
                {"pid": p, "name": s.name, "color": s.color, "connected": s.connected, "bot": s.bot,
                 "score": scores.get(p, 0), "night": self.night.get(p, 0),
                 "playing": self.game is None or p in self.game.pids,
                 "champ": champ is not None and s.profile == champ}
                for p, s in self.seats.items()
            ],
            "games": [{"key": g.key, "title": g.title, "min": g.min_players, "max": g.max_players} for g in GAMES.values()],
        }

    def host_state(self) -> dict[str, Any]:
        view = self.game.host_view(self.clock()) if self.game else self.results
        return {"type": "state", "room": self.room_view(), "view": view, "fame": self.fame()}

    def player_state(self, pid: str) -> dict[str, Any]:
        if self.game is not None and pid in self.game.pids:
            view: dict[str, Any] | None = self.game.player_view(pid, self.clock())
        elif self.game is not None:
            view = {"game": self.game.key, "phase": "next_game"}
        else:
            view = self.results
        return {"type": "state", "room": self.room_view(), "view": view, "you": pid, "profile": self.card(pid)}

    def snapshot(self) -> dict[str, Any]:
        g = self.game
        return {
            "room_code": self.code,
            "room": self.state,
            "game": g.title if g else (self.results or {}).get("title", ""),
            "phase": g.phase if g else "",
            "players": [{"name": s.name, "score": (g.scores.get(p, 0) if g else 0), "tonight": self.night.get(p, 0),
                         "connected": s.connected} for p, s in self.seats.items()],
            "online": sum(1 for s in self.seats.values() if s.connected),
            "games_played": self.games_played,
            **self._fame_summary(),
        }

    def _fame_summary(self) -> dict[str, Any]:
        """The hall of fame in short, for Kernel's status (and so the assistant can answer
        "who's winning this season?" without a tool call)."""
        f = self.fame()
        if not f:
            return {}
        champ = f["champion"]
        return {
            "season": f["season"],
            "season_top": [{"name": r["name"], "points": r["points"], "wins": r["wins"]} for r in f["board"][:5]],
            "champion": {"name": champ["name"], "season": champ["label"], "points": champ["points"]} if champ else None,
            "greatest_hits": [{"id": h["id"], "text": h["text"], "by": h["name"], "game": h["title"]} for h in f["hits"]],
            "games_recorded": f["games"],
        }


# -- the web server -----------------------------------------------------------------------


class PartyServer:
    """The pages and WebSockets on 127.0.0.1:`port`. Kernel's own room (`room`) is always open;
    with `make_room`, anyone on the share link can open more rooms ("Host a game"), each with
    its own code and host key, up to `max_rooms`. A room someone else opened closes after
    `idle_s` with nobody in it."""

    NEW_ROOM_GAP_S = 5.0  # between two new rooms, so the link can't be used to spam rooms

    def __init__(self, room: Room, host_key: str, port: int, max_connections: int = 64,
                 log: logging.Logger | None = None, make_room: Callable[[], Room] | None = None,
                 max_rooms: int = 4, idle_s: float = 30 * 60, clock: Callable[[], float] = time.monotonic) -> None:
        room.host_key = host_key
        self.room = room  # Kernel's own room
        self.rooms: list[Room] = [room]
        self.port = port
        self.max_connections = max_connections
        self.log = log or logging.getLogger("party")
        self.make_room = make_room
        self.max_rooms = max(0, int(max_rooms))
        self.idle_s = idle_s
        self.clock = clock
        self.idle_since: dict[Room, float] = {}
        self.last_new = -1e9
        self.hosts: dict[Room, set[web.WebSocketResponse]] = {}
        self.sockets: set[web.WebSocketResponse] = set()
        self.runner: web.AppRunner | None = None
        self.link: Callable[[], str] = lambda: ""  # the share link, shown on the big screen
        self._ticker: asyncio.Task[None] | None = None
        self._closing: set[asyncio.Task[Any]] = set()

    @property
    def host_key(self) -> str:
        return self.room.host_key

    def app(self) -> web.Application:
        app = web.Application(client_max_size=64 * 1024, middlewares=[self._headers])
        app.router.add_get("/", self._page("play.html"))
        app.router.add_get("/host", self._host_page)
        app.router.add_post("/host/new", self._new_room)
        app.router.add_get("/health", self._health)
        app.router.add_get("/static/{name}", self._static)
        app.router.add_get("/ws", self._ws)
        app.on_startup.append(self._start_ticker)
        app.on_cleanup.append(self._stop_ticker)
        return app

    @web.middleware
    async def _headers(self, request: web.Request, handler: Callable[..., Any]) -> web.StreamResponse:
        resp = await handler(request)
        resp.headers.setdefault("Content-Security-Policy", CSP)
        resp.headers.setdefault("X-Content-Type-Options", "nosniff")
        resp.headers.setdefault("Referrer-Policy", "no-referrer")
        resp.headers.setdefault("Cache-Control", "no-cache")
        return resp

    def _page(self, name: str) -> Callable[[web.Request], Any]:
        async def page(request: web.Request) -> web.StreamResponse:
            return web.FileResponse(WEB / name, headers={"Content-Type": "text/html; charset=utf-8"})

        return page

    async def _health(self, request: web.Request) -> web.Response:
        return web.Response(text="ok")

    # -- rooms ----------------------------------------------------------------------------

    @property
    def hosting_open(self) -> bool:
        return self.make_room is not None and self.max_rooms > 0

    def room_for_key(self, key: str) -> Room | None:
        return next((r for r in self.rooms if key and hmac.compare_digest(key, r.host_key)), None)

    def room_for_code(self, code: Any) -> Room | None:
        if not isinstance(code, str):
            return None
        code = code.strip().upper()
        return next((r for r in self.rooms if r.code == code), None)

    def open_room(self) -> Room:
        """A new room for someone on the share link; raises Invalid when hosting is off or full."""
        if not self.hosting_open:
            raise Invalid("Hosting is turned off here. Ask for a room code instead.")
        guests = [r for r in self.rooms if r.guest]
        if len(guests) >= self.max_rooms:
            raise Invalid(f"All {self.max_rooms} rooms are in use. Join one with its code, or try again later.")
        now = self.clock()
        if now - self.last_new < self.NEW_ROOM_GAP_S:
            raise Invalid("A room was just opened. Try again in a few seconds.")
        assert self.make_room is not None
        room = self.make_room()
        while any(r.code == room.code for r in self.rooms):
            room.code = "".join(room.rng.choice(CODE_LETTERS) for _ in range(4))
        room.guest = True
        room.host_key = secrets.token_urlsafe(18)
        self.rooms.append(room)
        self.last_new = now
        self.idle_since[room] = now
        return room

    async def close_room(self, room: Room) -> None:
        """Sends everyone in a guest room back to the join screen and forgets the room."""
        if room is self.room or room not in self.rooms:
            return
        self.rooms.remove(room)
        self.idle_since.pop(room, None)
        sockets = list(self.hosts.pop(room, set())) + [ws for s in room.seats.values() for ws in s.sockets]
        await asyncio.gather(*(self._send(ws, {"type": "closed"}) for ws in sockets))
        for ws in sockets:
            # in the background: one of these may be the socket whose message asked for this
            task = asyncio.create_task(ws.close())
            self._closing.add(task)
            task.add_done_callback(self._closing.discard)

    def _busy(self, room: Room) -> bool:
        return bool(self.hosts.get(room)) or any(s.sockets for s in room.seats.values())

    async def close_idle_rooms(self) -> None:
        now = self.clock()
        for room in [r for r in self.rooms if r.guest]:
            if self._busy(room):
                self.idle_since.pop(room, None)
            elif now - self.idle_since.setdefault(room, now) >= self.idle_s:
                await self.close_room(room)

    async def _new_room(self, request: web.Request) -> web.Response:
        try:
            room = self.open_room()
        except Invalid as e:
            return web.json_response({"error": str(e)}, status=429 if self.hosting_open else 403)
        self.log.info("room %s opened from the share link", room.code)
        return web.json_response({"code": room.code, "url": f"/host?key={room.host_key}"})

    async def _host_page(self, request: web.Request) -> web.StreamResponse:
        if self.room_for_key(request.query.get("key", "")) is None:
            return web.Response(status=403, content_type="text/html", text=GONE_PAGE)
        return web.FileResponse(WEB / "host.html", headers={"Content-Type": "text/html; charset=utf-8"})

    async def _static(self, request: web.Request) -> web.StreamResponse:
        name = request.match_info["name"]
        if name not in STATIC:
            raise web.HTTPNotFound()
        return web.FileResponse(WEB / name, headers={"Content-Type": f"{STATIC[name]}; charset=utf-8"})

    # -- sockets --------------------------------------------------------------------------

    async def _ws(self, request: web.Request) -> web.StreamResponse:
        host = request.query.get("role") == "host"
        room: Room | None = self.room_for_key(request.query.get("key", "")) if host else None
        if host and room is None:
            raise web.HTTPForbidden()
        if len(self.sockets) >= self.max_connections:
            raise web.HTTPServiceUnavailable(text="too many connections")
        ws = web.WebSocketResponse(heartbeat=20, max_msg_size=64 * 1024)  # drawings
        await ws.prepare(request)
        self.sockets.add(ws)
        seat: Seat | None = None
        if host:
            assert room is not None
            self.hosts.setdefault(room, set()).add(ws)
            await self._send(ws, self.host_state(room))
        else:
            await self._send(ws, {"type": "hello", "code_hint": len(self.room.code), "hosting": self.hosting_open})
        budget, refilled = 20.0, time.monotonic()
        try:
            async for msg in ws:
                if msg.type != WSMsgType.TEXT:
                    continue
                now = time.monotonic()
                budget = min(20.0, budget + (now - refilled) * 10)
                refilled = now
                if budget < 1:
                    continue  # more than ~10 messages a second: drop
                budget -= 1
                try:
                    data = json.loads(msg.data)
                except ValueError:
                    continue
                if not isinstance(data, dict):
                    continue
                try:
                    if host:
                        assert room is not None
                        if data.get("type") == "close" and room.guest:
                            await self.close_room(room)
                            break
                        room.host_command(data)
                    elif seat is None or room is None or room not in self.rooms or seat.pid not in room.seats:
                        if data.get("type") != "join":
                            await self._send(ws, {"type": "joined", "pid": None})
                            continue
                        if seat is not None and room is not None:
                            seat.sockets.discard(ws)
                            room.connections_changed()
                        target = self.room_for_code(data.get("code"))
                        if target is None:
                            raise Invalid("There's no room with that code. Check the big screen.")
                        seat = target.join(data.get("code"), data.get("name"), data.get("token"),
                                           data.get("device"), data.get("pin"))
                        room = target
                        seat.sockets.add(ws)
                        room.connections_changed()
                        await self._send(ws, {"type": "joined", "pid": seat.pid, "token": seat.token, "name": seat.name,
                                              "device": seat.device})
                    elif data.get("type") == "leave":
                        room.leave(seat.pid)
                        seat = None
                        await self._send(ws, {"type": "left"})
                    else:
                        room.handle(seat.pid, data)
                except Invalid as e:
                    await self._send(ws, {"type": "error", "message": str(e), "code": e.code, "name": e.name})
                    continue
                if room is not None:
                    await self.broadcast(room)
        finally:
            self.sockets.discard(ws)
            if host and room is not None:
                self.hosts.get(room, set()).discard(ws)
            if seat is not None and room is not None:
                seat.sockets.discard(ws)
                room.connections_changed()
                await self.broadcast(room)
        return ws

    def host_state(self, room: Room | None = None) -> dict[str, Any]:
        return (room or self.room).host_state() | {"link": self.link()}

    async def _send(self, ws: web.WebSocketResponse, data: dict[str, Any]) -> None:
        if ws.closed:
            return
        with contextlib.suppress(ConnectionError, RuntimeError):
            await ws.send_str(json.dumps(data, separators=(",", ":")))

    async def broadcast(self, room: Room | None = None) -> None:
        """Sends the latest state to every screen in `room` (default: Kernel's room)."""
        room = room or self.room
        if room not in self.rooms:
            return
        host = self.host_state(room)
        sends = [self._send(ws, host) for ws in list(self.hosts.get(room, ()))]
        for seat in list(room.seats.values()):
            state = room.player_state(seat.pid)
            sends += [self._send(ws, state) for ws in list(seat.sockets)]
        await asyncio.gather(*sends)

    async def broadcast_all(self) -> None:
        for room in list(self.rooms):
            await self.broadcast(room)

    async def kick_sockets(self, seat: Seat) -> None:
        for ws in list(seat.sockets):
            await self._send(ws, {"type": "kicked"})
            await ws.close()

    async def _start_ticker(self, app: web.Application) -> None:
        self._ticker = asyncio.create_task(self._tick_forever())

    async def _stop_ticker(self, app: web.Application) -> None:
        if self._ticker:
            self._ticker.cancel()
            with contextlib.suppress(asyncio.CancelledError):
                await self._ticker
        for ws in list(self.sockets):
            with contextlib.suppress(Exception):
                await ws.close()

    async def _tick_forever(self) -> None:
        n = 0
        while True:
            await asyncio.sleep(TICK_S)
            n += 1
            for room in list(self.rooms):
                try:
                    if room.tick():
                        await self.broadcast(room)
                except Exception:
                    self.log.exception("tick failed in room %s", room.code)
            if n % 40 == 0:  # about every 10 s
                try:
                    await self.close_idle_rooms()
                except Exception:
                    self.log.exception("closing idle rooms failed")

    async def start(self) -> None:
        self.runner = web.AppRunner(self.app(), access_log=None, handle_signals=False)
        await self.runner.setup()
        site = web.TCPSite(self.runner, "127.0.0.1", self.port)
        try:
            await site.start()
        except OSError as e:
            await self.runner.cleanup()
            self.runner = None
            raise ActionError("module_failed", f"couldn't listen on port {self.port}: {e.strerror or e}") from None

    async def stop(self) -> None:
        if self.runner is not None:
            await self.runner.cleanup()
            self.runner = None

    @property
    def running(self) -> bool:
        return self.runner is not None


# -- Kernel's side ------------------------------------------------------------------------


class Party(TailscaleShare):
    """What main.py drives: the server, the room, sharing and the status Kernel shows."""

    def __init__(self, settings: dict[str, Any], data_dir: Path, emit: Callable[..., None] | None = None,
                 log: logging.Logger | None = None, run: Callable[..., Any] = run_command) -> None:
        super().__init__("Party Games", settings, int(settings.get("port", 8097)), emit, log, run)
        self.data = data_dir
        (data_dir / "content").mkdir(parents=True, exist_ok=True)
        self.store = Store(data_dir / "party.db")
        self.room = Room(settings, [HERE / "content", data_dir / "content"], emit=self.emit, store=self.store)
        open_hosting = bool(settings.get("open_hosting", True))
        self.server = PartyServer(self.room, self._host_key(), self.port, int(settings.get("max_connections", 64)), self.log,
                                  make_room=self._guest_room if open_hosting else None,
                                  max_rooms=int(settings.get("max_rooms", 4)))
        self.server.link = lambda: self.link
        self.want_running = bool(settings.get("autostart", True))
        self.error = ""

    def _guest_room(self) -> Room:
        """A room someone opened from the share link: same games, content and hall of fame, but
        their joins don't go to Kernel's events (only finished games, badges and champions do)."""

        def emit(kind: str, message: str, **data: Any) -> None:
            if kind != "player.joined":
                self.emit(kind, message, **data)

        return Room(self.settings, [HERE / "content", self.data / "content"], emit=emit, store=self.store)

    def _host_key(self) -> str:
        path = self.data / "host_key.txt"
        try:
            key = path.read_text().strip()
            if len(key) >= 16:
                return key
        except OSError:
            pass
        key = secrets.token_urlsafe(18)
        path.write_text(key)
        return key

    def host_url(self) -> str:
        return f"http://127.0.0.1:{self.port}/host?key={self.server.host_key}"

    def join_link(self) -> str:
        if self.link:
            return f"{self.link}/?code={self.room.code}"
        return f"http://127.0.0.1:{self.port}/?code={self.room.code}" if self.server.running else ""

    async def start(self) -> dict[str, Any]:
        self.want_running = True
        if not self.server.running:
            try:
                await self.server.start()
            except ActionError as e:
                self.error = e.message
                raise
            self.error = ""
            self.emit("room.opened", f"Party Games is open: room {self.room.code}"
                      + (f", join at {self.join_link()}" if self.link else ""), code=self.room.code, link=self.join_link())
        return {"running": True, "room_code": self.room.code, "host_url": self.host_url()}

    async def stop(self) -> dict[str, Any]:
        self.want_running = False
        await self.server.stop()
        await self.send_recap()
        return {"stopped": True}

    async def restart(self) -> dict[str, Any]:
        await self.server.stop()
        return await self.start()

    async def new_room(self) -> dict[str, Any]:
        await self.send_recap()
        for seat in list(self.room.seats.values()):
            await self.server.kick_sockets(seat)
        self.room.new_room()
        await self.server.broadcast()
        self.emit("room.opened", f"New Party Games room: {self.room.code}", code=self.room.code, link=self.join_link())
        return {"room_code": self.room.code, "link": self.join_link()}

    async def end_game(self) -> dict[str, Any]:
        if self.room.game is None:
            raise ActionError("disabled", "no game is being played")
        title = self.room.game.title
        self.room.end_game()
        await self.server.broadcast()
        return {"ended": title}

    async def kick(self, name: str) -> dict[str, Any]:
        try:
            seat = self.room.kick(name)
        except Invalid as e:
            raise ActionError("invalid_params", str(e)) from None
        await self.server.kick_sockets(seat)
        await self.server.broadcast()
        return {"kicked": seat.name}

    async def add_bots(self, count: int = 1) -> dict[str, Any]:
        added = []
        try:
            for _ in range(max(1, min(int(count), 8))):
                added.append(self.room.add_bot().name)
        except Invalid as e:
            if not added:
                raise ActionError("invalid_params", str(e)) from None
        await self.server.broadcast()
        return {"added": added, "players": len(self.room.seats)}

    async def remove_bots(self) -> dict[str, Any]:
        try:
            n = self.room.remove_bots()
        except Invalid as e:
            raise ActionError("invalid_params", str(e)) from None
        await self.server.broadcast()
        return {"removed": n}

    # -- the hall of fame -----------------------------------------------------------------

    async def reload_content(self) -> dict[str, Any]:
        for room in self.server.rooms:
            room.content = Content.load(*room.content_dirs)
        c = self.room.content
        return {"quips": len(c.quips), "facts": len(c.facts), "doodle_ideas": len(c.doodle_ideas),
                "slogan_ideas": len(c.slogan_ideas)}

    async def _changed(self) -> None:
        await self.server.broadcast_all()

    async def remove_hit(self, hit_id: int) -> dict[str, Any]:
        try:
            self.store.remove_hit(hit_id)
        except ValueError as e:
            raise ActionError("invalid_params", str(e)) from None
        await self._changed()
        return {"removed": hit_id}

    async def rename(self, name: str, new_name: str) -> dict[str, Any]:
        new_name = clean(new_name, MAX_NAME)
        if not new_name:
            raise ActionError("invalid_params", "the new name is empty")
        try:
            self.store.rename(name, new_name)
        except ValueError as e:
            raise ActionError("invalid_params", str(e)) from None
        for seat in (s for r in self.server.rooms for s in r.seats.values()):
            if seat.name.lower() == name.lower():
                seat.name = new_name
        await self._changed()
        return {"renamed": name, "to": new_name}

    async def forget(self, name: str) -> dict[str, Any]:
        p = self.store.profile_by_name(name)
        try:
            self.store.forget(name)
        except ValueError as e:
            raise ActionError("invalid_params", str(e)) from None
        for seat in (s for r in self.server.rooms for s in r.seats.values()):
            if p is not None and seat.profile == p["id"]:
                seat.profile, seat.device = None, ""
        await self._changed()
        return {"forgot": name}

    async def set_pin(self, name: str, pin: str) -> dict[str, Any]:
        p = self.store.profile_by_name(name)
        if p is None:
            raise ActionError("invalid_params", f"no profile called {name}")
        try:
            self.store.set_pin(p["id"], pin)
        except ValueError as e:
            raise ActionError("invalid_params", str(e)) from None
        await self._changed()
        return {"pin_set_for": p["name"]}

    def recap(self) -> str:
        """Tonight in a few lines, or "" if nothing was played."""
        r = self.room
        if not r.night_games:
            return ""
        names = {p: s.name for p, s in r.seats.items()}
        board = sorted(((names.get(p), pts) for p, pts in r.night.items() if names.get(p)), key=lambda x: -x[1])
        lines = [f"**Party Night recap** (room {r.code}): {len(r.night_games)} game{'s' if len(r.night_games) != 1 else ''}"
                 f" ({', '.join(dict.fromkeys(r.night_games))})"]
        lines += [f"{i + 1}. {n}: {pts:,} points" for i, (n, pts) in enumerate(board[:8])]
        best = sorted(r.night_hits, key=lambda h: -(h["votes"] / max(h["of"], 1)))[:3]
        if best:
            lines.append("Best moments: " + " · ".join(f"“{h['text']}” ({h['name']})" for h in best))
        return "\n".join(lines)

    async def send_recap(self) -> bool:
        """Posts tonight's recap to the Discord webhook in the settings (if any), once."""
        url = str(self.settings.get("discord_webhook") or "").strip()
        text = self.recap()
        if not url or not text:
            return False
        if not url.startswith("https://"):
            self.log.warning("discord_webhook must be an https:// address")
            return False
        self.room.night_games = []  # sent: don't send the same night twice
        body = json.dumps({"content": text[:1900], "username": "Party Night", "allowed_mentions": {"parse": []}}).encode()

        def post() -> bool:
            req = urllib.request.Request(url, data=body, headers={"Content-Type": "application/json",
                                                                   "User-Agent": "kernel-party"})
            try:
                with urllib.request.urlopen(req, timeout=10):
                    return True
            except (urllib.error.URLError, OSError) as e:
                self.log.warning("couldn't post the recap to Discord: %s", e)
                return False

        return await asyncio.to_thread(post)

    async def send_recap_now(self) -> dict[str, Any]:
        if not str(self.settings.get("discord_webhook") or "").strip():
            raise ActionError("disabled", "set discord_webhook in Party Games' settings first")
        text = self.recap()
        if not text:
            raise ActionError("disabled", "nothing has been played in this room yet")
        if not await self.send_recap():
            raise ActionError("offline", "Discord didn't take the recap; see the module log")
        return {"sent": True}

    async def run_forever(self) -> None:
        if self.want_running:
            with contextlib.suppress(ActionError):
                await self.start()
        last_share = 0.0
        while True:
            now = time.monotonic()
            if now - last_share > 60:
                last_share = now
                await asyncio.to_thread(self.refresh_sharing)
            if self.want_running and not self.server.running:
                with contextlib.suppress(ActionError):
                    await self.start()
            await asyncio.sleep(float(self.settings.get("poll_s", 5.0)))

    def snapshot(self) -> dict[str, Any]:
        running = self.server.running
        return {
            "state": "running" if running else ("crashed" if self.error else "stopped"),
            **self.room.snapshot(),
            "host_url": self.host_url() if running else "",
            "link": self.join_link() if running else "",
            "local_url": f"http://127.0.0.1:{self.port}" if running else "",
            "sharing": self.sharing,
            "error": self.error,
            "open_hosting": self.server.hosting_open,
            "other_rooms": [
                {"code": r.code, "players": [s.name for s in r.seats.values() if s.connected],
                 "game": r.game.title if r.game else ""}
                for r in self.server.rooms if r.guest
            ],
        }
