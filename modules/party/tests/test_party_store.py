import time

import pytest

from partystore import PIN_TRIES, PinNeeded, PinWrong, Store, season_of


class Clock:
    def __init__(self, t):
        self.t = t

    def __call__(self):
        return self.t


def at(y, m, d=15):
    return time.mktime((y, m, d, 12, 0, 0, 0, 0, -1))


@pytest.fixture
def store(tmp_path):
    clock = Clock(at(2026, 9))
    s = Store(tmp_path / "party.db", clock)
    s.test_clock = clock
    yield s
    s.close()


def test_migrates_once_and_reopens(tmp_path):
    a = Store(tmp_path / "p.db")
    a.sign_in("Ann", None)
    a.close()
    b = Store(tmp_path / "p.db")
    assert b.db.execute("PRAGMA user_version").fetchone()[0] == 1
    assert b.profile_by_name("ann")["name"] == "Ann"


def test_profiles_devices_and_pins(store):
    pid, name, device = store.sign_in("Ann", None, pin="1234")
    assert name == "Ann" and device
    # The same phone comes back without a PIN, whatever the name's case.
    assert store.sign_in("ann", device) == (pid, "Ann", device)
    # A new phone needs the PIN.
    with pytest.raises(PinNeeded):
        store.sign_in("ANN", None)
    with pytest.raises(PinWrong, match="4 tries left"):
        store.sign_in("Ann", None, pin="0000")
    pid2, _, device2 = store.sign_in("Ann", "someone-else's", pin="1234")
    assert pid2 == pid and device2 != device
    # Without a PIN, a taken name can't be claimed from another phone.
    store.sign_in("Bo", None)
    with pytest.raises(ValueError, match="already plays"):
        store.sign_in("bo", None, pin="1111")


def test_too_many_wrong_pins_lock_for_a_while(store):
    store.sign_in("Ann", None, pin="1234")
    for _ in range(PIN_TRIES - 1):
        with pytest.raises(PinWrong, match="left"):
            store.sign_in("Ann", None, pin="9999")
    with pytest.raises(PinWrong, match="10 minutes"):
        store.sign_in("Ann", None, pin="9999")
    with pytest.raises(PinWrong, match="Too many"):
        store.sign_in("Ann", None, pin="1234")  # even the right one, while locked
    store.test_clock.t += 601
    assert store.sign_in("Ann", None, pin="1234")[1] == "Ann"


def play(store, standings, **kw):
    return store.record("quip", "Quip Clash", standings, kw.get("hits", []), kw.get("duels", []),
                        kw.get("feats", []), kw.get("night_wins", {}))


def test_results_seasons_badges_and_champions(store):
    a = store.sign_in("Ann", None)[0]
    b = store.sign_in("Bo", None)[0]
    out = play(store, [(a, 900), (b, 300)], feats=[(b, "clean_sweep"), (b, "nonsense")],
               duels=[(a, b, "quip"), (a, b, "quip"), (b, a, "fooled")])
    got = {(x["profile_id"], x["badge"]) for x in out["badges"]}
    assert got == {(a, "first_win"), (b, "clean_sweep")}
    out = play(store, [(a, 500), (b, 100)], night_wins={a: 3})
    assert [x["badge"] for x in out["badges"]] == ["hat_trick"]  # first win only once
    board = store.season_board()
    assert [(r["name"], r["points"], r["wins"], r["games"]) for r in board] == [("Ann", 1400, 2, 2), ("Bo", 400, 0, 2)]
    # This month's leader wears the crown until a month has finished.
    assert store.champion()["name"] == "Ann" and store.champion()["current"]
    assert store.rivals(a) == [{"name": "Bo", "won": 2, "lost": 1}]
    assert store.top_rivalries() == [{"a": "Ann", "b": "Bo", "a_won": 2, "b_won": 1}]
    # A new month: Bo leads it, Ann is September's champion (announced once).
    store.test_clock.t = at(2026, 10)
    out = play(store, [(b, 2000), (a, 0)])
    assert out["champion"]["name"] == "Ann" and out["champion"]["season"] == "2026-09"
    assert {x["badge"] for x in out["badges"]} == {"season_champ", "first_win"}
    assert play(store, [(b, 10), (a, 0)])["champion"] is None
    assert store.season_board()[0]["name"] == "Bo"
    champ = store.champion()
    assert champ["name"] == "Ann" and champ["label"] == "September 2026" and not champ["current"]
    assert [c["name"] for c in store.champions()] == ["Ann"]
    prof = store.profile(a)
    assert prof["games"] == 4 and prof["wins"] == 2 and prof["points"] == 1400 and prof["season_points"] == 0
    assert {x["badge"] for x in prof["badges"]} == {"first_win", "hat_trick", "season_champ"}
    fame = store.fame()
    assert fame["season"] == "October 2026" and fame["games"] == 4
    assert season_of(store.test_clock.t) == "2026-10"


def test_veteran_after_ten_games(store):
    a = store.sign_in("Ann", None)[0]
    for i in range(10):
        out = play(store, [(a, 0)])
    assert [x["badge"] for x in out["badges"]] == ["veteran"]


def test_greatest_hits_rank_by_share_of_votes_and_can_be_removed(store):
    a = store.sign_in("Ann", None)[0]
    b = store.sign_in("Bo", None)[0]
    play(store, [(a, 1), (b, 0)], hits=[
        {"kind": "quip", "prompt": "p", "text": "four of five", "profile_id": a, "votes": 4, "of": 5},
        {"kind": "lie", "prompt": "q", "text": "all three", "profile_id": b, "votes": 3, "of": 3},
        {"kind": "shirt", "text": "Nap Champion", "profile_id": a, "votes": 2, "of": 3,
         "shirt": {"color": 1, "strokes": [], "slogan": "Nap Champion"}},
    ])
    hits = store.hits()
    assert [h["text"] for h in hits] == ["all three", "four of five", "Nap Champion"]
    assert hits[0]["name"] == "Bo" and hits[2]["shirt"]["color"] == 1 and hits[1]["shirt"] is None
    store.remove_hit(hits[0]["id"])
    assert len(store.hits()) == 2
    with pytest.raises(ValueError):
        store.remove_hit(999)
    assert store.profile(a)["best"]["text"] == "four of five"


def test_rename_and_forget(store):
    a = store.sign_in("Ann", None)[0]
    b = store.sign_in("Bo", None)[0]
    play(store, [(a, 10), (b, 5)], duels=[(a, b, "quip")])
    store.rename("ann", "Annie")
    assert store.profile(a)["name"] == "Annie"
    with pytest.raises(ValueError, match="taken"):
        store.rename("Annie", "bo")
    store.forget("Bo")
    assert store.profile_by_name("Bo") is None and store.rivals(a) == []
    assert [r["name"] for r in store.season_board()] == ["Annie"]
    with pytest.raises(ValueError):
        store.forget("Bo")
    store.set_pin(a, "4321")
    with pytest.raises(PinNeeded):
        store.sign_in("Annie", None)
    with pytest.raises(ValueError):
        store.set_pin(a, "12")
