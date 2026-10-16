import json

from kernel_sdk.webgame import Ran, TailscaleShare, share_url, sharing_from

DNS = "pluto.tail1234.ts.net"


def status(handlers: dict[str, int], funnel=True, https=10000):
    key = f"{DNS}:{https}"
    s = {"Web": {key: {"Handlers": {p: {"Proxy": f"http://127.0.0.1:{port}"} for p, port in handlers.items()}}}}
    if funnel:
        s["AllowFunnel"] = {key: True}
    return s


def test_sharing_from_reads_the_right_path():
    both = status({"/": 8097, "/petlink": 8098})
    assert sharing_from(both, DNS, 10000, 8097) == "public"
    assert sharing_from(both, DNS, 10000, 8098, "/petlink") == "public"
    assert sharing_from(status({"/": 8097}), DNS, 10000, 8098, "/petlink") == "off"
    assert share_url(DNS, 10000, "/petlink") == f"https://{DNS}:10000/petlink"
    assert share_url(DNS, 443) == f"https://{DNS}"


class PathTailscale:
    """Keeps handlers per path, like the real `tailscale serve`."""

    def __init__(self):
        self.calls, self.handlers, self.funnel = [], {}, False

    def __call__(self, cmd, cwd=None, timeout=0, env=None):
        args = cmd[1:]
        self.calls.append(args)
        if args[:2] == ["status", "--json"]:
            return Ran(0, json.dumps({"Self": {"DNSName": DNS + "."}}))
        if args[:3] == ["serve", "status", "--json"]:
            return Ran(0, json.dumps(status(self.handlers, self.funnel) if self.handlers else {}))
        path = next((a.split("=", 1)[1] for a in args if a.startswith("--set-path=")), "/")
        if args[-1] == "off":
            self.handlers.pop(path, None)
            return Ran(0, "")
        self.handlers[path] = int(args[-1].rsplit(":", 1)[1])
        self.funnel = args[0] == "funnel"
        return Ran(0, "ok")


async def test_stopping_one_path_keeps_the_other():
    ts = PathTailscale()
    party = TailscaleShare("Party", {"share_port": 10000}, 8097, run=ts)
    pet = TailscaleShare("PetLink", {"share_port": 10000}, 8098, run=ts, path="/petlink")
    await party.share("anyone with the link")
    await pet.share("anyone with the link")
    assert ["funnel", "--bg", "--yes", "--https=10000", "--set-path=/petlink", "http://127.0.0.1:8098"] in ts.calls
    assert pet.link == f"https://{DNS}:10000/petlink"
    await party.unshare()
    assert ["funnel", "--https=10000", "--set-path=/", "off"] in ts.calls
    pet.refresh_sharing()
    assert pet.sharing == "public" and set(ts.handlers) == {"/petlink"}
