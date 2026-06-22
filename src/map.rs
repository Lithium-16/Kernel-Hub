//! Tilemap with strategic geography. The map is structured — not pure noise — so terrain
//! creates real decisions: mountain ridges force units through passes, rivers require
//! crossings, ore basins and oil fields give expansion targets.
//!
//! Layout (256×256): player spawns bottom-left, enemy top-right. A mountain ridge runs
//! diagonally through the middle with 1–2 passes. A river runs roughly N–S with crossings.
//! Ore basins and oil fields are distributed across the map.

use macroquad::prelude::*;

pub const TILE_SIZE: f32 = 32.0;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tile {
    Ground,
    Water,       // impassable (lakes/sea)
    Cliff,       // impassable (mountains, border)
    OreBasin,    // strategic — passable ground, marks an ore region
    OilField,    // strategic — passable ground, marks an oil region
    MountainPass,// narrow passage through the cliff ridge — slow movement
    River,       // impassable water barrier
    RiverCrossing, // passable river tile (bridge/ford) — slow
    Chokepoint,  // regular ground but flagged as a chokepoint for AI/UI
    Road,        // built by engineers later; fast movement
}

impl Tile {
    pub fn passable(self) -> bool {
        !matches!(self, Tile::Cliff | Tile::Water | Tile::River)
    }

    /// Movement cost multiplier (1.0 = normal ground speed).
    pub fn move_cost(self) -> f32 {
        match self {
            Tile::Road           => 0.5,  // roads are faster
            Tile::Ground         => 1.0,
            Tile::OreBasin       => 1.1,
            Tile::OilField       => 1.1,
            Tile::Chokepoint     => 1.0,
            Tile::MountainPass   => 2.0,  // passes are slow
            Tile::RiverCrossing  => 2.5,  // crossings are very slow
            Tile::Water | Tile::River | Tile::Cliff => f32::INFINITY,
        }
    }

    pub fn is_resource(self) -> bool {
        matches!(self, Tile::OreBasin | Tile::OilField)
    }
}

pub struct TileMap {
    pub width: usize,
    pub height: usize,
    tiles: Vec<Tile>,
}

