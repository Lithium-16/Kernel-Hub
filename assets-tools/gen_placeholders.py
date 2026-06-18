#!/usr/bin/env python
"""Generate placeholder sprite assets (CC0 / self-made) for the RTS.

Unit sprites are near-white silhouettes (tinted at runtime by faction/unit color);
tile textures are full-color. Output -> assets/sprites/*.png. Reproducible (seeded).
Run: python assets-tools/gen_placeholders.py
"""
import os
import random
from PIL import Image, ImageDraw

S = 64  # sprite canvas size
OUT = os.path.join(os.path.dirname(__file__), "..", "assets", "sprites")
os.makedirs(OUT, exist_ok=True)

MAIN = (228, 228, 232, 255)   # tints to the unit color at runtime
MID = (150, 154, 160, 255)
DARK = (34, 37, 42, 255)
OUTLINE = 3


def canvas():
    img = Image.new("RGBA", (S, S), (0, 0, 0, 0))
    return img, ImageDraw.Draw(img)


def save(img, name):
    img.save(os.path.join(OUT, name + ".png"))
    print("wrote", name + ".png")


def rrect(d, box, r, fill, outline=DARK, width=OUTLINE):
    d.rounded_rectangle(box, radius=r, fill=fill, outline=outline, width=width)


def infantry(toolbox=False):
    img, d = canvas()
    c = S // 2
    d.ellipse([c - 15, c - 15, c + 15, c + 15], fill=MAIN, outline=DARK, width=OUTLINE)
    d.ellipse([c - 7, c - 7, c + 7, c + 7], fill=MID, outline=DARK, width=2)
    d.line([c + 6, c - 12, c + 18, c - 18], fill=DARK, width=4)  # rifle
    if toolbox:
        rrect(d, [c + 6, c + 6, c + 18, c + 18], 3, MID)  # engineer kit
    return img


def tank():
    img, d = canvas()
    c = S // 2
    rrect(d, [c - 16, c - 18, c + 16, c + 18], 7, MAIN)        # hull
    d.line([c, c - 14, c, c - 30], fill=DARK, width=6)          # barrel
    d.ellipse([c - 11, c - 11, c + 11, c + 11], fill=MID, outline=DARK, width=OUTLINE)  # turret
    return img


def artillery():
    img, d = canvas()
    c = S // 2
    rrect(d, [c - 13, c - 8, c + 13, c + 18], 6, MAIN)         # chassis
    d.line([c, c + 4, c, c - 28], fill=DARK, width=6)           # long barrel
    d.ellipse([c - 8, c - 2, c + 8, c + 14], fill=MID, outline=DARK, width=2)
    return img


def aa():
    img, d = canvas()
    c = S // 2
    rrect(d, [c - 14, c - 12, c + 14, c + 16], 6, MAIN)
    d.line([c - 5, c - 6, c - 9, c - 28], fill=DARK, width=4)   # twin barrels
    d.line([c + 5, c - 6, c + 9, c - 28], fill=DARK, width=4)
    d.ellipse([c - 7, c - 7, c + 7, c + 7], fill=MID, outline=DARK, width=2)
    return img


def truck():
    img, d = canvas()
    c = S // 2
    rrect(d, [c - 12, c - 20, c + 12, c + 20], 5, MAIN)         # body
    d.line([c - 12, c - 4, c + 12, c - 4], fill=DARK, width=3)  # cab divider
    rrect(d, [c - 9, c - 18, c + 9, c - 7], 3, MID, outline=DARK, width=2)  # cab
    return img


UNIT_SPRITES = {
    "infantry": lambda: infantry(False),
    "engineer": lambda: infantry(True),
    "tank": tank,
    "artillery": artillery,
    "aa": aa,
    "truck": truck,
}


def speckle(img, n, amt):
    d = ImageDraw.Draw(img)
    w, h = img.size
    base = img.getpixel((0, 0))
    for _ in range(n):
        x, y = random.randint(0, w - 1), random.randint(0, h - 1)
        k = random.randint(-amt, amt)
        col = tuple(max(0, min(255, base[i] + k)) for i in range(3)) + (255,)
        s = random.randint(1, 3)
        d.rectangle([x, y, x + s, y + s], fill=col)


def tile(color, kind):
    img = Image.new("RGBA", (S, S), color + (255,))
    d = ImageDraw.Draw(img)
    if kind == "water":
        for i in range(5):
            y = 8 + i * 12
            d.line([4, y, 60, y + random.randint(-3, 3)], fill=(70, 110, 150, 120), width=2)
    elif kind == "cliff":
        for _ in range(7):
            x, y = random.randint(4, 50), random.randint(4, 50)
            k = random.choice([-25, 25])
            col = tuple(max(0, min(255, color[i] + k)) for i in range(3)) + (255,)
            d.polygon([(x, y), (x + 12, y + 4), (x + 6, y + 14)], fill=col)
    elif kind == "resource":
        for _ in range(14):
            x, y = random.randint(4, 58), random.randint(4, 58)
            d.ellipse([x, y, x + 4, y + 4], fill=(225, 205, 120, 255))
    speckle(img, 90, 14)
    # subtle border so the grid reads
    d.rectangle([0, 0, S - 1, S - 1], outline=(0, 0, 0, 40), width=1)
    return img


TILES = {
    "ground": ((40, 54, 44), "ground"),
    "water": ((38, 60, 92), "water"),
    "cliff": ((58, 58, 66), "cliff"),
    "resource": ((150, 120, 55), "resource"),
}


def selection_ring():
    img, d = canvas()
    d.ellipse([5, 5, S - 6, S - 6], outline=(255, 255, 255, 255), width=4)
    return img


# Fixed atlas slot order — MUST match src/assets.rs.
ATLAS_ORDER = [
    "infantry", "engineer", "tank", "artillery", "aa", "truck",
    "tile_ground", "tile_water", "tile_cliff", "tile_resource", "selection",
]
ATLAS_COLS = 4


def build_atlas():
    rows = (len(ATLAS_ORDER) + ATLAS_COLS - 1) // ATLAS_COLS
    atlas = Image.new("RGBA", (ATLAS_COLS * S, rows * S), (0, 0, 0, 0))
    for i, name in enumerate(ATLAS_ORDER):
        img = Image.open(os.path.join(OUT, name + ".png")).convert("RGBA")
        atlas.alpha_composite(img, ((i % ATLAS_COLS) * S, (i // ATLAS_COLS) * S))
    atlas.save(os.path.join(OUT, "atlas.png"))
    print("wrote atlas.png", atlas.size)


def main():
    random.seed(42)
    for name, fn in UNIT_SPRITES.items():
        save(fn(), name)
    for name, (color, kind) in TILES.items():
        save(tile(color, kind), "tile_" + name)
    save(selection_ring(), "selection")
    build_atlas()
    print("done")


if __name__ == "__main__":
    main()
