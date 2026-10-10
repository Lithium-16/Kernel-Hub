import random
from pathlib import Path

import pytest

from partygames import BluffBuffet, Content, Fact, Invalid, QuipClash, clean, norm

HERE = Path(__file__).resolve().parent.parent
P3 = ["a", "b", "c"]
P5 = ["a", "b", "c", "d", "e"]


def content():
    return Content(
        quips=[f"prompt {i}" for i in range(30)],
        facts=[Fact(f"fact {i} is ___.", f"truth{i}", (f"real{i}",)) for i in range(8)],
    )


def test_bundled_content_loads_and_data_folder_adds_to_it(tmp_path):
    base = Content.load(HERE / "content")
    assert len(base.quips) >= 90 and len(base.facts) >= 35
    assert all("___" in f.question for f in base.facts)
    emus = next(f for f in base.facts if "Australia" in f.question)
    assert emus.is_truth("Emus!") and emus.is_truth("the emu") and not emus.is_truth("kangaroos")
    (tmp_path / "quips.txt").write_text("# mine\nMy own prompt\nThe worst thing to find inside a fortune cookie\n")
    assert len(emus.decoys) == 4 and not any(emus.is_truth(d) for d in emus.decoys)
    assert all(len(f.decoys) >= 3 for f in base.facts)
    (tmp_path / "facts.txt").write_text("No blank here\tx\nMy fact is ___.\tyes\tyep|yeah\tno|Yep|maybe|no\n")
    both = Content.load(HERE / "content", tmp_path)
    assert both.quips[-1] == "My own prompt" and len(both.quips) == len(base.quips) + 1
    assert both.facts[-1] == Fact("My fact is ___.", "yes", ("yep", "yeah"), ("no", "maybe"))


def test_text_cleaning():
    assert clean("  hi\n\tthere\x00 ", 80) == "hi there"
    assert len(clean("x" * 200, 80)) == 80
    with pytest.raises(Invalid):
        clean(None, 80)
    assert norm("The Emus!") == norm("emu") == "emu"
    assert norm("A Cricket Team") == "cricket team"


def quip(pids=P5, **kw):
    return QuipClash(pids, content(), random.Random(1), 0.0, **kw)


def test_quip_pairs_every_player_twice_with_different_partners():
    g = quip()
    assert g.phase == "write" and g.deadline == 90
    for p in P5:
        mine = g.my_prompts(p)
        assert len(mine) == 2
        partners = {a for i in mine for a in g.matchups[i].authors} - {p}
        assert len(partners) == 2
    assert len({m.prompt for m in g.matchups}) == len(P5)


def test_quip_order_and_sides_are_random_and_nobody_plays_twice_in_a_row():
    lefts, firsts = set(), set()
    for seed in range(20):
        g = QuipClash(P5, content(), random.Random(seed), 0.0)
        ms = g.matchups
        for a, b in zip(ms, ms[1:]):
            assert not set(a.authors) & set(b.authors)  # back to back matchups share nobody
        lefts.add(ms[0].authors[0])
        firsts.add(frozenset(ms[0].authors))
    assert len(lefts) > 2 and len(firsts) > 2  # who opens, and on which side, changes from game to game


def test_quip_final_answers_are_shuffled_the_same_for_everyone():
    orders = set()
    for seed in range(10):
        g = QuipClash(P5, content(), random.Random(seed), 0.0)
        g._start_final(1.0)
        for p in P5:
            g.handle(p, {"type": "answer", "text": f"answer {p}"}, 1.0)
        g.tick(1.0)
        host = [a["id"] for a in g.host_view(1.0)["final"]["answers"]]
        assert sorted(host) == sorted(P5)
        for p in P5:  # each phone sees the same order, minus its own answer
            assert [c["id"] for c in g.player_view(p, 1.0)["choices"]] == [x for x in host if x != p]
        orders.add(tuple(host))
    assert len(orders) > 3


def test_quip_needs_enough_players():
    with pytest.raises(Invalid, match="3 to 8"):
        quip(["a", "b"])


def answer_all(g, now=1.0):
    for p in g.pids:
        while g._todo(p) is not None:
            g.handle(p, {"type": "answer", "text": f"{p} says hi {g._todo(p)}"}, now)


