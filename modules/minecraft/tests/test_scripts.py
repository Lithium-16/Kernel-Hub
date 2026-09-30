"""Runs the real bash scripts against a fake server: a fake folder, fake systemctl, screen
and runuser, and a small TCP server that answers the Minecraft status ping."""

import gzip
import json
import os
import shutil
import socket
import socketserver
import struct
import subprocess
import sys
import textwrap
import threading
import time
from pathlib import Path

import pytest

from kernel_sdk import ActionError
from mc import Server, Shell

pytestmark = pytest.mark.skipif(
    sys.platform == "win32" or shutil.which("bash") is None, reason="needs a Linux shell (GNU coreutils)"
)

STATUS = {
    "version": {"name": "NeoForge 1.21.1", "protocol": 767},
    "players": {"online": 1, "max": 20, "sample": [{"name": "Steve", "id": "0"}]},
    "description": {"text": "Kernel"},
}

FAKES = {
    # The unit's state lives in $FAKE/state.
    "systemctl": r"""
        case "$1" in
          is-active)
            [ "$2" = "--quiet" ] && unit=$3 || unit=$2
            if [ "$unit" = "$FAKE_SERVICE" ]; then s=$(cat "$FAKE/state"); else s=active; fi
            [ "$2" = "--quiet" ] || echo "$s"
            [ "$s" = active ] ;;
          show)
            case "$4" in
              ActiveEnterTimestamp) echo "Thu 2026-09-24 20:00:00 UTC" ;;
              MemoryCurrent) echo 1073741824 ;;
            esac ;;
          start|restart) echo active > "$FAKE/state" ;;
          stop) echo inactive > "$FAKE/state" ;;
        esac
    """,
    "runuser": r"""
        [ "$1" = "-u" ] && { echo "$2" >> "$FAKE/runuser"; shift 2; }
        [ "$1" = "--" ] && shift
        exec "$@"
    """,
    # readbuf copies the file; paste "types" it into the console, one command per line.
    "screen": r"""
        echo "SCREENDIR=$SCREENDIR $*" >> "$FAKE/screen-calls"
        while [ $# -gt 0 ] && [ "$1" != "-X" ]; do shift; done
        shift
        case "$1" in
          readbuf) cp "$2" "$FAKE/buffer" ;;
          paste)
            line=$(tr -d '\r' < "$FAKE/buffer")
            printf '[12:00:00] [Server thread/INFO] [minecraft/DedicatedServer]: ran: %s\n' "$line" >> "$FAKE_LOG"
            printf '%s
' "$line" >> "$FAKE/commands"
            if [ "$line" = "save-all flush" ] && [ ! -e "$FAKE/no-save" ]; then
              echo "[12:00:00] [Server thread/INFO] [minecraft/MinecraftServer]: Saved the game" >> "$FAKE_LOG"
            fi ;;
        esac
    """,
}


def read_varint(f) -> int:
    n = shift = 0
    while True:
        b = f.read(1)
        if not b:
            raise EOFError
        n |= (b[0] & 0x7F) << shift
        shift += 7
        if not b[0] & 0x80:
            return n


def varint(n: int) -> bytes:
    out = b""
    while True:
        b, n = n & 0x7F, n >> 7
        out += bytes([b | (0x80 if n else 0)])
        if not n:
            return out


def ping_server(fake: Path) -> socketserver.TCPServer:
    """Answers the status ping like a Minecraft server, unless $FAKE/no-ping exists."""

    class Handler(socketserver.StreamRequestHandler):
        def handle(self):
            if (fake / "no-ping").exists():
                return
            for _ in range(2):  # handshake, then status request
                self.rfile.read(read_varint(self.rfile))
            body = json.dumps(STATUS).encode()
            data = varint(0) + varint(len(body)) + body
            self.wfile.write(varint(len(data)) + data)

    server = socketserver.ThreadingTCPServer(("127.0.0.1", 0), Handler)
    server.daemon_threads = True
    threading.Thread(target=server.serve_forever, daemon=True).start()
    return server


@pytest.fixture
def env(tmp_path):
    fake = tmp_path / "fake"
    bin_dir = fake / "bin"
    bin_dir.mkdir(parents=True)
    for name, body in FAKES.items():
        path = bin_dir / name
        path.write_text("#!/usr/bin/env bash\n" + textwrap.dedent(body))
        path.chmod(0o755)
    (fake / "state").write_text("active\n")
    pinger = ping_server(fake)

    mc = tmp_path / "srv" / "minecraft"
    (mc / "world").mkdir(parents=True)
    (mc / "logs").mkdir()
    (mc / "mods").mkdir()
    (mc / "world" / "level.dat").write_bytes(gzip.compress(b"\x0a\x00\x00fake nbt"))
    (mc / "world" / "region.mca").write_bytes(os.urandom(4096))
    (mc / "server.properties").write_text(f"level-name=world\nserver-port={pinger.server_address[1]}\n")
    (mc / "logs" / "latest.log").write_text("[11:59:59] [main/INFO]: Done (42.0s)!\n")
    (mc / "whitelist.json").write_text(json.dumps([{"uuid": "1", "name": "steve"}, {"uuid": "2", "name": "Alex"}]))
    for jar in ("create-6.0.jar", "terralith-2.5.jar"):
        (mc / "mods" / jar).write_bytes(b"PK")
    backups = tmp_path / "backups"

    shell_env = {
        **os.environ,
        "PATH": f"{bin_dir}:{os.environ['PATH']}",
        "FAKE": str(fake),
        "FAKE_SERVICE": "minecraft",
        "FAKE_LOG": str(mc / "logs" / "latest.log"),
    }
    settings = {
        "transport": "direct",
        "distro": "Ubuntu",
        "ssh_host": "pluto",
        "server_dir": str(mc),
        "service": "minecraft",
        "screen_dir": "/run/screen-mc",
        "screen_user": "minecraft",
        "screen_session": "mc",
        "extra_services": ["playit", "mc-notify"],
        "backup_dir": str(backups),
        "keep_backups": 1,
        "keep_wsl_alive": True,
        "refresh_s": 10.0,
        "start_timeout_s": 10.0,
    }
    server = Server(settings, shell=Shell(["bash", "-s"], env=shell_env))
    server.save_timeout_s = 2
    yield server, fake, mc, backups
    pinger.shutdown()
    pinger.server_close()


