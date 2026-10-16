//! Two pets on one screen: the odd bump (both blush) and the odd headpat. Pure logic.

use crate::fx::Hearts;
use crate::pet::{
    BUMP_COOLDOWN_S, HEADPAT_MAX_S, HEADPAT_MIN_S, HEADPAT_S, Pet, State, floor_span,
};
use crate::world::{Rect, World};
use rand::Rng;

/// Gives up on a headpat walk after this long.
const APPROACH_MAX_S: f32 = 20.0;
/// How long the pet being patted waits for the patter to arrive.
const WAIT_S: f32 = 22.0;

#[derive(Clone, Copy, PartialEq, Debug)]
enum Act {
    Free,
    Approaching { a_pats: bool, t: f32 },
    Patting { a_pats: bool },
}

pub struct Social {
    bump_cd: f32,
    headpat_in: f32,
    act: Act,
    /// Divides the waits (debug testing only).
    k: f32,
}

fn overlap(a: Rect, b: Rect) -> bool {
    a.x < b.right() && b.x < a.right() && a.y < b.y + b.h && b.y < a.y + a.h
}

impl Social {
    pub fn new(k: f32, rng: &mut impl Rng) -> Social {
        let k = k.max(1.0);
        Social {
            bump_cd: 0.0,
            headpat_in: rng.random_range(HEADPAT_MIN_S..HEADPAT_MAX_S) / k,
            act: Act::Free,
            k,
        }
    }

    fn next_headpat(&self, rng: &mut impl Rng) -> f32 {
        rng.random_range(HEADPAT_MIN_S..HEADPAT_MAX_S) / self.k
    }

    /// `px` is points per sprite pixel. Call every frame while both pets are on screen.
    #[allow(clippy::too_many_arguments)]
    pub fn update(
        &mut self,
        dt: f32,
        world: &World,
        a: &mut Pet,
        b: &mut Pet,
        px: f32,
        rng: &mut impl Rng,
        hearts: &mut Hearts,
    ) {
        self.bump_cd = (self.bump_cd - dt).max(0.0);
        match self.act {
            Act::Free => {
                self.headpat_in -= dt;
                if (a.y - b.y).abs() > 1.0 {
                    return;
                }
                if self.bump_cd <= 0.0
                    && a.strolling()
                    && b.strolling()
                    && overlap(a.hitbox(px), b.hitbox(px))
                {
                    let a_left = a.x <= b.x;
                    a.blush(a_left);
                    b.blush(!a_left);
                    hearts.burst((a.x + b.x) / 2.0, a.y - 50.0 * px, 3, rng);
                    self.bump_cd = BUMP_COOLDOWN_S / self.k;
                    tracing::info!("bump");
                } else if self.headpat_in <= 0.0 && a.calm() && b.calm() {
                    let a_pats = rng.random_bool(0.5);
                    let (p, t) = if a_pats { (a, b) } else { (b, a) };
                    let dir = if t.x >= p.x { 1.0 } else { -1.0 };
                    let span = floor_span(world, t.x, t.y);
                    let to_x = (t.x - dir * 34.0 * px)
                        .min(span.1 - 16.0)
                        .max(span.0 + 16.0);
                    t.linger(WAIT_S, p.x);
                    p.walk_then_linger(to_x, HEADPAT_S, t.x);
                    self.act = Act::Approaching { a_pats, t: 0.0 };
                    tracing::info!("headpat: walking over");
                }
            }
            Act::Approaching { a_pats, t } => {
                let (p, tg) = if a_pats { (a, b) } else { (b, a) };
                if p.state == State::Linger {
                    tg.headpatted();
                    hearts.burst(tg.x, tg.y - 50.0 * px, 3, rng);
                    self.act = Act::Patting { a_pats };
                    tracing::info!("headpat");
                } else if !matches!(p.state, State::Approach { .. })
                    || tg.state != State::Linger
                    || t > APPROACH_MAX_S
                {
                    if tg.state == State::Linger {
                        tg.release();
                    }
                    if matches!(p.state, State::Approach { .. }) {
                        p.release();
                    }
                    self.act = Act::Free;
                    self.headpat_in = self.next_headpat(rng);
                } else {
                    self.act = Act::Approaching { a_pats, t: t + dt };
                }
            }
            Act::Patting { a_pats } => {
                let (p, tg) = if a_pats { (a, b) } else { (b, a) };
                if p.state != State::Linger && tg.state != State::HeadPatted {
                    self.act = Act::Free;
                    self.headpat_in = self.next_headpat(rng);
                }
            }
        }
    }

