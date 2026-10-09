import random
from pathlib import Path

import pytest

from partydraw import MAX_POINTS, MAX_STROKES, BadDrawing, check_drawing
from partygames import Content, Invalid, ShirtShowdown

HERE = Path(__file__).resolve().parent.parent
P3 = ["a", "b", "c"]
LINE = [{"c": 0, "w": 1, "p": [10, 10, 200, 200]}]


def test_drawings_are_checked():
    assert check_drawing(LINE) == LINE
    full = [
        {"fill": "#FFEEDD"},
        {"c": "#A1B2C3", "w": 40, "p": [0, 0, 400, 400]},
        {"e": True, "w": 12, "p": [5, 5]},
        {"c": 7, "w": 1, "p": [1, 2, 3, 4]},
    ]
    assert check_drawing(full) == [
        {"fill": "#ffeedd"},
        {"c": "#a1b2c3", "w": 40, "p": [0, 0, 400, 400]},
        {"e": True, "w": 12, "p": [5, 5]},
        {"c": 7, "w": 1, "p": [1, 2, 3, 4]},
    ]
    for bad in (None, [], "x", ["stroke"],
                [{"c": 8, "w": 4, "p": [1, 1]}], [{"c": "red", "w": 4, "p": [1, 1]}], [{"c": "#12345", "w": 4, "p": [1, 1]}],
                [{"c": 0, "w": 0, "p": [1, 1]}], [{"c": 0, "w": 41, "p": [1, 1]}], [{"c": 0, "w": 4.5, "p": [1, 1]}],
                [{"c": 0, "w": 4, "p": [1]}], [{"c": 0, "w": 4, "p": [1, 401]}], [{"c": 0, "w": 4, "p": [1, -1]}],
                [{"c": 0, "w": 4, "p": [1.5, 2]}], [{"c": True, "w": 4, "p": [1, 2]}], [{"w": 4, "p": [1, 2]}],
                [{"e": False, "c": 0, "w": 4, "p": [1, 2]}], [{"c": 0, "w": 4, "p": [1, 2], "x": 1}],
                [{"fill": "#zzzzzz"}], [{"fill": 0, "c": 1}], [{"e": True, "w": 4, "p": [1, 2]}]):
        with pytest.raises(BadDrawing):
            check_drawing(bad)
    with pytest.raises(BadDrawing, match="too many"):
        check_drawing(LINE * (MAX_STROKES + 1))
    with pytest.raises(BadDrawing, match="detailed"):
        check_drawing([{"c": 0, "w": 4, "p": [5, 5] * 4000}] * 3)
    assert MAX_POINTS == 8000


def test_bundled_ideas_load():
    c = Content.load(HERE / "content")
    assert len(c.doodle_ideas) >= 50 and len(c.slogan_ideas) >= 50


def game(pids=P3, **kw):
    c = Content(doodle_ideas=["draw a thing"], slogan_ideas=["say a thing"])
    return ShirtShowdown(pids, c, random.Random(4), 0.0, **kw)


def fill_pool(g, now=1.0):
    for p in g.pids:
        g.handle(p, {"type": "drawing", "strokes": LINE}, now)
        g.handle(p, {"type": "drawing", "strokes": LINE}, now)
        g.handle(p, {"type": "done"}, now)
    assert g.tick(now) and g.phase == "slogan"
    for p in g.pids:
        for k in range(3):
            g.handle(p, {"type": "slogan", "text": f"{p} slogan {k} round {g.round}"}, now)
        g.handle(p, {"type": "done"}, now)
    assert g.tick(now) and g.phase == "assemble"


def test_draw_and_slogan_limits():
    g = game()
    assert g.phase == "draw" and g.player_view("a", 0.0)["idea"] == "draw a thing"
    with pytest.raises(Invalid, match="Draw something"):
        g.handle("a", {"type": "drawing", "strokes": []}, 1.0)
    for _ in range(ShirtShowdown.MAX_DRAWINGS):
        g.handle("a", {"type": "drawing", "strokes": LINE}, 1.0)
    with pytest.raises(Invalid, match="plenty"):
        g.handle("a", {"type": "drawing", "strokes": LINE}, 1.0)
    assert g.host_view(1.0)["counts"]["a"] == ShirtShowdown.MAX_DRAWINGS
    with pytest.raises(Invalid):
        g.handle("a", {"type": "slogan", "text": "too early"}, 1.0)
    g.skip(1.0)
    g.handle("a", {"type": "slogan", "text": "Nap Champion"}, 1.0)
    with pytest.raises(Invalid, match="already wrote"):
        g.handle("b", {"type": "slogan", "text": "nap champion!"}, 1.0)
    assert g.player_view("a", 1.0)["mine"] == ["Nap Champion"]


