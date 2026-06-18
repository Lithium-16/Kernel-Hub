//! Navigation: a passability grid derived from the tilemap, and flow fields that
//! steer whole groups toward a goal (the primary mover for large unit counts).

use std::collections::VecDeque;

use macroquad::prelude::*;

use crate::map::{Tile, TileMap, TILE_SIZE};

pub struct NavGrid {
    pub w: usize,
    pub h: usize,
    passable: Vec<bool>,
}

impl NavGrid {
    pub fn from_map(map: &TileMap) -> Self {
        let mut passable = vec![false; map.width * map.height];
        for y in 0..map.height {
            for x in 0..map.width {
                passable[y * map.width + x] = matches!(map.get(x, y), Tile::Ground | Tile::Resource);
            }
        }
        Self { w: map.width, h: map.height, passable }
    }

    pub fn passable(&self, x: usize, y: usize) -> bool {
        self.passable[y * self.w + x]
    }
}

/// A per-tile flow direction toward a goal, computed by BFS over passable tiles.
pub struct FlowField {
    w: usize,
    h: usize,
    dir: Vec<Vec2>,
}

fn relax(
    nav: &NavGrid,
    dist: &mut [u32],
    q: &mut VecDeque<(usize, usize)>,
    nx: i32,
    ny: i32,
    d: u32,
) {
    if nx < 0 || ny < 0 || nx >= nav.w as i32 || ny >= nav.h as i32 {
        return;
    }
    let (nx, ny) = (nx as usize, ny as usize);
    if !nav.passable(nx, ny) {
        return;
    }
    let i = ny * nav.w + nx;
    if dist[i] != u32::MAX {
        return;
    }
    dist[i] = d + 1;
    q.push_back((nx, ny));
}

impl FlowField {
    pub fn to_goal(nav: &NavGrid, goal: (usize, usize)) -> Self {
        let (w, h) = (nav.w, nav.h);
        let mut dist = vec![u32::MAX; w * h];
        let mut q = VecDeque::new();

        if nav.passable(goal.0, goal.1) {
            dist[goal.1 * w + goal.0] = 0;
            q.push_back(goal);
        }
        // 4-neighbour BFS integration field.
        while let Some((x, y)) = q.pop_front() {
            let d = dist[y * w + x];
            relax(nav, &mut dist, &mut q, x as i32 + 1, y as i32, d);
            relax(nav, &mut dist, &mut q, x as i32 - 1, y as i32, d);
            relax(nav, &mut dist, &mut q, x as i32, y as i32 + 1, d);
            relax(nav, &mut dist, &mut q, x as i32, y as i32 - 1, d);
        }

        // Flow direction = toward the lowest-distance 8-neighbour (allows diagonals).
        let mut dir = vec![Vec2::ZERO; w * h];
        for y in 0..h {
            for x in 0..w {
                if dist[y * w + x] == u32::MAX {
                    continue;
                }
                let mut best = dist[y * w + x];
                let (mut bx, mut by) = (0i32, 0i32);
                for dy in -1..=1i32 {
                    for dx in -1..=1i32 {
                        if dx == 0 && dy == 0 {
                            continue;
                        }
                        let (nx, ny) = (x as i32 + dx, y as i32 + dy);
                        if nx < 0 || ny < 0 || nx >= w as i32 || ny >= h as i32 {
                            continue;
                        }
                        let nd = dist[ny as usize * w + nx as usize];
                        if nd < best {
                            best = nd;
                            bx = dx;
                            by = dy;
                        }
                    }
                }
                if bx != 0 || by != 0 {
                    dir[y * w + x] = vec2(bx as f32, by as f32).normalize();
                }
            }
        }

        Self { w, h, dir }
    }

    pub fn dir_at(&self, world_pos: Vec2) -> Vec2 {
        let x = (world_pos.x / TILE_SIZE).clamp(0.0, (self.w - 1) as f32) as usize;
        let y = (world_pos.y / TILE_SIZE).clamp(0.0, (self.h - 1) as f32) as usize;
        self.dir[y * self.w + x]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::{TileMap, TILE_SIZE};

    #[test]
    fn flow_points_toward_goal() {
        let map = TileMap::generate_test(64, 64);
        let nav = NavGrid::from_map(&map);
        let goal = (50usize, 15usize);
        let cell = (45usize, 15usize);
        assert!(nav.passable(goal.0, goal.1) && nav.passable(cell.0, cell.1));

        let ff = FlowField::to_goal(&nav, goal);
        let w = vec2((cell.0 as f32 + 0.5) * TILE_SIZE, (cell.1 as f32 + 0.5) * TILE_SIZE);
        let dir = ff.dir_at(w);
        assert!(dir.length() > 0.5, "flow direction should be set");
        // Goal is to the right (greater x), so flow should carry a positive x.
        assert!(dir.x > 0.0, "flow should point toward goal, got {dir:?}");
    }
}
