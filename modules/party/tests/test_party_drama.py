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
        g.handle(p, {"type": "theme", "text": f"theme {p}", "problem": f"problem {p}"}, 1.0)
    g.tick(1.0)
    assert g.phase == "pitch_vote"
    assert {"pid": "p1", "text": "theme p1", "problem": "problem p1"} in g.player_view("p0", 1.0)["themes"]
    with pytest.raises(Invalid, match="someone else"):
        g.handle("p0", {"type": "vote", "choice": "p0"}, 1.0)
    for p in ("p0", "p2", "p3"):
        g.handle(p, {"type": "vote", "choice": "p1"}, 1.0)
    g.handle("p1", {"type": "vote", "choice": "p0"}, 1.0)
    g.tick(1.0)
    assert g.theme == "theme p1" and g.problem == "problem p1"
    assert g.phase == "create" and g.scores["p1"] == DramaClub.THEME_PTS
    assert g.player_view("p2", 1.0)["problem"] == "problem p1"


def test_a_pitch_without_a_problem_gets_one():
    pids, g = drama(3)
    g.handle("p0", {"type": "theme", "text": "Space prom"}, 1.0)
    g.tick(1000.0)
    g.tick(2000.0)
    assert g.phase == "create" and g.problem


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


@pytest.mark.parametrize("n", range(3, 9))
def test_nobody_writes_the_headline_of_their_own_chapter(n):
    pids, g = drama(n)
    heads = [ch.headliner for ch in g.chapters]
    assert sorted(heads) == sorted(pids)  # one headline each
    assert all(ch.headliner != ch.writer for ch in g.chapters)


def test_the_outline_is_a_relay_in_chapter_order():
    pids, g = drama(4)
    play_to(g, "headline")
    heads = [ch.headliner for ch in g.chapters]
    v = g.player_view(heads[0], 1.0)
    assert v["step"] == 4 and v["chapter"]["turn"] == "now" and v["outline"] == []
    assert v["for"] == g.chapters[0].writer != heads[0]  # someone else writes the chapter
    last = g.player_view(heads[3], 1.0)["chapter"]
    assert last["turn"] == "waiting" and last["turns_left"] == 3
    with pytest.raises(Invalid, match="Not your turn"):
        g.handle(heads[1], {"type": "headline", "text": "too soon"}, 1.0)
    with pytest.raises(Invalid, match="one sentence"):
        g.handle(heads[0], {"type": "headline", "text": " "}, 1.0)
    for k, p in enumerate(heads):
        assert g.phase == "headline" and g.host_view(1.0)["relay"]["writer"] == p
        seen = g.player_view(p, 1.0)["outline"]
        assert [x["headline"] for x in seen] == [f"{q} happens" for q in heads[:k]]  # the story so far
        g.handle(p, {"type": "headline", "text": f"{p} happens"}, 1.0)
        with pytest.raises(Invalid, match="Not your turn"):
            g.handle(p, {"type": "headline", "text": "again"}, 1.0)
        g.tick(1.0)  # the writer is done: the next one is up straight away
    assert g.phase == "write"
    for p in pids:
        outline = g.player_view(p, 1.0)["chapter"]["outline"]
        assert [(x["headline"], x["by"]) for x in outline] == [(f"{q} happens", q) for q in heads]
    play_to(g, "show")
    assert g.host_view(1.0)["chapter"]["headline"] == f"{heads[0]} happens"


def test_a_relay_turn_times_out_or_skips_someone_who_left():
    pids, g = drama(4)
    play_to(g, "headline")
    heads = [ch.headliner for ch in g.chapters]
    g.tick(1000.0)  # the first writer ran out of time: a stand-in, then the next writer's turn
    assert g.phase == "headline" and g.relay == 1 and g.chapters[0].headline
    g.set_active(set(pids) - {heads[1]})  # the second writer left: skipped at once
    g.tick(1001.0)
    assert g.relay == 2 and g.chapters[1].headline


