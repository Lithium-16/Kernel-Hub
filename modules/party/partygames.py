"""Party Games: the games themselves, as plain state machines with no networking.

Every game gets the players (ids, in join order), the content, a random generator and the
current time, and is driven by three calls from the room: `handle(pid, msg, now)` for what a
phone sends, `tick(now)` to move on when a timer runs out (or everyone is done), and `skip(now)`
when the VIP or the host hurries things along. `host_view()` is what the big screen shows and
`player_view(pid)` what one phone shows; both are plain JSON-ready dicts, keyed by `phase`.

Times are seconds on a monotonic clock; views carry `ends_in` (seconds left) so the pages can
count down on their own.
"""

from __future__ import annotations

import random
import re
from collections.abc import Iterable
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any

from partydraw import SHIRTS, BadDrawing, check_drawing, doodle

MAX_TEXT = 80
MAX_LIE = 40

# What bots say (test players the host can add). Original, one per line of a Quip Clash answer.
BOT_QUIPS = [
    "A suspiciously large spoon", "Grandma's group chat", "Three raccoons in a coat", "Soup, but angry",
    "A strongly worded email", "My accountant", "Gary", "A goose in a tuxedo", "Interpretive dance",
    "The forbidden lasagna", "A haunted Roomba", "Emotional support cactus", "Wet socks",
    "A motivational pigeon", "Free samples", "The vibes", "Sixteen hot dogs", "A tiny violin",
    "Beep boop", "A very confident toddler", "Expired coupons", "My other personality",
    "A llama in a bow tie", "Unseasoned chicken", "The group project", "A dramatic gasp",
    "Crocs with socks", "An inflatable castle", "Reply all", "The neighbor's wifi",
    "Glitter. Everywhere.", "A sandwich with no bread", "Cheese, but louder", "Hot tub time",
    "A ghost who's bad at haunting", "Pineapple on everything", "Mild panic", "The microwave beep",
    "A trench coat full of snacks", "Error 404: joke not found",
]


class Invalid(Exception):
    """A message a player sent that can't be accepted; the text is shown on their phone. `code`
    tells the page when it should do something besides showing it (pin_needed: ask for a PIN)."""

    def __init__(self, message: str, code: str = "", name: str = "") -> None:
        super().__init__(message)
        self.code = code
        self.name = name  # for pin_needed: the profile's own spelling


def clean(text: Any, limit: int) -> str:
    """One line of player text: control characters out, whitespace collapsed, cut to `limit`."""
    if not isinstance(text, str):
        raise Invalid("Type something first.")
    text = re.sub(r"[\x00-\x1f\x7f]", " ", text)
    return re.sub(r"\s+", " ", text).strip()[:limit]


def norm(text: str) -> str:
    """Loose form for comparing answers: case, punctuation, articles and a plural s don't count."""
    t = re.sub(r"[^a-z0-9 ]", "", text.lower())
    t = re.sub(r"\b(the|a|an)\b", " ", t)
    t = re.sub(r"\s+", " ", t).strip()
    return t[:-1] if len(t) > 3 and t.endswith("s") else t


# -- content ------------------------------------------------------------------------------


@dataclass(frozen=True)
class Fact:
    question: str  # with ___ where the answer goes
    answer: str
    also: tuple[str, ...] = ()  # other spellings of the truth
    decoys: tuple[str, ...] = ()  # house lies: wrong answers for "Lie for me" and small rooms

    def is_truth(self, text: str) -> bool:
        n = norm(text)
        return bool(n) and n in {norm(a) for a in (self.answer, *self.also)}


@dataclass
class Content:
    quips: list[str] = field(default_factory=list)
    facts: list[Fact] = field(default_factory=list)
    doodle_ideas: list[str] = field(default_factory=list)
    slogan_ideas: list[str] = field(default_factory=list)
    drama_themes: list[str] = field(default_factory=list)
    drama_premises: list[str] = field(default_factory=list)

    @staticmethod
    def _lines(paths: Iterable[Path]) -> list[str]:
        out: list[str] = []
        for p in paths:
            try:
                text = p.read_text(encoding="utf-8")
            except OSError:
                continue
            for line in text.splitlines():
                line = line.rstrip()
                if line.strip() and not line.lstrip().startswith("#"):
                    out.append(line)
        return out

    @classmethod
    def load(cls, *folders: Path) -> Content:
        """content/quips.txt, doodle_ideas.txt and slogan_ideas.txt (one a line) and
        content/facts.txt (question, tab, answer, tab, other accepted answers separated by |, tab,
        house lies separated by |)
        from each folder, later folders adding to earlier ones. Duplicates are dropped."""

        def plain(name: str) -> list[str]:
            return list(dict.fromkeys(line.strip() for line in cls._lines(f / name for f in folders)))

        quips = plain("quips.txt")
        facts: list[Fact] = []
        seen: set[str] = set()
        for line in cls._lines(f / "facts.txt" for f in folders):
            parts = [p.strip() for p in line.split("\t")]
            if len(parts) < 2 or "___" not in parts[0] or not parts[1] or parts[0] in seen:
                continue
            seen.add(parts[0])
            also = tuple(a.strip() for a in parts[2].split("|") if a.strip()) if len(parts) > 2 else ()
            fact = Fact(parts[0], parts[1], also)
            decoys = parts[3].split("|") if len(parts) > 3 else []
            decoys = [d.strip()[:MAX_LIE] for d in decoys if d.strip() and not fact.is_truth(d)]
            facts.append(Fact(parts[0], parts[1], also, tuple(dict.fromkeys(decoys))))
        return cls(quips, facts, plain("doodle_ideas.txt"), plain("slogan_ideas.txt"),
                   plain("drama_themes.txt"), plain("drama_premises.txt"))


def deal(pool: list[Any], n: int, rng: random.Random, used: set[Any]) -> list[Any]:
    """n items from pool, preferring ones not used yet this session (then reusing)."""
    fresh = [x for x in pool if x not in used]
    if len(fresh) < n:
        used.clear()
        fresh = list(pool)
    picked = rng.sample(fresh, min(n, len(fresh)))
    used.update(picked)
    return picked


# -- the game base ------------------------------------------------------------------------


class Game:
    key = ""
    title = ""
    min_players = 3
    max_players = 8

    def __init__(
        self,
        pids: list[str],
        content: Content,
        rng: random.Random,
        now: float,
        timer_scale: float = 1.0,
        names: dict[str, str] | None = None,
    ):
        if not self.min_players <= len(pids) <= self.max_players:
            raise Invalid(f"{self.title} needs {self.min_players} to {self.max_players} players.")
        self.pids = list(pids)
        self.names = names or {p: p for p in pids}
        self.active = set(pids)  # connected and not kicked; only they hold up a phase
        self.content = content
        self.rng = rng
        self.scale = max(0.25, float(timer_scale))
        self.scores = {p: 0 for p in pids}
        self.gained: dict[str, int] = {}
        self.phase = ""
        self.deadline: float | None = None
        self.done = False
        self.hits: list[dict[str, Any]] = []  # standout answers, for the greatest hits
        self.duels: list[tuple[str, str, str]] = []  # (winner, loser, kind), for rivalries
        self.feats: list[tuple[str, str]] = []  # (player, badge key), for badges

    # timing

    def _go(self, phase: str, now: float, seconds: float | None, scaled: bool = True) -> None:
        self.phase = phase
        self.deadline = None if seconds is None else now + seconds * (self.scale if scaled else 1)

    def ends_in(self, now: float) -> float | None:
        return None if self.deadline is None else max(0.0, round(self.deadline - now, 1))

    def tick(self, now: float) -> bool:
        """Moves on if the timer ran out or nobody is left to wait for. True if anything changed."""
        if self.done:
            return False
        if (self.deadline is not None and now >= self.deadline) or self.everyone_done():
            self.advance(now)
            return True
        return False

    def skip(self, now: float) -> None:
        if not self.done:
            self.advance(now)

    def set_active(self, pids: set[str]) -> None:
        self.active = set(pids) & set(self.pids)

    def everyone_done(self) -> bool:
        waiting = self.waiting_on()
        return waiting is not None and not (waiting & self.active)

    # for subclasses

    def waiting_on(self) -> set[str] | None:
        """Players who still have to do something this phase; None for phases that only show."""
        return None

    def advance(self, now: float) -> None:
        raise NotImplementedError

    def bot_move(self, pid: str) -> dict[str, Any] | None:
        """What a bot sends next in this phase (a message for handle), or None."""
        return None

    def _bot_line(self) -> str:
        """A bot answer nobody has used yet this game, when there's one left."""
        said = self.__dict__.setdefault("_bot_said", set())
        fresh = [q for q in BOT_QUIPS if q not in said] or BOT_QUIPS
        line = self.rng.choice(fresh)
        said.add(line)
        return line

    def handle(self, pid: str, msg: dict[str, Any], now: float) -> None:
        raise NotImplementedError

    def host_view(self, now: float) -> dict[str, Any]:
        raise NotImplementedError

    def player_view(self, pid: str, now: float) -> dict[str, Any]:
        raise NotImplementedError

    def fill(self, text: str, avoid: Iterable[str] = ()) -> str:
        """Puts a friend's name in a prompt's {player}: someone not in `avoid` when possible."""
        if "{player}" not in text:
            return text
        others = [p for p in self.pids if p not in set(avoid)] or self.pids
        return text.replace("{player}", self.names.get(self.rng.choice(others), "someone"))

    def _award(self, pid: str, points: int) -> None:
        if pid in self.scores and points:
            self.scores[pid] += points
            self.gained[pid] = self.gained.get(pid, 0) + points

    def standings(self) -> list[dict[str, Any]]:
        order = sorted(self.pids, key=lambda p: (-self.scores[p], self.pids.index(p)))
        return [{"pid": p, "score": self.scores[p], "gained": self.gained.get(p, 0)} for p in order]

    def base_view(self, now: float) -> dict[str, Any]:
        return {"game": self.key, "phase": self.phase, "ends_in": self.ends_in(now)}


