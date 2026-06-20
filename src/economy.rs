//! Economy stub (Phase 2.5 UI hook). A minimal resource pool so the HUD has real
//! numbers to display and the bar widgets have something to bind to. The production /
//! supply-network systems that actually drive these values arrive in Phase 4; for now
//! it's a static starting stockpile.

#[derive(Clone, Copy, Debug)]
pub struct Economy {
    pub metal: i32,
    pub fuel: i32,
    pub components: i32,
    pub power_used: i32,
    pub power_cap: i32,
}

impl Default for Economy {
    fn default() -> Self {
        Self { metal: 500, fuel: 300, components: 0, power_used: 20, power_cap: 100 }
    }
}

impl Economy {
    /// Fraction of power capacity currently drawn (0..1+, can exceed 1 when overdrawn).
    pub fn power_frac(&self) -> f32 {
        if self.power_cap <= 0 {
            return 1.0;
        }
        self.power_used as f32 / self.power_cap as f32
    }

    pub fn overdrawn(&self) -> bool {
        self.power_used > self.power_cap
    }
}
