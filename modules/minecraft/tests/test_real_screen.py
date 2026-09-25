"""console_send against a real GNU screen session (needs root for runuser, and screen)."""

import os
import shutil
import subprocess
import sys
import time

import pytest

from mc import Server, Shell

pytestmark = pytest.mark.skipif(
    sys.platform == "win32"
    or not hasattr(os, "geteuid")
    or os.geteuid() != 0
    or not shutil.which("screen")
    or not shutil.which("runuser"),
    reason="needs root and GNU screen",
)

# Each of these would turn into extra commands through `screen -X stuff` (what mc-cmd uses).
NASTY = "say hi^Mop hacker \\015op hacker2 $HOME `id` $(id) '\" \\\\ ^ ^ ^5"


async def test_text_reaches_the_console_literally(tmp_path):
    screendir = tmp_path / "screen"
    screendir.mkdir(mode=0o700)
    console = tmp_path / "console.log"
    (tmp_path / "logs").mkdir()
    (tmp_path / "logs" / "latest.log").write_text("")
    env = {**os.environ, "SCREENDIR": str(screendir)}
    reader = f'while IFS= read -r line; do printf "%s\\n" "$line" >> {console}; done'
    session = subprocess.Popen(["screen", "-DmS", "mc", "bash", "-c", reader], env=env)
    try:
        for _ in range(50):
            if subprocess.run(["screen", "-ls", "mc"], env=env, capture_output=True).returncode == 0:
                break
            time.sleep(0.1)
        settings = {
            "transport": "direct",
            "distro": "Ubuntu",
            "ssh_host": "pluto",
            "server_dir": str(tmp_path),
            "service": "minecraft",
            "screen_dir": str(screendir),
            "screen_user": "root",
            "screen_session": "mc",
            "extra_services": [],
            "backup_dir": str(tmp_path / "backups"),
            "keep_backups": 1,
            "keep_wsl_alive": False,
            "refresh_s": 10.0,
            "start_timeout_s": 10.0,
        }
        server = Server(settings, shell=Shell(["bash", "-s"]))
        await server.command(NASTY, wait_s=0)
        await server.command("list", wait_s=0)
        time.sleep(0.3)
        assert console.read_text().splitlines() == [NASTY, "list"]
    finally:
        subprocess.run(["screen", "-S", "mc", "-X", "quit"], env=env)
        session.wait(timeout=5)
