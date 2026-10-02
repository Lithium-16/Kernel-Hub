import asyncio
import io
import json
import shutil
import socket
import tarfile
import urllib.request

import pytest
from flowrace import FlowRace, Ran, http_get, run_command, share_url, sharing_from
from kernel_sdk import ActionError

DNS = "pluto.tail1234.ts.net"


def serve_status(port=8095, funnel=True, https=443):
    key = f"{DNS}:{https}"
    s = {"TCP": {str(https): {"HTTPS": True}}, "Web": {key: {"Handlers": {"/": {"Proxy": f"http://127.0.0.1:{port}"}}}}}
    if funnel:
        s["AllowFunnel"] = {key: True}
    return s


def test_reads_tailscale_serve_status():
    assert sharing_from(serve_status(), DNS + ".", 443, 8095) == "public"
    assert sharing_from(serve_status(funnel=False), DNS, 443, 8095) == "tailnet"
    assert sharing_from(serve_status(port=3000), DNS, 443, 8095) == "off"  # something else on 443
    assert sharing_from({}, DNS, 443, 8095) == "off"
    assert share_url(DNS + ".", 443) == f"https://{DNS}"
    assert share_url(DNS, 8443) == f"https://{DNS}:8443"


class FakeTailscale:
    def __init__(self, funnel_allowed=True):
        self.calls = []
        self.status = {}
        self.funnel_allowed = funnel_allowed

    def __call__(self, cmd, cwd=None, timeout=0, env=None):
        args = cmd[1:]
        self.calls.append(args)
        if args[:2] == ["status", "--json"]:
            return Ran(0, json.dumps({"Self": {"DNSName": DNS + "."}}))
        if args[:3] == ["serve", "status", "--json"]:
            return Ran(0, json.dumps(self.status))
        if args[-1] == "off":
            self.status = {}
            return Ran(0, "")
        if args[0] == "funnel" and not self.funnel_allowed:
            return Ran(1, "Funnel is not enabled on your tailnet.\nTo enable, visit:\n\n  https://login.tailscale.com/f/funnel?node=abc\n")
        self.status = serve_status(funnel=args[0] == "funnel")
        return Ran(0, "Available on the internet:\nhttps://...")


async def test_share_with_anyone_then_the_tailnet_then_stop(tmp_path):
    ts = FakeTailscale()
    events = []
    g = FlowRace({"tailscale": "tailscale"}, tmp_path, emit=lambda k, m, **d: events.append(k), run=ts)
    out = await g.share("anyone with the link")
    assert out["link"] == f"https://{DNS}" and g.sharing == "public"
    assert ["funnel", "--bg", "--yes", "--https=443", "http://127.0.0.1:8095"] in ts.calls
    assert ["serve", "--https=443", "off"] in ts.calls  # the other kind is switched off first
    await g.share("my tailnet")
    assert g.sharing == "tailnet"
    await g.unshare()
    assert (g.sharing, g.link) == ("off", "")
    assert events == ["share.on", "share.on", "share.off"]
    with pytest.raises(ActionError):
        await g.share("everyone")


async def test_share_explains_when_funnel_needs_turning_on(tmp_path):
    g = FlowRace({}, tmp_path, run=FakeTailscale(funnel_allowed=False))
    with pytest.raises(ActionError) as e:
        await g.share("anyone with the link")
    assert "https://login.tailscale.com/f/funnel?node=abc" in e.value.message


def test_needs_a_recent_node(tmp_path):
    old = FlowRace({}, tmp_path, run=lambda cmd, **k: Ran(0, "v20.11.1\n"))
    with pytest.raises(ActionError) as e:
        old.check_node()
    assert "22.18" in e.value.message
    missing = FlowRace({}, tmp_path, run=lambda cmd, **k: Ran(127, "node not found"))
    with pytest.raises(ActionError) as e:
        missing.check_node()
    assert "winget" in e.value.message
    assert FlowRace({}, tmp_path, run=lambda cmd, **k: Ran(0, "v24.1.0")).check_node() == "v24.1.0"


# -- install, run, crash, update: a stand-in game served by real Node ---------------------

# Plain CommonJS so any Node runs it. Kernel's host can't run against it (no game core), so
# this also covers falling back to the game's own server.
FAKE_MAIN = """
const http = require('http');
if (process.env.CRASH_FILE && require('fs').existsSync(process.env.CRASH_FILE)) process.exit(3);
http.createServer((req, res) => res.end(req.url === '/health' ? 'ok' : 'game ' + process.env.VERSION))
  .listen(Number(process.env.PORT), '127.0.0.1');
process.stdin.on('end', () => process.exit(0));
process.stdin.resume();
"""