    /// A headpat is in progress.
    #[cfg(test)]
    pub fn busy(&self) -> bool {
        self.act != Act::Free
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::{SeedableRng, rngs::StdRng};

    fn world() -> World {
        let screen = Rect {
            x: 0.0,
            y: 0.0,
            w: 1920.0,
            h: 1080.0,
        };
        World::build(
            screen,
            Some(Rect {
                x: 0.0,
                y: 1032.0,
                w: 1920.0,
                h: 48.0,
            }),
            &[],
            1700.0,
        )
    }

    struct Rig {
        w: World,
        a: Pet,
        b: Pet,
        s: Social,
        h: Hearts,
        rng: StdRng,
    }

    impl Rig {
        fn new(ax: f32, bx: f32) -> Rig {
            let mut rng = StdRng::seed_from_u64(5);
            Rig {
                w: world(),
                a: Pet::new(ax, 1032.0),
                b: Pet::new(bx, 1032.0),
                s: Social::new(1.0, &mut rng),
                h: Hearts::default(),
                rng,
            }
        }
        fn tick(&mut self, secs: f32) {
            for _ in 0..(secs * 60.0) as usize {
                let dt = 1.0 / 60.0;
                // keep them from wandering off by themselves: only the interaction moves them
                for p in [&mut self.a, &mut self.b] {
                    if p.state == State::Idle {
                        p.t = 0.0;
                    }
                    p.step(dt, &self.w, &mut self.rng);
                }
                self.s.update(
                    dt,
                    &self.w,
                    &mut self.a,
                    &mut self.b,
                    1.0,
                    &mut self.rng,
                    &mut self.h,
                );
                self.h.update(dt);
            }
        }
    }

    #[test]
    fn overlapping_pets_blush_facing_away_then_resume_and_wait_out_the_cooldown() {
        let mut r = Rig::new(500.0, 520.0);
        r.s.headpat_in = 1e9;
        r.tick(0.1);
        assert_eq!(r.a.state, State::Blush);
        assert_eq!(r.b.state, State::Blush);
        assert!(r.a.facing_left && !r.b.facing_left, "facing away");
        assert!(r.h.len() >= 1);
        r.tick(1.7);
        assert_eq!(r.a.state, State::Idle);
        assert_eq!(r.b.state, State::Idle);
        // still overlapping, but the cooldown holds
        r.tick(60.0);
        assert_eq!(r.a.state, State::Idle);
        r.tick(28.4);
        assert_eq!(r.a.state, State::Blush, "cooldown over: bumps again");
    }

    #[test]
    fn no_bump_when_one_is_busy_or_on_another_floor() {
        let mut r = Rig::new(500.0, 520.0);
        r.s.headpat_in = 1e9;
        r.b.state = State::Dizzy;
        r.tick(0.1);
        assert_eq!(r.a.state, State::Idle);
        let mut r = Rig::new(500.0, 520.0);
        r.s.headpat_in = 1e9;
        r.b.y = 800.0;
        r.b.state = State::Held;
        r.tick(0.1);
        assert_eq!(r.a.state, State::Idle);
    }

    #[test]
    fn a_headpat_walks_over_pats_with_hearts_and_both_resume() {
        let mut r = Rig::new(300.0, 700.0);
        r.s.headpat_in = 0.0;
        r.tick(0.1);
        assert!(r.s.busy());
        let mut seen_pat = false;
        for _ in 0..(20 * 60) {
            r.tick(0.0167);
            if r.a.state == State::HeadPatted || r.b.state == State::HeadPatted {
                seen_pat = true;
                assert!(r.h.len() >= 1);
                assert!((r.a.x - r.b.x).abs() < 50.0, "adjacent");
                break;
            }
        }
        assert!(seen_pat);
        r.tick(2.0);
        assert!(!r.s.busy());
        assert!(r.a.calm() && r.b.calm());
        // the next one is minutes away
        r.tick(60.0);
        assert!(!r.s.busy());
        assert!(r.s.headpat_in > 60.0);
    }

    #[test]
    fn a_headpat_is_called_off_if_someone_is_grabbed() {
        let mut r = Rig::new(300.0, 900.0);
        r.s.headpat_in = 0.0;
        r.tick(0.3);
        assert!(r.s.busy());
        r.a.grab();
        r.b.grab();
        r.tick(0.1);
        assert!(!r.s.busy());
    }
}
