"""PetLink's SQLite store: users and tokens, the peek toggle, queued mail and uploaded files."""

import hashlib
import json
import secrets
import sqlite3
import time
from collections.abc import Callable
from dataclasses import dataclass
from pathlib import Path

FILE_TTL_S = 7 * 86400
MAIL_TTL_S = 30 * 86400
CODE_TTL_S = 7 * 86400

SCHEMA = """
CREATE TABLE IF NOT EXISTS users (
  id TEXT PRIMARY KEY, name TEXT NOT NULL, bow_hue INTEGER NOT NULL,
  token_hash TEXT NOT NULL UNIQUE, peek INTEGER NOT NULL DEFAULT 0);
CREATE TABLE IF NOT EXISTS mail (
  id TEXT PRIMARY KEY, sender TEXT NOT NULL, recipient TEXT NOT NULL,
  message TEXT NOT NULL, created REAL NOT NULL, delivered INTEGER NOT NULL DEFAULT 0);
CREATE TABLE IF NOT EXISTS files (
  id TEXT PRIMARY KEY, owner TEXT NOT NULL, recipient TEXT NOT NULL, name TEXT NOT NULL,
  size INTEGER NOT NULL, path TEXT NOT NULL, created REAL NOT NULL);
CREATE TABLE IF NOT EXISTS install_codes (
  code_hash TEXT PRIMARY KEY, user_id TEXT NOT NULL, expires REAL NOT NULL);
"""


def _hash(token: str) -> str:
    return hashlib.sha256(token.encode()).hexdigest()


@dataclass(frozen=True)
class User:
    id: str
    name: str
    bow_hue: int
    peek: bool


@dataclass(frozen=True)
class FileRow:
    id: str
    owner: str
    to: str
    name: str
    size: int
    path: Path
    created: float


