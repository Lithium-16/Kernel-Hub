//! Fixed-timestep simulation core.
//!
//! Milestone 0: a bare tick counter that proves the loop runs at a fixed rate,
//! independent of the render framerate. Phase 1 grows this into the real ECS sim.

/// Simulation ticks per second (sim rate, not render rate).
pub const TICK_RATE: u32 = 20;

pub struct Sim {
    pub tick_count: u64,
}

impl Sim {
    pub fn new() -> Self {
        Self { tick_count: 0 }
    }

    /// Advance the simulation by exactly one fixed tick.
    pub fn tick(&mut self) {
        self.tick_count = self.tick_count.wrapping_add(1);
    }

    /// Elapsed simulated seconds.
    pub fn elapsed_secs(&self) -> f32 {
        self.tick_count as f32 / TICK_RATE as f32
    }
}

impl Default for Sim {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tick_increments_count() {
        let mut sim = Sim::new();
        sim.tick();
        sim.tick();
        assert_eq!(sim.tick_count, 2);
    }

    #[test]
    fn elapsed_matches_tick_rate() {
        let mut sim = Sim::new();
        for _ in 0..TICK_RATE {
            sim.tick();
        }
        assert!((sim.elapsed_secs() - 1.0).abs() < f32::EPSILON);
    }
}
