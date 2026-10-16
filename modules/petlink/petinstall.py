"""Where the relay gets the PetLink installer exe: the newest node-build-N release, cached and verified."""

import asyncio
import hashlib
import json
import logging
import time
import urllib.request
from collections.abc import Callable
from pathlib import Path

RELEASES = "https://api.github.com/repos/Lithium-16/Kernel-Hub/releases?per_page=30"
ASSET = "petlink-windows-x64.exe"
EVERY_S = 3600
log = logging.getLogger("petlink")


def http_fetch(url: str) -> bytes:
    req = urllib.request.Request(url, headers={"User-Agent": "kernel-hub-petlink", "Accept": "application/vnd.github+json, */*"})
    with urllib.request.urlopen(req, timeout=60) as r:
        return r.read()


def pick_latest(releases: list[dict]) -> tuple[int, str, str] | None:
    """(build, exe url, sha url) of the newest non-draft, non-prerelease node-build-N with both assets."""
    best = None
    for r in releases:
        tag = str(r.get("tag_name", ""))
        if r.get("draft") or r.get("prerelease") or not tag.startswith("node-build-") or not tag[11:].isdigit():
            continue
        urls = {a.get("name"): a.get("browser_download_url") for a in r.get("assets", [])}
        if ASSET in urls and f"{ASSET}.sha256" in urls and (best is None or int(tag[11:]) > best[0]):
            best = (int(tag[11:]), urls[ASSET], urls[f"{ASSET}.sha256"])
    return best


class ExeSource:
    def __init__(self, data_dir: Path, fetch: Callable[[str], bytes] = http_fetch, *, override: str = "",
                 clock: Callable[[], float] = time.time) -> None:
        self.dir, self.fetch, self.override, self.clock = data_dir / "installer", fetch, override, clock
        self.checked = float("-inf")
        self.lock = asyncio.Lock()

    def cached(self) -> Path | None:
        found = sorted(self.dir.glob("petlink-*.exe"), key=lambda p: int(p.stem.split("-")[1]))
        return found[-1] if found else None

    async def get(self) -> Path | None:
        if self.override:
            p = Path(self.override)
            return p if p.is_file() else None
        async with self.lock:
            if self.clock() - self.checked >= EVERY_S:
                self.checked = self.clock()  # also on failure, so GitHub isn't hammered
                try:
                    await asyncio.to_thread(self._refresh)
                except Exception as e:  # noqa: BLE001 - offline / rate-limited: keep serving the cache
                    log.warning("installer refresh failed: %s", e)
            return self.cached()

    def _refresh(self) -> None:
        latest = pick_latest(json.loads(self.fetch(RELEASES)))
        if latest is None:
            return
        build, exe_url, sha_url = latest
        target = self.dir / f"petlink-{build}.exe"
        if target.exists():
            return
        want = self.fetch(sha_url).split()[0].decode().lower()
        data = self.fetch(exe_url)
        if hashlib.sha256(data).hexdigest() != want:
            raise ValueError(f"build {build} doesn't match its SHA-256")
        self.dir.mkdir(parents=True, exist_ok=True)
        tmp = target.with_suffix(".part")
        tmp.write_bytes(data)
        tmp.replace(target)
        for old in self.dir.glob("petlink-*.exe"):
            if old != target:
                old.unlink(missing_ok=True)
