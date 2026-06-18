# Sprite credits

## Placeholder art — Kenney "Top-down Tanks Redux"
- **Author:** Kenney (kenney.nl)
- **License:** CC0 1.0 Universal (public domain — no attribution required; credited anyway)
- **Source:** https://kenney.nl/assets/top-down-tanks-redux
  (mirror: https://opengameart.org/content/top-down-tanks-redux)
- **Used for:** unit sprites (`src/tank_*.png`) and the ground tile (`src/tileGrass1.png`).

These are **placeholders** — they map onto our unit slots for now and will be replaced
with project-specific art later.

## Locally generated (CC0, self-made)
- `tile_water.png`, `tile_cliff.png`, `tile_resource.png`, `selection.png` — produced by
  `assets-tools/gen_placeholders.py`.

## Atlas
`atlas.png` is composed by `assets-tools/compose_atlas.py` from the sources above (slot order
must match `src/assets.rs`).