def test_quip_round_flow_shares_the_pool_and_sweeps():
    g = quip()
    v = g.player_view("a", 0.0)
    assert v["todo"]["number"] == 1 and v["todo"]["of"] == 2
    answer_all(g)
    with pytest.raises(Invalid):
        g.handle("a", {"type": "answer", "text": "again"}, 1.0)
    assert g.tick(1.0) and g.phase == "vote"  # everyone answered: no need to wait
    m = g.matchups[0]
    voters = g.voters(m)
    assert len(voters) == 3
    with pytest.raises(Invalid, match="own"):
        g.handle(m.authors[0], {"type": "vote", "choice": 1}, 2.0)
    with pytest.raises(Invalid):
        g.handle(voters[0], {"type": "vote", "choice": 7}, 2.0)
    for v in voters:
        g.handle(v, {"type": "vote", "choice": 1}, 2.0)
    assert g.tick(2.0) and g.phase == "reveal"
    view = g.host_view(2.0)["matchup"]
    # all 1000 by vote share + 100 for winning + 250 for the clean sweep
    assert view["points"] == [0, 1350] and view["sweep"] == 1 and view["winner"] == 1 and view["outcome"] == "vote"
    assert g.scores[m.authors[1]] == 1350
    assert g.hits and g.hits[0]["votes"] == 3 and g.hits[0]["pid"] == m.authors[1]
    pv = g.player_view(m.authors[1], 2.0)
    assert pv["mine"] and pv["won"] and pv["gain"] == 1350 and pv["sweep_by"] == m.authors[1]
    # the next matchup: a 2-1 split shares the pool, the winner gets the bonus, no sweep
    g.skip(2.0)
    m = g.matchups[1]
    v0, v1, v2 = g.voters(m)
    for voter, side in ((v0, 0), (v1, 0), (v2, 1)):
        g.handle(voter, {"type": "vote", "choice": side}, 3.0)
    g.tick(3.0)
    assert m.points == [670 + 100, 330] and m.sweep is None


def test_quip_jinx_and_answers_that_win_by_default():
    g = quip(P3)
    a, b = g.matchups[0].authors
    # matchup 0: the same answer from both (a jinx); everything else: one side never answers
    for i, m in enumerate(g.matchups):
        if i == 0:
            m.answers = {a: "The Moon!", b: "the moon"}
        else:
            m.answers = {m.authors[0]: f"only me {i}"}
    g.skip(1.0)
    assert g.phase == "reveal" and g.matchups[0].outcome == "jinx"  # no vote needed
    assert g.host_view(1.0)["matchup"]["points"] == [0, 0]
    assert g.scores[a] == g.scores[b] == 0
    g.skip(1.0)
    m = g.matchups[1]
    assert g.phase == "reveal" and m.outcome == "forfeit" and m.winner == 0
    assert m.points == [1100, 0] and g.scores[m.authors[0]] >= 1100


def test_quip_matchups_nobody_answered_are_skipped():
    g = quip(P3)
    g.tick(g.deadline)  # nobody answered anything
    assert g.phase == "scores" and all(m.outcome == "empty" for m in g.matchups)


def test_quip_timeouts_carry_the_whole_game_to_the_end():
    g = quip(P3)
    now = 0.0
    seen = []
    while not g.done:
        now = g.deadline if g.deadline is not None else now
        g.tick(now)
        seen.append(g.phase)
    assert seen.count("scores") == 3 and "final_write" in seen and seen[-1] == "over"
    assert g.round == 3 and set(g.scores.values()) == {0}
    assert g.host_view(now)["standings"][0]["score"] == 0


def test_quip_round_two_doubles_and_the_last_round_has_three_votes():
    g = quip(["a", "b", "c", "d", "e"])
    for _ in range(2):
        answer_all(g)
        g.tick(1.0)
        while g.phase in ("vote", "reveal"):
            if g.phase == "vote":
                m = g.matchups[g.current]
                g.handle(g.voters(m)[0], {"type": "vote", "choice": 0}, 1.0)
            g.tick(1.0) or g.skip(1.0)
        assert g.phase == "scores"
        g.skip(1.0)
    assert g.phase == "final_write" and g.mult() == 2
    # one vote each: 1000 + 100 to the winner; round 2 doubles it
    assert sum(g.scores.values()) == 5 * 1100 + 5 * 2200
    for p in g.pids:
        g.handle(p, {"type": "answer", "text": f"final {p}"}, 1.0)
    g.tick(1.0)
    assert g.phase == "final_vote" and g.player_view("a", 1.0)["votes_each"] == 3
    with pytest.raises(Invalid, match="other answers"):
        g.handle("a", {"type": "vote", "choices": ["a", "b"]}, 1.0)
    with pytest.raises(Invalid, match="other answers"):
        g.handle("a", {"type": "vote", "choices": ["b", "c", "d", "e"]}, 1.0)
    with pytest.raises(Invalid):
        g.handle("a", {"type": "vote", "choices": ["b", "b"]}, 1.0)
    before = dict(g.scores)
    g.handle("a", {"type": "vote", "choices": ["b", "c", "d"]}, 1.0)
    with pytest.raises(Invalid, match="already"):
        g.handle("a", {"type": "vote", "choices": ["e"]}, 1.0)
    g.handle("c", {"type": "vote", "choices": ["b"]}, 1.0)
    g.handle("b", {"type": "vote", "choices": ["a", "c"]}, 1.0)
    g.skip(1.0)
    assert g.phase == "final_reveal"
    gained = {p: g.scores[p] - before[p] for p in g.pids}
    assert gained == {"a": 300, "b": 600, "c": 600, "d": 300, "e": 0}
    top = g.host_view(1.0)["final"]["answers"][0]
    assert len(top["voters"]) == 2 and top["points"] == 600
    assert g.player_view("b", 1.0)["gain"] == 600
    g.skip(1.0)
    g.skip(1.0)
    assert g.done and g.phase == "over"


