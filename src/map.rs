//! Tilemap. Organic terrain from value-noise fbm (lakes, cliff ridges, ore clusters),
//! with a guaranteed-clear central battlefield so spawns and the opening fight stay on
//! solid ground while the map edges get detail. Deterministic (no per-run randomness),
//! so tests and the noise stay stable.

use macroquad::prelude::*;

/// World-pixel size of one tile.
pub const TILE_SIZE: f32 = 32.0;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tile {
    Ground,
    Water,
    Cliff,
    Resource,
}

pub struct TileMap {
    pub width: usize,
    pub height: usize,
    tiles: Vec<Tile>,
}

/// Deterministic 2D integer hash → 0..1.
fn hash01(x: i32, y: i32) -> f32 {
    let mut h = (x as u32).wrapping_mul(374761393) ^ (y as u32).wrapping_mul(2246822519);
    h = h.wrapping_add(3266489917);
    h = (h ^ (h >> 15)).wrapping_mul(2246822519);
    h = (h ^ (h >> 13)).wrapping_mul(3266489917);
    ((h ^ (h >> 16)) & 0xffff) as f32 / 65535.0
}

/// Smoothed value noise at a continuous point.
fn value_noise(x: f32, y: f32) -> f32 {
    let (xi, yi) = (x.floor(), y.floor());
    let (xf, yf) = (x - xi, y - yi);
    let (x0, y0) = (xi as i32, yi as i32);
    let sx = xf * xf * (3.0 - 2.0 * xf);
    let sy = yf * yf * (3.0 - 2.0 * yf);
    let v00 = hash01(x0, y0);
    let v10 = hash01(x0 + 1, y0);
    let v01 = hash01(x0, y0 + 1);
    let v11 = hash01(x0 + 1, y0 + 1);
    let a = v00 + (v10 - v00) * sx;
    let b = v01 + (v11 - v01) * sx;
    a + (b - a) * sy
}

/// 3-octave fractal noise, ~0..1.
fn fbm(x: f32, y: f32) -> f32 {
    value_noise(x, y) * 0.6 + value_noise(x * 2.1 + 5.0, y * 2.1 + 5.0) * 0.3 + value_noise(x * 4.3 + 9.0, y * 4.3 + 9.0) * 0.1
}

/// Small deterministic per-tile brightness jitter (-1..1) — used by the renderer to
/// break up flat terrain so it doesn't look like one solid color.
pub fn tile_jitter(x: usize, y: usize) -> f32 {
    hash01(x as i32 ^ 0x5bd1, y as i32 ^ 0x9e37) * 2.0 - 1.0
}

impl TileMap {
    /// Noise terrain with a clear central battlefield (radius in tiles) so units always
    /// spawn and open the fight on passable ground.
    pub fn generate_test(width: usize, height: usize) -> Self {
        let mut tiles = vec![Tile::Ground; width * height];
        let idx = |x: usize, y: usize| y * width + x;
        let (cx, cy) = (width as f32 * 0.5, height as f32 * 0.5);
        let clearing = 28.0;

        for y in 0..height {
            for x in 0..width {
                let e = fbm(x as f32 * 0.045, y as f32 * 0.045);
                let mut t = if e < 0.36 {
                    Tile::Water
                } else if e > 0.72 {
                    Tile::Cliff
                } else {
                    Tile::Ground
                };
                if t == Tile::Ground {
                    let r = value_noise(x as f32 * 0.08 + 100.0, y as f32 * 0.08 + 70.0);
                    if r > 0.80 {
                        t = Tile::Resource;
                    }
                }
                // Solid cliff border.
                if x < 2 || y < 2 || x >= width - 2 || y >= height - 2 {
                    t = Tile::Cliff;
                }
                // Keep the central battlefield free of water/cliff.
                let in_border = x < 2 || y < 2 || x >= width - 2 || y >= height - 2;
                let dist = ((x as f32 - cx).powi(2) + (y as f32 - cy).powi(2)).sqrt();
                if dist < clearing && !in_border && (t == Tile::Water || t == Tile::Cliff) {
                    t = Tile::Ground;
                }
                tiles[idx(x, y)] = t;
            }
        }

        Self { width, height, tiles }
    }

    pub fn get(&self, x: usize, y: usize) -> Tile {
        self.tiles[y * self.width + x]
    }

    /// Map size in world pixels.
    pub fn size_px(&self) -> Vec2 {
        vec2(self.width as f32 * TILE_SIZE, self.height as f32 * TILE_SIZE)
    }
}

pub fn tile_color(t: Tile) -> Color {
    match t {
        Tile::Ground => Color::from_rgba(40, 54, 44, 255),
        Tile::Water => Color::from_rgba(38, 60, 92, 255),
        Tile::Cliff => Color::from_rgba(58, 58, 66, 255),
        Tile::Resource => Color::from_rgba(190, 165, 90, 255),
    }
}