def test_hands_are_mostly_other_peoples_and_reroll_once():
    g = game()
    fill_pool(g)
    for p in P3:
        v = g.player_view(p, 1.0)
        assert len(v["drawings"]) == 3 and len(v["slogans"]) == 3
        by = {x["by"] for x in g.drawings if x["id"] in [d["id"] for d in v["drawings"]]}
        # 6 drawings by 3 players: 4 are someone else's, so a hand of 3 never needs your own.
        assert p not in by
    before = g.hands["a"]["slogans"]
    g.handle("a", {"type": "reroll", "what": "slogans"}, 1.0)
    assert g.hands["a"]["slogans"] != before
    with pytest.raises(Invalid, match="already rerolled"):
        g.handle("a", {"type": "reroll", "what": "slogans"}, 1.0)
    assert g.player_view("a", 1.0)["rerolls"] == ["drawings"]


def test_shirts_must_come_from_your_hand_and_pieces_are_used_once():
    g = game()
    fill_pool(g)
    ha = g.hands["a"]
    mine = next(d["id"] for d in g.drawings if d["by"] == "a")
    with pytest.raises(Invalid, match="hand"):
        g.handle("a", {"type": "shirt", "drawing": mine, "slogan": ha["slogans"][0], "color": 2}, 1.0)
    g.handle("a", {"type": "shirt", "drawing": ha["drawings"][0], "slogan": ha["slogans"][0], "color": 2}, 1.0)
    assert g.shirts["a"].color == 2
    with pytest.raises(Invalid, match="already made"):
        g.handle("a", {"type": "shirt", "drawing": ha["drawings"][1], "slogan": ha["slogans"][1]}, 1.0)
    # Someone else holding the same piece can't use it any more.
    for p in ("b", "c"):
        shared = set(g.hands[p]["drawings"]) & {ha["drawings"][0]}
        if shared:
            with pytest.raises(Invalid, match="hand|used"):
                g.handle(p, {"type": "shirt", "drawing": ha["drawings"][0], "slogan": g.hands[p]["slogans"][0]}, 1.0)
    # Time runs out: everyone else gets a shirt from their hand anyway.
    g.tick(g.deadline)
    assert set(g.shirts) == set(P3) and g.phase == "vote"
    used = [x["id"] for s in g.shirts.values() for x in (s.drawing, s.slogan) if x]
    assert len(used) == len(set(used))


def test_king_of_the_hill_scoring():
    g = game(["a", "b", "c", "d"], rounds=1)
    fill_pool(g)
    g.tick(g.deadline)
    order = g.order
    champ, chall = g.pair
    assert (champ.maker, chall.maker) == (order[0], order[1])
    # Nobody can vote for a shirt they made part of.
    for p in g.pids:
        for m in g.can_vote_for(p):
            assert p not in g.shirts[m].helpers()
    with pytest.raises(Invalid, match="helped make"):
        g.handle(champ.maker, {"type": "vote", "choice": champ.maker}, 2.0)
    for p in g.pids:
        if champ.maker in g.can_vote_for(p):
            g.handle(p, {"type": "vote", "choice": champ.maker}, 2.0)
    votes_for_champ = sum(1 for v in g.votes.values() if v == champ.maker)
    g.tick(g.deadline)
    assert g.phase == "reveal"
    battle = g.host_view(2.0)["battle"]
    assert battle["winner"] == 0 and battle["streak"] == 1 and battle["shirts"][0]["id"] == champ.maker
    assert battle["shirts"][0]["artist"] == champ.drawing["by"]
    artist = champ.drawing["by"]
    expected = votes_for_champ * 100 + 200 + (votes_for_champ * 100 + 200 if champ.slogan["by"] == artist else 0)
    assert g.gained.get(artist, 0) == expected
    while g.phase != "round_over":
        g.skip(3.0)
    assert g.winners and g.host_view(3.0)["winner"]["id"] == g.winners[0].maker
    assert g.hits[0]["kind"] == "shirt"
    g.skip(3.0)
    assert g.phase == "scores"  # one round: no final
    g.skip(3.0)
    assert g.done


def test_a_challenger_takes_over_and_two_rounds_lead_to_a_final():
    g = game(rounds=2)
    fill_pool(g)
    g.tick(g.deadline)
    champ, chall = g.pair
    # With 3 players everyone made part of one of the two shirts; set the votes directly.
    g.votes = {"x": chall.maker, "y": chall.maker}
    g.tick(g.deadline)
    assert g.champion == chall.maker and g.streak == 0
    reveal = g.host_view(2.0)["battle"]
    assert reveal["winner"] == 1 and reveal["shirts"][0]["id"] == champ.maker  # the pair stays put
    while g.phase != "draw":
        g.skip(3.0)
    assert g.round == 2 and g.phase == "draw"
    fill_pool(g, 4.0)
    while not g.phase.startswith("final"):
        g.skip(5.0)
    assert g.phase == "final_vote" and len(g.winners) == 2
    g.skip(5.0)
    assert g.phase == "final_reveal" and g.hits[0]["kind"] == "shirt"
    g.skip(5.0)
    g.skip(5.0)
    assert g.done and sum(g.scores.values()) > 0


def test_nobody_making_anything_still_ends():
    g = game(rounds=1)
    now = 0.0
    while not g.done:
        now = g.deadline if g.deadline is not None else now
        g.tick(now)
    assert set(g.scores.values()) == {0}
