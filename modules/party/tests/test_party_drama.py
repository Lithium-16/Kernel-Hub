import json
import random
from pathlib import Path

import pytest

from partydraw import BadDrawing, check_drawing
from partygames import BACKGROUNDS, EMOTIONS, Content, DramaClub, Invalid

HERE = Path(__file__).resolve().parent.parent
LINE = [{"c": 0, "w": 3, "p": [10, 10, 200, 200]}]


def content():
    return Content.load(HERE / "content")


def drama(n, **kw):
    pids = [f"p{i}" for i in range(n)]
    return pids, DramaClub(pids, content(), random.Random(n), 0.0, **kw)


def play_to(g, phase):
    """Every player does their job until the game reaches `phase`."""
    for _ in range(40):
        if g.phase == phase:
            return
        for p in g.pids:
            msg = g.bot_move(p)
            while msg is not None:
                g.handle(p, msg, 1.0)
                msg = g.bot_move(p) if g.phase == "cast" else None
        g.tick(1.0)
        while g.phase == "show" and phase != "show":
            g.skip(1.0)
    raise AssertionError(f"never reached {phase}, stuck in {g.phase}")


def test_sprites_cant_have_a_fill():
    assert check_drawing(LINE, sprite=True) == LINE
    with pytest.raises(BadDrawing, match="background fill"):
        check_drawing([{"fill": 2}] + LINE, sprite=True)


def test_theme_pitch_and_vote():
    pids, g = drama(4)
    assert g.phase == "pitch" and g.player_view("p0", 1.0)["idea"]
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
    assert g.theme == "theme p1" and g.phase == "cast" and g.scores["p1"] == DramaClub.THEME_PTS


def test_nobody_sees_a_character_before_the_show():
    pids, g = drama(5)
    play_to(g, "script")
    secret = json.dumps(g.characters["p0"].faces["neutral"])
    for ph in ("script", "twist"):
        assert g.phase == ph
        assert secret not in json.dumps(g.host_view(1.0))
        for p in pids:
            view = json.dumps(g.player_view(p, 1.0))
            assert secret not in view and '"faces"' not in view
            job = g.player_view(p, 1.0)["job"]
            assert job and [c["name"] for c in job["cast"]]  # names and bios only
        for p in pids:
            msg = g.bot_move(p)
            if msg:
                g.handle(p, msg, 1.0)
        g.tick(1.0)
    assert g.phase == "show" and secret in json.dumps(g.host_view(1.0))  # the reveal


def test_while_drawing_you_only_see_your_own_character():
    pids, g = drama(3)
    play_to(g, "cast")
    g.handle("p0", {"type": "character", "name": "Vlad", "bio": "nervous vampire baker"}, 1.0)
    g.handle("p0", {"type": "face", "emotion": "neutral", "strokes": LINE}, 1.0)
    assert g.player_view("p0", 1.0)["character"]["faces"]["neutral"] == LINE
    assert "Vlad" not in json.dumps(g.player_view("p1", 1.0)) and "Vlad" not in json.dumps(g.host_view(1.0))
    with pytest.raises(Invalid):
        g.handle("p0", {"type": "face", "emotion": "smug", "strokes": LINE}, 1.0)


@pytest.mark.parametrize("n", range(3, 9))
def test_every_step_gives_each_player_one_job(n):
    pids, g = drama(n)
    play_to(g, "script")
    for st in g.stories:
        assert not st.writers() & set(st.cast) and st.cast[0] != st.cast[1]
        assert st.twist_by != st.script_by
    for role in ("script_by",) + (("twist_by",) if n >= 4 else ()):
        assert sorted(getattr(st, role) for st in g.stories) == sorted(pids)
    assert g.has_twist() == (n >= 4)