# -- Quip Clash ---------------------------------------------------------------------------


@dataclass
class Matchup:
    prompt: str
    authors: tuple[str, str]
    answers: dict[str, str] = field(default_factory=dict)
    votes: dict[str, int] = field(default_factory=dict)  # voter -> 0 or 1
    outcome: str = ""  # after scoring: "vote", "jinx", "forfeit" or "empty"
    points: list[int] = field(default_factory=lambda: [0, 0])
    sweep: int | None = None
    winner: int | None = None


class QuipClash(Game):
    """Quiplash rules. Two rounds where every prompt goes to two players and everyone else votes
    on the funnier answer: each matchup shares 1000 points by vote share, the winner gets a 100
    bonus and a clean sweep (every vote, from at least two voters) another 250; round 2 counts
    double. Identical answers are a jinx (nobody scores); an answer against nothing wins by
    default. Then the last round: everyone answers the same prompt and gets three votes for other
    people's answers, 300 points a vote."""

    key = "quip"
    title = "Quip Clash"
    WRITE_S, VOTE_S, REVEAL_S, SCORES_S = 90, 20, 7, 7
    FINAL_WRITE_S, FINAL_VOTE_S, FINAL_REVEAL_S = 75, 30, 10
    POOL, WIN_BONUS, SWEEP_BONUS, FINAL_VOTE_PTS, FINAL_VOTES = 1000, 100, 250, 300, 3

    def __init__(self, pids, content, rng, now, timer_scale=1.0, names=None, used: set[str] | None = None):
        super().__init__(pids, content, rng, now, timer_scale, names)
        if len(content.quips) < 2 * len(pids) + 1:
            raise Invalid("Not enough Quip Clash prompts loaded.")
        self.used = used if used is not None else set()
        self.round = 0
        self.matchups: list[Matchup] = []
        self.current = 0
        self.final: Matchup | None = None  # authors unused; answers from everyone
        self.final_answers: dict[str, str] = {}
        self.final_votes: dict[str, list[str]] = {}  # voter -> the authors they voted for
        self._start_round(now)

    def _start_round(self, now: float) -> None:
        self.round += 1
        self.gained = {}
        order = self.pids[:]
        self.rng.shuffle(order)
        n = len(order)
        prompts = deal(self.content.quips, n, self.rng, self.used)
        pairs = [(order[i], order[(i + 1) % n]) for i in range(n)]
        self.matchups = [Matchup(self.fill(prompts[i], pairs[i]), pairs[i]) for i in range(n)]
        self.current = 0
        self._go("write", now, self.WRITE_S)

    def _start_final(self, now: float) -> None:
        self.round = 3
        self.gained = {}
        self.final = Matchup(self.fill(deal(self.content.quips, 1, self.rng, self.used)[0]), ("", ""))
        self._go("final_write", now, self.FINAL_WRITE_S)

    def mult(self) -> int:
        return min(self.round, 2)

    def my_prompts(self, pid: str) -> list[int]:
        return [i for i, m in enumerate(self.matchups) if pid in m.authors]

    def _todo(self, pid: str) -> int | None:
        return next((i for i in self.my_prompts(pid) if pid not in self.matchups[i].answers), None)

    def voters(self, m: Matchup) -> list[str]:
        return [p for p in self.pids if p not in m.authors]

    def waiting_on(self) -> set[str] | None:
        if self.phase == "write":
            return {p for p in self.pids if self._todo(p) is not None}
        if self.phase == "vote":
            m = self.matchups[self.current]
            return {p for p in self.voters(m) if p not in m.votes}
        if self.phase == "final_write":
            return {p for p in self.pids if p not in self.final_answers}
        if self.phase == "final_vote":
            return {p for p in self.pids if p not in self.final_votes and self._final_choices(p)}
        return None

    def handle(self, pid: str, msg: dict[str, Any], now: float) -> None:
        kind = msg.get("type")
        if self.phase == "write" and kind == "answer":
            i = self._todo(pid)
            if i is None:
                raise Invalid("You've answered both prompts.")
            text = clean(msg.get("text"), MAX_TEXT)
            if not text:
                raise Invalid("Type an answer first, or tap the safety quip.")
            self.matchups[i].answers[pid] = text
        elif self.phase == "vote" and kind == "vote":
            m = self.matchups[self.current]
            if pid in m.authors:
                raise Invalid("You can't vote on your own matchup.")
            if msg.get("choice") not in (0, 1):
                raise Invalid("Pick A or B.")
            m.votes.setdefault(pid, int(msg["choice"]))
        elif self.phase == "final_write" and kind == "answer":
            text = clean(msg.get("text"), MAX_TEXT)
            if not text:
                raise Invalid("Type an answer first.")
            self.final_answers.setdefault(pid, text)
        elif self.phase == "final_vote" and kind == "vote":
            if pid in self.final_votes:
                raise Invalid("Your votes are already in.")
            picks = msg.get("choices")
            allowed = self._final_choices(pid)
            if not isinstance(picks, list) or not picks or len(set(map(str, picks))) != len(picks):
                raise Invalid(f"Pick up to {self.FINAL_VOTES} answers.")
            if len(picks) > min(self.FINAL_VOTES, len(allowed)) or any(p not in allowed for p in picks):
                raise Invalid(f"Pick up to {self.FINAL_VOTES} of the other answers.")
            self.final_votes[pid] = list(picks)
        else:
            raise Invalid("Not now: look at the big screen.")

    def bot_move(self, pid: str) -> dict[str, Any] | None:
        if self.phase == "write" and self._todo(pid) is not None:
            return {"type": "answer", "text": self._bot_line()}
        if self.phase == "vote" and pid not in self.matchups[self.current].authors:
            return {"type": "vote", "choice": self.rng.randrange(2)}
        if self.phase == "final_write":
            return {"type": "answer", "text": self._bot_line()}
        if self.phase == "final_vote" and (allowed := self._final_choices(pid)):
            n = self.rng.randint(1, min(self.FINAL_VOTES, len(allowed)))
            return {"type": "vote", "choices": self.rng.sample(allowed, n)}
        return None

    def _final_choices(self, pid: str) -> list[str]:
        return [p for p in self.pids if p in self.final_answers and p != pid]

    # -- scoring --------------------------------------------------------------------------

    def _prepare(self, m: Matchup) -> str:
        """What happens to a matchup before anyone votes: "vote", or it's decided already."""
        texts = [m.answers.get(a, "") for a in m.authors]
        if not texts[0] and not texts[1]:
            return "empty"
        if not texts[0] or not texts[1]:
            return "forfeit"
        if norm(texts[0]) == norm(texts[1]):
            return "jinx"
        return "vote"

    def _score_matchup(self, m: Matchup, outcome: str) -> None:
        mult = self.mult()
        m.outcome = outcome
        if outcome == "jinx" or outcome == "empty":
            return
        if outcome == "forfeit":
            k = 0 if m.answers.get(m.authors[0]) else 1
            m.winner = k
            m.points[k] = (self.POOL + self.WIN_BONUS) * mult
        else:
            votes = [sum(1 for v in m.votes.values() if v == k) for k in (0, 1)]
            total = sum(votes)
            for k in (0, 1):
                m.points[k] = round(self.POOL * mult * votes[k] / total / 10) * 10 if total else 0
            if votes[0] != votes[1]:
                k = 0 if votes[0] > votes[1] else 1
                m.winner = k
                m.points[k] += self.WIN_BONUS * mult
                if votes[1 - k] == 0 and total >= 2:
                    m.sweep = k
                    m.points[k] += self.SWEEP_BONUS * mult
                    self.feats.append((m.authors[k], "clean_sweep"))
                if total >= 2 and votes[k] / total >= 0.75:
                    self.hits.append({"kind": "quip", "prompt": m.prompt, "text": m.answers[m.authors[k]],
                                      "pid": m.authors[k], "votes": votes[k], "of": total})
        for k, author in enumerate(m.authors):
            self._award(author, m.points[k])
        if m.winner is not None:
            self.duels.append((m.authors[m.winner], m.authors[1 - m.winner], "quip"))

    def _open_matchup(self, now: float) -> None:
        """Voting for the current matchup, unless it's already decided (then straight to the
        reveal), or skipped when neither player answered."""
        while self.current < len(self.matchups):
            m = self.matchups[self.current]
            outcome = self._prepare(m)
            if outcome == "vote":
                self._go("vote", now, self.VOTE_S)
                return
            if outcome != "empty":
                self._score_matchup(m, outcome)
                self._go("reveal", now, self.REVEAL_S, scaled=False)
                return
            m.outcome = "empty"
            self.current += 1
        self._go("scores", now, self.SCORES_S, scaled=False)

    def advance(self, now: float) -> None:
        if self.phase == "write":
            self.current = 0
            self._open_matchup(now)
        elif self.phase == "vote":
            self._score_matchup(self.matchups[self.current], "vote")
            self._go("reveal", now, self.REVEAL_S, scaled=False)
        elif self.phase == "reveal":
            self.current += 1
            self._open_matchup(now)
        elif self.phase == "scores":
            if self.round == 1:
                self._start_round(now)
            elif self.round == 2:
                self._start_final(now)
            else:
                self.phase, self.deadline, self.done = "over", None, True
        elif self.phase == "final_write":
            self._go("final_vote", now, self.FINAL_VOTE_S)
        elif self.phase == "final_vote":
            counts = self._final_counts()
            for author, n in counts.items():
                self._award(author, n * self.FINAL_VOTE_PTS)
            if counts:
                top = max(counts.values())
                best = [a for a, n in counts.items() if n == top]
                if len(best) == 1:
                    self.feats.append((best[0], "crowd_favorite"))
                    voters = len(self.final_votes)
                    if top >= 2 and top / max(1, voters) >= 0.5:
                        self.hits.append({"kind": "quip", "prompt": self.final.prompt if self.final else "",
                                          "text": self.final_answers[best[0]], "pid": best[0],
                                          "votes": top, "of": voters})
            self._go("final_reveal", now, self.FINAL_REVEAL_S, scaled=False)
        elif self.phase == "final_reveal":
            self._go("scores", now, self.SCORES_S, scaled=False)

    def _final_counts(self) -> dict[str, int]:
        counts: dict[str, int] = {}
        for picks in self.final_votes.values():
            for author in picks:
                counts[author] = counts.get(author, 0) + 1
        return counts

    # -- views ----------------------------------------------------------------------------

    def _matchup_view(self, m: Matchup, reveal: bool) -> dict[str, Any]:
        view: dict[str, Any] = {
            "prompt": m.prompt,
            "answers": [m.answers.get(a, "") for a in m.authors],
            "number": self.current + 1,
            "of": len(self.matchups),
        }
        if reveal:
            votes = [[v for v, k in m.votes.items() if k == side] for side in (0, 1)]
            view.update(authors=list(m.authors), voters=votes, points=list(m.points), sweep=m.sweep,
                        winner=m.winner, outcome=m.outcome)
        return view

    def _final_view(self, reveal: bool) -> dict[str, Any]:
        order = [p for p in self.pids if p in self.final_answers]
        view: dict[str, Any] = {
            "prompt": self.final.prompt if self.final else "",
            "answers": [{"id": p, "text": self.final_answers[p]} for p in order],
            "votes_each": self.FINAL_VOTES,
        }
        if reveal:
            for a in view["answers"]:
                a["by"] = a["id"]
                a["voters"] = [v for v, picks in self.final_votes.items() if a["id"] in picks]
                a["points"] = len(a["voters"]) * self.FINAL_VOTE_PTS
            view["answers"].sort(key=lambda a: -len(a["voters"]))
        return view

    def host_view(self, now: float) -> dict[str, Any]:
        v = self.base_view(now) | {"round": self.round, "mult": self.mult()}
        if self.phase in ("write", "vote", "final_write", "final_vote"):
            v["waiting"] = sorted(self.waiting_on() or set())
        if self.phase in ("vote", "reveal"):
            v["matchup"] = self._matchup_view(self.matchups[self.current], self.phase == "reveal")
        elif self.phase in ("final_vote", "final_reveal"):
            v["final"] = self._final_view(self.phase == "final_reveal")
        elif self.phase == "final_write":
            v["final"] = {"prompt": self.final.prompt if self.final else ""}
        elif self.phase in ("scores", "over"):
            v["standings"] = self.standings()
        return v

    def player_view(self, pid: str, now: float) -> dict[str, Any]:
        v = self.base_view(now) | {"round": self.round}
        if self.phase == "write":
            i = self._todo(pid)
            mine = self.my_prompts(pid)
            if i is None:
                v["todo"] = None
            else:
                v["todo"] = {"prompt": self.matchups[i].prompt, "number": mine.index(i) + 1, "of": len(mine)}
        elif self.phase == "vote":
            m = self.matchups[self.current]
            v["matchup"] = {"prompt": m.prompt, "answers": [m.answers.get(a, "") for a in m.authors]}
            v["can_vote"] = pid not in m.authors
            v["voted"] = m.votes.get(pid)
        elif self.phase == "reveal":
            m = self.matchups[self.current]
            v["number"] = self.current + 1
            v["mine"] = pid in m.authors
            v["gain"] = m.points[m.authors.index(pid)] if pid in m.authors else 0
            v["sweep_by"] = m.authors[m.sweep] if m.sweep is not None else None
            v["outcome"] = m.outcome
            v["won"] = m.winner is not None and m.authors[m.winner] == pid
        elif self.phase == "final_write":
            v["prompt"] = self.final.prompt if self.final else ""
            v["answered"] = pid in self.final_answers
        elif self.phase == "final_vote":
            v["prompt"] = self.final.prompt if self.final else ""
            v["choices"] = [{"id": p, "text": self.final_answers[p]} for p in self._final_choices(pid)]
            v["votes_each"] = self.FINAL_VOTES
            v["voted"] = self.final_votes.get(pid)
        elif self.phase == "final_reveal":
            v["gain"] = self._final_counts().get(pid, 0) * self.FINAL_VOTE_PTS
        elif self.phase in ("scores", "over"):
            v["standings"] = self.standings()
        return v


