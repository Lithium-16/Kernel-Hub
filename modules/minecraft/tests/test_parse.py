import json

import pytest

from kernel_sdk import ActionError
from mc import check_player, check_settings, linux_argv, one_line, parse_ping, parse_status

PING = json.dumps(
    {
        "version": {"name": "NeoForge 1.21.1", "protocol": 767},
        "players": {"online": 2, "max": 20, "sample": [{"name": "Steve", "id": "x"}, {"name": "Alex", "id": "y"}]},
        "description": {"text": "§aHello ", "extra": [{"text": "world"}]},
    }
)

SETTINGS = {
    "transport": "wsl",
    "distro": "Ubuntu",
    "ssh_host": "pluto",
    "server_dir": "/srv/minecraft",
    "service": "minecraft",
    "screen_dir": "/run/screen-mc",
    "screen_user": "minecraft",
    "screen_session": "mc",
    "extra_services": ["playit", "mc-notify"],
    "backup_dir": "/srv/minecraft-backups",
    "keep_backups": 1,
    "keep_wsl_alive": True,
    "refresh_s": 10.0,
    "start_timeout_s": 300.0,
}


def test_running_server_with_players():
    out = (
        "now=1000\nstate=active\nservice.playit=active\nservice.mc-notify=failed\nsince=400\n"
        "memory=2147483648\nmods=29\nbackup=900.5 1234 world-20260920T030000Z.tar.gz\n"
        f"--ping--\n{PING}\n"
    )
    s = parse_status(out)
    assert s["state"] == "running"
    assert s["players_online"] == 2 and s["players_max"] == 20
    assert s["players"] == ["Steve", "Alex"]
    assert s["motd"] == "Hello world"
    assert s["version"] == "NeoForge 1.21.1"
    assert s["uptime_s"] == 600
    assert s["memory_mb"] == 2048
    assert s["mods_count"] == 29
    assert s["services"] == {"playit": "active", "mc-notify": "failed"}
    assert s["backups"] == [{"file": "world-20260920T030000Z.tar.gz", "bytes": 1234, "at": "1970-01-01T00:15:00Z"}]
    assert s["last_backup"] == "1970-01-01T00:15:00Z"


@pytest.mark.parametrize(
    ("unit", "ping", "state"),
    [
        ("active", "--ping--\n--ping-failed--\n", "starting"),
        ("activating", "", "starting"),
        ("deactivating", "", "stopping"),
        ("inactive", "", "stopped"),
        ("failed", "", "crashed"),
        ("", "", "unknown"),
    ],
)
def test_unit_states(unit, ping, state):
    s = parse_status(f"now=1\nstate={unit}\nmemory=[not set]\n{ping}")
    assert s["state"] == state
    assert s["memory_mb"] is None
    assert s["uptime_s"] is None


def test_ping_text_fallback():
    info = parse_ping("Server online: 3/20 players")
    assert info["players_online"] == 3 and info["players_max"] == 20


def test_commands_must_be_one_line():
    assert one_line("  say hi ", "x", 10) == "say hi"
    for bad in ["", "say hi\nop me", "a\rb", "x" * 11]:
        with pytest.raises(ActionError) as e:
            one_line(bad, "x", 10)
        assert e.value.code == "invalid_params"


def test_player_names():
    assert check_player("Steve_01") == "Steve_01"
    for bad in ["ab", "a" * 17, "Steve;op", "@a"]:
        with pytest.raises(ActionError):
            check_player(bad)


def test_transports():
    assert linux_argv(SETTINGS, "bash", "-s") == ["wsl.exe", "-d", "Ubuntu", "-u", "root", "--", "bash", "-s"]
    ssh = linux_argv({**SETTINGS, "transport": "ssh"}, "bash", "-s")
    assert ssh == ["ssh", "-o", "BatchMode=yes", "pluto", "wsl -d Ubuntu -u root -- bash -s"]
    assert linux_argv({**SETTINGS, "transport": "direct"}, "bash", "-s") == ["bash", "-s"]


@pytest.mark.parametrize(
    "change",
    [
        {"transport": "telnet"},
        {"distro": "Ubuntu & calc"},
        {"ssh_host": "pluto; rm"},
        {"server_dir": "srv/minecraft"},
        {"keep_backups": 0},
        {"extra_services": ["ok", "bad name"]},
        {"screen_dir": "run/screen-mc"},
        {"screen_session": "mc -X quit"},
    ],
)
def test_rejects_bad_settings(change):
    with pytest.raises(ValueError):
        check_settings({**SETTINGS, **change})
