//! Where pets can stand: the taskbar and the uncovered top edges of windows.

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    pub fn right(&self) -> f32 {
        self.x + self.w
    }
    pub fn contains(&self, x: f32, y: f32) -> bool {
        x >= self.x && x < self.right() && y >= self.y && y < self.y + self.h
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Floor {
    pub x0: f32,
    pub x1: f32,
    pub y: f32,
}

pub struct World {
    pub screen: Rect,
    pub taskbar_top: f32,
    pub floors: Vec<Floor>,
    pub door_x: f32,
}

const MIN_WINDOW_W: f32 = 120.0;
const TOP_MARGIN: f32 = 40.0;
const FULL_SLACK: f32 = 8.0;
const DOOR_GAP: f32 = 40.0;

/// Subtracts [a, b) from every span.
fn cut(spans: Vec<(f32, f32)>, a: f32, b: f32) -> Vec<(f32, f32)> {
    spans
        .into_iter()
        .flat_map(|(x0, x1)| {
            if b <= x0 || a >= x1 {
                vec![(x0, x1)]
            } else {
                [(x0, a), (b, x1)]
                    .into_iter()
                    .filter(|(l, r)| r - l >= 1.0)
                    .collect()
            }
        })
        .collect()
}

impl World {
    /// `windows` front-most first.
    pub fn build(screen: Rect, taskbar: Option<Rect>, windows: &[Rect], tray_left: f32) -> World {
        let ground = taskbar.map_or(screen.y + screen.h, |t| t.y);
        let mut floors = vec![Floor {
            x0: screen.x,
            x1: screen.right(),
            y: ground,
        }];
        for (i, w) in windows.iter().enumerate() {
            if w.w < MIN_WINDOW_W || w.y < screen.y + TOP_MARGIN || w.y >= ground {
                continue;
            }
            let mut spans = vec![(w.x.max(screen.x), w.right().min(screen.right()))];
            for front in &windows[..i] {
                // A screen-sized window is fullscreen (overlay hides anyway) or maximised.
                let big =
                    front.w >= screen.w - FULL_SLACK && front.h >= ground - screen.y - FULL_SLACK;
                if !big && w.y >= front.y && w.y < front.y + front.h {
                    spans = cut(spans, front.x, front.right());
                }
            }
            floors.extend(spans.into_iter().map(|(x0, x1)| Floor { x0, x1, y: w.y }));
        }
        let door_x = (tray_left - DOOR_GAP).clamp(screen.x + 20.0, screen.right() - 20.0);
        World {
            screen,
            taskbar_top: ground,
            floors,
            door_x,
        }
    }

    pub fn ground(&self) -> f32 {
        self.taskbar_top
    }

    pub fn floor_below(&self, x: f32, y: f32) -> Option<f32> {
        self.floors
            .iter()
            .filter(|f| x >= f.x0 && x < f.x1 && f.y > y - 1.0)
            .map(|f| f.y)
            .min_by(f32::total_cmp)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const SCREEN: Rect = Rect {
        x: 0.0,
        y: 0.0,
        w: 1920.0,
        h: 1080.0,
    };
    const TASKBAR: Rect = Rect {
        x: 0.0,
        y: 1032.0,
        w: 1920.0,
        h: 48.0,
    };

    #[test]
    fn taskbar_and_window_tops_are_floors() {
        let win = Rect {
            x: 100.0,
            y: 300.0,
            w: 800.0,
            h: 500.0,
        };
        let w = World::build(SCREEN, Some(TASKBAR), &[win], 1700.0);
        assert_eq!(w.ground(), 1032.0);
        assert_eq!(w.floor_below(500.0, 0.0), Some(300.0));
        assert_eq!(w.floor_below(50.0, 0.0), Some(1032.0));
        assert_eq!(w.floor_below(500.0, 301.0), Some(1032.0));
        assert_eq!(w.door_x, 1660.0);
    }

    #[test]
    fn covered_tiny_and_maximised_windows_are_not_floors() {
        let front = Rect {
            x: 0.0,
            y: 200.0,
            w: 600.0,
            h: 600.0,
        };
        let behind = Rect {
            x: 300.0,
            y: 400.0,
            w: 600.0,
            h: 300.0,
        };
        let tiny = Rect {
            x: 1200.0,
            y: 500.0,
            w: 80.0,
            h: 80.0,
        };
        let maxed = Rect {
            x: 0.0,
            y: 0.0,
            w: 1920.0,
            h: 1032.0,
        };
        let w = World::build(SCREEN, Some(TASKBAR), &[front, behind, tiny, maxed], 1700.0);
        assert_eq!(w.floor_below(450.0, 0.0), Some(200.0)); // behind's top is covered here
        assert_eq!(w.floor_below(800.0, 0.0), Some(400.0)); // ...but not here
        assert_eq!(w.floor_below(1240.0, 0.0), Some(1032.0));
    }

    #[test]
    fn fullscreen_window_in_front_does_not_cover() {
        let full = Rect {
            x: 0.0,
            y: 0.0,
            w: 1920.0,
            h: 1080.0,
        };
        let small = Rect {
            x: 100.0,
            y: 300.0,
            w: 800.0,
            h: 500.0,
        };
        let w = World::build(SCREEN, Some(TASKBAR), &[full, small], 1700.0);
        assert_eq!(w.floor_below(500.0, 0.0), Some(300.0));
    }

    #[test]
    fn hidden_taskbar_uses_screen_bottom() {
        let w = World::build(SCREEN, None, &[], 1900.0);
        assert_eq!(w.ground(), 1080.0);
        assert!(w.door_x <= 1920.0 - 20.0);
    }
}
