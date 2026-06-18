#!/usr/bin/env python
"""Build a labeled contact sheet of the placeholder sprites for review.
Units are shown tinted with their in-game color. Output -> target/asset_sheet.png.
"""
import os
from PIL import Image, ImageDraw, ImageChops, ImageFont

ROOT = os.path.join(os.path.dirname(__file__), "..")
SPR = os.path.join(ROOT, "assets", "sprites")
os.makedirs(os.path.join(ROOT, "target"), exist_ok=True)
OUT = os.path.join(ROOT, "target", "asset_sheet.png")

UNITS = [
    ("engineer", (210, 200, 120)),
    ("infantry", (180, 190, 170)),
    ("tank", (140, 200, 150)),
    ("artillery", (220, 150, 120)),
    ("aa", (160, 170, 220)),
    ("truck", (200, 180, 140)),
]
TILES = ["tile_ground", "tile_water", "tile_cliff", "tile_resource"]

CELL = 120
PAD = 18
LBL = 22
BG = (24, 28, 34, 255)
font = ImageFont.load_default()


def tint(img, color):
    solid = Image.new("RGBA", img.size, color + (255,))
    out = ImageChops.multiply(img, solid)
    out.putalpha(img.split()[3])
    return out


def cell_count_row(items):
    return len(items)


cols = max(len(UNITS), len(TILES))
W = cols * (CELL + PAD) + PAD
row_h = CELL + LBL + PAD
H = 2 * row_h + PAD + 30

sheet = Image.new("RGBA", (W, H), BG)
d = ImageDraw.Draw(sheet)
d.text((PAD, 8), "UNITS (tinted by faction/unit color)", fill=(230, 230, 230), font=font)


def place(name, x, y, color=None):
    img = Image.open(os.path.join(SPR, name + ".png")).convert("RGBA")
    if color:
        img = tint(img, color)
    img = img.resize((CELL, CELL), Image.NEAREST)
    sheet.alpha_composite(img, (x, y))
    label = name.replace("tile_", "")
    d.text((x + 4, y + CELL + 3), label, fill=(200, 200, 205), font=font)


y0 = 30
for i, (name, color) in enumerate(UNITS):
    place(name, PAD + i * (CELL + PAD), y0, color)

y1 = 30 + row_h
d.text((PAD, y1 - 22), "TILES", fill=(230, 230, 230), font=font)
for i, name in enumerate(TILES):
    place(name, PAD + i * (CELL + PAD), y1)

sheet.save(OUT)
print("wrote", OUT, sheet.size)