async def test_status_of_a_running_server(env):
    server, *_ = env
    s = await server.refresh()
    assert s["state"] == "running"
    assert s["players"] == ["Steve"]
    assert s["motd"] == "Kernel"
    assert s["mods_count"] == 2
    assert s["memory_mb"] == 1024
    assert s["services"] == {"playit": "active", "mc-notify": "active"}
    assert s["uptime_s"] is not None
    assert "keepalive" not in server.status()  # direct transport never needs one


async def test_stop_and_start(env):
    server, fake, *_ = env
    got = []
    server.emit = lambda kind, message, level="info", **data: got.append((kind, level))
    await server.refresh()
    assert (await server.stop())["state"] == "stopped"
    assert (await server.start())["state"] == "running"
    assert got == [("server.stopped", "info"), ("server.started", "info")]
    (fake / "no-ping").touch()
    await server.stop()
    with pytest.raises(ActionError) as e:
        server.settings["start_timeout_s"] = 0.1
        await server.start()
    assert e.value.code == "timeout"


async def test_command_returns_new_log_lines(env):
    server, fake, *_ = env
    r = await server.command("/list", wait_s=0)
    assert r["command"] == "list"
    assert r["output"] == ["[12:00:00] [Server thread/INFO] [minecraft/DedicatedServer]: ran: list"]
    await server.say("hi there")
    assert (fake / "commands").read_text().splitlines() == ["list", "say hi there"]
    # Pasted into the server's own screen session, as the server's user.
    assert set((fake / "runuser").read_text().split()) == {"minecraft"}
    calls = (fake / "screen-calls").read_text().splitlines()
    assert all(c.startswith("SCREENDIR=/run/screen-mc -S mc -p 0 -X ") for c in calls)


async def test_values_cannot_escape_the_script(env):
    server, fake, *_ = env
    text = "say $(touch pwned) `id` '; echo x ^M \\015 $HOME"
    await server.command(text, wait_s=0)
    assert (fake / "commands").read_text().splitlines() == [text]
    assert not Path("pwned").exists()


async def test_tail_whitelist_and_mods(env):
    server, *_ = env
    assert (await server.tail(5))["lines"] == ["[11:59:59] [main/INFO]: Done (42.0s)!"]
    assert (await server.whitelist())["players"] == ["Alex", "steve"]
    mods = await server.mods()
    assert mods["count"] == 2
    assert mods["mods"][0] == {"file": "create-6.0.jar", "bytes": 2}


async def test_backup_verifies_then_replaces_the_old_one(env):
    server, fake, mc, backups = env
    backups.mkdir()
    old = backups / "world-20200101T000000Z.tar.gz"
    old.write_bytes(b"old")
    os.utime(old, (time.time() - 3600, time.time() - 3600))

    got = []
    server.emit = lambda kind, message, level="info", **data: got.append((kind, level))
    r = await server.backup()
    assert got == [("backup.done", "info")]
    assert r["file"].startswith("world-") and r["file"].endswith(".tar.gz")
    assert r["bytes"] > 0
    assert r["deleted"] == [old.name]
    assert [p.name for p in backups.iterdir()] == [r["file"]]
    assert (fake / "commands").read_text().splitlines() == ["save-off", "save-all flush", "save-on"]
    assert server.snapshot["last_backup"] is not None


async def test_backup_that_fails_to_verify_keeps_the_old_one(env):
    server, fake, mc, backups = env
    backups.mkdir()
    (backups / "world-20200101T000000Z.tar.gz").write_bytes(b"old")
    (mc / "world" / "level.dat").write_bytes(b"not gzip")
    got = []
    server.emit = lambda kind, message, level="info", **data: got.append((kind, level))
    with pytest.raises(ActionError, match="did not verify"):
        await server.backup()
    assert got == [("backup.failed", "error")]
    assert [p.name for p in backups.iterdir()] == ["world-20200101T000000Z.tar.gz"]
    assert (fake / "commands").read_text().splitlines()[-1] == "save-on"


async def test_backup_waits_for_the_save_and_always_turns_saving_back_on(env):
    server, fake, mc, backups = env
    (fake / "no-save").touch()
    with pytest.raises(ActionError, match="did not finish saving"):
        await server.backup()
    assert (fake / "commands").read_text().splitlines() == ["save-off", "save-all flush", "save-on"]
    assert not backups.exists() or not any(backups.iterdir())


async def test_backup_of_a_stopped_server_skips_the_save_commands(env):
    server, fake, mc, backups = env
    await server.stop()
    r = await server.backup()
    assert r["file"] is not None
    assert not (fake / "commands").exists()


async def test_only_one_lifecycle_operation_at_a_time(env):
    server, *_ = env
    async with server.exclusive():
        with pytest.raises(ActionError) as e:
            await server.backup()
    assert e.value.code == "busy"


async def test_unknown_helper_service_is_rejected(env):
    server, *_ = env
    with pytest.raises(ActionError) as e:
        await server.restart_service("sshd")
    assert e.value.code == "invalid_params"
    assert (await server.restart_service("playit"))["state"] == "active"