# -- Bluff Buffet -------------------------------------------------------------------------


@dataclass
class Option:
    text: str
    authors: list[str]
    truth: bool = False
    house: bool = False  # a lie the game added, not a player's


class BluffRound:
    """Write a fake answer, then find the real one among everyone's fakes. Shared by Bluff Buffet
    (and later games that work the same way). Small rooms get house lies so there are always a
    few to choose from, and stuck players can take one of the house lies as their own."""

    MIN_OPTIONS = 5
    SUGGEST = 2

    def __init__(self, truth: str, is_truth, decoys: Iterable[str] = (), pids: Iterable[str] = (), rng: random.Random | None = None) -> None:
        self.truth = truth
        self.is_truth = is_truth
        self.decoys = [d for d in dict.fromkeys(decoys) if not is_truth(d)]
        self.lies: dict[str, str] = {}
        self.options: list[Option] = []
        self.picks: dict[str, int] = {}
        self.likes: dict[int, set[str]] = {}
        # "Lie for me": a couple of house lies per player, spread so friends get different ones
        rng = rng or random.Random()
        pool = list(self.decoys)
        rng.shuffle(pool)
        self.suggestions: dict[str, list[str]] = {}
        for n, pid in enumerate(pids):
            if pool:
                k = n * self.SUGGEST
                self.suggestions[pid] = [pool[(k + j) % len(pool)] for j in range(min(self.SUGGEST, len(pool)))]

    def lie(self, pid: str, text: str) -> None:
        if not text:
            raise Invalid("Write a lie first.")
        if self.is_truth(text):
            raise Invalid("That's the real answer! Write a lie instead.")
        self.lies.setdefault(pid, text)

    def build(self, rng: random.Random, auto: Iterable[str] = ()) -> None:
        """Merge the lies (same lie, shared authors), give a house lie to each player in auto who
        didn't write one, then add house lies until there are MIN_OPTIONS to choose from."""
        for pid in auto:
            if pid not in self.lies and self.suggestions.get(pid):
                self.lies[pid] = self.suggestions[pid][0]
        merged: dict[str, Option] = {}
        for pid, text in self.lies.items():
            key = norm(text) or text.lower()
            if key in merged:
                merged[key].authors.append(pid)
            else:
                merged[key] = Option(text, [pid])
        spare = [d for d in self.decoys if (norm(d) or d.lower()) not in merged]
        rng.shuffle(spare)
        while len(merged) + 1 < self.MIN_OPTIONS and spare:
            d = spare.pop()
            merged[norm(d) or d.lower()] = Option(d, [], house=True)
        self.options = list(merged.values()) + [Option(self.truth, [], truth=True)]
        rng.shuffle(self.options)

    def can_pick(self, pid: str) -> list[int]:
        return [i for i, o in enumerate(self.options) if pid not in o.authors]

    def pick(self, pid: str, index: Any) -> None:
        if not isinstance(index, int) or isinstance(index, bool) or index not in self.can_pick(pid):
            raise Invalid("You can't pick that one.")
        self.picks.setdefault(pid, index)

    def like(self, pid: str, index: Any) -> None:
        """Toggle a like on a lie you enjoyed (not your own, not the truth)."""
        if not isinstance(index, int) or isinstance(index, bool) or not 0 <= index < len(self.options):
            raise Invalid("You can't like that one.")
        if pid in self.options[index].authors:
            raise Invalid("No liking your own lie!")
        fans = self.likes.setdefault(index, set())
        fans.symmetric_difference_update({pid})

    def pickers(self, i: int) -> list[str]:
        return [p for p, k in self.picks.items() if k == i]

    def score(self, truth_points: int, fool_points: int) -> dict[str, int]:
        out: dict[str, int] = {}
        for i, o in enumerate(self.options):
            got = self.pickers(i)
            if o.truth:
                for p in got:
                    out[p] = out.get(p, 0) + truth_points
            else:
                for a in o.authors:
                    out[a] = out.get(a, 0) + fool_points * len([p for p in got if p not in o.authors])
        return out

    def reveal(self) -> list[dict[str, Any]]:
        """Lies first (least fooled first), the truth last. Lies nobody picked aren't shown."""
        rows = [
            {"text": o.text, "truth": o.truth, "house": o.house, "authors": o.authors,
             "pickers": self.pickers(i), "likes": len(self.likes.get(i, ()))}
            for i, o in enumerate(self.options)
        ]
        lies = sorted((r for r in rows if not r["truth"] and r["pickers"]), key=lambda r: len(r["pickers"]))
        return lies + [r for r in rows if r["truth"]]