def test_quip_waits_only_for_connected_players():
    g = quip(P3)
    for p in ("a", "b"):
        while g._todo(p) is not None:
            g.handle(p, {"type": "answer", "text": f"{p} {g._todo(p)}"}, 1.0)
    assert not g.tick(1.0)  # c still has prompts
    g.set_active({"a", "b"})
    assert g.tick(1.0) and g.phase in ("vote", "reveal")


def test_quip_fills_in_a_friends_name():
    c = Content(quips=["What {player} is good at"] * 1 + [f"p{i}" for i in range(20)], facts=[])
    g = QuipClash(P3, c, random.Random(3), 0.0, names={"a": "Ann", "b": "Bo", "c": "Cy"})
    for m in g.matchups:
        if m.prompt.startswith("What "):
            other = ({"a", "b", "c"} - set(m.authors)).pop()
            assert m.prompt == f"What {g.names[other]} is good at"


def bluff(pids=P3, **kw):
    return BluffBuffet(pids, content(), random.Random(2), 0.0, **kw)


def test_bluff_refuses_the_truth_and_merges_duplicate_lies():
    g = bluff(questions=2)
    t = g.fact.answer
    with pytest.raises(Invalid, match="real answer"):
        g.handle("a", {"type": "lie", "text": t.upper() + "s"}, 1.0)
    with pytest.raises(Invalid, match="real answer"):
        g.handle("a", {"type": "lie", "text": g.fact.also[0]}, 1.0)
    g.handle("a", {"type": "lie", "text": "Rabbits"}, 1.0)
    g.handle("b", {"type": "lie", "text": "the rabbit"}, 1.0)
    with pytest.raises(Invalid, match="already"):
        g.handle("a", {"type": "lie", "text": "other"}, 1.0)
    g.handle("c", {"type": "lie", "text": "koalas"}, 1.0)
    assert g.tick(1.0) and g.phase == "pick"
    opts = g.bluff.options
    assert len(opts) == 3 and sum(o.truth for o in opts) == 1
    rabbits = next(i for i, o in enumerate(opts) if o.text == "Rabbits")
    assert opts[rabbits].authors == ["a", "b"]
    mine = [o["mine"] for o in g.player_view("a", 1.0)["options"]]
    assert mine[rabbits] and sum(mine) == 1
    with pytest.raises(Invalid):
        g.handle("a", {"type": "pick", "choice": rabbits}, 1.0)


def test_bluff_points_grow_through_the_game():
    def mults(n):
        g = bluff(questions=n)
        out = []
        while not g.done:
            out.append(g.mult())
            g.skip(1.0)
            g.skip(1.0)
            g.skip(1.0)
            g.skip(1.0)
        return out

    assert mults(1) == [1]
    assert mults(3) == [1, 1.5, 2]
    assert mults(5) == [1, 1, 1.5, 1.5, 2]
    assert mults(7) == [1, 1, 1, 1.5, 1.5, 1.5, 2]


def decoy_content():
    c = content()
    c.facts = [Fact(f"fact {i} is ___.", f"truth{i}", (), ("lie one", "lie two", "lie three", f"truth{i}", "lie four"))
               for i in range(3)]
    return c


def test_bluff_house_lies_fill_small_rooms_and_score_nobody():
    g = BluffBuffet(["a", "b"], decoy_content(), random.Random(3), 0.0, questions=1)
    assert "truth0" not in g.bluff.decoys
    g.handle("a", {"type": "lie", "text": "Lie One"}, 1.0)
    g.handle("b", {"type": "lie", "text": "zebras"}, 1.0)
    g.tick(1.0)
    opts = g.bluff.options
    assert len(opts) == 5 and sum(o.house for o in opts) == 2
    assert [o.text.lower() for o in opts].count("lie one") == 1  # a house lie never repeats a player's
    house = next(i for i, o in enumerate(opts) if o.house)
    g.handle("a", {"type": "pick", "choice": house}, 1.0)
    g.handle("b", {"type": "pick", "choice": house}, 1.0)
    g.tick(1.0)
    assert g.gained == {} and not g.duels
    reveal = g.host_view(1.0)["reveal"]
    assert [r["house"] for r in reveal] == [True, False] and reveal[-1]["truth"]  # unpicked lies are skipped
    assert g.player_view("a", 1.0)["house"]


