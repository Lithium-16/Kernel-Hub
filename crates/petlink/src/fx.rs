//! Heart particles: spawned at a point, drift up with a little wobble and fade out.

use rand::Rng;

pub const HEART_CAP: usize = 40;
pub const HEART_LIFE_S: f32 = 1.2;

pub struct Heart {
    x0: f32,
    y0: f32,
    age: f32,
    rise: f32,
    phase: f32,
}

#[derive(Default)]
pub struct Hearts {
    list: Vec<Heart>,
}

impl Hearts {
    /// False (and nothing added) when the list is full.
    pub fn spawn(&mut self, x: f32, y: f32, rng: &mut impl Rng) -> bool {
        if self.list.len() >= HEART_CAP {
            return false;
        }
        self.list.push(Heart {
            x0: x,
            y0: y,
            age: 0.0,
            rise: rng.random_range(30.0..50.0),
            phase: rng.random_range(0.0..std::f32::consts::TAU),
        });
        true
    }

    /// `n` hearts spread a little around (x, y), each a little later in the air than the last.
    pub fn burst(&mut self, x: f32, y: f32, n: usize, rng: &mut impl Rng) {
        for i in 0..n {
            let dx = (i as f32 - (n as f32 - 1.0) / 2.0) * 14.0;
            if self.spawn(x + dx, y + rng.random_range(-4.0..4.0), rng)
                && let Some(h) = self.list.last_mut()
            {
                h.age = -0.12 * i as f32; // staggered start
            }
        }
    }

    pub fn update(&mut self, dt: f32) {
        for h in &mut self.list {
            h.age += dt;
        }
        self.list.retain(|h| h.age < HEART_LIFE_S);
    }

    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.list.len()
    }

    /// (x, y, alpha 0..=1) of each visible heart.
    pub fn visible(&self) -> impl Iterator<Item = (f32, f32, f32)> + '_ {
        self.list.iter().filter(|h| h.age >= 0.0).map(|h| {
            let t = h.age / HEART_LIFE_S;
            (
                h.x0 + (h.age * 7.0 + h.phase).sin() * 3.0,
                h.y0 - h.rise * t,
                1.0 - t,
            )
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::{SeedableRng, rngs::StdRng};

    #[test]
    fn hearts_rise_fade_and_expire() {
        let mut rng = StdRng::seed_from_u64(1);
        let mut h = Hearts::default();
        h.spawn(100.0, 200.0, &mut rng);
        h.update(0.6);
        let (_, y, a) = h.visible().next().unwrap();
        assert!(y < 200.0 - 12.0 && y > 200.0 - 50.0);
        assert!((a - 0.5).abs() < 0.01);
        h.update(0.7);
        assert_eq!(h.len(), 0);
    }

    #[test]
    fn the_list_is_capped() {
        let mut rng = StdRng::seed_from_u64(1);
        let mut h = Hearts::default();
        for _ in 0..1000 {
            h.spawn(0.0, 0.0, &mut rng);
            h.burst(0.0, 0.0, 3, &mut rng);
        }
        assert_eq!(h.len(), HEART_CAP);
        h.update(2.0);
        assert!(h.spawn(0.0, 0.0, &mut rng));
    }
}