class BluffBuffet(Game):
    """Weird true facts with a blank. Everyone writes a believable lie, then tries to pick the
    truth: +1000 for finding it, +500 for every friend your lie fools. Points grow as the game
    goes on: x1 for the opening questions, x1.5 in the middle and x2 for the final question."""

    key = "bluff"
    title = "Bluff Buffet"
    min_players = 2
    LIE_S, PICK_S, SCORES_S = 60, 25, 6

    def __init__(self, pids, content, rng, now, timer_scale=1.0, names=None, questions: int = 5, used: set[Fact] | None = None):
        super().__init__(pids, content, rng, now, timer_scale, names)
        if not content.facts:
            raise Invalid("No Bluff Buffet facts loaded.")
        self.used = used if used is not None else set()
        self.facts = deal(content.facts, max(1, questions), rng, self.used)
        self.number = 0
        self.found: dict[str, int] = {}  # questions each player got right
        self._ask(now)

    def _ask(self, now: float) -> None:
        self.fact = self.facts[self.number]
        self.bluff = BluffRound(self.fact.answer, self.fact.is_truth, self.fact.decoys, self.pids, self.rng)
        self._go("lie", now, self.LIE_S)

    def mult(self) -> float:
        n, i = len(self.facts), self.number
        if n > 1 and i == n - 1:
            return 2
        if i > 0 and i >= (n - 1) // 2:
            return 1.5
        return 1

    def waiting_on(self) -> set[str] | None:
        if self.phase == "lie":
            return {p for p in self.pids if p not in self.bluff.lies}
        if self.phase == "pick":
            return {p for p in self.pids if p not in self.bluff.picks and self.bluff.can_pick(p)}
        return None

    def handle(self, pid: str, msg: dict[str, Any], now: float) -> None:
        kind = msg.get("type")
        if self.phase == "lie" and kind == "lie":
            if pid in self.bluff.lies:
                raise Invalid("Your lie is already in.")
            self.bluff.lie(pid, clean(msg.get("text"), MAX_LIE))
        elif self.phase == "pick" and kind == "pick":
            self.bluff.pick(pid, msg.get("choice"))
        elif self.phase == "pick" and kind == "like":
            self.bluff.like(pid, msg.get("choice"))
        else:
            raise Invalid("Not now: look at the big screen.")

    def bot_move(self, pid: str) -> dict[str, Any] | None:
        if self.phase == "lie":
            ideas = self.bluff.suggestions.get(pid) or [self._bot_line()]
            return {"type": "lie", "text": self.rng.choice(ideas)}
        if self.phase == "pick" and (allowed := self.bluff.can_pick(pid)):
            truth = next((i for i in allowed if self.bluff.options[i].truth), None)
            if truth is not None and self.rng.random() < 0.35:
                return {"type": "pick", "choice": truth}
            return {"type": "pick", "choice": self.rng.choice(allowed)}
        return None

    def advance(self, now: float) -> None:
        if self.phase == "lie":
            self.bluff.build(self.rng, auto=self.pids)
            self._go("pick", now, self.PICK_S)
        elif self.phase == "pick":
            self.gained = {}
            m = self.mult()
            for pid, pts in self.bluff.score(int(1000 * m), int(500 * m)).items():
                self._award(pid, pts)
            for row in self.bluff.reveal():
                fooled = [p for p in row["pickers"] if p not in row["authors"]]
                if row["truth"]:
                    for p in row["pickers"]:
                        self.found[p] = self.found.get(p, 0) + 1
                    continue
                for author in row["authors"]:
                    self.duels += [(author, p, "fooled") for p in fooled]
                    others = [p for p in self.pids if p not in row["authors"]]
                    if len(fooled) >= 2 and len(fooled) == len(others):
                        self.feats.append((author, "master_fibber"))
            mine = [r for r in self.bluff.reveal() if not r["truth"] and r["authors"]]
            best = max(mine, key=lambda r: (len(r["pickers"]), r["likes"]), default=None)
            if best and len(best["pickers"]) >= 2:
                self.hits.append({"kind": "lie", "prompt": self.fact.question, "text": best["text"],
                                  "pid": best["authors"][0], "votes": len(best["pickers"]), "of": len(self.bluff.picks)})
            self._go("reveal", now, 4 + 2.5 * len(self.bluff.reveal()), scaled=False)
        elif self.phase == "reveal":
            self._go("scores", now, self.SCORES_S, scaled=False)
        elif self.phase == "scores":
            if self.number + 1 < len(self.facts):
                self.number += 1
                self._ask(now)
            else:
                if len(self.facts) >= 3:
                    self.feats += [(p, "truth_seeker") for p, n in self.found.items() if n == len(self.facts)]
                self.phase, self.deadline, self.done = "over", None, True

    def _question(self) -> dict[str, Any]:
        last = len(self.facts) > 1 and self.number == len(self.facts) - 1
        return {"question": self.fact.question, "number": self.number + 1, "of": len(self.facts),
                "mult": self.mult(), "final": last}

    def host_view(self, now: float) -> dict[str, Any]:
        v = self.base_view(now) | self._question()
        if self.phase in ("lie", "pick"):
            v["waiting"] = sorted(self.waiting_on() or set())
        if self.phase == "pick":
            v["options"] = [o.text for o in self.bluff.options]
        elif self.phase == "reveal":
            v["reveal"] = self.bluff.reveal()
            v["answer"] = self.fact.answer
        elif self.phase in ("scores", "over"):
            v["standings"] = self.standings()
        return v

    def player_view(self, pid: str, now: float) -> dict[str, Any]:
        v = self.base_view(now) | self._question()
        if self.phase == "lie":
            v["lied"] = self.bluff.lies.get(pid)
            v["suggestions"] = self.bluff.suggestions.get(pid, [])
        elif self.phase == "pick":
            allowed = set(self.bluff.can_pick(pid))
            v["options"] = [
                {"text": o.text, "mine": i not in allowed, "liked": pid in self.bluff.likes.get(i, ())}
                for i, o in enumerate(self.bluff.options)
            ]
            v["picked"] = self.bluff.picks.get(pid)
        elif self.phase == "reveal":
            picked = self.bluff.picks.get(pid)
            v["answer"] = self.fact.answer
            v["found"] = picked is not None and self.bluff.options[picked].truth
            v["picked"] = self.bluff.options[picked].text if picked is not None else None
            v["house"] = picked is not None and self.bluff.options[picked].house
            mine = next((i for i, o in enumerate(self.bluff.options) if pid in o.authors), None)
            v["my_lie"] = self.bluff.options[mine].text if mine is not None else None
            v["fooled"] = [p for p in self.bluff.pickers(mine) if p != pid] if mine is not None else []
            v["likes"] = len(self.bluff.likes.get(mine, ())) if mine is not None else 0
            v["gain"] = self.gained.get(pid, 0)
        elif self.phase in ("scores", "over"):
            v["standings"] = self.standings()
        return v


# -- Shirt Showdown -----------------------------------------------------------------------


@dataclass
class Shirt:
    maker: str
    color: int
    drawing: dict[str, Any] | None  # {"id", "by", "strokes"}
    slogan: dict[str, Any] | None  # {"id", "by", "text"}

    def helpers(self) -> set[str]:
        """Everyone who made part of it."""
        return {self.maker} | {x["by"] for x in (self.drawing, self.slogan) if x}

    def view(self, credits: bool) -> dict[str, Any]:
        v: dict[str, Any] = {
            "id": self.maker,
            "color": self.color,
            "strokes": self.drawing["strokes"] if self.drawing else [],
            "slogan": self.slogan["text"] if self.slogan else "",
        }
        if credits:
            v |= {"maker": self.maker, "artist": self.drawing["by"] if self.drawing else None,
                  "writer": self.slogan["by"] if self.slogan else None}
        return v