/// Named strategic regions on this map.
#[derive(Clone)]
pub struct Region {
    pub name: &'static str,
    /// Center tile coordinate.
    pub cx: usize,
    pub cy: usize,
    pub kind: RegionKind,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum RegionKind { OreBasin, OilField, Pass, Valley, Chokepoint }

// ── noise helpers ─────────────────────────────────────────────────────────────

fn hash01(x: i32, y: i32) -> f32 {
    let mut h = (x as u32).wrapping_mul(374761393) ^ (y as u32).wrapping_mul(2246822519);
    h = h.wrapping_add(3266489917);
    h = (h ^ (h >> 15)).wrapping_mul(2246822519);
    h = (h ^ (h >> 13)).wrapping_mul(3266489917);
    ((h ^ (h >> 16)) & 0xffff) as f32 / 65535.0
}

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

fn fbm(x: f32, y: f32) -> f32 {
    value_noise(x, y) * 0.6
        + value_noise(x * 2.1 + 5.0, y * 2.1 + 5.0) * 0.3
        + value_noise(x * 4.3 + 9.0, y * 4.3 + 9.0) * 0.1
}

pub fn tile_jitter(x: usize, y: usize) -> f32 {
    hash01(x as i32 ^ 0x5bd1, y as i32 ^ 0x9e37) * 2.0 - 1.0
}

// ── map generation ─────────────────────────────────────────────────────────────

impl TileMap {
    pub fn generate(width: usize, height: usize) -> (Self, Vec<Region>) {
        let mut tiles = vec![Tile::Ground; width * height];
        let mut regions: Vec<Region> = Vec::new();
        let idx = |x: usize, y: usize| y * width + x;
        let w = width as f32;
        let h = height as f32;

        // ── 1. Base noise terrain ──────────────────────────────────────────
        for y in 0..height {
            for x in 0..width {
                let e = fbm(x as f32 * 0.04, y as f32 * 0.04);
                tiles[idx(x, y)] = if e < 0.30 { Tile::Water } else { Tile::Ground };
            }
        }

        // ── 2. Mountain ridge (diagonal, top-left to bottom-right midpoint) ─
        // Ridge runs from (0.15w, 0.55h) to (0.55w, 0.15h) — cuts across the map.
        // Cells within `ridge_half` tiles of the line become Cliff.
        let ridge_half = 5i32;
        let (rx0, ry0) = (w * 0.15, h * 0.55);
        let (rx1, ry1) = (w * 0.55, h * 0.15);
        let rdx = rx1 - rx0;
        let rdy = ry1 - ry0;
        let rlen2 = rdx * rdx + rdy * rdy;
        for y in 0..height {
            for x in 0..width {
                let px = x as f32 - rx0;
                let py = y as f32 - ry0;
                let t = ((px * rdx + py * rdy) / rlen2).clamp(0.0, 1.0);
                let nx = rx0 + t * rdx;
                let ny = ry0 + t * rdy;
                let dist = ((x as f32 - nx).powi(2) + (y as f32 - ny).powi(2)).sqrt();
                if dist < ridge_half as f32 {
                    tiles[idx(x, y)] = Tile::Cliff;
                }
            }
        }

        // ── 3. Mountain passes (two gaps in the ridge) ─────────────────────
        // Pass 1: ~1/3 along the ridge
        let pass1_t = 0.33f32;
        let (p1x, p1y) = (
            (rx0 + pass1_t * rdx) as usize,
            (ry0 + pass1_t * rdy) as usize,
        );
        let pass_half = 2i32;
        carve_pass(&mut tiles, p1x, p1y, pass_half, width, height);
        regions.push(Region { name: "Northern Pass", cx: p1x, cy: p1y, kind: RegionKind::Pass });

        // Pass 2: ~2/3 along the ridge
        let pass2_t = 0.67f32;
        let (p2x, p2y) = (
            (rx0 + pass2_t * rdx) as usize,
            (ry0 + pass2_t * rdy) as usize,
        );
        carve_pass(&mut tiles, p2x, p2y, pass_half, width, height);
        regions.push(Region { name: "Southern Pass", cx: p2x, cy: p2y, kind: RegionKind::Pass });

        // Label pass tiles as MountainPass
        for y in 0..height {
            for x in 0..width {
                if tiles[idx(x, y)] == Tile::Ground {
                    // Check if surrounded by cliffs on ridge direction
                    let near_p1 = dist2(x, y, p1x, p1y) < 25;
                    let near_p2 = dist2(x, y, p2x, p2y) < 25;
                    if near_p1 || near_p2 {
                        tiles[idx(x, y)] = Tile::MountainPass;
                    }
                }
            }
        }

        // ── 4. River (roughly N–S, west side of map) ───────────────────────
        let river_x_base = (w * 0.28) as i32;
        for y in 0..height {
            // Meander the river with noise
            let meander = (value_noise(0.0, y as f32 * 0.03) * 2.0 - 1.0) * 8.0;
            let rx = (river_x_base + meander as i32).clamp(4, width as i32 - 5) as usize;
            // 1-tile-wide river
            if tiles[idx(rx, y)] != Tile::Cliff {
                tiles[idx(rx, y)] = Tile::River;
            }
        }

        // River crossings at 1/3 and 2/3 height
        let cross1_y = (h * 0.33) as usize;
        let cross2_y = (h * 0.67) as usize;
        for &cy in &[cross1_y, cross2_y] {
            let meander = (value_noise(0.0, cy as f32 * 0.03) * 2.0 - 1.0) * 8.0;
            let rx = (river_x_base + meander as i32).clamp(4, width as i32 - 5) as usize;
            for dy in -1i32..=1 {
                let ry = (cy as i32 + dy).clamp(0, height as i32 - 1) as usize;
                if rx < width {
                    tiles[idx(rx, ry)] = Tile::RiverCrossing;
                }
            }
        }
        regions.push(Region { name: "Northern Ford", cx: river_x_base as usize, cy: cross1_y, kind: RegionKind::Chokepoint });
        regions.push(Region { name: "Southern Ford", cx: river_x_base as usize, cy: cross2_y, kind: RegionKind::Chokepoint });

        // ── 5. Ore Basins ──────────────────────────────────────────────────
        let ore_sites: &[(f32, f32, &str)] = &[
            (0.72, 0.25, "Northern Ore Basin"),
            (0.20, 0.72, "Southern Ore Basin"),
            (0.65, 0.65, "Eastern Ore Basin"),
        ];
        for &(fx, fy, name) in ore_sites {
            let (bx, by) = ((fx * w) as usize, (fy * h) as usize);
            place_resource(&mut tiles, bx, by, 6, Tile::OreBasin, width, height);
            regions.push(Region { name, cx: bx, cy: by, kind: RegionKind::OreBasin });
        }

        // ── 6. Oil Fields ──────────────────────────────────────────────────
        let oil_sites: &[(f32, f32, &str)] = &[
            (0.82, 0.55, "Eastern Oil Field"),
            (0.18, 0.35, "Western Oil Field"),
        ];
        for &(fx, fy, name) in oil_sites {
            let (bx, by) = ((fx * w) as usize, (fy * h) as usize);
            place_resource(&mut tiles, bx, by, 5, Tile::OilField, width, height);
            regions.push(Region { name, cx: bx, cy: by, kind: RegionKind::OilField });
        }

        // ── 7. Chokepoints (pass entrances) ────────────────────────────────
        // Already named the passes above; also mark the fordentrance tiles.

        // ── 8. Border and spawn-area clearings ─────────────────────────────
        // Solid cliff border.
        for y in 0..height {
            for x in 0..width {
                if x < 2 || y < 2 || x >= width - 2 || y >= height - 2 {
                    tiles[idx(x, y)] = Tile::Cliff;
                }
            }
        }

        // Player spawn clearing: bottom-left quadrant.
        let spawn_player = (w * 0.12, h * 0.80);
        clear_spawn(&mut tiles, spawn_player, 18.0, width, height);

        // Enemy spawn clearing: top-right quadrant.
        let spawn_enemy = (w * 0.88, h * 0.20);
        clear_spawn(&mut tiles, spawn_enemy, 18.0, width, height);

        (Self { width, height, tiles }, regions)
    }