def test_writing_a_chapter():
    pids, g = drama(3)
    play_to(g, "write")
    v = g.player_view("p0", 1.0)
    assert v["step"] == 4 and v["chapter"]["of"] == 3 and len(v["cast"]) == 3
    assert v["chapter"]["part"] == g.part(g.chapter_of("p0"))
    with pytest.raises(Invalid, match="background"):
        g.handle("p0", chapter(["p1"], bg="moon"), 1.0)
    with pytest.raises(Invalid, match="who's in"):
        g.handle("p0", chapter([]), 1.0)
    with pytest.raises(Invalid, match="who's in"):
        g.handle("p0", chapter(["p1", "p1"]), 1.0)
    with pytest.raises(Invalid, match="who says"):  # one character: speaker 1 doesn't exist
        g.handle("p0", chapter(["p1"], [{"who": 1, "emotion": "sad", "text": "hi"}]), 1.0)
    with pytest.raises(Invalid, match="At most"):
        g.handle("p0", chapter(["p1"], [{"who": 0, "emotion": "sad", "text": "hi"}] * (DramaClub.MAX_LINES + 1)), 1.0)
    g.handle("p0", chapter(["p2", "p1"], [{"who": NARRATOR, "emotion": "neutral", "text": "Midnight."},
                                         {"who": 1, "emotion": "flustered", "text": "Oh no."}]), 1.0)
    ch = g.chapters[g.chapter_of("p0")]
    assert ch.cast == ["p2", "p1"] and ch.bg == "cafe" and len(ch.lines) == 2
    assert g.player_view("p0", 1.0)["chapter"]["done"]


def test_missing_work_gets_filled_in():
    pids, g = drama(4)
    for _ in range(12):  # nobody does anything: every timer runs out
        if g.phase == "show":
            break
        g.tick(10_000.0 * (_ + 1))
    assert g.phase == "show"
    for c in g.characters.values():
        assert c.name and set(c.faces) == set(EMOTIONS)
    for ch in g.chapters:
        assert ch.headline and ch.bg in BACKGROUNDS and 1 <= len(ch.cast) <= 2 and ch.lines


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


def test_characters_are_listed_in_their_own_random_order():
    same_as_seats = 0
    for seed in range(12):
        pids, g = drama(5, seed=seed)
        listed = list(g.characters)
        assert sorted(listed) == sorted(pids)
        same_as_seats += listed == g.order or listed == pids
    assert same_as_seats <= 2  # not the seating order, which would give away who drew whom


def test_unsent_work_is_used_when_time_runs_out():
    pids, g = drama(4)
    g.handle("p0", {"type": "draft", "text": "A haunted bakery", "problem": "Croissants are missing"}, 1.0)
    g.tick(1000.0)  # pitch time's up: p0's draft counts as their pitch
    assert g.themes["p0"] == "A haunted bakery" and g.problems["p0"] == "Croissants are missing"
    play_to(g, "create")
    for p in pids[1:]:
        g.handle(p, g.bot_move(p), 1.0)
    g.handle("p0", {"type": "draft", "name": "Vlad", "look": "cape"}, 1.0)
    g.tick(2000.0)
    assert g.phase == "draw" and g.characters["p0"].name == "Vlad" and g.characters["p0"].look == "cape"
    artist = g.characters["p0"].artist
    g.handle(artist, {"type": "draft", "emotion": "sad", "strokes": LINE}, 1.0)
    g.tick(3000.0)
    assert g.characters["p0"].faces["sad"] == LINE  # the mood left on the pad was kept
    assert g.phase == "headline"
    head = g.chapters[0].headliner
    g.handle(head, {"type": "draft", "text": "Vlad gets blamed"}, 1.0)
    g.tick(4000.0)
    assert g.chapters[0].headline == "Vlad gets blamed"
    t = 4000.0
    while g.phase == "headline":  # the rest of the relay times out, turn by turn
        t += 1000.0
        g.tick(t)
    writer = g.chapters[1].writer
    g.handle(writer, {"type": "draft", "bg": "cafe", "cast": ["p0", "nobody"], "lines": [
        {"who": 0, "emotion": "angry", "text": "Who ate it?"},
        {"who": 1, "emotion": "sad", "text": "not me"},  # speaker 1 isn't in the cast: narrator
        {"who": 0, "emotion": "sad", "text": "   "},  # blank: dropped
    ]}, 1.0)
    g.tick(t + 10_000.0)
    ch = g.chapters[1]
    assert g.phase == "show" and ch.bg == "cafe" and ch.cast == ["p0"]
    assert ch.lines == [{"who": 0, "emotion": "angry", "text": "Who ate it?"},
                        {"who": NARRATOR, "emotion": "sad", "text": "not me"}]