class ShirtShowdown(Game):
    """Everyone draws designs and writes slogans into a shared pool, then makes a shirt from a
    hand of other people's pieces. Shirts battle king-of-the-hill style: each one challenges the
    champion and everyone votes; votes score for both the artist and the slogan writer, and a
    champion earns a bonus for every challenge it survives. Two rounds, then the round winners
    face off in the final."""

    key = "shirt"
    title = "Shirt Showdown"
    DRAW_S, SLOGAN_S, AGAIN_S, ASSEMBLE_S = 90, 60, 45, 60
    VOTE_S, REVEAL_S, ROUND_S, FINAL_VOTE_S, FINAL_REVEAL_S, SCORES_S = 15, 6, 8, 20, 8, 7
    MAX_DRAWINGS, MAX_SLOGANS, HAND = 6, 8, 3
    VOTE_PTS, DEFEND_PTS, ROUND_WIN_PTS, FINAL_WIN_PTS = 100, 200, 500, 1000
    MAX_SLOGAN = 40

    def __init__(self, pids, content, rng, now, timer_scale=1.0, names=None, rounds: int = 2, used=None):
        super().__init__(pids, content, rng, now, timer_scale, names)
        self.rounds = max(1, min(int(rounds), 3))
        self.round = 0
        self.next_id = 0
        self.drawings: list[dict[str, Any]] = []
        self.slogans: list[dict[str, Any]] = []
        self.spent: set[int] = set()  # pool items already on a shirt
        self.ideas = {p: i for i, p in enumerate(pids)}  # where each player is in the idea lists
        self.winners: list[Shirt] = []
        self._start_round(now)

    # -- phases ---------------------------------------------------------------------------

    def _start_round(self, now: float) -> None:
        self.round += 1
        self.ready: set[str] = set()
        self.hands: dict[str, dict[str, Any]] = {}
        self.shirts: dict[str, Shirt] = {}
        self.order: list[str] = []
        self.champion: str | None = None
        self.challenger: str | None = None
        self.streak = 0
        self.votes: dict[str, str] = {}
        self.pair: tuple[Shirt, Shirt] | None = None
        self.last: dict[str, Any] | None = None
        self._go("draw", now, self.DRAW_S if self.round == 1 else self.AGAIN_S)

    def _deal(self, pid: str, items: list[dict[str, Any]], avoid: set[int]) -> list[int]:
        """HAND items for pid: unused ones by other people first, then anything unused."""
        free = [x for x in items if x["id"] not in self.spent and x["id"] not in avoid]
        dealt = {i for h in self.hands.values() for k in ("drawings", "slogans") for i in h.get(k, [])}
        tiers = [
            [x for x in free if x["by"] != pid and x["id"] not in dealt],
            [x for x in free if x["by"] != pid and x["id"] in dealt],
            [x for x in free if x["by"] == pid],
        ]
        out: list[int] = []
        for tier in tiers:
            self.rng.shuffle(tier)
            out += [x["id"] for x in tier[: self.HAND - len(out)]]
        return out

    def _start_assemble(self, now: float) -> None:
        order = self.pids[:]
        self.rng.shuffle(order)
        for pid in order:
            self.hands[pid] = {"drawings": [], "slogans": [], "rerolled": []}
            self.hands[pid]["drawings"] = self._deal(pid, self.drawings, set())
            self.hands[pid]["slogans"] = self._deal(pid, self.slogans, set())
        self._go("assemble", now, self.ASSEMBLE_S)

    def _item(self, items: list[dict[str, Any]], item_id: Any) -> dict[str, Any] | None:
        return next((x for x in items if x["id"] == item_id), None)

    def _make_shirt(self, pid: str, drawing: Any, slogan: Any, color: Any) -> None:
        hand = self.hands[pid]
        d = self._item(self.drawings, drawing) if drawing in hand["drawings"] else None
        sl = self._item(self.slogans, slogan) if slogan in hand["slogans"] else None
        if (drawing is not None and d is None) or (slogan is not None and sl is None):
            raise Invalid("Pick a drawing and a slogan from your hand.")
        if (d and d["id"] in self.spent) or (sl and sl["id"] in self.spent):
            raise Invalid("Someone just used that one. Pick another.")
        if d is None and sl is None:
            raise Invalid("Pick a drawing and a slogan.")
        if not isinstance(color, int) or isinstance(color, bool) or not 0 <= color < SHIRTS:
            color = 0
        self.shirts[pid] = Shirt(pid, color, d, sl)
        self.spent |= {x["id"] for x in (d, sl) if x}

    def _auto_shirts(self) -> None:
        """Players who didn't finish get a shirt from their hand anyway."""
        for pid in self.pids:
            if pid in self.shirts:
                continue
            hand = self.hands.get(pid) or {"drawings": [], "slogans": []}
            ds = [i for i in hand["drawings"] if i not in self.spent]
            ss = [i for i in hand["slogans"] if i not in self.spent]
            if ds or ss:
                self._make_shirt(pid, ds[0] if ds else None, ss[0] if ss else None, self.rng.randrange(SHIRTS))

    def _start_showdown(self, now: float) -> None:
        self._auto_shirts()
        self.order = list(self.shirts)
        self.rng.shuffle(self.order)
        if not self.order:
            self._go("round_over", now, self.ROUND_S, scaled=False)
            return
        self.champion = self.order[0]
        self._next_challenge(now)

    def _next_challenge(self, now: float) -> None:
        idx = self.order.index(self.challenger) + 1 if self.challenger else 1
        if idx >= len(self.order):
            champ = self.shirts[self.champion] if self.champion else None
            self.gained = {}
            if champ:
                for p in {x["by"] for x in (champ.drawing, champ.slogan) if x}:
                    self._award(p, self.ROUND_WIN_PTS)
                self.winners.append(champ)
                self.hits.append(self._hit(champ, self.streak))
            self._go("round_over", now, self.ROUND_S, scaled=False)
            return
        self.challenger = self.order[idx]
        assert self.champion is not None
        self.pair = (self.shirts[self.champion], self.shirts[self.challenger])
        self.votes = {}
        self._go("vote", now, self.VOTE_S)

    def _pair(self) -> tuple[Shirt, Shirt]:
        """The two shirts facing off (fixed at the start of the vote, so the reveal still
        shows them after the champion changes)."""
        assert self.pair is not None
        return self.pair

    def can_vote_for(self, pid: str) -> list[str]:
        a, b = self._pair()
        return [s.maker for s in (a, b) if pid not in s.helpers()]

    def _score_votes(self, a: Shirt, b: Shirt) -> tuple[int, int]:
        counts = [sum(1 for v in self.votes.values() if v == s.maker) for s in (a, b)]
        for s, n in zip((a, b), counts):
            for part in (s.drawing, s.slogan):
                if part:
                    self._award(part["by"], n * self.VOTE_PTS)
        return counts[0], counts[1]

    def _hit(self, s: Shirt, streak: int) -> dict[str, Any]:
        return {"kind": "shirt", "text": s.slogan["text"] if s.slogan else "", "pid": s.maker,
                "votes": streak, "of": len(self.order), "shirt": s.view(True)}

    def advance(self, now: float) -> None:
        ph = self.phase
        if ph == "draw":
            self.ready = set()
            self._go("slogan", now, self.SLOGAN_S if self.round == 1 else self.AGAIN_S)
        elif ph == "slogan":
            self.ready = set()
            self._start_assemble(now)
        elif ph == "assemble":
            self._start_showdown(now)
        elif ph == "vote":
            self.gained = {}
            champ, chall = self._pair()
            a, b = self._score_votes(champ, chall)
            defended = a >= b
            if defended:
                self.streak += 1
                for part in (champ.drawing, champ.slogan):
                    if part:
                        self._award(part["by"], self.DEFEND_PTS)
                        if self.streak == 3:
                            self.feats.append((part["by"], "unbeatable_tee"))
            if a != b:
                win, lose = (champ, chall) if a > b else (chall, champ)
                self.duels.append((win.maker, lose.maker, "shirt"))
            self.last = {"votes": [a, b], "voters": [[v for v, m in self.votes.items() if m == s.maker] for s in (champ, chall)],
                         "winner": 0 if defended else 1, "streak": self.streak}
            if not defended:
                self.champion, self.streak = chall.maker, 0
            self._go("reveal", now, self.REVEAL_S, scaled=False)
        elif ph == "reveal":
            self._next_challenge(now)
        elif ph == "round_over":
            if self.round < self.rounds:
                self._start_round(now)
            elif len(self.winners) >= 2 and self.winners[0] is not self.winners[1]:
                self.winners = self.winners[-2:]
                self.pair = (self.winners[0], self.winners[1])
                self.votes = {}
                self._go("final_vote", now, self.FINAL_VOTE_S)
            else:
                self._go("scores", now, self.SCORES_S, scaled=False)
        elif ph == "final_vote":
            self.gained = {}
            a, b = self.winners
            na, nb = self._score_votes(a, b)
            win = a if na >= nb else b
            for part in (win.drawing, win.slogan):
                if part:
                    self._award(part["by"], self.FINAL_WIN_PTS)
                    self.feats.append((part["by"], "fashion_icon"))
            self.last = {"votes": [na, nb], "voters": [[v for v, m in self.votes.items() if m == s.maker] for s in (a, b)],
                         "winner": 0 if win is a else 1}
            self.hits.insert(0, self._hit(win, 0))
            self._go("final_reveal", now, self.FINAL_REVEAL_S, scaled=False)
        elif ph == "final_reveal":
            self._go("scores", now, self.SCORES_S, scaled=False)
        elif ph == "scores":
            self.phase, self.deadline, self.done = "over", None, True

    def waiting_on(self) -> set[str] | None:
        if self.phase in ("draw", "slogan"):
            return set(self.pids) - self.ready
        if self.phase == "assemble":
            return {p for p in self.pids if p not in self.shirts and self.hands.get(p, {}).get("drawings", []) + self.hands.get(p, {}).get("slogans", [])}
        if self.phase in ("vote", "final_vote"):
            return {p for p in self.pids if p not in self.votes and self.can_vote_for(p)}
        return None

    # -- messages -------------------------------------------------------------------------

    def _new_id(self) -> int:
        self.next_id += 1
        return self.next_id

    def handle(self, pid: str, msg: dict[str, Any], now: float) -> None:
        kind = msg.get("type")
        ph = self.phase
        if ph == "draw" and kind == "drawing":
            mine = [d for d in self.drawings if d["by"] == pid and d["round"] == self.round]
            if len(mine) >= self.MAX_DRAWINGS:
                raise Invalid("That's plenty of drawings for this round!")
            try:
                strokes = check_drawing(msg.get("strokes"))
            except BadDrawing as e:
                raise Invalid(str(e)) from None
            self.drawings.append({"id": self._new_id(), "by": pid, "strokes": strokes, "round": self.round})
            self.ideas[pid] += 1
        elif ph == "slogan" and kind == "slogan":
            mine = [x for x in self.slogans if x["by"] == pid and x["round"] == self.round]
            if len(mine) >= self.MAX_SLOGANS:
                raise Invalid("That's plenty of slogans for this round!")
            text = clean(msg.get("text"), self.MAX_SLOGAN)
            if not text:
                raise Invalid("Write a slogan first.")
            if any(norm(x["text"]) == norm(text) for x in self.slogans):
                raise Invalid("Someone already wrote that one.")
            self.slogans.append({"id": self._new_id(), "by": pid, "text": text, "round": self.round})
            self.ideas[pid] += 1
        elif ph in ("draw", "slogan") and kind == "done":
            self.ready.add(pid)
        elif ph == "assemble" and kind == "reroll":
            what = msg.get("what")
            hand = self.hands.get(pid)
            if what not in ("drawings", "slogans") or hand is None or pid in self.shirts:
                raise Invalid("Not now.")
            if what in hand["rerolled"]:
                raise Invalid("You already rerolled those.")
            fresh = self._deal(pid, self.drawings if what == "drawings" else self.slogans, set(hand[what]))
            if fresh:
                hand[what] = fresh
            hand["rerolled"].append(what)
        elif ph == "assemble" and kind == "shirt":
            if pid in self.shirts:
                raise Invalid("Your shirt is already made.")
            if pid not in self.hands:
                raise Invalid("Not now.")
            self._make_shirt(pid, msg.get("drawing"), msg.get("slogan"), msg.get("color"))
        elif ph in ("vote", "final_vote") and kind == "vote":
            choice = msg.get("choice")
            if choice not in self.can_vote_for(pid):
                raise Invalid("You can't vote for a shirt you helped make.")
            self.votes.setdefault(pid, choice)
        else:
            raise Invalid("Not now: look at the big screen.")

    def bot_move(self, pid: str) -> dict[str, Any] | None:
        ph = self.phase
        if ph == "draw":
            mine = sum(1 for d in self.drawings if d["by"] == pid and d["round"] == self.round)
            if mine < self.__dict__.setdefault("_bot_goal", {}).setdefault((pid, ph, self.round), self.rng.randint(1, 2)):
                return {"type": "drawing", "strokes": doodle(self.rng)}
            return {"type": "done"}
        if ph == "slogan":
            mine = sum(1 for x in self.slogans if x["by"] == pid and x["round"] == self.round)
            if mine < 2:
                ideas = BOT_QUIPS
                taken = {norm(x["text"]) for x in self.slogans}
                fresh = [t[: self.MAX_SLOGAN] for t in ideas if norm(t[: self.MAX_SLOGAN]) not in taken]
                if fresh:
                    return {"type": "slogan", "text": self.rng.choice(fresh)}
            return {"type": "done"}
        if ph == "assemble" and pid not in self.shirts and (hand := self.hands.get(pid)):
            ds = [i for i in hand["drawings"] if i not in self.spent]
            ss = [i for i in hand["slogans"] if i not in self.spent]
            if ds or ss:
                return {"type": "shirt", "drawing": self.rng.choice(ds) if ds else None,
                        "slogan": self.rng.choice(ss) if ss else None, "color": self.rng.randrange(SHIRTS)}
        if ph in ("vote", "final_vote") and (allowed := self.can_vote_for(pid)):
            return {"type": "vote", "choice": self.rng.choice(allowed)}
        return None

    # -- views ----------------------------------------------------------------------------

    def _idea(self, pid: str, ideas: list[str]) -> str:
        return ideas[(self.ideas[pid] * 7 + self.round) % len(ideas)] if ideas else ""

    def _counts(self) -> dict[str, int]:
        if self.phase == "draw":
            items = self.drawings
        elif self.phase == "slogan":
            items = self.slogans
        else:
            return {}
        return {p: sum(1 for x in items if x["by"] == p and x["round"] == self.round) for p in self.pids}

    def _battle(self, credits: bool) -> dict[str, Any]:
        a, b = self._pair()
        v: dict[str, Any] = {"shirts": [a.view(credits), b.view(credits)]}
        if not self.phase.startswith("final"):
            v |= {"number": self.order.index(self.challenger) if self.challenger else 0,
                  "of": len(self.order) - 1, "streak": self.streak}
        if credits and self.last:
            v |= self.last
        return v

    def host_view(self, now: float) -> dict[str, Any]:
        v = self.base_view(now) | {"round": self.round, "rounds": self.rounds}
        ph = self.phase
        if ph in ("draw", "slogan", "assemble", "vote", "final_vote"):
            v["waiting"] = sorted(self.waiting_on() or set())
        if ph in ("draw", "slogan"):
            v["counts"] = self._counts()
        elif ph in ("vote", "final_vote"):
            v["battle"] = self._battle(False)
        elif ph in ("reveal", "final_reveal"):
            v["battle"] = self._battle(True)
        elif ph == "round_over":
            champ = self.winners[-1] if self.winners and len(self.winners) == self.round else None
            v["winner"] = champ.view(True) if champ else None
            v["streak"] = self.streak
            v["standings"] = self.standings()
        elif ph in ("scores", "over"):
            v["standings"] = self.standings()
        return v

    def player_view(self, pid: str, now: float) -> dict[str, Any]:
        v = self.base_view(now) | {"round": self.round}
        ph = self.phase
        if ph == "draw":
            v |= {"idea": self._idea(pid, self.content.doodle_ideas), "count": self._counts().get(pid, 0),
                  "max": self.MAX_DRAWINGS, "done": pid in self.ready}
        elif ph == "slogan":
            v |= {"idea": self._idea(pid, self.content.slogan_ideas), "done": pid in self.ready, "max": self.MAX_SLOGANS,
                  "mine": [x["text"] for x in self.slogans if x["by"] == pid and x["round"] == self.round]}
        elif ph == "assemble":
            hand = self.hands.get(pid, {"drawings": [], "slogans": [], "rerolled": []})
            v["made"] = pid in self.shirts
            v["drawings"] = [{"id": i, "strokes": d["strokes"]} for i in hand["drawings"]
                             if (d := self._item(self.drawings, i)) and i not in self.spent]
            v["slogans"] = [{"id": i, "text": x["text"]} for i in hand["slogans"]
                            if (x := self._item(self.slogans, i)) and i not in self.spent]
            v["rerolls"] = [w for w in ("drawings", "slogans") if w not in hand["rerolled"]]
        elif ph in ("vote", "final_vote"):
            a, b = self._pair()
            v["shirts"] = [a.view(False), b.view(False)]
            v["can_vote"] = self.can_vote_for(pid)
            v["voted"] = self.votes.get(pid)
        elif ph in ("reveal", "final_reveal", "round_over"):
            v["gain"] = self.gained.get(pid, 0)
            v["battle"] = [x.maker for x in self.pair] if self.pair and ph != "round_over" else []
            champ = self.winners[-1] if ph == "round_over" and len(self.winners) == self.round else None
            parts = {x["by"] for x in (champ.drawing, champ.slogan) if x} if champ else set()
            v["mine"] = pid in parts  # drew or wrote the winning shirt: gets the bonus
            v["made"] = bool(champ and champ.maker == pid)  # put it together
        elif ph in ("scores", "over"):
            v["standings"] = self.standings()
        return v


