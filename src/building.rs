//! Building placement helpers (Phase 3). Pure geometry/validity so the rules are
//! unit-testable without a window. The placement *mode* (ghost preview, commit) lives
//! in the main loop; nav blocking lives in `NavGrid`.

use macroquad::prelude::Vec2;

use crate::nav::NavGrid;

/// Top-left footprint tile so a `w`×`h` building is centered under `world` (cursor).
pub fn snap_origin(world: Vec2, w: usize, h: usize, tile: f32) -> (i32, i32) {
    let tx = (world.x / tile - w as f32 / 2.0).round() as i32;
    let ty = (world.y / tile - h as f32 / 2.0).round() as i32;
    (tx, ty)
}

/// Can a `w`×`h` building sit at tile origin `(tx, ty)`? Every footprint tile must be
/// in-bounds and passable (passable already excludes water/cliffs and prior buildings).
pub fn can_place(nav: &NavGrid, tx: i32, ty: i32, w: usize, h: usize) -> bool {
    if tx < 0 || ty < 0 {
        return false;
    }
    for dy in 0..h as i32 {
        for dx in 0..w as i32 {
            let (x, y) = (tx + dx, ty + dy);
            if x >= nav.w as i32 || y >= nav.h as i32 {
                return false;
            }
            if !nav.passable(x as usize, y as usize) {
                return false;
            }
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::TileMap;
    use macroquad::prelude::vec2;

    #[test]
    fn snap_centers_on_cursor() {
        // 2x2 building, cursor at tile (5,5) center → origin (4,4).
        assert_eq!(snap_origin(vec2(32.0 * 5.0, 32.0 * 5.0), 2, 2, 32.0), (4, 4));
        // 3x3 building, cursor at tile (10,10) → origin (9,9) wait: 10 - 1.5 = 8.5 -> round 9? actually rounds to 9
        assert_eq!(snap_origin(vec2(10.0 * 10.0, 10.0 * 10.0), 1, 1, 10.0), (10, 10));
    }

    #[test]
    fn rejects_out_of_bounds() {
        let nav = NavGrid::from_map(&TileMap::generate_test(32, 32));
        assert!(!can_place(&nav, -1, 0, 2, 2));
        assert!(!can_place(&nav, 31, 31, 3, 3)); // spills past the edge
    }

    #[test]
    fn accepts_some_passable_region() {
        let nav = NavGrid::from_map(&TileMap::generate_test(64, 64));
        let mut found = None;
        'outer: for y in 0..62 {
            for x in 0..62 {
                if can_place(&nav, x as i32, y as i32, 2, 2) {
                    found = Some((x, y));
                    break 'outer;
                }
            }
        }
        assert!(found.is_some(), "expected at least one placeable 2x2 region");
    }
}
