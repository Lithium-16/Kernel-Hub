//! One pet: a small state machine with gravity. Pure logic; app.rs draws it.

use crate::sounds::Sound;
use crate::world::{Rect, World};
use rand::Rng;

pub const WALK_SPEED: f32 = 40.0;
const GRAVITY: f32 = 1400.0;
const BOUNCE: f32 = 0.45;
const PAT_S: f32 = 0.6;
const DIZZY_S: f32 = 1.5;
const PEEK_S: f32 = 2.0;
pub const HALF_W: f32 = 16.0;
/// Running after a treat is quicker than a stroll.
const CHASE_SPEED: f32 = 110.0;
/// A little hop in place (how high, and the pause before the next one).
const HOP_H: f32 = 36.0;
const HOP_GAP_S: f32 = 0.45;
/// Failing to get a treat for this long makes her angry; she then stays calm for a while.
const ANGER_AFTER_S: f32 = 6.0;
const ANGER_COOLDOWN_S: f32 = 15.0;
const ANGRY_S: f32 = 2.8;
const FRAME_S: f32 = 0.1;
/// Bouncing on landing: an impact faster than the minimum bounces back up by the factor.
const BOUNCE_MIN_SPEED: f32 = 150.0;
const LAND_BOUNCE: f32 = 0.3;
const JUMP_BOUNCE: f32 = 0.2;
const BOUNCE_VX_DAMP: f32 = 0.6;
/// Pick-up / put-down pose when carrying a parcel.
const CARRY_POSE_S: f32 = 0.6;
// Emote bubbles.
pub const EMOTE_MIN_S: f32 = 60.0;
pub const EMOTE_MAX_S: f32 = 180.0;
pub const EMOTE_SHOW_S: f32 = 2.5;
// Walking towards the cursor.
pub const CURSOR_RANGE: f32 = 400.0;
pub const CURSOR_STOP: f32 = 24.0;
pub const CURSOR_CHANCE: f64 = 0.15;
const LINGER_MIN_S: f32 = 2.0;
const LINGER_MAX_S: f32 = 4.0;
const HOVER_R: f32 = 60.0;
const HOVER_FACE_S: f32 = 1.5;
// Jumping onto window tops.
pub const JUMP_CHANCE: f64 = 0.2;
pub const JUMP_REACH_X: f32 = 200.0;
pub const JUMP_MIN_UP: f32 = 40.0;
pub const JUMP_MAX_UP: f32 = 300.0;
const JUMP_OVERSHOOT: f32 = 20.0;
// Two pets together.
pub const BUMP_COOLDOWN_S: f32 = 90.0;
pub const HEADPAT_MIN_S: f32 = 180.0;
pub const HEADPAT_MAX_S: f32 = 300.0;
pub const BLUSH_S: f32 = 1.5;
pub const HEADPAT_S: f32 = 1.5;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum State {
    Idle,
    Wander {
        to_x: f32,
    },
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
    /// Walking somewhere on purpose (cursor, under a window, to a friend); `Plan` says what next.
    Approach {
        to_x: f32,
    },
    /// Standing still for a moment, facing something.
    Linger,
    Jump,
    /// Embarrassed after a bump: pat sprite, no hop.
    Blush,
    /// Being patted on the head by the friend: pat sprite and the happy hop.
    HeadPatted,
    /// Putting the parcel down.
    Place,
    /// Furious that she can't get the treat.
    Angry,
    /// Running after a treat.
    Chase {
        to_x: f32,
    },
    /// A small hop in place, trying to reach something above her.
    Hop,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Event {
    ReachedDoor,
    Landed,
    PeekDone,
    /// The parcel was put down (a visiting pet's delivery).
    Placed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    PickUp,
    Walk,
    PutDown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Emote {
    Note,
    Dots,
    Question,
    Bang,
    Heart,
    /// Swearing: !%@#
    Curse,
}

const EMOTES: [Emote; 5] = [
    Emote::Note,
    Emote::Dots,
    Emote::Question,
    Emote::Bang,
    Emote::Heart,
];

#[derive(Debug, Clone, Copy, PartialEq)]
enum Plan {
    None,
    Jump { y: f32, x: f32 },
    Linger { secs: f32, face_x: f32 },
}

/// The span of the floor she is standing on.
pub fn floor_span(world: &World, x: f32, y: f32) -> (f32, f32) {
    world
        .floors
        .iter()
        .find(|f| (f.y - y).abs() < 1.0 && x >= f.x0 && x <= f.x1)
        .map_or((world.screen.x, world.screen.right()), |f| (f.x0, f.x1))
}

fn within(v: f32, span: (f32, f32)) -> f32 {
    v.min(span.1 - HALF_W).max(span.0 + HALF_W)
}

/// Where to stop when walking to the cursor, if that is worth doing.
pub fn cursor_target(world: &World, span: (f32, f32), x: f32, cursor: (f32, f32)) -> Option<f32> {
    let (cx, cy) = cursor;
    if cy >= world.ground() || (cx - x).abs() > CURSOR_RANGE || (cx - x).abs() < CURSOR_STOP + 10.0
    {
        return None;
    }
    let dir = if cx >= x { 1.0 } else { -1.0 };
    let to = within(cx - dir * CURSOR_STOP, span);
    ((to - x).abs() >= 20.0).then_some(to)
}

/// A window top she can jump onto from (x, y): (floor y, x to take off and land at).
pub fn pick_jump(
    world: &World,
    span: (f32, f32),
    x: f32,
    y: f32,
    rng: &mut impl Rng,
) -> Option<(f32, f32)> {
    let cands: Vec<(f32, f32)> = world
        .floors
        .iter()
        .filter(|f| {
            let up = y - f.y;
            (JUMP_MIN_UP..=JUMP_MAX_UP).contains(&up)
                && f.x1 > x - JUMP_REACH_X
                && f.x0 < x + JUMP_REACH_X
                && f.x1 - f.x0 >= 2.0 * HALF_W + 8.0
        })
        .map(|f| {
            let lx = x.min(f.x1 - HALF_W - 4.0).max(f.x0 + HALF_W + 4.0);
            (f.y, lx)
        })
        // The walk to the take-off point must stay on the floor she is on.
        .filter(|(_, lx)| *lx >= span.0 + HALF_W && *lx <= span.1 - HALF_W)
        .collect();
    if cands.is_empty() {
        None
    } else {
        Some(cands[rng.random_range(0..cands.len())])
    }
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
    /// Carrying a parcel (shown on her head while walking).
    pub carry: bool,
    pub phase: Phase,
    /// The mouse pointer in points, fed by the app each frame.
    pub cursor: Option<(f32, f32)>,
    pub emote: Option<(Emote, f32)>,
    emote_cd: f32,
    hop_cd: f32,
    /// Seconds of failing to get a treat (negative while she is cooling off).
    frustration: f32,
    /// The sound this pet wants played (bounce, purr, meow); the app takes it.
    sound: Option<Sound>,
    hover_t: f32,
    hold: f32,
    plan: Plan,
    /// Divides the random emote wait (debug testing only; 1.0 normally).
    k: f32,
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
            carry: false,
            phase: Phase::Walk,
            cursor: None,
            emote: None,
            emote_cd: rand::rng().random_range(EMOTE_MIN_S..EMOTE_MAX_S),
            hop_cd: 0.0,
            frustration: 0.0,
            sound: None,
            hover_t: 0.0,
            hold: 0.0,
            plan: Plan::None,
            k: 1.0,
            owner_away: false,
            walk_clock: 0.0,
        }
    }

    /// Debug testing: shortens the random emote wait by `k`.
    #[cfg(debug_assertions)]
    pub fn set_timescale(&mut self, k: f32) {
        self.k = k.max(1.0);
        self.emote_cd /= self.k;
    }

    fn set(&mut self, s: State) {
        self.state = s;
        self.t = 0.0;
        self.plan = Plan::None;
    }

    /// True if the pat took (she is in a state where patting works).
    pub fn pat(&mut self) -> bool {
        if matches!(
            self.state,
            State::Held
                | State::Thrown
                | State::Fall
                | State::Carry
                | State::Exit
                | State::Enter
                | State::Peek
                | State::Jump
                | State::Place
        ) {
            return false;
        }
        self.set(State::Patted);
        true
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
    /// Picks the parcel up, walks it to the door and puts it down.
    pub fn carry_to_door(&mut self) {
        self.carry = true;
        self.phase = Phase::PickUp;
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
    /// Standing still in the pick-up / put-down pose (front-facing).
    pub fn carry_posing(&self) -> bool {
        self.state == State::Place || (self.state == State::Carry && self.phase != Phase::Walk)
    }

    // --- idle life & two-pet hooks ---------------------------------------------------------
    /// Idle, wandering or sitting: free to be interacted with.
    pub fn calm(&self) -> bool {
        matches!(self.state, State::Idle | State::Wander { .. } | State::Sit)
    }
    pub fn strolling(&self) -> bool {
        matches!(self.state, State::Idle | State::Wander { .. })
    }
    pub fn blush(&mut self, face_left: bool) {
        self.set(State::Blush);
        self.hold = BLUSH_S;
        self.facing_left = face_left;
    }
    pub fn headpatted(&mut self) {
        self.sound = Some(Sound::Purr);
        self.set(State::HeadPatted);
        self.hold = HEADPAT_S;
    }
    pub fn linger(&mut self, secs: f32, face_x: f32) {
        self.set(State::Linger);
        self.hold = secs;
        self.facing_left = face_x < self.x;
    }
    pub fn walk_then_linger(&mut self, to_x: f32, secs: f32, face_x: f32) {
        self.set(State::Approach { to_x });
        self.plan = Plan::Linger { secs, face_x };
    }
    /// Free to be sent after a treat (not being held, patted, carrying, sleeping...).
    pub fn free_to_chase(&self) -> bool {
        matches!(
            self.state,
            State::Idle | State::Wander { .. } | State::Sit | State::Chase { .. } | State::Linger
        )
    }

    /// Runs towards `to_x`; call it every frame with the latest target.
    pub fn chase_to(&mut self, to_x: f32) {
        if let State::Chase { .. } = self.state {
            self.state = State::Chase { to_x };
        } else if self.free_to_chase() {
            self.set(State::Chase { to_x });
        }
    }

    pub fn face(&mut self, x: f32) {
        self.facing_left = x < self.x;
    }

    /// A small hop in place (at most one every so often).
    pub fn hop(&mut self) {
        if self.hop_cd > 0.0 || !self.free_to_chase() {
            return;
        }
        self.vy = -(2.0 * GRAVITY * HOP_H).sqrt();
        self.vx = 0.0;
        self.hop_cd = HOP_GAP_S;
        self.set(State::Hop);
    }

    #[cfg(test)]
    pub fn is_hopping(&self) -> bool {
        self.state == State::Hop
    }

    /// A moment more of failing to get a treat; true when she has had enough.
    pub fn stew(&mut self, dt: f32) -> bool {
        self.frustration += dt;
        if self.frustration >= ANGER_AFTER_S {
            self.frustration = -ANGER_COOLDOWN_S;
            true
        } else {
            false
        }
    }

    /// Things are going well again (she is getting somewhere, or has eaten).
    pub fn cool_off(&mut self) {
        self.frustration = self.frustration.min(0.0);
    }

    /// Stamping mad: flushed, with a cloud of swearing.
    pub fn get_angry(&mut self) {
        self.set(State::Angry);
        self.hold = ANGRY_S;
        self.show_emote(Emote::Curse);
    }

    /// Happy about the treat she just got.
    pub fn eat(&mut self) {
        self.cool_off();
        self.headpatted();
        self.show_emote(Emote::Heart);
    }

    pub fn release(&mut self) {
        self.set(State::Idle);
    }
    /// Jumps from where she stands onto a floor at `ty`, landing at `land_x` (it may be a
    /// little sideways).
    pub fn jump_to(&mut self, ty: f32, land_x: f32) {
        if self.free_to_chase() {
            self.start_jump(ty, land_x);
        }
    }

    /// Walks under a window top and jumps onto it.
    pub fn go_jump(&mut self, ty: f32, land_x: f32) {
        if (land_x - self.x).abs() < 1.0 {
            self.start_jump(ty, land_x);
        } else {
            self.set(State::Approach { to_x: land_x });
            self.plan = Plan::Jump { y: ty, x: land_x };
        }
    }
    fn start_jump(&mut self, ty: f32, land_x: f32) {
        tracing::info!("jump: {:.0} px up", self.y - ty);
        let apex = self.y - ty + JUMP_OVERSHOOT;
        self.vy = -(2.0 * GRAVITY * apex).sqrt();
        let flight = -self.vy / GRAVITY + (2.0 * JUMP_OVERSHOOT / GRAVITY).sqrt();
        self.vx = (land_x - self.x) / flight;
        self.set(State::Jump);
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
        self.walk_at(to_x, dt, WALK_SPEED)
    }

    fn walk_at(&mut self, to_x: f32, dt: f32, speed: f32) -> bool {
        let d = to_x - self.x;
        self.facing_left = d < 0.0;
        let step = speed * dt;
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

    fn update_emote(&mut self, dt: f32, rng: &mut impl Rng) {
        let free = matches!(
            self.state,
            State::Idle
                | State::Wander { .. }
                | State::Sit
                | State::Linger
                | State::Angry
                | State::Approach { .. }
        );
        if !free {
            self.emote = None;
        } else if let Some((_, left)) = &mut self.emote {
            *left -= dt;
            if *left <= 0.0 {
                self.emote = None;
            }
        } else {
            self.emote_cd -= dt;
            if self.emote_cd <= 0.0 {
                self.show_emote(EMOTES[rng.random_range(0..EMOTES.len())]);
                self.emote_cd = rng.random_range(EMOTE_MIN_S..EMOTE_MAX_S) / self.k;
            }
        }
    }

    pub fn take_sound(&mut self) -> Option<Sound> {
        self.sound.take()
    }

    pub fn show_emote(&mut self, e: Emote) {
        tracing::info!("emote: {e:?}");
        self.sound = match e {
            Emote::Heart => Some(Sound::Purr),
            Emote::Question | Emote::Bang => Some(Sound::Meow),
            Emote::Curse => Some(Sound::Angry),
            Emote::Note | Emote::Dots => self.sound,
        };
        self.emote = Some((e, EMOTE_SHOW_S));
    }

    fn update_hover(&mut self, dt: f32) {
        let near = self.cursor.is_some_and(|(cx, cy)| {
            ((cx - self.x).powi(2) + (cy - (self.y - 24.0)).powi(2)).sqrt() < HOVER_R
        });
        if near && matches!(self.state, State::Idle | State::Sit | State::Linger) {
            self.hover_t += dt;
            if self.hover_t > HOVER_FACE_S
                && let Some((cx, _)) = self.cursor
            {
                self.facing_left = cx < self.x;
            }
        } else {
            self.hover_t = 0.0;
        }
    }

    /// Her next idle action: now and then the cursor or a window top, otherwise a wander.
    fn decide(&mut self, world: &World, rng: &mut impl Rng, left: f32, right: f32) {
        let span = floor_span(world, self.x, self.y);
        if let Some(c) = self.cursor
            && let Some(to_x) = cursor_target(world, span, self.x, c)
            && rng.random_bool(CURSOR_CHANCE)
        {
            tracing::info!("cursor approach: {:.0} -> {to_x:.0}", self.x);
            let secs = rng.random_range(LINGER_MIN_S..LINGER_MAX_S);
            self.set(State::Approach { to_x });
            self.plan = Plan::Linger { secs, face_x: c.0 };
            return;
        }
        if let Some((ty, lx)) = pick_jump(world, span, self.x, self.y, rng)
            && rng.random_bool(JUMP_CHANCE)
        {
            self.go_jump(ty, lx);
            return;
        }
        let to_x = (self.x + rng.random_range(-300.0..300.0)).clamp(left, right);
        tracing::info!("wander: {:.0} -> {to_x:.0}", self.x);
        self.set(State::Wander { to_x });
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
        self.hop_cd = (self.hop_cd - dt).max(0.0);
        if self.state == State::Hop {
            // Hopping for something she can't reach is what wears her patience down.
            self.frustration += dt;
        }
        self.update_emote(dt, rng);
        self.update_hover(dt);
        match self.state {
            State::Held => None,
            State::Thrown | State::Fall | State::Jump | State::Hop => {
                self.vy += GRAVITY * dt;
                let old_y = self.y;
                self.x += self.vx * dt;
                self.y += self.vy * dt;
                if self.state == State::Thrown {
                    self.angle += self.vx.signum() * 12.0 * dt;
                }
                if self.x < left || self.x > right {
                    self.x = self.x.clamp(left, right);
                    if self.vx.abs() > BOUNCE_MIN_SPEED {
                        self.sound = Some(Sound::Bounce);
                    }
                    self.vx = -self.vx * BOUNCE;
                }
                if self.state != State::Jump && self.y < world.screen.y + 48.0 {
                    self.y = world.screen.y + 48.0;
                    self.vy = self.vy.abs() * BOUNCE;
                }
                // Land on the first floor crossed while falling.
                if self.vy > 0.0
                    && let Some(f) = world.floor_below(self.x, old_y)
                    && self.y >= f
                {
                    self.y = f;
                    let impact = self.vy;
                    if impact > BOUNCE_MIN_SPEED && self.state != State::Hop {
                        let k = if self.state == State::Jump {
                            JUMP_BOUNCE
                        } else {
                            LAND_BOUNCE
                        };
                        self.sound = Some(Sound::Bounce);
                        self.vy = -impact * k;
                        self.vx *= BOUNCE_VX_DAMP;
                        return None;
                    }
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
            State::Blush | State::HeadPatted | State::Linger | State::Angry
                if self.t >= self.hold =>
            {
                self.set(State::Idle);
                None
            }
            State::Place if self.t >= CARRY_POSE_S => {
                self.carry = false;
                self.set(State::Idle);
                Some(Event::Placed)
            }
            State::Chase { to_x } => {
                if self.walk_at(to_x, dt, CHASE_SPEED) {
                    self.set(State::Idle);
                }
                None
            }
            State::Approach { to_x } => {
                if self.walk_towards(to_x, dt) {
                    match self.plan {
                        Plan::Jump { y, x } => self.start_jump(y, x),
                        Plan::Linger { secs, face_x } => {
                            self.linger(secs, face_x);
                            if self.emote.is_none() && rng.random_bool(0.4) {
                                let e = if rng.random_bool(0.5) {
                                    Emote::Question
                                } else {
                                    Emote::Heart
                                };
                                self.show_emote(e);
                            }
                        }
                        Plan::None => self.set(State::Idle),
                    }
                }
                None
            }
            State::Peek if self.t >= PEEK_S => {
                self.set(State::Idle);
                Some(Event::PeekDone)
            }
            State::Carry => match self.phase {
                Phase::PickUp => {
                    if self.t >= CARRY_POSE_S {
                        self.phase = Phase::Walk;
                    }
                    None
                }
                Phase::Walk => {
                    if self.walk_towards(world.door_x, dt) {
                        self.phase = Phase::PutDown;
                        self.t = 0.0;
                    }
                    None
                }
                Phase::PutDown => (self.t >= CARRY_POSE_S).then(|| {
                    self.carry = false;
                    self.set(State::Idle);
                    Event::ReachedDoor
                }),
            },
            State::Exit => self.walk_towards(world.door_x, dt).then(|| {
                self.set(State::Idle);
                Event::ReachedDoor
            }),
            State::Enter => {
                if self.walk_towards(world.door_x - 60.0, dt) {
                    self.set(if self.carry {
                        State::Place
                    } else {
                        State::Idle
                    });
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
                    let sit = rng.random_bool(0.3);
                    if sit {
                        tracing::info!("sit at {:.0}", self.x);
                    }
                    self.set(if sit { State::Sit } else { State::Idle });
                }
                None
            }
            State::Idle if self.t > rng.random_range(2.0..6.0) => {
                self.decide(world, rng, left, right);
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

    fn win_at(x: f32, y: f32, w: f32) -> Rect {
        Rect { x, y, w, h: 400.0 }
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
    fn bouncing_off_a_wall_asks_for_the_bounce_sound() {
        let mut p = Pet::new(960.0, 1032.0);
        p.grab();
        p.drag_to(960.0, 400.0);
        p.throw(4000.0, -800.0);
        let mut heard = Vec::new();
        let mut rng = StdRng::seed_from_u64(1);
        for _ in 0..600 {
            p.step(1.0 / 60.0, &world(&[]), &mut rng);
            heard.extend(p.take_sound());
        }
        assert!(heard.contains(&Sound::Bounce), "heard {heard:?}");
        assert_eq!(p.take_sound(), None, "taking a sound clears it");
    }

    #[test]
    fn stewing_long_enough_makes_her_angry_then_she_cools_off_for_a_while() {
        let mut p = Pet::new(500.0, 1032.0);
        assert!(!p.stew(5.0));
        assert!(p.stew(1.5), "6 s of being teased is enough");
        assert!(!p.stew(10.0), "no second outburst straight away");
        p.get_angry();
        assert_eq!(p.state, State::Angry);
        assert_eq!(p.emote.map(|(e, _)| e), Some(Emote::Curse));
        assert_eq!(p.take_sound(), Some(Sound::Angry));
        let w = world(&[]);
        let mut rng = StdRng::seed_from_u64(1);
        for _ in 0..(4.0 * 60.0) as usize {
            p.step(1.0 / 60.0, &w, &mut rng);
        }
        assert!(
            p.calm(),
            "she should be over it after a few seconds: {:?}",
            p.state
        );
    }

    #[test]
    fn cat_noises_come_with_headpats_and_some_emotes() {
        let mut p = Pet::new(500.0, 1032.0);
        p.headpatted();
        assert_eq!(p.take_sound(), Some(Sound::Purr));
        p.show_emote(Emote::Heart);
        assert_eq!(p.take_sound(), Some(Sound::Purr));
        p.show_emote(Emote::Question);
        assert_eq!(p.take_sound(), Some(Sound::Meow));
        p.show_emote(Emote::Dots);
        assert_eq!(p.take_sound(), None);
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
    fn a_thrown_pet_still_ends_dizzy_after_bouncing() {
        let w = world(&[]);
        let mut p = Pet::new(960.0, 1032.0);
        p.grab();
        p.drag_to(960.0, 300.0);
        p.throw(0.0, 0.0);
        let mut rng = StdRng::seed_from_u64(1);
        let mut landed = false;
        for _ in 0..(3 * 60) {
            landed |= p.step(1.0 / 60.0, &w, &mut rng) == Some(Event::Landed);
            if landed {
                break;
            }
        }
        assert!(landed);
        assert_eq!(p.state, State::Dizzy);
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
        assert!(!p.carry);
    }

    #[test]
    fn carrying_picks_up_walks_then_puts_down() {
        let w = world(&[]);
        let mut p = Pet::new(1500.0, 1032.0);
        p.carry_to_door();
        let mut rng = StdRng::seed_from_u64(1);
        let mut step = |p: &mut Pet, secs: f32| {
            for _ in 0..(secs * 60.0) as usize {
                p.step(1.0 / 60.0, &w, &mut rng);
            }
        };
        step(&mut p, 0.3);
        assert!(p.carry_posing() && p.x == 1500.0, "picking up, not moving");
        step(&mut p, 0.6);
        assert!(!p.carry_posing() && p.x > 1500.0, "walking with it");
        step(&mut p, 4.0);
        assert!(
            p.carry_posing() && (p.x - 1660.0).abs() < 1.0,
            "putting down"
        );
        step(&mut p, 1.0);
        assert_eq!(p.state, State::Idle);
    }

    #[test]
    fn a_delivering_visitor_puts_the_parcel_down_where_it_stops() {
        let w = world(&[]);
        let mut p = Pet::new(0.0, 0.0);
        p.carry = true;
        p.enter_from_door(&w);
        let events = run(&mut p, &w, 2.3);
        assert!(events.contains(&Event::Placed));
        assert!(!p.carry);
        assert!((p.x - 1600.0).abs() < 1.0);
    }

    #[test]
    fn pat_interrupts_and_returns_to_idle() {
        let mut p = Pet::new(500.0, 1032.0);
        assert!(p.pat());
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

    #[test]
    fn a_pet_dropped_from_high_bounces_and_then_stands() {
        let w = world(&[]);
        let mut p = Pet::new(500.0, 200.0);
        p.state = State::Fall;
        let mut rng = StdRng::seed_from_u64(1);
        let (mut touched, mut rose, mut landed) = (false, false, false);
        for _ in 0..(5 * 60) {
            let before = p.y;
            let ev = p.step(1.0 / 60.0, &w, &mut rng);
            if (p.y - 1032.0).abs() < 0.01 && p.vy < 0.0 {
                touched = true;
            }
            if touched && p.y < before {
                rose = true;
            }
            landed |= ev == Some(Event::Landed);
        }
        assert!(touched && rose, "bounced back up off the floor");
        assert!(landed);
        assert!(p.calm());
        assert!((p.y - 1032.0).abs() < 0.5);
    }

    #[test]
    fn a_tiny_step_does_not_bounce() {
        let w = world(&[win_at(0.0, 1027.0, 800.0)]);
        let mut p = Pet::new(400.0, 1027.0);
        run(&mut p, &w, 0.1);
        let w2 = world(&[]);
        let mut rng = StdRng::seed_from_u64(1);
        let mut rose = false;
        for _ in 0..120 {
            let before = p.y;
            p.step(1.0 / 60.0, &w2, &mut rng);
            rose |= p.y < before;
        }
        assert!(!rose);
    }

    // --- emotes ---------------------------------------------------------------------------

    #[test]
    fn emotes_come_and_go_but_never_while_asleep_or_carrying() {
        let w = world(&[]);
        let mut p = Pet::new(500.0, 1032.0);
        p.emote_cd = 0.0;
        let mut rng = StdRng::seed_from_u64(3);
        p.step(0.016, &w, &mut rng);
        assert!(p.emote.is_some());
        run(&mut p, &w, EMOTE_SHOW_S + 0.2);
        assert!(p.emote.is_none());
        assert!(p.emote_cd >= EMOTE_MIN_S - 1.0);

        for state in [State::Sleep, State::Carry, State::Held, State::Fall] {
            let mut p = Pet::new(500.0, 1032.0);
            p.emote_cd = 0.0;
            p.state = state;
            p.owner_away = state == State::Sleep;
            p.phase = Phase::PickUp;
            p.step(0.016, &w, &mut rng);
            assert!(p.emote.is_none(), "{state:?}");
        }
    }

    // --- cursor ---------------------------------------------------------------------------

    #[test]
    fn walks_to_just_short_of_a_nearby_cursor() {
        let w = world(&[]);
        let span = floor_span(&w, 500.0, 1032.0);
        assert_eq!(span, (0.0, 1920.0));
        assert_eq!(cursor_target(&w, span, 500.0, (800.0, 600.0)), Some(776.0));
        assert_eq!(cursor_target(&w, span, 500.0, (300.0, 600.0)), Some(324.0));
        assert_eq!(
            cursor_target(&w, span, 500.0, (1000.0, 600.0)),
            None,
            "too far"
        );
        assert_eq!(
            cursor_target(&w, span, 500.0, (800.0, 1050.0)),
            None,
            "taskbar"
        );
        assert_eq!(
            cursor_target(&w, span, 500.0, (510.0, 600.0)),
            None,
            "already there"
        );
    }

    #[test]
    fn an_idle_pet_sometimes_goes_to_the_cursor_and_faces_it() {
        let w = world(&[]);
        let mut rng = StdRng::seed_from_u64(7);
        let mut went = 0;
        for _ in 0..60 {
            let mut p = Pet::new(500.0, 1032.0);
            p.emote_cd = 1e9;
            p.cursor = Some((700.0, 600.0));
            p.t = 10.0;
            for _ in 0..60 {
                p.step(1.0 / 60.0, &w, &mut rng);
                if matches!(p.state, State::Approach { .. }) {
                    went += 1;
                    break;
                }
                if p.state != State::Idle {
                    break;
                }
            }
        }
        assert!((3..=25).contains(&went), "about 15%: {went}");

        let mut p = Pet::new(500.0, 1032.0);
        p.emote_cd = 1e9;
        p.walk_then_linger(676.0, 3.0, 700.0);
        run(&mut p, &w, 5.0);
        assert_eq!(p.state, State::Linger);
        assert!((p.x - 676.0).abs() < 1.0 && !p.facing_left);
        run(&mut p, &w, 3.2);
        assert_ne!(p.state, State::Linger, "lingered, then went on");
    }

    #[test]
    fn faces_a_cursor_that_hovers_close_for_a_while() {
        let w = world(&[]);
        let mut p = Pet::new(500.0, 1032.0);
        p.emote_cd = 1e9;
        p.facing_left = false;
        p.cursor = Some((470.0, 1000.0));
        let mut rng = StdRng::seed_from_u64(1);
        for _ in 0..60 {
            p.step(1.0 / 60.0, &w, &mut rng); // 1 s: not yet
        }
        assert!(!p.facing_left);
        for _ in 0..40 {
            p.step(1.0 / 60.0, &w, &mut rng);
        }
        assert!(p.facing_left);
    }

    // --- jumping --------------------------------------------------------------------------

    #[test]
    fn picks_only_reachable_window_tops() {
        let ground = 1032.0;
        let w = world(&[
            win_at(300.0, 800.0, 400.0),  // 232 up, overlaps: ok
            win_at(300.0, 1000.0, 400.0), // 32 up: too low
            win_at(300.0, 600.0, 400.0),  // 432 up: too high
            win_at(900.0, 800.0, 400.0),  // 232 up but starts 400 px away
        ]);
        let span = floor_span(&w, 500.0, ground);
        let mut rng = StdRng::seed_from_u64(1);
        for _ in 0..30 {
            let (ty, lx) = pick_jump(&w, span, 500.0, ground, &mut rng).unwrap();
            assert_eq!(ty, 800.0);
            assert!((300.0..=700.0).contains(&lx));
        }
        let none = world(&[win_at(300.0, 600.0, 400.0)]);
        assert!(pick_jump(&none, span, 500.0, ground, &mut rng).is_none());
    }

    #[test]
    fn never_targets_a_covered_span() {
        let front = win_at(0.0, 760.0, 1000.0);
        let behind = win_at(100.0, 840.0, 600.0); // top edge covered by `front`
        let w = world(&[front, behind]);
        let span = floor_span(&w, 500.0, 1032.0);
        let mut rng = StdRng::seed_from_u64(1);
        for _ in 0..30 {
            let (ty, _) = pick_jump(&w, span, 500.0, 1032.0, &mut rng).unwrap();
            assert_eq!(ty, 760.0);
        }
    }

    #[test]
    fn jumps_onto_the_window_and_stands_there() {
        let w = world(&[win_at(300.0, 800.0, 400.0)]);
        let mut p = Pet::new(200.0, 1032.0);
        p.emote_cd = 1e9;
        p.go_jump(800.0, 320.0);
        assert!(matches!(p.state, State::Approach { .. }));
        let mut rng = StdRng::seed_from_u64(1);
        let landed = (0..8 * 60).any(|_| p.step(1.0 / 60.0, &w, &mut rng) == Some(Event::Landed));
        assert!(landed);
        assert_eq!(p.state, State::Idle);
        assert!((p.y - 800.0).abs() < 0.5, "y {}", p.y);
        assert!((p.x - 320.0).abs() < 2.0);
    }

    #[test]
    fn a_jump_reaches_above_the_window_before_landing() {
        let w = world(&[win_at(300.0, 800.0, 400.0)]);
        let mut p = Pet::new(500.0, 1032.0);
        p.go_jump(800.0, 500.0);
        let mut rng = StdRng::seed_from_u64(1);
        let mut top = p.y;
        for _ in 0..120 {
            p.step(1.0 / 60.0, &w, &mut rng);
            top = top.min(p.y);
        }
        assert!((top - 780.0).abs() < 10.0, "apex {top}");
    }

    #[test]
    fn falls_if_the_window_vanishes_mid_jump() {
        let w = world(&[win_at(300.0, 800.0, 400.0)]);
        let mut p = Pet::new(500.0, 1032.0);
        p.go_jump(800.0, 500.0);
        run(&mut p, &w, 0.2);
        let events = run(&mut p, &world(&[]), 4.0);
        assert!(events.contains(&Event::Landed));
        assert!((p.y - 1032.0).abs() < 0.5);
    }

    #[test]
    fn jump_candidates_must_be_on_the_floor_she_can_walk() {
        // Standing on a small window top: a taller window far to the side is out of reach.
        let small = win_at(400.0, 800.0, 100.0);
        let tall = win_at(1000.0, 600.0, 300.0);
        let w = world(&[small, tall]);
        let span = floor_span(&w, 450.0, 800.0);
        let mut rng = StdRng::seed_from_u64(1);
        assert!(pick_jump(&w, span, 450.0, 800.0, &mut rng).is_none());
    }
}
