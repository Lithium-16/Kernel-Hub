import json
import random
from pathlib import Path

import pytest

from partydraw import BadDrawing, check_drawing
from partygames import BACKGROUNDS, EMOTIONS, NARRATOR, Content, DramaClub, Invalid

HERE = Path(__file__).resolve().parent.parent
LINE = [{"c": 0, "w": 3, "p": [10, 10, 200, 200]}]


def content():
    return Content.load(HERE / "content")


def drama(n, seed=None):
    pids = [f"p{i}" for i in range(n)]
    return pids, DramaClub(pids, content(), random.Random(n if seed is None else seed), 0.0)


def play_to(g, phase):
    """Every player does their job until the game reaches `phase`."""
    for _ in range(60):
        if g.phase == phase:
            return
        for p in g.pids:
            msg = g.bot_move(p)
            while msg is not None:
                g.handle(p, msg, 1.0)
                msg = g.bot_move(p) if g.phase == "draw" else None
        g.tick(1.0)
        while g.phase in ("show", "credits") and phase not in ("show", "credits"):
            g.skip(1.0)
    raise AssertionError(f"never reached {phase}, stuck in {g.phase}")


def chapter(cast, lines=None, bg="cafe"):
    return {"type": "chapter", "bg": bg, "cast": cast,
            "lines": lines or [{"who": 0, "emotion": "angry", "text": "Who ate my cake?"}]}


def test_sprites_cant_have_a_fill():
    assert check_drawing(LINE, sprite=True) == LINE
    with pytest.raises(BadDrawing, match="background fill"):
        check_drawing([{"fill": 2}] + LINE, sprite=True)


def test_theme_pitch_and_vote():
    pids, g = drama(4)
    assert g.phase == "pitch" and g.player_view("p0", 1.0)["idea"]
    assert g.player_view("p0", 1.0)["step"] == 1
    for p in pids:
        g.handle(p, {"type": "theme", "text": f"theme {p}"}, 1.0)
    g.tick(1.0)
    assert g.phase == "pitch_vote"
    with pytest.raises(Invalid, match="someone else"):
        g.handle("p0", {"type": "vote", "choice": "p0"}, 1.0)
    for p in ("p0", "p2", "p3"):
        g.handle(p, {"type": "vote", "choice": "p1"}, 1.0)
    g.handle("p1", {"type": "vote", "choice": "p0"}, 1.0)
    g.tick(1.0)
    assert g.theme == "theme p1" and g.phase == "create" and g.scores["p1"] == DramaClub.THEME_PTS


@pytest.mark.parametrize("n", range(3, 9))
def test_everyone_draws_someone_elses_character(n):
    pids, g = drama(n)
    artists = [c.artist for c in g.characters.values()]
    assert sorted(artists) == sorted(pids)  # one character each to draw
    assert all(c.artist != c.creator for c in g.characters.values())
    assert sorted(ch.writer for ch in g.chapters) == sorted(pids)  # one chapter each
    assert [g.part(i) for i in range(n)] == ["beginning"] + ["middle"] * (n - 2) + ["ending"]


def test_create_then_draw_the_card_you_were_dealt():
    pids, g = drama(4)
    play_to(g, "create")
    assert g.player_view("p0", 1.0)["step"] == 2
    g.handle("p0", {"type": "character", "name": "Vlad", "look": "tall, cape, flour on his face",
                    "personality": "nervous about everything"}, 1.0)
    with pytest.raises(Invalid, match="already took"):
        g.handle("p1", {"type": "character", "name": "vlad"}, 1.0)
    with pytest.raises(Invalid, match="name"):
        g.handle("p1", {"type": "character", "name": "  "}, 1.0)
    g.tick(100.0)  # time's up: the rest get stand-in names
    assert g.phase == "draw" and all(c.name for c in g.characters.values())
    artist = g.characters["p0"].artist
    card = g.player_view(artist, 101.0)["drawing"]
    assert card["name"] == "Vlad" and card["look"] == "tall, cape, flour on his face"
    g.handle(artist, {"type": "face", "emotion": "neutral", "strokes": LINE}, 101.0)
    assert g.characters["p0"].faces["neutral"] == LINE
    with pytest.raises(Invalid, match="mood"):
        g.handle(artist, {"type": "face", "emotion": "smug", "strokes": LINE}, 101.0)
    assert g.waiting_on() == set(pids)  # 3 moods to go for this artist, 4 for the others