# -- Drama Club ----------------------------------------------------------------------------

EMOTIONS = ("neutral", "flustered", "sad", "angry")
# The preset visual-novel backgrounds (web/bg pictures and web/scenes.js drawings); each chapter's writer picks one.
PHOTO_BACKGROUNDS = ("classroom_day", "school_hallway", "bedroom_day", "livingroom_night", "kitchen_day",
                     "restaurant", "city_afternoon", "spring_street", "train_beach", "onsen")  # web/bg/*.webp
BACKGROUNDS = PHOTO_BACKGROUNDS + ("classroom", "rooftop", "cafe", "bedroom", "park", "beach", "street", "train",
                                   "festival", "castle", "spaceship", "haunted")
NARRATOR = 2  # a line's speaker: 0 and 1 are the chapter's characters, 2 the narrator
MAX_LINE, MAX_NAME_C, MAX_LOOK, MAX_PERSONALITY, MAX_THEME, MAX_PROBLEM, MAX_HEADLINE = 80, 18, 60, 50, 50, 70, 80
FALLBACK_NAMES = ["Mystery Guest", "The New Kid", "Someone Shady", "A Stranger", "The Understudy",
                  "Background Extra", "Plot Device", "Extra #4"]  # each fits MAX_NAME_C
LOST_CHAPTER = ["This chapter was lost in a tragic coffee accident.", "Nothing happened. For hours.",
                "Meanwhile, somewhere else, something very important happened offscreen."]
LOST_HEADLINE = ["Something happens. Nobody is sure what.", "Everyone has a snack and things calm down.",
                 "Things get weird."]
STEPS = {"pitch": 1, "pitch_vote": 1, "create": 2, "draw": 3, "headline": 4, "write": 4, "show": 5, "credits": 5,
         "vote": 5, "scores": 5, "over": 5}
STEP_NAMES = ["Pitch", "Create", "Draw", "Write", "Show"]


@dataclass
class Character:
    creator: str
    artist: str = ""
    name: str = ""
    look: str = ""
    personality: str = ""
    faces: dict[str, list[dict[str, Any]]] = field(default_factory=dict)

    def card(self) -> dict[str, Any]:
        """What players may see before the show: the description, never the art."""
        return {"pid": self.creator, "name": self.name, "look": self.look, "personality": self.personality}

    def full(self) -> dict[str, Any]:
        return self.card() | {"artist": self.artist, "faces": self.faces}


@dataclass
class Chapter:
    writer: str
    headline: str = ""  # one sentence: what happens in it, written before the chapter itself
    bg: str = ""
    cast: list[str] = field(default_factory=list)  # the characters in it (by creator), 1 or 2
    lines: list[dict[str, Any]] = field(default_factory=list)


