//! Uniform spatial grid for O(1)-ish neighbor queries (targeting, avoidance, picking).
//! Rebuilt from entity positions each tick.

use hecs::{Entity, World};
use macroquad::prelude::*;

use crate::components::Position;

pub struct SpatialGrid {
    cell: f32,
    cols: usize,
    rows: usize,
    buckets: Vec<Vec<(Entity, Vec2)>>,
}

impl SpatialGrid {
    pub fn new(world_px: Vec2, cell: f32) -> Self {
        let cols = (world_px.x / cell).ceil() as usize + 1;
        let rows = (world_px.y / cell).ceil() as usize + 1;
        Self { cell, cols, rows, buckets: vec![Vec::new(); cols * rows] }
    }

    fn cell_xy(&self, p: Vec2) -> (usize, usize) {
        let cx = (p.x / self.cell).clamp(0.0, (self.cols - 1) as f32) as usize;
        let cy = (p.y / self.cell).clamp(0.0, (self.rows - 1) as f32) as usize;
        (cx, cy)
    }

    pub fn rebuild(&mut self, world: &World) {
        for b in &mut self.buckets {
            b.clear();
        }
        for (e, pos) in world.query::<&Position>().iter() {
            let (cx, cy) = self.cell_xy(pos.0);
            self.buckets[cy * self.cols + cx].push((e, pos.0));
        }
    }

    /// Visit entities whose cell overlaps the radius around `p`.
    pub fn for_neighbors(&self, p: Vec2, radius: f32, mut f: impl FnMut(Entity, Vec2)) {
        let min_cx = (((p.x - radius) / self.cell).floor() as i32).max(0) as usize;
        let min_cy = (((p.y - radius) / self.cell).floor() as i32).max(0) as usize;
        let max_cx = (((p.x + radius) / self.cell).floor() as i32).clamp(0, self.cols as i32 - 1) as usize;
        let max_cy = (((p.y + radius) / self.cell).floor() as i32).clamp(0, self.rows as i32 - 1) as usize;
        for cy in min_cy..=max_cy {
            for cx in min_cx..=max_cx {
                for &(e, ep) in &self.buckets[cy * self.cols + cx] {
                    f(e, ep);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::Position;

    #[test]
    fn finds_nearby_not_far() {
        let mut world = World::new();
        world.spawn((Position(vec2(100.0, 100.0)),));
        world.spawn((Position(vec2(110.0, 100.0)),));
        world.spawn((Position(vec2(500.0, 500.0)),));

        let mut grid = SpatialGrid::new(vec2(1000.0, 1000.0), 24.0);
        grid.rebuild(&world);

        let mut found = 0;
        grid.for_neighbors(vec2(100.0, 100.0), 30.0, |_e, _p| found += 1);
        assert!(found >= 2, "expected the two nearby entities, found {found}");
    }
}
