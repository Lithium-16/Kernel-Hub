//! Navigation: a passability grid from the tilemap + flow fields that route whole
//! groups along the true shortest path (8-neighbour Dijkstra), with smooth
//! gradient-based, bilinearly-sampled directions so movement looks natural and
//! anticipates obstacles instead of veering at the last moment.

use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap, VecDeque};
use std::sync::Arc;

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
                passable[y * map.width + x] = map.get(x, y).passable();
            }
        }
        Self { w: map.width, h: map.height, passable }
    }

    pub fn passable(&self, x: usize, y: usize) -> bool {
        self.passable[y * self.w + x]
    }

    /// Mark a tile impassable (e.g. a placed building). Callers must invalidate any
    /// cached flow fields afterwards (see `FlowCache::clear`).
    pub fn set_blocked(&mut self, x: usize, y: usize) {
        if x < self.w && y < self.h {
            self.passable[y * self.w + x] = false;
        }
    }
}

const ORTHO: u32 = 10;
const DIAG: u32 = 14;
/// Treat blocked/edge neighbours as this much costlier than the current cell, so the
/// flow gradient steers units away from walls before they reach them.
const WALL_PENALTY: u32 = 40;

/// A per-tile flow direction toward a goal, from an 8-neighbour Dijkstra cost field.
pub struct FlowField {
    w: usize,
    h: usize,
    dir: Vec<Vec2>,
}

impl FlowField {
    pub fn to_goal(nav: &NavGrid, goal: (usize, usize)) -> Self {
        let (w, h) = (nav.w, nav.h);
        let mut cost = vec![u32::MAX; w * h];
        let mut heap: BinaryHeap<Reverse<(u32, usize, usize)>> = BinaryHeap::new();

        if nav.passable(goal.0, goal.1) {
            cost[goal.1 * w + goal.0] = 0;
            heap.push(Reverse((0, goal.0, goal.1)));
        }

        const NB: [(i32, i32, u32); 8] = [
            (1, 0, ORTHO), (-1, 0, ORTHO), (0, 1, ORTHO), (0, -1, ORTHO),
            (1, 1, DIAG), (1, -1, DIAG), (-1, 1, DIAG), (-1, -1, DIAG),
        ];

        while let Some(Reverse((c, x, y))) = heap.pop() {
            if c > cost[y * w + x] {
                continue;
            }
            for (dx, dy, step) in NB {
                let (nx, ny) = (x as i32 + dx, y as i32 + dy);
                if nx < 0 || ny < 0 || nx >= w as i32 || ny >= h as i32 {
                    continue;
                }
                let (nx, ny) = (nx as usize, ny as usize);
                if !nav.passable(nx, ny) {
                    continue;
                }
                // No diagonal corner-cutting through wall corners.
                if dx != 0 && dy != 0
                    && (!nav.passable((x as i32 + dx) as usize, y) || !nav.passable(x, (y as i32 + dy) as usize))
                {
                    continue;
                }
                let nc = c + step;
                if nc < cost[ny * w + nx] {
                    cost[ny * w + nx] = nc;
                    heap.push(Reverse((nc, nx, ny)));
                }
            }
        }

        // Flow = steepest descent of the cost field (central differences), with blocked
        // neighbours treated as costlier so the gradient bends away from walls.
        let at = |x: i32, y: i32, here: u32| -> u32 {
            if x < 0 || y < 0 || x >= w as i32 || y >= h as i32 {
                return here.saturating_add(WALL_PENALTY);
            }
            let c = cost[y as usize * w + x as usize];
            if c == u32::MAX { here.saturating_add(WALL_PENALTY) } else { c }
        };

        let mut dir = vec![Vec2::ZERO; w * h];
        for y in 0..h {
            for x in 0..w {
                let here = cost[y * w + x];
                if here == u32::MAX {
                    continue;
                }
                let (xi, yi) = (x as i32, y as i32);
                let gx = at(xi - 1, yi, here) as f32 - at(xi + 1, yi, here) as f32;
                let gy = at(xi, yi - 1, here) as f32 - at(xi, yi + 1, here) as f32;
                let g = vec2(gx, gy);
                if g.length_squared() > 0.0 {
                    dir[y * w + x] = g.normalize();
                }
            }
        }

        Self { w, h, dir }
    }

    fn dir_tile(&self, x: i32, y: i32) -> Vec2 {
        if x < 0 || y < 0 || x >= self.w as i32 || y >= self.h as i32 {
            return Vec2::ZERO;
        }
        self.dir[y as usize * self.w + x as usize]
    }

    /// Bilinearly-sampled flow direction at a world position (smooth, continuous turning).
    pub fn dir_at(&self, world_pos: Vec2) -> Vec2 {
        let fx = world_pos.x / TILE_SIZE - 0.5; // integer coords sit at tile centers
        let fy = world_pos.y / TILE_SIZE - 0.5;
        let (x0, y0) = (fx.floor() as i32, fy.floor() as i32);
        let (tx, ty) = (fx - x0 as f32, fy - y0 as f32);
        let top = self.dir_tile(x0, y0).lerp(self.dir_tile(x0 + 1, y0), tx);
        let bot = self.dir_tile(x0, y0 + 1).lerp(self.dir_tile(x0 + 1, y0 + 1), tx);
        let v = top.lerp(bot, ty);
        if v.length_squared() > 0.0001 {
            v.normalize()
        } else {
            Vec2::ZERO
        }
    }
}

/// Caches flow fields by goal tile so repeated / converging orders reuse the field
/// instead of recomputing the Dijkstra pass (a standard large-RTS optimization). The
/// foundation for the hierarchical (HPA*-portal + sector) flow-field upgrade later.
pub struct FlowCache {
    fields: HashMap<(usize, usize), Arc<FlowField>>,
    order: VecDeque<(usize, usize)>,
    cap: usize,
}

impl FlowCache {
    pub fn new(cap: usize) -> Self {
        Self { fields: HashMap::new(), order: VecDeque::new(), cap }
    }

    /// Drop all cached fields — call after the nav grid changes (e.g. a placed building)
    /// so stale routes aren't reused.
    pub fn clear(&mut self) {
        self.fields.clear();
        self.order.clear();
    }

    /// Reuse the cached field for `goal`, or build + cache it (evicting the oldest).
    pub fn get_or_build(&mut self, nav: &NavGrid, goal: (usize, usize)) -> Arc<FlowField> {
        if let Some(f) = self.fields.get(&goal) {
            return f.clone();
        }
        let f = Arc::new(FlowField::to_goal(nav, goal));
        self.fields.insert(goal, f.clone());
        self.order.push_back(goal);
        if self.order.len() > self.cap {
            if let Some(old) = self.order.pop_front() {
                self.fields.remove(&old);
            }
        }
        f
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::TILE_SIZE;

    #[test]
    fn flow_points_toward_goal() {
        let (map, _) = TileMap::generate(64, 64);
        let nav = NavGrid::from_map(&map);
        let goal = (50usize, 15usize);
        let cell = (45usize, 15usize);
        assert!(nav.passable(goal.0, goal.1) && nav.passable(cell.0, cell.1));

        let ff = FlowField::to_goal(&nav, goal);
        let w = vec2((cell.0 as f32 + 0.5) * TILE_SIZE, (cell.1 as f32 + 0.5) * TILE_SIZE);
        let dir = ff.dir_at(w);
        assert!(dir.length() > 0.5, "flow direction should be set");
        assert!(dir.x > 0.0, "flow should point toward goal (to the right), got {dir:?}");
    }
}