def tarball(sha):
    buf = io.BytesIO()
    with tarfile.open(fileobj=buf, mode="w:gz") as tar:
        for name, text in {"package.json": '{"name": "flow-race"}', "server/main.ts": FAKE_MAIN}.items():
            data = text.encode()
            info = tarfile.TarInfo(f"Lithium-16-FlowRace-{sha[:7]}/{name}")
            info.size = len(data)
            tar.addfile(info, io.BytesIO(data))
    return buf.getvalue()


def free_port():
    with socket.socket() as s:
        s.bind(("127.0.0.1", 0))
        return s.getsockname()[1]


class FakeGitHub:
    def __init__(self):
        self.sha = "a" * 40
        self.seen = []

    def __call__(self, url, timeout=5, headers=None):
        if "api.github.com" not in url:
            return http_get(url, timeout, headers)
        self.seen.append((url, headers))
        if "/commits/" in url:
            return self.sha.encode()
        return tarball(url.rsplit("/", 1)[1])


def fake_npm(cmd, cwd=None, timeout=0, env=None):
    if "npm" in str(cmd[0]):
        if cmd[1:3] == ["run", "build"]:
            (cwd / "public").mkdir(exist_ok=True)
            (cwd / "public" / "app.js").write_text("// built")
        return Ran(0, "ok")
    return run_command(cmd, cwd=cwd, timeout=timeout, env=env)


@pytest.mark.skipif(shutil.which("node") is None, reason="needs Node")
async def test_installs_runs_restarts_and_updates(tmp_path, monkeypatch):
    gh = FakeGitHub()
    events = []
    settings = {"repo": "Lithium-16/FlowRace", "ref": "main", "port": free_port(), "npm": "npm", "github_token": "t0k"}
    g = FlowRace(settings, tmp_path, emit=lambda k, m, **d: events.append(k), run=fake_npm, get=gh)
    monkeypatch.setattr(g, "check_node", lambda: "v22.18.0")
    monkeypatch.setenv("CRASH_FILE", str(tmp_path / "crash"))

    out = await g.install()
    assert out == {"version": "aaaaaaa", "updated": True, "was": ""}
    assert g.snapshot()["state"] == "running" and g.mode == "standalone"
    assert not g.snapshot()["saves_ratings"]
    assert gh.seen[0][1]["Authorization"] == "Bearer t0k"
    assert "game.fallback" in events
    assert urllib.request.urlopen(f"{g.local_url()}/health", timeout=2).read() == b"ok"

    # Up to date: nothing to do.
    assert (await g.install())["updated"] is False

    # It crashes: it comes back.
    g.process.kill()
    g.process.wait()
    await g.watch_process()
    assert "game.crashed" in events and g.process is None
    g.retry_at = 0
    await g.watch_process()
    assert g.process_alive()

    # A new commit: built next to the old one, then switched to.
    gh.sha = "b" * 40
    await g.check_update()
    assert g.snapshot()["update_available"] and "game.update_available" in events
    out = await g.install()
    assert out["version"] == "bbbbbbb" and out["was"] == "aaaaaaa"
    assert sorted(p.name for p in (tmp_path / "versions").iterdir()) == ["a" * 40, "b" * 40]
    assert g.process_alive() and not g.snapshot()["update_available"]

    # Stopped on purpose: stays stopped.
    await g.stop()
    await g.watch_process()
    assert g.process is None and g.snapshot()["state"] == "stopped"

    # Keeps failing: gives up after 5 tries in 10 minutes.
    (tmp_path / "crash").write_text("1")
    with pytest.raises(ActionError):
        await g.start()
    for _ in range(6):
        g.retry_at = 0
        await g.watch_process()
    assert not g.want_running and "game.gave_up" in events


async def test_update_waits_for_players(tmp_path):
    g = FlowRace({"repo": "x/y"}, tmp_path)
    g.process = type("P", (), {"poll": lambda self: None})()
    g.game = {"online": 3}
    with pytest.raises(ActionError) as e:
        await g.install()
    assert "3 playing" in e.value.message
    await asyncio.sleep(0)


def test_manifest_matches_the_handlers():
    from pathlib import Path

    from kernel_sdk.manifest import load_manifest

    m = load_manifest(Path(__file__).resolve().parent.parent / "module.toml")
    assert m.action("share.start").ai == "never"