    pub fn get(&self, x: usize, y: usize) -> Tile {
        self.tiles[y * self.width + x]
    }

    pub fn set(&mut self, x: usize, y: usize, t: Tile) {
        if x < self.width && y < self.height {
            self.tiles[y * self.width + x] = t;
        }
    }

    pub fn passable(&self, x: usize, y: usize) -> bool {
        self.get(x, y).passable()
    }

    pub fn size_px(&self) -> Vec2 {
        vec2(self.width as f32 * TILE_SIZE, self.height as f32 * TILE_SIZE)
    }

    /// Spawn positions in world pixels.
    pub fn player_spawn(&self) -> Vec2 {
        vec2(self.width as f32 * 0.12 * TILE_SIZE, self.height as f32 * 0.80 * TILE_SIZE)
    }

    pub fn enemy_spawn(&self) -> Vec2 {
        vec2(self.width as f32 * 0.88 * TILE_SIZE, self.height as f32 * 0.20 * TILE_SIZE)
    }

    /// Tile coordinate of the player spawn (for fog pre-reveal).
    pub fn player_spawn_tile(&self) -> (usize, usize) {
        ((self.width as f32 * 0.12) as usize, (self.height as f32 * 0.80) as usize)
    }

    pub fn enemy_spawn_tile(&self) -> (usize, usize) {
        ((self.width as f32 * 0.88) as usize, (self.height as f32 * 0.20) as usize)
    }
}

// ── helpers ───────────────────────────────────────────────────────────────────

fn dist2(ax: usize, ay: usize, bx: usize, by: usize) -> i64 {
    let dx = ax as i64 - bx as i64;
    let dy = ay as i64 - by as i64;
    dx * dx + dy * dy
}

fn carve_pass(tiles: &mut [Tile], cx: usize, cy: usize, half: i32, width: usize, height: usize) {
    for dy in -half..=half {
        for dx in -half..=half {
            let x = cx as i32 + dx;
            let y = cy as i32 + dy;
            if x >= 0 && y >= 0 && (x as usize) < width && (y as usize) < height {
                tiles[y as usize * width + x as usize] = Tile::Ground;
            }
        }
    }
}

fn place_resource(tiles: &mut [Tile], cx: usize, cy: usize, radius: i32, kind: Tile, width: usize, height: usize) {
    let r2 = radius * radius;
    for dy in -radius..=radius {
        for dx in -radius..=radius {
            if dx * dx + dy * dy <= r2 {
                let x = cx as i32 + dx;
                let y = cy as i32 + dy;
                if x >= 0 && y >= 0 && (x as usize) < width && (y as usize) < height {
                    let i = y as usize * width + x as usize;
                    if tiles[i] == Tile::Ground { tiles[i] = kind; }
                }
            }
        }
    }
}

fn clear_spawn(tiles: &mut [Tile], center: (f32, f32), radius: f32, width: usize, height: usize) {
    let r2 = radius * radius;
    for y in 0..height {
        for x in 0..width {
            let dx = x as f32 - center.0;
            let dy = y as f32 - center.1;
            if dx * dx + dy * dy < r2 {
                let i = y * width + x;
                if matches!(tiles[i], Tile::Water | Tile::Cliff | Tile::River) {
                    tiles[i] = Tile::Ground;
                }
            }
        }
    }
}

// ── colors ─────────────────────────────────────────────────────────────────────

pub fn tile_color(t: Tile) -> Color {
    match t {
        Tile::Ground        => Color::from_rgba(40,  54,  44,  255),
        Tile::Water         => Color::from_rgba(38,  60,  92,  255),
        Tile::Cliff         => Color::from_rgba(58,  58,  66,  255),
        Tile::OreBasin      => Color::from_rgba(160, 130, 60,  255),
        Tile::OilField      => Color::from_rgba(50,  45,  35,  255),
        Tile::MountainPass  => Color::from_rgba(90,  82,  70,  255),
        Tile::River         => Color::from_rgba(38,  60,  92,  255),
        Tile::RiverCrossing => Color::from_rgba(80,  90,  70,  255),
        Tile::Chokepoint    => Color::from_rgba(55,  65,  50,  255),
        Tile::Road          => Color::from_rgba(90,  80,  65,  255),
    }
}