def test_no_peeking_before_the_show():
    pids, g = drama(5)
    play_to(g, "write")
    blob = json.dumps(g.host_view(1.0)) + "".join(json.dumps(g.player_view(p, 1.0)) for p in pids)
    assert '"faces"' not in blob and '"p": [' not in blob
    play_to(g, "show")
    assert g.host_view(1.0)["chapter"]["cast"][0]["faces"]


def test_writing_a_chapter():
    pids, g = drama(3)
    play_to(g, "write")
    v = g.player_view("p0", 1.0)
    assert v["step"] == 4 and v["chapter"]["of"] == 3 and len(v["cast"]) == 3
    assert v["chapter"]["part"] == g.part(g.chapter_of("p0"))
    with pytest.raises(Invalid, match="background"):
        g.handle("p0", chapter(["p1"], bg="moon"), 1.0)
    with pytest.raises(Invalid, match="one or two"):
        g.handle("p0", chapter(["p1", "p2", "p0"]), 1.0)
    with pytest.raises(Invalid, match="one or two"):
        g.handle("p0", chapter(["p1", "p1"]), 1.0)
    with pytest.raises(Invalid, match="who says"):  # one character: speaker 1 doesn't exist
        g.handle("p0", chapter(["p1"], [{"who": 1, "emotion": "sad", "text": "hi"}]), 1.0)
    with pytest.raises(Invalid, match="At most"):
        g.handle("p0", chapter(["p1"], [{"who": 0, "emotion": "sad", "text": "hi"}] * 7), 1.0)
    g.handle("p0", chapter(["p2", "p1"], [{"who": NARRATOR, "emotion": "neutral", "text": "Midnight."},
                                         {"who": 1, "emotion": "flustered", "text": "Oh no."}]), 1.0)
    ch = g.chapters[g.chapter_of("p0")]
    assert ch.cast == ["p2", "p1"] and ch.bg == "cafe" and len(ch.lines) == 2
    assert g.player_view("p0", 1.0)["chapter"]["done"]


def test_missing_work_gets_filled_in():
    pids, g = drama(4)
    for _ in range(6):  # nobody does anything: every timer runs out
        g.tick(10_000.0 * (_ + 1))
    assert g.phase == "show"
    for c in g.characters.values():
        assert c.name and set(c.faces) == set(EMOTIONS)
    for ch in g.chapters:
        assert ch.bg in BACKGROUNDS and 1 <= len(ch.cast) <= 2 and ch.lines


def test_the_show_plays_every_chapter_in_order_then_credits():
    pids, g = drama(4)
    play_to(g, "show")
    seen = []
    while g.phase == "show":
        seen.append(g.host_view(1.0)["chapter"]["index"])
        g.skip(1.0)
    assert seen == [0, 1, 2, 3] and g.phase == "credits"
    cr = g.host_view(1.0)["credits"]
    assert len(cr["characters"]) == 4 and cr["writers"] == [ch.writer for ch in g.chapters]
    g.skip(1.0)
    assert g.phase == "vote"


def test_votes_and_scores():
    pids, g = drama(4)
    play_to(g, "vote")
    v = g.player_view("p0", 1.0)
    mine = g.chapter_of("p0")
    assert mine not in [c["index"] for c in v["chapters"]] and len(v["chapters"]) == 3
    drew = g.drawing_for("p0").creator
    assert {"p0", drew}.isdisjoint(d["pid"] for d in v["drawings"])
    with pytest.raises(Invalid, match="didn't write"):
        g.handle("p0", {"type": "vote", "chapter": mine}, 1.0)
    with pytest.raises(Invalid, match="didn't draw"):
        g.handle("p0", {"type": "vote", "drawing": drew}, 1.0)
    before = dict(g.scores)
    target = next(i for i in range(4) if g.chapters[i].writer != "p0")
    star = next(p for p in pids if p not in ("p0", drew))
    g.handle("p0", {"type": "vote", "chapter": target, "drawing": star}, 1.0)
    for p in pids[1:]:
        g.handle(p, {"type": "vote"}, 1.0)
    g.skip(1.0)
    assert g.phase == "scores"
    gained = {p: g.scores[p] - before[p] for p in pids}
    expect = dict.fromkeys(pids, 0)
    expect[g.chapters[target].writer] += DramaClub.CHAPTER_PTS
    expect[g.characters[star].artist] += DramaClub.ARTIST_PTS
    expect[star] += DramaClub.CREATOR_PTS
    assert gained == expect
    g.skip(1.0)
    assert g.done


@pytest.mark.parametrize("n", range(3, 9))
def test_bots_play_a_whole_game(n):
    pids, g = drama(n)
    play_to(g, "scores")
    assert g.counts and g.hits
    g.skip(1.0)
    assert g.done
