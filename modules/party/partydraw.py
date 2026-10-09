"""Party Games: drawings made on phones, checked before anyone else sees them.

A drawing is a list of strokes on a 400×400 grid (web/draw.js makes and draws them):

- `{"c": color, "w": width, "p": [x0, y0, x1, y1, …]}`: a pen line. `color` is an index into
  INK or a "#rrggbb" color; `width` is 1 to 40.
- `{"e": true, "w": width, "p": […]}`: an eraser line.
- `{"fill": color}`: paints the whole background (older drawings).
- `{"ff": color, "x": x, "y": y}`: a paint-bucket fill from that point (only the area it's in).
- a pen or eraser line with `"s": true` has straight segments (the line and box tools).

Anything else is refused, so a phone can't send a huge or broken drawing.
"""

from __future__ import annotations

import math
import random
import re
from typing import Any


class BadDrawing(ValueError):
    """A drawing that can't be accepted; the text is shown on the player's phone."""


SIZE = 400
INK = ["#1a1230", "#ffffff", "#ff5a5f", "#ffb100", "#2ec27e", "#3d7bff", "#a259ff", "#ff7fb0"]
SHIRTS = len(INK)  # shirts come in the same colors as the inks
MIN_W, MAX_W = 1, 40
MAX_STROKES = 300
MAX_POINTS = 8000
HEX = re.compile(r"^#[0-9a-fA-F]{6}$")
BROKEN = "That drawing didn't come through. Try again."


def _color(c: Any) -> int | str:
    if _int(c, 0, len(INK) - 1):
        return c
    if isinstance(c, str) and HEX.match(c):
        return c.lower()
    raise BadDrawing(BROKEN)


def check_drawing(strokes: Any, sprite: bool = False) -> list[dict[str, Any]]:
    """The drawing, cleaned, or BadDrawing with a reason a player can act on. A sprite (a
    character that stands on a background) can't have a background fill."""
    if not isinstance(strokes, list) or not strokes:
        raise BadDrawing("Draw something first.")
    if len(strokes) > MAX_STROKES:
        raise BadDrawing("That drawing has too many lines. Undo a few and send it again.")
    out: list[dict[str, Any]] = []
    points = 0
    for s in strokes:
        if not isinstance(s, dict):
            raise BadDrawing(BROKEN)
        if set(s) == {"ff", "x", "y"}:  # a paint-bucket fill from a point (fills that area only)
            if not _int(s["x"], 0, SIZE) or not _int(s["y"], 0, SIZE):
                raise BadDrawing(BROKEN)
            out.append({"ff": _color(s["ff"]), "x": s["x"], "y": s["y"]})
            continue
        if set(s) == {"fill"}:
            if sprite:
                raise BadDrawing("Characters can't have a background fill: the scene goes behind them.")
            out.append({"fill": _color(s["fill"])})
            continue
        w, p = s.get("w"), s.get("p")
        eraser = s.get("e") is True
        straight = s.get("s") is True  # straight segments (lines and boxes), not smoothed
        if set(s) - {"c", "w", "p", "e", "s"} or (not eraser and "c" not in s) or ("e" in s and not eraser):
            raise BadDrawing(BROKEN)
        if "s" in s and not straight:
            raise BadDrawing(BROKEN)
        if not _int(w, MIN_W, MAX_W) or not isinstance(p, list):
            raise BadDrawing(BROKEN)
        if len(p) < 2 or len(p) % 2 or not all(_int(v, 0, SIZE) for v in p):
            raise BadDrawing(BROKEN)
        points += len(p) // 2
        if points > MAX_POINTS:
            raise BadDrawing("That drawing is too detailed. Undo a few lines and send it again.")
        line: dict[str, Any] = {"e": True, "w": w, "p": list(p)} if eraser else {"c": _color(s["c"]), "w": w, "p": list(p)}
        if straight:
            line["s"] = True
        out.append(line)
    if all(s.get("e") for s in out):
        raise BadDrawing("Draw something first.")
    return out


def _int(v: Any, lo: int, hi: int) -> bool:
    return isinstance(v, int) and not isinstance(v, bool) and lo <= v <= hi


def doodle(rng: random.Random, sprite: bool = False) -> list[dict[str, Any]]:
    """A random squiggly drawing (for bots): a few smooth loops and waves in the inks."""
    out: list[dict[str, Any]] = []
    if not sprite and rng.random() < 0.4:
        out.append({"fill": rng.randrange(2, len(INK))})
    for _ in range(rng.randint(3, 6)):
        cx, cy = rng.randint(80, SIZE - 80), rng.randint(80, SIZE - 80)
        rx, ry = rng.randint(20, 120), rng.randint(20, 120)
        a, b, phase = rng.randint(1, 3), rng.randint(1, 3), rng.random() * math.tau
        pts: list[int] = []
        for i in range(41):
            t = i / 40 * math.tau
            pts += [min(SIZE, max(0, round(cx + rx * math.cos(a * t + phase)))),
                    min(SIZE, max(0, round(cy + ry * math.sin(b * t))))]
        out.append({"c": rng.randrange(len(INK)), "w": rng.randint(4, 16), "p": pts})
    return out