class DramaClub(Game):
    """One visual novel, written by the whole room. Everyone pitches a theme and the story's
    problem, and the room picks one. Everyone invents a character in words, then draws someone
    else's character in four moods. Then the outline is written as a relay: one headline per
    chapter, in order, each writer seeing the story so far. Then everyone writes their chapter
    at the same time with the whole outline in view, so the plot builds from start to end. The big screen plays the chapters
    in order as one story, and only then does anyone see the drawings. Everyone votes for the
    best chapter and the best drawing."""

    key = "drama"
    title = "Drama Club"
    PITCH_S, PITCH_VOTE_S, CREATE_S, DRAW_S, RELAY_S, WRITE_S, VOTE_S, SCORES_S, CREDITS_S = 50, 20, 60, 240, 25, 210, 30, 8, 10
    MAX_LINES = 6
    CHAPTER_PTS, ARTIST_PTS, CREATOR_PTS, THEME_PTS = 600, 400, 200, 250

    def __init__(self, pids, content, rng, now, timer_scale=1.0, names=None, used=None):
        super().__init__(pids, content, rng, now, timer_scale, names)
        self.order = self.pids[:]
        rng.shuffle(self.order)
        n = len(self.order)
        # player i draws the character player i+1 invents: nobody draws their own
        self.characters: dict[str, Character] = {
            p: Character(p, artist=self.order[(i - 1) % n]) for i, p in enumerate(self.order)}
        self.chapters = [Chapter(p) for p in self.order]
        self.used = used if used is not None else set()
        self.themes: dict[str, str] = {}
        self.problems: dict[str, str] = {}  # each pitch's problem: what the story is about solving
        self.theme_votes: dict[str, str] = {}
        self.theme = ""
        self.theme_by: str | None = None
        self.problem = ""
        self.relay = 0  # the chapter whose headline is being written
        self.showing = 0
        self.chapter_votes: dict[str, int] = {}
        self.drawing_votes: dict[str, str] = {}
        self._go("pitch", now, self.PITCH_S)

    # -- who does what -------------------------------------------------------------------------

    def drawing_for(self, pid: str) -> Character:
        """The character this player draws."""
        return next(c for c in self.characters.values() if c.artist == pid)

    def chapter_of(self, pid: str) -> int:
        return next(i for i, ch in enumerate(self.chapters) if ch.writer == pid)

    def part(self, i: int) -> str:
        """Where a chapter falls: only so the writer knows whether to start, continue or finish."""
        return "beginning" if i == 0 else "ending" if i == len(self.chapters) - 1 else "middle"

    # -- phases ----------------------------------------------------------------------------

    def waiting_on(self) -> set[str] | None:
        ph = self.phase
        if ph == "pitch":
            return {p for p in self.pids if p not in self.themes}
        if ph == "pitch_vote":
            return {p for p in self.pids if p not in self.theme_votes and self._theme_choices(p)}
        if ph == "create":
            return {p for p, c in self.characters.items() if not c.name}
        if ph == "draw":
            return {c.artist for c in self.characters.values() if set(EMOTIONS) - set(c.faces)}
        if ph == "headline":
            ch = self.chapters[self.relay]
            return set() if ch.headline else {ch.writer}
        if ph == "write":
            return {ch.writer for ch in self.chapters if not ch.lines}
        if ph == "vote":
            return {p for p in self.pids if (p not in self.chapter_votes and self._chapter_choices(p))
                    or (p not in self.drawing_votes and self._drawing_choices(p))}
        return None

    def _theme_choices(self, pid: str) -> list[str]:
        return [p for p in self.pids if p in self.themes and p != pid]

    def advance(self, now: float) -> None:
        ph = self.phase
        if ph == "pitch":
            for p in self.pids:  # a blank pitch gets an idea from the box
                if p not in self.themes and self.content.drama_themes and self.rng.random() < 0.5:
                    self.themes[p] = self._idea(self.content.drama_themes)
                    self.problems[p] = self._idea(self.content.drama_premises)[:MAX_PROBLEM]
            if len(self.themes) >= 2:
                self._go("pitch_vote", now, self.PITCH_VOTE_S)
            else:
                self._pick_theme()
                self._go("create", now, self.CREATE_S)
        elif ph == "pitch_vote":
            self._pick_theme()
            self._go("create", now, self.CREATE_S)
        elif ph == "create":
            self._fill_names()
            self._go("draw", now, self.DRAW_S)
        elif ph == "draw":
            self._fill_faces()
            self.relay = 0
            self._go("headline", now, self.RELAY_S)
        elif ph == "headline":  # the relay: one headline at a time, in chapter order
            ch = self.chapters[self.relay]
            ch.headline = ch.headline or self.rng.choice(LOST_HEADLINE)
            if self.relay + 1 < len(self.chapters):
                self.relay += 1
                self._go("headline", now, self.RELAY_S)
            else:
                self._go("write", now, self.WRITE_S)
        elif ph == "write":
            for i, ch in enumerate(self.chapters):
                if not ch.lines:
                    self._fill_chapter(ch, i)
            self.showing = 0
            self._go("show", now, self._show_s(), scaled=False)
        elif ph == "show":
            if self.showing + 1 < len(self.chapters):
                self.showing += 1
                self._go("show", now, self._show_s(), scaled=False)
            else:
                self._go("credits", now, self.CREDITS_S, scaled=False)
        elif ph == "credits":
            self._go("vote", now, self.VOTE_S)
        elif ph == "vote":
            self._score_votes()
            self._go("scores", now, self.SCORES_S, scaled=False)
        elif ph == "scores":
            self.phase, self.deadline, self.done = "over", None, True

    def _idea(self, ideas: list[str]) -> str:
        return self.fill(self.rng.choice(ideas)) if ideas else ""

    def _pick_theme(self) -> None:
        counts: dict[str, int] = {}
        for author in self.theme_votes.values():
            counts[author] = counts.get(author, 0) + 1
        if self.themes:
            best = max(counts.values(), default=0)
            top = [p for p in self.themes if counts.get(p, 0) == best]
            self.theme_by = self.rng.choice(top)
            self.theme = self.themes[self.theme_by]
            self.problem = self.problems.get(self.theme_by, "")
            if counts.get(self.theme_by):
                self._award(self.theme_by, self.THEME_PTS)
        else:
            self.theme = self._idea(self.content.drama_themes) or "A very dramatic afternoon"
            self.theme_by = None
        self.problem = self.problem or self._idea(self.content.drama_premises)[:MAX_PROBLEM] or "Nobody knows what's going on."

    def _fill_names(self) -> None:
        names = [n for n in FALLBACK_NAMES if n not in {c.name for c in self.characters.values()}]
        for c in self.characters.values():
            if not c.name:
                c.name = names.pop(0) if names else "Mystery Guest"

    def _fill_faces(self) -> None:
        for c in self.characters.values():
            base = c.faces.get("neutral") or next(iter(c.faces.values()), None) or doodle(self.rng, sprite=True)
            for e in EMOTIONS:
                c.faces.setdefault(e, base)

    def _fill_chapter(self, ch: Chapter, i: int) -> None:
        ch.bg = ch.bg or self.rng.choice(BACKGROUNDS)
        ch.cast = ch.cast or self.rng.sample(list(self.characters), min(2, len(self.characters)))
        ch.lines = [{"who": NARRATOR, "emotion": "neutral", "text": self.rng.choice(LOST_CHAPTER)}]

    def _show_s(self) -> float:
        ch = self.chapters[self.showing]
        title = 6.5 if self.showing == 0 else 3.5  # the first chapter opens with the novel's title card
        return title + 0.7 + sum(self.line_s(x["text"]) for x in ch.lines) + 1.5

    @staticmethod
    def line_s(text: str) -> float:
        """How long the big screen shows a line: time to type it out and to read it (the host
        page uses the same formula)."""
        return 1.6 + 0.045 * len(text)

    # -- messages ----------------------------------------------------------------------------

    def _lines(self, raw: Any, speakers: int) -> list[dict[str, Any]]:
        if not isinstance(raw, list) or not raw:
            raise Invalid("Write at least one line.")
        if len(raw) > self.MAX_LINES:
            raise Invalid(f"At most {self.MAX_LINES} lines.")
        out = []
        for x in raw:
            if not isinstance(x, dict):
                raise Invalid("That chapter didn't come through. Try again.")
            who, emotion, text = x.get("who"), x.get("emotion"), clean(x.get("text"), MAX_LINE)
            if isinstance(who, bool) or who not in (*range(speakers), NARRATOR) or emotion not in EMOTIONS:
                raise Invalid("Pick who says each line and how they feel.")
            if not text:
                raise Invalid("A line is empty: write something or remove it.")
            out.append({"who": who, "emotion": emotion, "text": text})
        return out

    def handle(self, pid: str, msg: dict[str, Any], now: float) -> None:
        kind, ph = msg.get("type"), self.phase
        if ph == "pitch" and kind == "theme":
            text = clean(msg.get("text"), MAX_THEME)
            if not text:
                raise Invalid("Write a theme first.")
            if pid not in self.themes:
                self.themes[pid] = text
                self.problems[pid] = clean(msg.get("problem") or "", MAX_PROBLEM)
        elif ph == "pitch_vote" and kind == "vote":
            if msg.get("choice") not in self._theme_choices(pid):
                raise Invalid("Vote for someone else's theme.")
            self.theme_votes.setdefault(pid, msg["choice"])
        elif ph == "create" and kind == "character":
            c = self.characters[pid]
            name = clean(msg.get("name"), MAX_NAME_C)
            if not name:
                raise Invalid("Give your character a name.")
            if any(o.name.lower() == name.lower() for o in self.characters.values() if o is not c):
                raise Invalid("Someone already took that name. Pick another.")
            c.name = name
            c.look = clean(msg.get("look") or "", MAX_LOOK)
            c.personality = clean(msg.get("personality") or "", MAX_PERSONALITY)
        elif ph == "draw" and kind == "face":
            emotion = msg.get("emotion")
            if emotion not in EMOTIONS:
                raise Invalid("Pick a mood to draw.")
            try:
                self.drawing_for(pid).faces[emotion] = check_drawing(msg.get("strokes"), sprite=True)
            except BadDrawing as e:
                raise Invalid(str(e)) from None
        elif ph == "headline" and kind == "headline":
            text = clean(msg.get("text"), MAX_HEADLINE)
            if not text:
                raise Invalid("Write what happens in your chapter, in one sentence.")
            ch = self.chapters[self.relay]
            if ch.writer != pid or ch.headline:
                raise Invalid("Not your turn yet: watch the outline grow on the big screen.")
            ch.headline = text
        elif ph == "write" and kind == "chapter":
            ch = self.chapters[self.chapter_of(pid)]
            bg, cast = msg.get("bg"), msg.get("cast")
            if bg not in BACKGROUNDS:
                raise Invalid("Pick a background.")
            if (not isinstance(cast, list) or not 1 <= len(cast) <= 2 or len(set(cast)) != len(cast)
                    or any(c not in self.characters for c in cast)):
                raise Invalid("Pick one or two characters for your chapter.")
            ch.lines = self._lines(msg.get("lines"), len(cast))
            ch.bg, ch.cast = bg, list(cast)
        elif ph == "vote" and kind == "vote":
            chapter, drawing = msg.get("chapter"), msg.get("drawing")
            if chapter is not None:
                if chapter not in self._chapter_choices(pid):
                    raise Invalid("Vote for a chapter you didn't write.")
                self.chapter_votes[pid] = chapter
            if drawing is not None:
                if drawing not in self._drawing_choices(pid):
                    raise Invalid("Vote for a drawing you didn't draw or invent.")
                self.drawing_votes[pid] = drawing
        else:
            raise Invalid("Not now: look at the big screen.")

    def _chapter_choices(self, pid: str) -> list[int]:
        return [i for i, ch in enumerate(self.chapters) if ch.writer != pid]

    def _drawing_choices(self, pid: str) -> list[str]:
        return [p for p, c in self.characters.items() if pid not in (c.creator, c.artist)]

    def _score_votes(self) -> None:
        self.gained = {}
        counts = [0] * len(self.chapters)
        for i in self.chapter_votes.values():
            counts[i] += 1
        for ch, n in zip(self.chapters, counts):
            self._award(ch.writer, self.CHAPTER_PTS * n)
        drawings: dict[str, int] = {}
        for p in self.drawing_votes.values():
            drawings[p] = drawings.get(p, 0) + 1
        for p, n in drawings.items():
            c = self.characters[p]
            self._award(c.artist, self.ARTIST_PTS * n)
            self._award(c.creator, self.CREATOR_PTS * n)
        self.counts = {"chapters": counts, "drawings": drawings}
        voters = len(self.chapter_votes)
        if counts and max(counts) > 0:
            best = max(range(len(counts)), key=lambda i: counts[i])
            ch = self.chapters[best]
            if max(counts) >= 2 and counts.count(max(counts)) == 1:
                self.feats.append((ch.writer, "plot_twist"))
            first = next((x["text"] for x in ch.lines if x["who"] != NARRATOR), ch.lines[0]["text"])
            self.hits.append({"kind": "scene", "prompt": self.theme, "text": first,
                              "pid": ch.writer, "votes": counts[best], "of": voters,
                              "scene": {"bg": ch.bg, "names": [self.characters[a].name for a in ch.cast],
                                        "faces": [self.characters[a].faces.get("neutral", []) for a in ch.cast],
                                        "line": first}})
        if drawings and max(drawings.values()) >= 2:
            top = max(drawings, key=lambda p: drawings[p])
            if list(drawings.values()).count(drawings[top]) == 1:
                self.feats.append((self.characters[top].artist, "leading_role"))
        ranked = sorted(range(len(counts)), key=lambda i: -counts[i])
        if len(ranked) >= 2 and counts[ranked[0]] > counts[ranked[1]]:
            self.duels.append((self.chapters[ranked[0]].writer, self.chapters[ranked[1]].writer, "drama"))

    # -- bots --------------------------------------------------------------------------------

    def bot_move(self, pid: str) -> dict[str, Any] | None:
        ph = self.phase
        if ph == "pitch":
            return {"type": "theme", "text": self._idea(self.content.drama_themes) or self._bot_line(),
                    "problem": self._idea(self.content.drama_premises)[:MAX_PROBLEM]}
        if ph == "pitch_vote" and (ch := self._theme_choices(pid)):
            return {"type": "vote", "choice": self.rng.choice(ch)}
        if ph == "create" and not self.characters[pid].name:
            names = [n for n in FALLBACK_NAMES if n not in {x.name for x in self.characters.values()}]
            return {"type": "character", "name": self.rng.choice(names or ["Extra " + pid[-2:]]),
                    "look": self._bot_line()[:MAX_LOOK], "personality": self._bot_line()[:MAX_PERSONALITY]}
        if ph == "draw":
            missing = [e for e in EMOTIONS if e not in self.drawing_for(pid).faces]
            if missing:
                return {"type": "face", "emotion": missing[0], "strokes": doodle(self.rng, sprite=True)}
            return None
        if ph == "headline" and (ch := self.chapters[self.relay]).writer == pid and not ch.headline:
            return {"type": "headline", "text": self._bot_line()[:MAX_HEADLINE]}
        if ph == "write" and not self.chapters[self.chapter_of(pid)].lines:
            cast = self.rng.sample(list(self.characters), min(2, len(self.characters)))
            lines = [{"who": NARRATOR, "emotion": "neutral",
                      "text": (self._idea(self.content.drama_premises) or self._bot_line())[:MAX_LINE]}]
            lines += [{"who": i % len(cast), "emotion": self.rng.choice(EMOTIONS), "text": self._bot_line()[:MAX_LINE]}
                      for i in range(self.rng.randint(2, 4))]
            return {"type": "chapter", "bg": self.rng.choice(BACKGROUNDS), "cast": cast, "lines": lines}
        if ph == "vote":
            out: dict[str, Any] = {"type": "vote"}
            if pid not in self.chapter_votes and (cc := self._chapter_choices(pid)):
                out["chapter"] = self.rng.choice(cc)
            if pid not in self.drawing_votes and (dc := self._drawing_choices(pid)):
                out["drawing"] = self.rng.choice(dc)
            return out if len(out) > 1 else None
        return None

    # -- views -------------------------------------------------------------------------------

    def _chapter_full(self, i: int) -> dict[str, Any]:
        ch = self.chapters[i]
        return {"index": i, "of": len(self.chapters), "part": self.part(i), "bg": ch.bg, "headline": ch.headline,
                "cast": [self.characters[a].full() for a in ch.cast], "lines": ch.lines, "writer": ch.writer}

    def _outline(self, upto: int) -> list[dict[str, Any]]:
        """The headlines of the first `upto` chapters, in order: the story so far."""
        return [{"number": i + 1, "part": self.part(i), "headline": ch.headline, "writer": ch.writer}
                for i, ch in enumerate(self.chapters[:upto])]

    def _common(self, now: float) -> dict[str, Any]:
        step = STEPS.get(self.phase, 1)
        return self.base_view(now) | {"theme": self.theme, "problem": self.problem, "emotions": list(EMOTIONS),
                                      "step": step, "steps": STEP_NAMES}

    def host_view(self, now: float) -> dict[str, Any]:
        v = self._common(now)
        ph = self.phase
        if ph in ("pitch", "pitch_vote", "create", "draw", "headline", "write", "vote"):
            v["waiting"] = sorted(self.waiting_on() or set())
        if ph == "pitch_vote":
            v["themes"] = [{"pid": p, "text": t, "problem": self.problems.get(p, "")} for p, t in self.themes.items()]
        elif ph == "headline":
            v["outline"] = self._outline(self.relay)
            v["relay"] = {"number": self.relay + 1, "of": len(self.chapters), "part": self.part(self.relay),
                          "writer": self.chapters[self.relay].writer}
        elif ph == "write":
            v["cast"] = [c.card() for c in self.characters.values()]  # names only: still no art
        elif ph == "show":
            v["chapter"] = self._chapter_full(self.showing)
        elif ph == "credits":
            v["credits"] = {"characters": [{"name": c.name, "creator": c.creator, "artist": c.artist,
                                            "face": c.faces.get("neutral", [])} for c in self.characters.values()],
                            "writers": [ch.writer for ch in self.chapters]}
        elif ph == "vote":
            v["gallery"] = [{"pid": p, "name": c.name, "artist": c.artist, "face": c.faces.get("flustered", [])}
                            for p, c in self.characters.items()]
        elif ph in ("scores", "over"):
            v["standings"] = self.standings()
            v["results"] = getattr(self, "counts", None)
        return v

    def player_view(self, pid: str, now: float) -> dict[str, Any]:
        v = self._common(now)
        ph = self.phase
        if ph == "pitch":
            v["mine"] = self.themes.get(pid)
            ideas = self.content.drama_themes
            v["idea"] = ideas[(self.pids.index(pid) * 7) % len(ideas)] if ideas else ""
        elif ph == "pitch_vote":
            v["themes"] = [{"pid": p, "text": self.themes[p], "problem": self.problems.get(p, "")}
                           for p in self._theme_choices(pid)]
            v["voted"] = self.theme_votes.get(pid)
        elif ph == "create":
            v["character"] = self.characters[pid].card()
        elif ph == "draw":
            c = self.drawing_for(pid)
            v["drawing"] = c.card() | {"faces": c.faces}  # only the art you're making
        elif ph == "headline":
            i = self.chapter_of(pid)
            turn = "done" if i < self.relay or self.chapters[i].headline else "now" if i == self.relay else "waiting"
            v["chapter"] = {"number": i + 1, "of": len(self.chapters), "part": self.part(i),
                            "turn": turn, "turns_left": max(0, i - self.relay)}
            v["outline"] = self._outline(self.relay + (1 if self.chapters[self.relay].headline else 0))
            v["writer"] = self.chapters[self.relay].writer
            v["cast"] = [c.card() for c in self.characters.values()]
        elif ph == "write":
            i = self.chapter_of(pid)
            chs = self.chapters
            v["chapter"] = {"number": i + 1, "of": len(chs), "part": self.part(i), "done": bool(chs[i].lines),
                            "outline": self._outline(len(chs))}
            v["cast"] = [c.card() for c in self.characters.values()]
            v["backgrounds"] = list(BACKGROUNDS)
            v["max_lines"] = self.MAX_LINES
        elif ph == "show":
            v["number"], v["of"] = self.showing + 1, len(self.chapters)
            v["yours"] = self.chapters[self.showing].writer == pid
        elif ph == "vote":
            v["chapters"] = [{"index": i, "number": i + 1, "bg": self.chapters[i].bg,
                              "first": self.chapters[i].lines[0]["text"] if self.chapters[i].lines else "",
                              "cast": [self.characters[a].name for a in self.chapters[i].cast]}
                             for i in self._chapter_choices(pid)]
            v["drawings"] = [{"pid": p, "name": self.characters[p].name,
                              "face": self.characters[p].faces.get("flustered", [])}
                             for p in self._drawing_choices(pid)]
            v["voted"] = {"chapter": self.chapter_votes.get(pid), "drawing": self.drawing_votes.get(pid)}
        elif ph in ("scores", "over"):
            v["standings"] = self.standings()
        return v


GAMES: dict[str, type[Game]] = {g.key: g for g in (QuipClash, BluffBuffet, ShirtShowdown, DramaClub)}
