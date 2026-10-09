"""Party Games: what the party remembers between nights.

A small SQLite database in the module's data folder (party.db), kept across restarts and Kernel
updates: profiles (a name, the phones that play as it, an optional PIN for new phones), every
finished game, the best moments (greatest hits), badges and head-to-head records. Seasons are
calendar months: the season leaderboard counts this month's games, and last month's leader is
the champion (the one wearing the crown).

Only players in a room see any of it (the host screen and their phones); nothing is on a
public page.
"""

from __future__ import annotations

import hashlib
import hmac
import json
import secrets
import sqlite3
import time
from collections.abc import Callable
from pathlib import Path
from typing import Any

PIN_TRIES = 5
PIN_LOCK_S = 600

# Badges: key -> (title, what it's for)
BADGES: dict[str, tuple[str, str]] = {
    "first_win": ("First Win", "Won a game for the first time"),
    "hat_trick": ("Hat Trick", "Won three games in one night"),
    "veteran": ("Veteran", "Played 10 games"),
    "regular": ("Regular", "Played 50 games"),
    "clean_sweep": ("Clean Sweep", "Got every vote in a Quip Clash matchup"),
    "crowd_favorite": ("Crowd Favorite", "Won the Quip Clash final round"),
    "master_fibber": ("Master Fibber", "Fooled everyone with one lie in Bluff Buffet"),
    "truth_seeker": ("Truth Seeker", "Found the truth in every Bluff Buffet question"),
    "unbeatable_tee": ("Unbeatable Tee", "A shirt you made part of survived 3 challenges"),
    "fashion_icon": ("Fashion Icon", "Made part of the winning shirt in a Shirt Showdown final"),
    "season_champ": ("Season Champion", "Topped a monthly season"),
}

MIGRATIONS = [
    """
    CREATE TABLE profiles (
        id INTEGER PRIMARY KEY,
        name TEXT NOT NULL UNIQUE COLLATE NOCASE,
        pin_hash TEXT,
        pin_salt TEXT,
        failed INTEGER NOT NULL DEFAULT 0,
        locked_until REAL NOT NULL DEFAULT 0,
        created REAL NOT NULL
    );
    CREATE TABLE devices (
        token TEXT PRIMARY KEY,
        profile_id INTEGER NOT NULL REFERENCES profiles(id) ON DELETE CASCADE,
        created REAL NOT NULL
    );
    CREATE TABLE games (
        id INTEGER PRIMARY KEY,
        game TEXT NOT NULL,
        title TEXT NOT NULL,
        season TEXT NOT NULL,
        played_at REAL NOT NULL,
        winner INTEGER REFERENCES profiles(id) ON DELETE SET NULL
    );
    CREATE TABLE results (
        game_id INTEGER NOT NULL REFERENCES games(id) ON DELETE CASCADE,
        profile_id INTEGER REFERENCES profiles(id) ON DELETE CASCADE,
        score INTEGER NOT NULL,
        place INTEGER NOT NULL
    );
    CREATE INDEX results_profile ON results(profile_id);
    CREATE TABLE hits (
        id INTEGER PRIMARY KEY,
        game_id INTEGER NOT NULL REFERENCES games(id) ON DELETE CASCADE,
        profile_id INTEGER REFERENCES profiles(id) ON DELETE CASCADE,
        kind TEXT NOT NULL,
        prompt TEXT NOT NULL DEFAULT '',
        text TEXT NOT NULL,
        votes INTEGER NOT NULL,
        of INTEGER NOT NULL,
        extra TEXT NOT NULL DEFAULT '{}'
    );
    CREATE TABLE badges (
        profile_id INTEGER NOT NULL REFERENCES profiles(id) ON DELETE CASCADE,
        badge TEXT NOT NULL,
        earned_at REAL NOT NULL,
        PRIMARY KEY (profile_id, badge)
    );
    CREATE TABLE duels (
        winner INTEGER NOT NULL REFERENCES profiles(id) ON DELETE CASCADE,
        loser INTEGER NOT NULL REFERENCES profiles(id) ON DELETE CASCADE,
        kind TEXT NOT NULL,
        count INTEGER NOT NULL DEFAULT 0,
        PRIMARY KEY (winner, loser, kind)
    );
    CREATE TABLE meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
    """,
]

