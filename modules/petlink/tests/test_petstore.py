from pathlib import Path

import pytest
from petstore import Store

DAY = 86400.0


@pytest.fixture
def clock():
    return {"now": 1_000_000.0}


@pytest.fixture
def store(tmp_path, clock):
    return Store(tmp_path / "petlink.db", clock=lambda: clock["now"])


def test_users_and_tokens(store):
    me, token = store.add_user("Pattles", 210)
    assert len(token) >= 40 and store.auth(token) == me
    assert store.auth("nope") is None and store.auth("") is None
    assert me.peek is False  # off by default
    store.set_peek(me.id, True)
    assert store.user(me.id).peek is True
    assert store.remove_user(me.id) and store.auth(token) is None


def test_token_is_not_stored_in_plain_text(store, tmp_path):
    _, token = store.add_user("Pattles", 0)
    assert token.encode() not in (tmp_path / "petlink.db").read_bytes()


def test_mail_queue_is_per_recipient_and_idempotent(store):
    a, _ = store.add_user("A", 0)
    b, _ = store.add_user("B", 120)
    assert store.queue_mail("m1", a.id, b.id, {"kind": "mail", "mail_id": "m1"})
    assert not store.queue_mail("m1", a.id, b.id, {"kind": "mail", "mail_id": "m1"})
    assert [m["mail_id"] for m in store.pending_mail(b.id)] == ["m1"]
    assert store.pending_mail(a.id) == []
    store.mail_delivered("m1")
    assert store.pending_mail(b.id) == []
    assert not store.queue_mail("m1", a.id, b.id, {"kind": "mail", "mail_id": "m1"})  # still remembered
    assert store.mail_state("m1") == "delivered" and store.mail_state("nope") is None


def test_files_and_expiry(store, clock, tmp_path):
    a, _ = store.add_user("A", 0)
    b, _ = store.add_user("B", 0)
    p = tmp_path / "f1"
    p.write_bytes(b"x" * 10)
    store.add_file("f1", a.id, b.id, "cat.png", 10, p)
    assert store.file("f1").to == b.id and store.files_size() == 10
    store.queue_mail("m1", a.id, b.id, {"kind": "mail"})
    clock["now"] += 8 * DAY
    assert store.expire(clock["now"]) == [p]
    assert store.file("f1") is None and store.pending_mail(b.id) != []  # mail lives 30 days
    clock["now"] += 30 * DAY
    store.expire(clock["now"])
    assert store.pending_mail(b.id) == []


def test_install_codes_single_use_expire_and_hashed(tmp_path):
    t = [0.0]
    s = Store(tmp_path / "d.db", clock=lambda: t[0])
    u, _ = s.add_user("Ann", 0)
    code = s.add_install_code(u.id, "inv")
    assert code not in str(s.db.execute("SELECT * FROM install_codes").fetchall())
    assert s.pending_codes() == {u.id: 1}
    assert s.take_install_code(code, consume=False) == (u.id, "inv") and s.pending_codes() == {u.id: 1}
    assert s.take_install_code(code) == (u.id, "inv") and s.take_install_code(code) is None
    c2 = s.add_install_code(u.id, "inv")
    t[0] = 8 * 86400
    assert s.take_install_code(c2) is None and s.pending_codes() == {}


def test_rotate_token_kills_old_and_codes(tmp_path):
    s = Store(tmp_path / "d.db")
    u, old = s.add_user("Ann", 0)
    s.add_install_code(u.id, "inv")
    new = s.rotate_token(u.id)
    assert s.auth(old) is None and s.auth(new).id == u.id and s.pending_codes() == {}
