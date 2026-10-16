//! One pet: a small state machine with gravity. Pure logic; app.rs draws it.

use crate::world::{Rect, World};
use rand::Rng;

pub const WALK_SPEED: f32 = 40.0;
const GRAVITY: f32 = 1400.0;
const BOUNCE: f32 = 0.45;
const PAT_S: f32 = 0.6;
const DIZZY_S: f32 = 1.5;
const PEEK_S: f32 = 2.0;
const HALF_W: f32 = 16.0;
const FRAME_S: f32 = 0.1;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum State {
    Idle,
    Wander { to_x: f32 },
    Sit,
    Fall,
    Held,
    Thrown,
    Dizzy,
    Patted,
    Sleep,
    Carry,
    Exit,
    Enter,
    Peek,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Event {
    ReachedDoor,
    Landed,
    PeekDone,
}

pub struct Pet {
    pub x: f32,
    pub y: f32,
    pub vx: f32,
    pub vy: f32,
    pub facing_left: bool,
    pub state: State,
    pub t: f32,
    pub frame: usize,
    pub angle: f32,
    owner_away: bool,
    walk_clock: f32,
}

impl Pet {
    pub fn new(x: f32, y: f32) -> Pet {
        Pet {
            x,
            y,
            vx: 0.0,
            vy: 0.0,
            facing_left: false,
            state: State::Idle,
            t: 0.0,
            frame: 0,
            angle: 0.0,
            owner_away: false,
            walk_clock: 0.0,
        }
    }

    fn set(&mut self, s: State) {
        self.state = s;
        self.t = 0.0;
    }

    pub fn pat(&mut self) {
        if !matches!(
            self.state,
            State::Held
                | State::Thrown
                | State::Fall
                | State::Carry
                | State::Exit
                | State::Enter
                | State::Peek
        ) {
            self.set(State::Patted);
        }
    }
    pub fn grab(&mut self) {
        if self.state == State::Peek {
            return; // the pose must finish: the friend is waiting on its PeekDone
        }
        self.set(State::Held);
        self.vx = 0.0;
        self.vy = 0.0;
    }
    pub fn drag_to(&mut self, x: f32, y: f32) {
        if self.state == State::Held {
            self.x = x;
            self.y = y;
        }
    }
    pub fn throw(&mut self, vx: f32, vy: f32) {
        if self.state == State::Held {
            self.vx = vx.clamp(-3000.0, 3000.0);
            self.vy = vy.clamp(-3000.0, 3000.0);
            self.set(State::Thrown);
        }
    }
    pub fn set_owner_away(&mut self, away: bool) {
        self.owner_away = away;
    }
    pub fn carry_to_door(&mut self) {
        self.set(State::Carry);
    }
    pub fn exit_to_door(&mut self) {
        self.set(State::Exit);
    }
    pub fn enter_from_door(&mut self, world: &World) {
        self.x = world.door_x;
        self.y = world.ground();
        self.facing_left = true;
        self.set(State::Enter);
    }
    pub fn peek(&mut self) {
        self.set(State::Peek);
    }

    pub fn hitbox(&self, scale: f32) -> Rect {
        Rect {
            x: self.x - HALF_W * scale,
            y: self.y - 48.0 * scale,
            w: 32.0 * scale,
            h: 48.0 * scale,
        }
    }

    fn walk_towards(&mut self, to_x: f32, dt: f32) -> bool {
        let d = to_x - self.x;
        self.facing_left = d < 0.0;
        let step = WALK_SPEED * dt;
        self.walk_clock += dt;
        self.frame = (self.walk_clock / FRAME_S) as usize % 8;
        if d.abs() <= step {
            self.x = to_x;
            true
        } else {
            self.x += step * d.signum();
            false
        }
    }

    fn supported(&self, world: &World) -> bool {
        world
            .floor_below(self.x, self.y)
            .is_some_and(|f| (f - self.y).abs() < 1.0)
    }

    pub fn step(&mut self, dt: f32, world: &World, rng: &mut impl Rng) -> Option<Event> {
        self.t += dt;
        let left = world.screen.x + HALF_W;
        let right = world.screen.right() - HALF_W;
        // Safety net: never stay lost (NaN, off screen, below the ground with nothing to land on).
        if self.state != State::Held
            && !(self.x.is_finite()
                && self.y.is_finite()
                && self.x >= world.screen.x
                && self.x <= world.screen.right()
                && self.y <= world.ground() + 1.0)
        {
            self.x = if self.x.is_finite() {
                self.x.clamp(left, right)
            } else {
                world.screen.x + world.screen.w * 0.5
            };
            self.y = world.ground();
            self.vx = 0.0;
            self.vy = 0.0;
            self.angle = 0.0;
            self.set(State::Idle);
        }
        match self.state {
            State::Held => None,
            State::Thrown | State::Fall => {
                self.vy += GRAVITY * dt;
                let old_y = self.y;
                self.x += self.vx * dt;
                self.y += self.vy * dt;
                if self.state == State::Thrown {
                    self.angle += self.vx.signum() * 12.0 * dt;
                }
                if self.x < left || self.x > right {
                    self.x = self.x.clamp(left, right);
                    self.vx = -self.vx * BOUNCE;
                }
                if self.y < world.screen.y + 48.0 {
                    self.y = world.screen.y + 48.0;
                    self.vy = self.vy.abs() * BOUNCE;
                }
                // Land on the first floor crossed while falling.
                if self.vy > 0.0
                    && let Some(f) = world.floor_below(self.x, old_y)
                    && self.y >= f
                {
                    self.y = f;
                    self.angle = 0.0;
                    self.vx = 0.0;
                    self.vy = 0.0;
                    let thrown = self.state == State::Thrown;
                    self.set(if thrown { State::Dizzy } else { State::Idle });
                    return Some(Event::Landed);
                }
                None
            }
            _ if !self.supported(world) && !matches!(self.state, State::Peek) => {
                self.set(State::Fall);
                None
            }
            State::Patted if self.t >= PAT_S => {
                self.set(State::Idle);
                None
            }
            State::Dizzy if self.t >= DIZZY_S => {
                self.set(State::Idle);
                None
            }
            State::Peek if self.t >= PEEK_S => {
                self.set(State::Idle);
                Some(Event::PeekDone)
            }
            State::Carry | State::Exit => self.walk_towards(world.door_x, dt).then(|| {
                self.set(State::Idle);
                Event::ReachedDoor
            }),
            State::Enter => {
                if self.walk_towards(world.door_x - 60.0, dt) {
                    self.set(State::Idle);
                }
                None
            }
            State::Sleep if !self.owner_away => {
                self.set(State::Idle);
                None
            }
            State::Idle | State::Sit | State::Wander { .. } if self.owner_away => {
                self.set(State::Sleep);
                None
            }
            State::Wander { to_x } => {
                if self.walk_towards(to_x, dt) {
                    self.set(if rng.random_bool(0.3) {
                        State::Sit
                    } else {
                        State::Idle
                    });
                }
                None
            }
            State::Idle if self.t > rng.random_range(2.0..6.0) => {
                let to_x = (self.x + rng.random_range(-300.0..300.0)).clamp(left, right);
                self.set(State::Wander { to_x });
                None
            }
            State::Sit if self.t > 8.0 => {
                self.set(State::Idle);
                None
            }
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::{Rect, World};
    use rand::{SeedableRng, rngs::StdRng};

    fn world(windows: &[Rect]) -> World {
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
            windows,
            1700.0,
        )
    }

    fn run(p: &mut Pet, w: &World, secs: f32) -> Vec<Event> {
        let mut rng = StdRng::seed_from_u64(1);
        (0..(secs * 60.0) as usize)
            .filter_map(|_| p.step(1.0 / 60.0, w, &mut rng))
            .collect()
    }

    #[test]
    fn pat_and_grab_cannot_interrupt_the_peek_pose() {
        let w = world(&[]);
        let mut p = Pet::new(500.0, w.ground());
        p.peek();
        p.pat();
        p.grab();
        assert_eq!(p.state, State::Peek);
        assert_eq!(run(&mut p, &w, 3.0), vec![Event::PeekDone]);
    }

    #[test]
    fn falls_when_its_window_goes_away_and_lands_on_the_taskbar() {
        let win = Rect {
            x: 0.0,
            y: 300.0,
            w: 800.0,
            h: 400.0,
        };
        let mut p = Pet::new(400.0, 300.0);
        run(&mut p, &world(&[win]), 0.5);
        assert!((p.y - 300.0).abs() < 0.5, "stands on the window");
        let events = run(&mut p, &world(&[]), 3.0);
        assert!(events.contains(&Event::Landed));
        assert!((p.y - 1032.0).abs() < 0.5);
    }

    #[test]
    fn thrown_pet_bounces_inside_the_screen_and_gets_dizzy() {
        let mut p = Pet::new(960.0, 1032.0);
        p.grab();
        p.drag_to(960.0, 400.0);
        p.throw(4000.0, -800.0);
        run(&mut p, &world(&[]), 0.2);
        assert!(p.x <= 1920.0 - 16.0 && p.x >= 16.0);
        run(&mut p, &world(&[]), 5.0);
        assert!(matches!(
            p.state,
            State::Dizzy | State::Idle | State::Wander { .. }
        ));
        assert!((p.y - 1032.0).abs() < 0.5);
    }

    #[test]
    fn sleeps_while_the_owner_is_away_and_wakes_up() {
        let mut p = Pet::new(500.0, 1032.0);
        p.set_owner_away(true);
        run(&mut p, &world(&[]), 0.1);
        assert_eq!(p.state, State::Sleep);
        run(&mut p, &world(&[]), 10.0);
        assert_eq!(p.state, State::Sleep, "doesn't wander off in its sleep");
        p.set_owner_away(false);
        run(&mut p, &world(&[]), 0.1);
        assert_ne!(p.state, State::Sleep);
    }

    #[test]
    fn carries_mail_to_the_door() {
        let mut p = Pet::new(200.0, 1032.0);
        p.carry_to_door();
        let w = world(&[]);
        let mut rng = StdRng::seed_from_u64(1);
        let reached =
            (0..60 * 60).any(|_| p.step(1.0 / 60.0, &w, &mut rng) == Some(Event::ReachedDoor));
        assert!(reached);
        assert!((p.x - 1660.0).abs() < 2.0);
    }

    #[test]
    fn pat_interrupts_and_returns_to_idle() {
        let mut p = Pet::new(500.0, 1032.0);
        p.pat();
        assert_eq!(p.state, State::Patted);
        run(&mut p, &world(&[]), 1.0);
        assert_ne!(p.state, State::Patted);
    }

    #[test]
    fn a_lost_pet_is_put_back_on_the_ground() {
        let w = world(&[]);
        for (x, y) in [
            (500.0, 5000.0),
            (f32::NAN, 100.0),
            (-900.0, 1032.0),
            (500.0, f32::NAN),
        ] {
            let mut p = Pet::new(x, y);
            p.state = State::Fall;
            run(&mut p, &w, 0.1);
            assert!(
                p.x.is_finite() && p.x >= 0.0 && p.x <= 1920.0,
                "x for {x},{y}"
            );
            assert!((p.y - 1032.0).abs() < 0.5, "y for {x},{y}");
        }
    }
}
