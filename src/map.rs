//! Tilemap. Phase 1: a large grid of terrain tiles with a deterministic test map.
//! Tile *types* will move to data-driven definitions in a later pass; the colors
//! here are placeholders.

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

impl TileMap {
    /// Deterministic test map: ground, a lake, a cliff border, scattered nodes.
    pub fn generate_test(width: usize, height: usize) -> Self {
        let mut tiles = vec![Tile::Ground; width * height];
        let idx = |x: usize, y: usize| y * width + x;

        for y in 0..height {
            for x in 0..width {
                let mut t = Tile::Ground;
                if x < 2 || y < 2 || x >= width - 2 || y >= height - 2 {
                    t = Tile::Cliff;
                }
                let dx = x as f32 - width as f32 * 0.32;
                let dy = y as f32 - height as f32 * 0.62;
                if (dx * dx + dy * dy).sqrt() < 16.0 {
                    t = Tile::Water;
                }
                tiles[idx(x, y)] = t;
            }
        }

        for (x, y) in [(40, 40), (210, 50), (60, 200), (200, 210), (128, 128), (90, 150), (170, 90)] {
            if x < width && y < height {
                tiles[idx(x, y)] = Tile::Resource;
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