def test_bluff_lie_for_me_and_auto_lies_on_time_out():
    g = BluffBuffet(P3, decoy_content(), random.Random(4), 0.0, questions=1)
    sugg = {p: g.player_view(p, 1.0)["suggestions"] for p in P3}
    assert all(len(s) == 2 for s in sugg.values()) and sugg["a"] != sugg["b"]
    g.handle("a", {"type": "lie", "text": sugg["a"][1]}, 1.0)
    g.tick(g.deadline)
    assert g.bluff.lies["a"] == sugg["a"][1] and g.bluff.lies["b"] == sugg["b"][0] and g.bluff.lies["c"] == sugg["c"][0]
    assert all(o.authors for o in g.bluff.options if not o.truth and not o.house)


def test_bluff_likes_toggle_and_show_in_the_reveal():
    g = bluff(questions=1)
    for p, t in zip(P3, ["rabbits", "koalas", "wombats"]):
        g.handle(p, {"type": "lie", "text": t}, 1.0)
    g.tick(1.0)
    idx = {o.text: i for i, o in enumerate(g.bluff.options)}
    with pytest.raises(Invalid, match="own"):
        g.handle("a", {"type": "like", "choice": idx["rabbits"]}, 1.0)
    g.handle("b", {"type": "like", "choice": idx["rabbits"]}, 1.0)
    g.handle("c", {"type": "like", "choice": idx["rabbits"]}, 1.0)
    g.handle("c", {"type": "like", "choice": idx["koalas"]}, 1.0)
    g.handle("c", {"type": "like", "choice": idx["koalas"]}, 1.0)  # changed their mind
    assert g.player_view("b", 1.0)["options"][idx["rabbits"]]["liked"]
    g.handle("b", {"type": "pick", "choice": idx["rabbits"]}, 1.0)
    g.handle("c", {"type": "pick", "choice": idx["rabbits"]}, 1.0)
    g.handle("a", {"type": "pick", "choice": idx["koalas"]}, 1.0)
    g.tick(1.0)
    rows = {r["text"]: r for r in g.host_view(1.0)["reveal"]}
    assert rows["rabbits"]["likes"] == 2 and rows["koalas"]["likes"] == 0 and "wombats" not in rows
    assert g.player_view("a", 1.0)["likes"] == 2


def test_bluff_scores_truth_and_fooling_with_a_double_last_question():
    g = bluff(questions=2)
    for q in range(2):
        g.handle("a", {"type": "lie", "text": "rabbits"}, 1.0)
        g.handle("b", {"type": "lie", "text": "koalas"}, 1.0)
        g.handle("c", {"type": "lie", "text": "wombats"}, 1.0)
        g.tick(1.0)
        idx = {o.text: i for i, o in enumerate(g.bluff.options)}
        truth = next(i for i, o in enumerate(g.bluff.options) if o.truth)
        g.handle("a", {"type": "pick", "choice": truth}, 1.0)
        g.handle("b", {"type": "pick", "choice": idx["rabbits"]}, 1.0)
        g.handle("c", {"type": "pick", "choice": idx["rabbits"]}, 1.0)
        g.tick(1.0)
        assert g.phase == "reveal"
        m = 2 if q == 1 else 1
        assert g.gained == {"a": 1000 * m + 2 * 500 * m}
        pv = g.player_view("a", 1.0)
        assert pv["found"] and sorted(pv["fooled"]) == ["b", "c"] and pv["gain"] == 2000 * m
        reveal = g.host_view(1.0)["reveal"]
        assert reveal[-1]["truth"] and reveal[-2]["text"] == "rabbits"
        g.skip(1.0)
        assert g.phase == "scores"
        g.skip(1.0)
    assert g.done and g.scores == {"a": 6000, "b": 0, "c": 0}
    assert g.hits and g.hits[0]["kind"] == "lie" and g.hits[0]["text"] == "rabbits"


def test_bluff_with_nobody_lying_still_moves_on():
    g = bluff(["a", "b"], questions=1)
    g.tick(g.deadline)
    assert g.phase == "pick" and len(g.bluff.options) == 1
    g.tick(g.deadline)
    assert g.phase == "reveal"
