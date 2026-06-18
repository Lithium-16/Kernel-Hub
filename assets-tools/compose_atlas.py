#!/usr/bin/env python
"""Compose the sprite atlas from sourced CC0 art (Kenney "Top-down Tanks Redux", CC0)
for units + ground, plus the locally generated tiles for water/cliff/resource and the
selection ring. Output -> assets/sprites/atlas.png. Run after gen_placeholders.py.

Slot order MUST match src/assets.rs (UNIT_ORDER then ground/water/cliff/resource/selection).
"""
import os
from PIL import Image

ROOT = os.path.join(os.path.dirname(__file__), "..")
SPR = os.path.join(ROOT, "assets", "sprites")
SRC = os.path.join(SPR, "src")
CELL = 64
COLS = 4

SLOTS = [
    os.path.join(SRC, "tank_green.png"),     # 0 infantry
    os.path.join(SRC, "tank_sand.png"),      # 1 engineer
    os.path.join(SRC, "tank_dark.png"),      # 2 tank
    os.path.join(SRC, "tank_bigRed.png"),    # 3 artillery
    os.path.join(SRC, "tank_blue.png"),      # 4 aa
    os.path.join(SRC, "tank_huge.png"),      # 5 truck
    os.path.join(SRC, "tileGrass1.png"),     # 6 ground (Kenney)
    os.path.join(SPR, "tile_water.png"),     # 7 water (generated)
    os.path.join(SPR, "tile_cliff.png"),     # 8 cliff (generated)
    os.path.join(SPR, "tile_resource.png"),  # 9 resource (generated)
    os.path.join(SPR, "selection.png"),      # 10 selection (generated)
]


def fit(path):
    """Resize to fit a CELL×CELL cell, preserving aspect, centered on transparency."""
    img = Image.open(path).convert("RGBA")
    img.thumbnail((CELL, CELL), Image.LANCZOS)
    cell = Image.new("RGBA", (CELL, CELL), (0, 0, 0, 0))
    cell.alpha_composite(img, ((CELL - img.width) // 2, (CELL - img.height) // 2))
    return cell


def main():
    rows = (len(SLOTS) + COLS - 1) // COLS
    atlas = Image.new("RGBA", (COLS * CELL, rows * CELL), (0, 0, 0, 0))
    for i, p in enumerate(SLOTS):
        atlas.alpha_composite(fit(p), ((i % COLS) * CELL, (i // COLS) * CELL))
    out = os.path.join(SPR, "atlas.png")
    atlas.save(out)
    print("wrote", out, atlas.size)


if __name__ == "__main__":
    main()