def test_scripts_are_checked_and_late_players_get_fallbacks():
    pids, g = drama(4)
    play_to(g, "cast")
    g.handle("p0", {"type": "character", "name": "Vlad"}, 1.0)
    g.handle("p0", {"type": "face", "emotion": "neutral", "strokes": LINE}, 1.0)
    g.skip(1.0)  # time's up for everyone else
    vlad = g.characters["p0"]
    assert all(vlad.faces[e] == LINE for e in EMOTIONS)  # missing moods copy the neutral one
    assert all(c.name and len(c.faces) == 4 for c in g.characters.values())
    assert g.phase == "script"
    writer = g.job("p2")
    good = [{"who": 1, "emotion": "flustered", "text": "B-baka!"}]
    with pytest.raises(Invalid, match="background"):
        g.handle("p2", {"type": "script", "bg": "moon", "premise": "x", "lines": good}, 1.0)
    with pytest.raises(Invalid, match="title"):
        g.handle("p2", {"type": "script", "bg": "onsen", "premise": " ", "lines": good}, 1.0)
    bad = [{"who": 0, "emotion": "smug", "text": "hi"}]
    with pytest.raises(Invalid):
        g.handle("p2", {"type": "script", "bg": "onsen", "premise": "x", "lines": bad}, 1.0)
    with pytest.raises(Invalid, match="At most"):
        g.handle("p2", {"type": "script", "bg": "onsen", "premise": "x",
                        "lines": [{"who": 0, "emotion": "sad", "text": "x"}] * 9}, 1.0)
    g.handle("p2", {"type": "script", "bg": "onsen", "premise": "Only one cupcake left", "lines": good}, 1.0)
    g.skip(1.0)
    assert writer.lines[0]["text"] == "B-baka!" and writer.bg == "onsen" and writer.premise == "Only one cupcake left"
    assert all(s.lines and s.bg in BACKGROUNDS and s.premise for s in g.stories)  # late writers get fallbacks
    assert g.phase == "twist" and g.player_view("p0", 1.0)["job"]["lines"]  # the twist writer reads the scene


def test_votes_score_every_role():
    pids, g = drama(5)
    play_to(g, "vote")
    st = g.stories[0]
    with pytest.raises(Invalid, match="didn't write"):
        g.handle(st.script_by, {"type": "vote", "scene": 0}, 1.0)
    with pytest.raises(Invalid, match="someone else"):
        g.handle("p1", {"type": "vote", "character": "p1"}, 1.0)
    voters = [p for p in pids if p not in st.writers()]
    before = dict(g.scores)
    for p in voters:
        g.handle(p, {"type": "vote", "scene": 0}, 1.0)
    fans = [p for p in pids if p != st.cast[0]]
    for p in fans:
        g.handle(p, {"type": "vote", "character": st.cast[0]}, 1.0)
    g.skip(1.0)
    gain = {p: g.scores[p] - before[p] for p in pids}
    n = len(voters)
    assert gain[st.script_by] >= 550 * n and gain[st.twist_by] >= 300 * n
    assert gain[st.cast[0]] >= 75 * n + 500 * len(fans) and gain[st.cast[1]] >= 75 * n
    assert ("leading_role" in {b for _, b in g.feats}) and g.hits[0]["kind"] == "scene"
    assert g.hits[0]["scene"]["bg"] == st.bg and len(g.hits[0]["scene"]["faces"]) == 2
    assert g.phase == "scores"
    g.skip(1.0)
    assert g.done


def test_three_players_have_no_twist_and_two_rounds_keep_the_cast():
    pids, g = drama(3, rounds=2)
    play_to(g, "show")
    assert not g.has_twist() and all(s.twist_by is None for s in g.stories)
    first = {p: c.name for p, c in g.characters.items()}
    play_to(g, "scores")
    g.skip(1.0)
    assert g.round == 2 and g.phase == "pitch"
    play_to(g, "script")  # straight from the theme to writing: no drawing again
    assert {p: c.name for p, c in g.characters.items()} == first
    play_to(g, "scores")
    g.skip(1.0)
    assert g.done