class Store:
    def __init__(self, path: Path, clock: Callable[[], float] = time.time) -> None:
        path.parent.mkdir(parents=True, exist_ok=True)
        self.db = sqlite3.connect(path, isolation_level=None, check_same_thread=False)
        self.db.executescript(SCHEMA)
        self.clock = clock

    # -- users ----------------------------------------------------------------------------

    @staticmethod
    def _user(row: tuple | None) -> User | None:
        return User(row[0], row[1], row[2], bool(row[3])) if row else None

    def add_user(self, name: str, bow_hue: int) -> tuple[User, str]:
        token = secrets.token_urlsafe(32)
        user = User("u_" + secrets.token_hex(4), name, bow_hue % 360, False)
        self.db.execute(
            "INSERT INTO users (id, name, bow_hue, token_hash) VALUES (?, ?, ?, ?)",
            (user.id, user.name, user.bow_hue, _hash(token)),
        )
        return user, token

    def remove_user(self, user_id: str) -> bool:
        self.db.execute("DELETE FROM install_codes WHERE user_id = ?", (user_id,))
        return self.db.execute("DELETE FROM users WHERE id = ?", (user_id,)).rowcount > 0

    def users(self) -> list[User]:
        rows = self.db.execute("SELECT id, name, bow_hue, peek FROM users ORDER BY name").fetchall()
        return [User(r[0], r[1], r[2], bool(r[3])) for r in rows]

    def user(self, user_id: str) -> User | None:
        return self._user(self.db.execute("SELECT id, name, bow_hue, peek FROM users WHERE id = ?", (user_id,)).fetchone())

    def auth(self, token: str) -> User | None:
        if not token:
            return None
        row = self.db.execute("SELECT id, name, bow_hue, peek FROM users WHERE token_hash = ?", (_hash(token),)).fetchone()
        return self._user(row)

    def rotate_token(self, user_id: str) -> str:
        """New token; the old one stops working and so do the user's unused install codes."""
        token = secrets.token_urlsafe(32)
        self.db.execute("UPDATE users SET token_hash = ? WHERE id = ?", (_hash(token), user_id))
        self.db.execute("DELETE FROM install_codes WHERE user_id = ?", (user_id,))
        return token

    # -- install codes ---------------------------------------------------------------------

    def add_install_code(self, user_id: str) -> str:
        code = secrets.token_urlsafe(24)
        self.db.execute("INSERT INTO install_codes VALUES (?, ?, ?)", (_hash(code), user_id, self.clock() + CODE_TTL_S))
        return code

    def take_install_code(self, code: str) -> str | None:
        """The user id behind a live code, without using it up."""
        row = self.db.execute("SELECT user_id, expires FROM install_codes WHERE code_hash = ?", (_hash(code),)).fetchone()
        return row[0] if row and row[1] > self.clock() else None

    def redeem_install_code(self, code: str) -> tuple[str, str] | None:
        """Use the code up and mint the user's new token in one transaction: (user_id, token)."""
        self.db.execute("BEGIN IMMEDIATE")
        try:
            user_id = self.take_install_code(code)
            if user_id is None or self.db.execute("DELETE FROM install_codes WHERE code_hash = ?", (_hash(code),)).rowcount == 0:
                self.db.execute("ROLLBACK")
                return None
            token = self.rotate_token(user_id)  # also drops the user's other codes
            self.db.execute("COMMIT")
            return user_id, token
        except BaseException:
            self.db.execute("ROLLBACK")
            raise

    def pending_codes(self) -> dict[str, int]:
        rows = self.db.execute(
            "SELECT user_id, COUNT(*) FROM install_codes WHERE expires > ? GROUP BY user_id", (self.clock(),)
        ).fetchall()
        return dict(rows)

    def set_peek(self, user_id: str, on: bool) -> None:
        self.db.execute("UPDATE users SET peek = ? WHERE id = ?", (int(on), user_id))

    # -- mail -----------------------------------------------------------------------------

    def queue_mail(self, mail_id: str, sender: str, to: str, message: dict) -> bool:
        cur = self.db.execute(
            "INSERT OR IGNORE INTO mail (id, sender, recipient, message, created) VALUES (?, ?, ?, ?, ?)",
            (mail_id, sender, to, json.dumps(message), self.clock()),
        )
        return cur.rowcount > 0

    def pending_mail(self, to: str) -> list[dict]:
        rows = self.db.execute(
            "SELECT message FROM mail WHERE recipient = ? AND delivered = 0 ORDER BY created", (to,)
        ).fetchall()
        return [json.loads(r[0]) for r in rows]

    def mail_delivered(self, mail_id: str) -> None:
        # The row stays (until it expires) so a client re-sending the same mail_id after a
        # restart doesn't get it delivered twice.
        self.db.execute("UPDATE mail SET delivered = 1, message = '{}' WHERE id = ?", (mail_id,))

    def mail_state(self, mail_id: str) -> str | None:
        row = self.db.execute("SELECT delivered FROM mail WHERE id = ?", (mail_id,)).fetchone()
        return None if row is None else ("delivered" if row[0] else "queued")

    # -- files ----------------------------------------------------------------------------

    def add_file(self, file_id: str, owner: str, to: str, name: str, size: int, path: Path) -> None:
        self.db.execute(
            "INSERT INTO files (id, owner, recipient, name, size, path, created) VALUES (?, ?, ?, ?, ?, ?, ?)",
            (file_id, owner, to, name, size, str(path), self.clock()),
        )

    def file(self, file_id: str) -> FileRow | None:
        r = self.db.execute(
            "SELECT id, owner, recipient, name, size, path, created FROM files WHERE id = ?", (file_id,)
        ).fetchone()
        return FileRow(r[0], r[1], r[2], r[3], r[4], Path(r[5]), r[6]) if r else None

    def drop_file(self, file_id: str) -> None:
        self.db.execute("DELETE FROM files WHERE id = ?", (file_id,))

    def files_size(self) -> int:
        return int(self.db.execute("SELECT COALESCE(SUM(size), 0) FROM files").fetchone()[0])

    def expire(self, now: float) -> list[Path]:
        old = self.db.execute("SELECT id, path FROM files WHERE created < ?", (now - FILE_TTL_S,)).fetchall()
        self.db.execute("DELETE FROM files WHERE created < ?", (now - FILE_TTL_S,))
        self.db.execute("DELETE FROM mail WHERE created < ?", (now - MAIL_TTL_S,))
        self.db.execute("DELETE FROM install_codes WHERE expires <= ?", (now,))
        return [Path(p) for _, p in old]