DUEL_WORDS = {"quip": "Quip Clash matchups", "fooled": "fooled in Bluff Buffet", "shirt": "shirt battles"}


class PinNeeded(Exception):
    """The name belongs to a profile with a PIN, and this phone isn't one of its phones."""


class PinWrong(Exception):
    pass


def season_of(t: float) -> str:
    return time.strftime("%Y-%m", time.localtime(t))


def season_label(season: str) -> str:
    return time.strftime("%B %Y", time.strptime(season + "-01", "%Y-%m-%d"))


def _hash(pin: str, salt: str) -> str:
    return hashlib.pbkdf2_hmac("sha256", pin.encode(), bytes.fromhex(salt), 100_000).hex()


def valid_pin(pin: Any) -> bool:
    return isinstance(pin, str) and len(pin) == 4 and pin.isdigit()


class Store:
    def __init__(self, path: Path | str, clock: Callable[[], float] = time.time) -> None:
        self.clock = clock
        self.db = sqlite3.connect(str(path), isolation_level=None, check_same_thread=False)
        self.db.row_factory = sqlite3.Row
        self.db.execute("PRAGMA foreign_keys = ON")
        self.db.execute("PRAGMA journal_mode = WAL")
        self._migrate()

    def _migrate(self) -> None:
        version = self.db.execute("PRAGMA user_version").fetchone()[0]
        for i, sql in enumerate(MIGRATIONS[version:], start=version + 1):
            with self.db:
                self.db.execute("BEGIN")
                for statement in _statements(sql):
                    self.db.execute(statement)
                self.db.execute(f"PRAGMA user_version = {i}")

    def close(self) -> None:
        self.db.close()

    def _one(self, sql: str, *args: Any) -> sqlite3.Row | None:
        return self.db.execute(sql, args).fetchone()

    def _all(self, sql: str, *args: Any) -> list[sqlite3.Row]:
        return self.db.execute(sql, args).fetchall()

    def meta(self, key: str, value: str | None = None) -> str | None:
        if value is not None:
            self.db.execute("INSERT OR REPLACE INTO meta VALUES (?, ?)", (key, value))
            return value
        row = self._one("SELECT value FROM meta WHERE key = ?", key)
        return row["value"] if row else None

    # -- profiles -------------------------------------------------------------------------

    def profile_by_name(self, name: str) -> sqlite3.Row | None:
        return self._one("SELECT * FROM profiles WHERE name = ?", name)

    def profile_for_device(self, token: Any) -> sqlite3.Row | None:
        if not isinstance(token, str) or not token:
            return None
        return self._one("SELECT p.* FROM profiles p JOIN devices d ON d.profile_id = p.id WHERE d.token = ?", token)

    def _new_device(self, profile_id: int) -> str:
        token = secrets.token_urlsafe(24)
        self.db.execute("INSERT INTO devices VALUES (?, ?, ?)", (token, profile_id, self.clock()))
        return token

    def sign_in(self, name: str, device: Any, pin: Any = None) -> tuple[int, str, str]:
        """(profile id, canonical name, device token) for someone joining as `name`.

        A new name makes a profile (with the PIN, if one was given). A known name needs one of
        its phones (the device token) or, from a new phone, its PIN. Raises PinNeeded or
        PinWrong; ValueError when the name is taken and has no PIN to prove it."""
        mine = self.profile_for_device(device)
        if mine is not None and mine["name"].lower() == name.lower():
            return mine["id"], mine["name"], device
        p = self.profile_by_name(name)
        now = self.clock()
        if p is None:
            pid = self.db.execute("INSERT INTO profiles (name, created) VALUES (?, ?)", (name, now)).lastrowid
            assert pid is not None
            if valid_pin(pin):
                self.set_pin(pid, pin)
            return pid, name, self._new_device(pid)
        if not p["pin_hash"]:
            raise ValueError(f"Someone already plays as {p['name']}. Pick another name.")
        if p["locked_until"] > now:
            mins = max(1, round((p["locked_until"] - now) / 60))
            raise PinWrong(f"Too many wrong PINs. Try again in {mins} minute{'s' if mins != 1 else ''}.")
        if pin is None or pin == "":
            raise PinNeeded(p["name"])
        if not valid_pin(pin) or not hmac.compare_digest(_hash(pin, p["pin_salt"]), p["pin_hash"]):
            failed = p["failed"] + 1
            locked = now + PIN_LOCK_S if failed >= PIN_TRIES else 0
            self.db.execute("UPDATE profiles SET failed = ?, locked_until = ? WHERE id = ?",
                            (0 if locked else failed, locked, p["id"]))
            if locked:
                raise PinWrong("Too many wrong PINs. Try again in 10 minutes.")
            left = PIN_TRIES - failed
            raise PinWrong(f"Wrong PIN. {left} tr{'ies' if left != 1 else 'y'} left.")
        self.db.execute("UPDATE profiles SET failed = 0, locked_until = 0 WHERE id = ?", (p["id"],))
        return p["id"], p["name"], self._new_device(p["id"])

    def set_pin(self, profile_id: int, pin: str) -> None:
        if not valid_pin(pin):
            raise ValueError("A PIN is 4 digits.")
        salt = secrets.token_hex(16)
        self.db.execute("UPDATE profiles SET pin_hash = ?, pin_salt = ?, failed = 0, locked_until = 0 WHERE id = ?",
                        (_hash(pin, salt), salt, profile_id))

    def has_pin(self, profile_id: int) -> bool:
        row = self._one("SELECT pin_hash FROM profiles WHERE id = ?", profile_id)
        return bool(row and row["pin_hash"])

    def rename(self, old: str, new: str) -> str:
        p = self.profile_by_name(old)
        if p is None:
            raise ValueError(f"No profile called {old}.")
        other = self.profile_by_name(new)
        if other is not None and other["id"] != p["id"]:
            raise ValueError(f"{new} is taken.")
        self.db.execute("UPDATE profiles SET name = ? WHERE id = ?", (new, p["id"]))
        return new

    def forget(self, name: str) -> None:
        p = self.profile_by_name(name)
        if p is None:
            raise ValueError(f"No profile called {name}.")
        self.db.execute("DELETE FROM profiles WHERE id = ?", (p["id"],))

    # -- recording games ------------------------------------------------------------------

    def record(
        self,
        game: str,
        title: str,
        standings: list[tuple[int | None, int]],
        hits: list[dict[str, Any]],
        duels: list[tuple[int, int, str]],
        feats: list[tuple[int, str]],
        night_wins: dict[int, int],
    ) -> dict[str, Any]:
        """Saves a finished game. standings: (profile id, score) best first. Returns the new
        badges ({"profile_id", "badge"}) and, on the first game of a new month, last month's
        champion ({"name", "points", "season"})."""
        now = self.clock()
        season = season_of(now)
        prev_season = self.meta("season")
        new: list[dict[str, Any]] = []
        champion = None
        with self.db:
            self.db.execute("BEGIN")
            if prev_season and prev_season != season:
                top = self.season_board(prev_season, 1)
                if top:
                    champion = top[0] | {"season": prev_season}
                    new += self._award(top[0]["id"], "season_champ", now)
            self.meta("season", season)
            winner = standings[0][0] if standings and standings[0][1] > 0 else None
            gid = self.db.execute("INSERT INTO games (game, title, season, played_at, winner) VALUES (?, ?, ?, ?, ?)",
                                  (game, title, season, now, winner)).lastrowid
            for place, (pid, score) in enumerate(standings, start=1):
                self.db.execute("INSERT INTO results VALUES (?, ?, ?, ?)", (gid, pid, score, place))
            for h in hits:
                extra = json.dumps(h.get("shirt") or {})
                self.db.execute(
                    "INSERT INTO hits (game_id, profile_id, kind, prompt, text, votes, of, extra) VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
                    (gid, h.get("profile_id"), h["kind"], h.get("prompt", ""), h["text"], int(h["votes"]), int(h["of"]), extra),
                )
            for w, loser, kind in duels:
                if w != loser:
                    self.db.execute(
                        "INSERT INTO duels VALUES (?, ?, ?, 1) ON CONFLICT(winner, loser, kind) DO UPDATE SET count = count + 1",
                        (w, loser, kind),
                    )
            for pid, badge in feats:
                new += self._award(pid, badge, now)
            if winner is not None:
                if self._count("SELECT COUNT(*) FROM games WHERE winner = ?", winner) == 1:
                    new += self._award(winner, "first_win", now)
                if night_wins.get(winner, 0) >= 3:
                    new += self._award(winner, "hat_trick", now)
            for pid, _ in standings:
                if pid is None:
                    continue
                played = self._count("SELECT COUNT(*) FROM results WHERE profile_id = ?", pid)
                if played >= 10:
                    new += self._award(pid, "veteran", now)
                if played >= 50:
                    new += self._award(pid, "regular", now)
        return {"badges": new, "champion": champion, "game_id": gid}

    def _count(self, sql: str, *args: Any) -> int:
        return int(self.db.execute(sql, args).fetchone()[0])

    def _award(self, pid: int | None, badge: str, now: float) -> list[dict[str, Any]]:
        if pid is None or badge not in BADGES:
            return []
        cur = self.db.execute("INSERT OR IGNORE INTO badges VALUES (?, ?, ?)", (pid, badge, now))
        return [{"profile_id": pid, "badge": badge, "title": BADGES[badge][0], "about": BADGES[badge][1]}] if cur.rowcount else []

    def remove_hit(self, hit_id: int) -> None:
        if self.db.execute("DELETE FROM hits WHERE id = ?", (hit_id,)).rowcount == 0:
            raise ValueError(f"No greatest hit with id {hit_id}.")

    # -- reading --------------------------------------------------------------------------

    def season_board(self, season: str | None = None, limit: int = 8) -> list[dict[str, Any]]:
        season = season or season_of(self.clock())
        rows = self._all(
            """SELECT p.id, p.name, SUM(r.score) AS points, COUNT(*) AS games,
                      SUM(CASE WHEN g.winner = p.id THEN 1 ELSE 0 END) AS wins
               FROM results r JOIN games g ON g.id = r.game_id JOIN profiles p ON p.id = r.profile_id
               WHERE g.season = ? GROUP BY p.id ORDER BY points DESC, wins DESC, p.name LIMIT ?""",
            season, limit,
        )
        return [dict(r) for r in rows]

    def all_time(self, limit: int = 8) -> list[dict[str, Any]]:
        rows = self._all(
            """SELECT p.id, p.name, COALESCE(SUM(r.score), 0) AS points, COUNT(r.game_id) AS games,
                      (SELECT COUNT(*) FROM games g WHERE g.winner = p.id) AS wins
               FROM profiles p LEFT JOIN results r ON r.profile_id = p.id
               GROUP BY p.id HAVING games > 0 ORDER BY wins DESC, points DESC, p.name LIMIT ?""",
            limit,
        )
        return [dict(r) for r in rows]

    def champion(self) -> dict[str, Any] | None:
        """Last month's leader (or this month's, before any month has ended)."""
        now = season_of(self.clock())
        row = self._one("SELECT season FROM games WHERE season < ? ORDER BY season DESC LIMIT 1", now)
        season = row["season"] if row else now
        top = self.season_board(season, 1)
        return top[0] | {"season": season, "label": season_label(season), "current": season == now} if top else None

    def champions(self, limit: int = 6) -> list[dict[str, Any]]:
        now = season_of(self.clock())
        out = []
        for r in self._all("SELECT DISTINCT season FROM games WHERE season < ? ORDER BY season DESC LIMIT ?", now, limit):
            top = self.season_board(r["season"], 1)
            if top:
                out.append(top[0] | {"season": r["season"], "label": season_label(r["season"])})
        return out

    def hits(self, limit: int = 8) -> list[dict[str, Any]]:
        rows = self._all(
            """SELECT h.id, h.kind, h.prompt, h.text, h.votes, h.of, h.extra, p.name, g.title, g.played_at
               FROM hits h JOIN games g ON g.id = h.game_id LEFT JOIN profiles p ON p.id = h.profile_id
               ORDER BY (h.votes * 1.0 / MAX(h.of, 1)) DESC, h.votes DESC, h.id DESC LIMIT ?""",
            limit,
        )
        out = []
        for r in rows:
            d = dict(r)
            d["shirt"] = json.loads(d.pop("extra") or "{}") or None
            d["date"] = time.strftime("%b %d", time.localtime(d.pop("played_at")))
            d["name"] = d["name"] or "someone"
            out.append(d)
        return out

    def rivals(self, pid: int, limit: int = 4) -> list[dict[str, Any]]:
        """Head-to-head with the friends this profile has met most."""
        rows = self._all(
            """SELECT other, SUM(won) AS won, SUM(lost) AS lost FROM (
                   SELECT loser AS other, count AS won, 0 AS lost FROM duels WHERE winner = ?
                   UNION ALL SELECT winner AS other, 0 AS won, count AS lost FROM duels WHERE loser = ?)
               GROUP BY other ORDER BY won + lost DESC LIMIT ?""",
            pid, pid, limit,
        )
        out = []
        for r in rows:
            other = self._one("SELECT name FROM profiles WHERE id = ?", r["other"])
            if other:
                out.append({"name": other["name"], "won": r["won"], "lost": r["lost"]})
        return out

    def top_rivalries(self, limit: int = 3) -> list[dict[str, Any]]:
        rows = self._all(
            """SELECT MIN(winner, loser) AS a, MAX(winner, loser) AS b,
                      SUM(CASE WHEN winner < loser THEN count ELSE 0 END) AS a_won,
                      SUM(CASE WHEN winner > loser THEN count ELSE 0 END) AS b_won
               FROM duels GROUP BY a, b ORDER BY a_won + b_won DESC LIMIT ?""",
            limit,
        )
        names = {r["id"]: r["name"] for r in self._all("SELECT id, name FROM profiles")}
        return [{"a": names.get(r["a"], "?"), "b": names.get(r["b"], "?"), "a_won": r["a_won"], "b_won": r["b_won"]}
                for r in rows]

    def profile(self, pid: int) -> dict[str, Any] | None:
        p = self._one("SELECT id, name, created FROM profiles WHERE id = ?", pid)
        if p is None:
            return None
        totals = self._one(
            """SELECT COALESCE(SUM(score), 0) AS points, COUNT(*) AS games FROM results WHERE profile_id = ?""", pid)
        wins = self._count("SELECT COUNT(*) FROM games WHERE winner = ?", pid)
        season = self._one(
            """SELECT COALESCE(SUM(r.score), 0) AS points FROM results r JOIN games g ON g.id = r.game_id
               WHERE r.profile_id = ? AND g.season = ?""", pid, season_of(self.clock()))
        badges = [{"badge": r["badge"], "title": BADGES[r["badge"]][0], "about": BADGES[r["badge"]][1]}
                  for r in self._all("SELECT badge FROM badges WHERE profile_id = ? ORDER BY earned_at", pid)
                  if r["badge"] in BADGES]
        best = self._one(
            """SELECT text, votes, of, kind FROM hits WHERE profile_id = ?
               ORDER BY (votes * 1.0 / MAX(of, 1)) DESC, votes DESC LIMIT 1""", pid)
        return {
            "name": p["name"],
            "since": time.strftime("%B %Y", time.localtime(p["created"])),
            "points": totals["points"],
            "games": totals["games"],
            "wins": wins,
            "season_points": season["points"] if season else 0,
            "badges": badges,
            "rivals": self.rivals(pid),
            "best": dict(best) if best else None,
            "pin": self.has_pin(pid),
        }

    def fame(self) -> dict[str, Any]:
        season = season_of(self.clock())
        return {
            "season": season_label(season),
            "board": self.season_board(season),
            "all_time": self.all_time(),
            "champion": self.champion(),
            "champions": self.champions(),
            "hits": self.hits(),
            "rivalries": self.top_rivalries(),
            "games": self._count("SELECT COUNT(*) FROM games"),
        }


def _statements(script: str) -> list[str]:
    return [s.strip() for s in script.split(";") if s.strip()]