def test_a_sent_answer_beats_the_draft_and_late_drafts_are_ignored():
    pids, g = drama(3)
    g.handle("p0", {"type": "draft", "text": "draft theme"}, 1.0)
    g.handle("p0", {"type": "theme", "text": "sent theme"}, 1.0)
    g.tick(1000.0)
    assert g.themes["p0"] == "sent theme"
    play_to(g, "show")
    g.handle("p0", {"type": "draft", "text": "too late"}, 1.0)  # no error, nothing stored
    assert ("show", "p0") not in g.drafts


def test_a_chapter_can_use_the_whole_cast_and_long_lines():
    pids, g = drama(5)
    play_to(g, "write")
    long = "and then " * 30  # 270 letters of narration
    lines = [{"who": k, "emotion": "neutral", "text": f"line {k}"} for k in range(5)]
    lines += [{"who": NARRATOR, "emotion": "neutral", "text": long}] * 10
    g.handle("p0", chapter(pids, lines), 1.0)  # all five characters, 15 lines
    ch = g.chapters[g.chapter_of("p0")]
    assert ch.cast == pids and len(ch.lines) == 15 and ch.lines[-1]["text"] == long.strip()


def test_the_show_waits_for_the_host_line_by_line():
    pids, g = drama(3)
    play_to(g, "show")
    assert g.deadline is None and g.line == -1  # the title card, until the host clicks
    g.tick(1e9)
    assert g.phase == "show" and g.showing == 0
    n = len(g.chapters[0].lines)
    for k in range(n):
        g.next_line(1.0)
        assert g.host_view(1.0)["chapter"]["line"] == k
    g.next_line(1.0)  # past the last line: the next chapter, from its title card
    assert g.showing == 1 and g.line == -1


def test_odd_drafts_dont_stall_the_game():
    pids, g = drama(4)
    g.handle("p0", {"type": "draft", "text": ["a"], "problem": 5}, 1.0)
    g.handle("p1", {"type": "draft", "text": "ok", "problem": None}, 1.0)
    g.tick(1e6)
    assert g.phase != "pitch" and g.themes["p1"] == "ok" and not isinstance(g.themes.get("p0"), list)
    play_to(g, "create")
    g.handle("p0", {"type": "draft", "name": 7, "look": ["x"], "personality": {}}, 1.0)
    g.tick(1e6)
    assert g.phase == "draw" and all(c.name for c in g.characters.values())
    play_to(g, "headline")
    g.handle(g.chapters[0].headliner, {"type": "draft", "text": {"a": 1}}, 1.0)
    g.tick(1e6)
    assert g.chapters[0].headline
    play_to(g, "write")
    g.handle("p0", {"type": "draft", "cast": [pids[1]], "lines": [{"who": 0, "text": ["x"]},
                                                                  {"who": 0, "emotion": "sad", "text": "hi"}]}, 1.0)
    g.tick(1e6)
    assert g.phase == "show"
    ch = g.chapters[g.chapter_of("p0")]
    assert [x["text"] for x in ch.lines] == ["hi"] and ch.bg in BACKGROUNDS  # no background picked: one is chosen


def test_votes_must_be_whole_numbers_and_real_ids():
    pids, g = drama(4)
    play_to(g, "write")
    for bad in ([["x"]], [1.5], [True], [None]):
        with pytest.raises(Invalid):
            g.handle("p0", chapter(bad), 1.0)
    with pytest.raises(Invalid):
        g.handle("p0", chapter([pids[0]], [{"who": 0.0, "emotion": "sad", "text": "hi"}]), 1.0)
    play_to(g, "vote")
    i = g._chapter_choices("p0")[0]
    for bad in (float(i), True, "1", [i]):
        with pytest.raises(Invalid):
            g.handle("p0", {"type": "vote", "chapter": bad}, 1.0)
    with pytest.raises(Invalid):
        g.handle("p0", {"type": "vote", "drawing": ["p1"]}, 1.0)
    g.handle("p0", {"type": "vote", "chapter": i}, 1.0)


def test_the_themes_bonus_shows_in_the_final_gains():
    pids, g = drama(4)
    for p in pids:
        g.handle(p, {"type": "theme", "text": f"theme {p}"}, 1.0)
    g.tick(1.0)
    for p in ("p0", "p2", "p3"):
        g.handle(p, {"type": "vote", "choice": "p1"}, 1.0)
    g.handle("p1", {"type": "vote", "choice": "p0"}, 1.0)
    g.tick(1.0)
    play_to(g, "scores")
    assert g.gained["p1"] >= DramaClub.THEME_PTS
    assert all(g.gained.get(p, 0) == g.scores[p] for p in pids)
