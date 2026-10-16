//! Things on the desktop that pets can go after: a slice of cake you can drag and throw, a ball
//! of yarn she bats about (with a trailing thread that drapes over windows), and room for more
//! kinds. Pure logic (the drawing and the mouse live in `app.rs`).

use crate::pet::{HALF_W, JUMP_MAX_UP, JUMP_MIN_UP, JUMP_REACH_X, Pet, floor_span};
use crate::world::{Rect, World};

const GRAVITY: f32 = 1400.0;
/// An impact faster than this bounces back up by `BOUNCE`; slower ones come to rest.
const BOUNCE_MIN_SPEED: f32 = 120.0;
const BOUNCE_VX_DAMP: f32 = 0.7;
/// A pet can take the treat when it is this close sideways...
const EAT_REACH_X: f32 = 22.0;
/// ...and no higher than this above her feet (she stretches up for it).
const EAT_REACH_UP: f32 = 48.0;
/// A treat dragged faster than this is being kept away from her.
const TEASE_SPEED: f32 = 60.0;
/// Playing: she can swat a toy from this close, but only once it has nearly stopped.
const BAT_REACH_X: f32 = 26.0;
const TOY_CALM_SPEED: f32 = 90.0;
/// A rolling ball slows by this fraction per second and stops below the minimum speed.
const ROLL_FRICTION: f32 = 1.6;
const ROLL_MIN_SPEED: f32 = 8.0;
/// The yarn thread: points, and the gap between them in sprite pixels.
const ROPE_START: usize = 10;
const ROPE_MAX: usize = 46;
/// Each swat unwinds this many more points of thread.
const UNRAVEL_PER_SWAT: usize = 3;
const ROPE_SEG: f32 = 2.2;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Cake,
    Yarn,
}

impl Kind {
    /// Width and height in sprite pixels.
    pub fn size(self) -> (f32, f32) {
        match self {
            Kind::Cake => (16.0, 11.0),
            Kind::Yarn => (11.0, 11.0),
        }
    }

    pub fn edible(self) -> bool {
        self == Kind::Cake
    }

    /// Something she plays with (bats about) instead of eating.
    pub fn toy(self) -> bool {
        self == Kind::Yarn
    }

    fn bounce(self) -> f32 {
        match self {
            Kind::Cake => 0.35,
            Kind::Yarn => 0.55,
        }
    }

    fn rolls(self) -> bool {
        self == Kind::Yarn
    }

    fn spins(self) -> bool {
        self == Kind::Yarn
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Motion {
    /// In the owner's hand.
    Held,
    Falling,
    Resting,
    /// Moving along a floor (a ball), slowing down.
    Rolling,
}

/// `x` is the middle and `y` the bottom, like a pet's feet.
#[derive(Clone, Debug)]
pub struct Object {
    pub kind: Kind,
    pub x: f32,
    pub y: f32,
    vx: f32,
    vy: f32,
    pub motion: Motion,
    /// How fast it moved over the last frame (px/s): being dragged away shows up here.
    pub speed: f32,
    last: (f32, f32),
    /// The yarn's trailing thread.
    pub rope: Option<Rope>,
    /// How far a ball has turned (radians) and how fast it is turning.
    pub angle: f32,
    omega: f32,
}

impl Object {
    pub fn new(kind: Kind, x: f32, y: f32) -> Object {
        Object {
            kind,
            x,
            y,
            vx: 0.0,
            vy: 0.0,
            motion: Motion::Falling,
            speed: 0.0,
            last: (x, y),
            rope: (kind == Kind::Yarn).then(|| Rope::trailing(x, y, -1.0)),
            angle: 0.0,
            omega: 0.0,
        }
    }

    /// Where the thread is tied: the middle of the ball.
    fn anchor(&self, px: f32) -> (f32, f32) {
        (self.x, self.y - self.kind.size().1 * px / 2.0)
    }

    /// A swat: it flies off at (vx, vy).
    pub fn nudge(&mut self, vx: f32, vy: f32) {
        if self.motion != Motion::Held {
            self.vx = vx;
            self.vy = vy;
            self.motion = Motion::Falling;
            if self.kind.spins() {
                self.omega = if vx >= 0.0 { 12.0 } else { -12.0 };
            }
        }
    }

    /// Which quarter turn to draw (0..4): sprites turn in whole quarters to stay crisp.
    pub fn quarter(&self) -> usize {
        ((self.angle / std::f32::consts::FRAC_PI_2).floor() as i64).rem_euclid(4) as usize
    }

    pub fn hitbox(&self, px: f32) -> Rect {
        let (w, h) = self.kind.size();
        Rect {
            x: self.x - w * px / 2.0,
            y: self.y - h * px,
            w: w * px,
            h: h * px,
        }
    }

    pub fn grab(&mut self) {
        self.motion = Motion::Held;
        self.vx = 0.0;
        self.vy = 0.0;
    }

    pub fn drag_to(&mut self, x: f32, y: f32) {
        if self.motion == Motion::Held {
            self.x = x;
            self.y = y;
        }
    }

    pub fn throw(&mut self, vx: f32, vy: f32) {
        if self.motion == Motion::Held {
            self.vx = vx.clamp(-3000.0, 3000.0);
            self.vy = vy.clamp(-3000.0, 3000.0);
            self.motion = Motion::Falling;
        }
    }

    /// `px` is points per sprite pixel (it sets how wide the object is against the screen edges).
    pub fn step(&mut self, dt: f32, world: &World, px: f32) {
        match self.motion {
            Motion::Held => {}
            Motion::Rolling => {
                self.vx *= (1.0 - ROLL_FRICTION * dt).max(0.0);
                self.x += self.vx * dt;
                let half = self.kind.size().0 * px / 2.0;
                // rolling without slipping: v = omega * r
                self.omega = self.vx / half;
                self.angle += self.omega * dt;
                let (left, right) = (world.screen.x + half, world.screen.right() - half);
                if self.x < left || self.x > right {
                    self.x = self.x.clamp(left, right);
                    self.vx = -self.vx * 0.6;
                    self.omega = -self.omega * 0.8;
                }
                let supported = world
                    .floor_below(self.x, self.y - 1.0)
                    .is_some_and(|f| (f - self.y).abs() < 1.0);
                if !supported {
                    // rolled off the edge of a window: fall with the speed it had
                    self.vy = 0.0;
                    self.motion = Motion::Falling;
                } else if self.vx.abs() < ROLL_MIN_SPEED {
                    self.vx = 0.0;
                    self.omega = 0.0;
                    self.motion = Motion::Resting;
                }
            }
            Motion::Resting => {
                // The window it sat on moved away: fall again.
                let supported = world
                    .floor_below(self.x, self.y - 1.0)
                    .is_some_and(|f| (f - self.y).abs() < 1.0);
                if !supported {
                    self.motion = Motion::Falling;
                }
            }
            Motion::Falling => {
                let half = self.kind.size().0 * px / 2.0;
                self.vy += GRAVITY * dt;
                self.angle += self.omega * dt;
                self.omega *= (1.0 - 0.3 * dt).max(0.0);
                let old_y = self.y;
                self.x += self.vx * dt;
                self.y += self.vy * dt;
                let (left, right) = (world.screen.x + half, world.screen.right() - half);
                if self.x < left || self.x > right {
                    self.x = self.x.clamp(left, right);
                    self.vx = -self.vx * self.kind.bounce();
                    self.omega = -self.omega * 0.8;
                }
                let top = world.screen.y + self.kind.size().1 * px;
                if self.y < top {
                    self.y = top;
                    self.vy = self.vy.abs() * self.kind.bounce();
                }
                if self.vy > 0.0
                    && let Some(f) = world.floor_below(self.x, old_y)
                    && self.y >= f
                {
                    self.y = f;
                    if self.vy > BOUNCE_MIN_SPEED {
                        if self.kind.spins() {
                            // a bounce sets it spinning the way it is travelling
                            let spin = 9.0 + (self.vy / 120.0).min(8.0);
                            self.omega = if self.vx >= 0.0 { spin } else { -spin };
                        }
                        self.vy = -self.vy * self.kind.bounce();
                        self.vx *= BOUNCE_VX_DAMP;
                    } else {
                        self.vy = 0.0;
                        if self.kind.rolls() && self.vx.abs() >= ROLL_MIN_SPEED {
                            self.motion = Motion::Rolling;
                        } else {
                            self.vx = 0.0;
                            self.omega = 0.0;
                            self.motion = Motion::Resting;
                        }
                    }
                }
            }
        }
    }
}

/// The yarn's thread: a chain of points (Verlet integration) tied to the ball. It falls under
/// gravity, lies on floors and window tops, drapes over their edges and drags behind the ball.
#[derive(Clone, Debug)]
pub struct Rope {
    p: Vec<(f32, f32)>,
    prev: Vec<(f32, f32)>,
}

impl Rope {
    /// A thread that starts out stretched along the floor behind the ball (`dir` -1 = to its
    /// left). Starting spread out matters: a thread folded onto one point can never lie down.
    pub fn trailing(x: f32, y: f32, dir: f32) -> Rope {
        let pts: Vec<(f32, f32)> = (0..ROPE_START)
            .map(|i| (x + dir * i as f32 * ROPE_SEG * 0.9, y))
            .collect();
        Rope {
            p: pts.clone(),
            prev: pts,
        }
    }

    pub fn points(&self) -> &[(f32, f32)] {
        &self.p
    }

    /// Unwinds more thread from the ball (up to a limit): new points start at the tail.
    pub fn unravel(&mut self, extra: usize) {
        let (last, prev) = (*self.p.last().unwrap(), *self.prev.last().unwrap());
        for _ in 0..extra {
            if self.p.len() >= ROPE_MAX {
                break;
            }
            self.p.push(last);
            self.prev.push(prev);
        }
    }

    fn step(&mut self, anchor: (f32, f32), dt: f32, world: &World, px: f32) {
        let seg = ROPE_SEG * px;
        let n = self.p.len();
        self.p[0] = anchor;
        self.prev[0] = anchor;
        for i in 1..n {
            let (x, y) = self.p[i];
            let (ox, oy) = self.prev[i];
            self.prev[i] = (x, y);
            self.p[i] = (
                x + (x - ox) * 0.98,
                y + (y - oy) * 0.995 + GRAVITY * dt * dt,
            );
        }
        for _ in 0..8 {
            for i in 0..n - 1 {
                let (ax, ay) = self.p[i];
                let (bx, by) = self.p[i + 1];
                let d = (bx - ax).hypot(by - ay).max(1e-4);
                let k = (d - seg) / d;
                let (dx, dy) = ((bx - ax) * k, (by - ay) * k);
                if i == 0 {
                    self.p[i + 1] = (bx - dx, by - dy);
                } else {
                    self.p[i] = (ax + dx * 0.5, ay + dy * 0.5);
                    self.p[i + 1] = (bx - dx * 0.5, by - dy * 0.5);
                }
            }
            for i in 1..n {
                let (mut x, mut y) = self.p[i];
                if let Some(f) = world.floor_below(x, self.prev[i].1.min(y))
                    && y > f
                {
                    y = f;
                    // lying on it: friction takes most of the sideways motion
                    let (ox, _) = self.prev[i];
                    self.prev[i] = (x - (x - ox) * 0.3, y);
                }
                x = x.clamp(world.screen.x, world.screen.right());
                y = y.clamp(world.screen.y, world.ground());
                self.p[i] = (x, y);
            }
        }
    }
}

/// What the chasing pet did this frame.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Chase {
    Going,
    Ate,
    /// Close enough to a calm toy to swat it.
    Bat,
}

/// One move over the floors: a jump up (possibly a little sideways) or a walk off an edge.
#[derive(Clone, Copy, Debug)]
enum Step {
    Jump { take_off: f32, land_x: f32, y: f32 },
    Walk { to_x: f32 },
}

/// The floor (an index into `world.floors`) standing at (x, y).
fn floor_at(world: &World, x: f32, y: f32) -> Option<usize> {
    world
        .floors
        .iter()
        .position(|f| (f.y - y).abs() < 3.0 && x >= f.x0 - 1.0 && x < f.x1 + 1.0)
}

/// Where she can go from floor `a`: up onto a window she can jump to, or off an edge.
fn moves(world: &World, a: usize, want_x: f32) -> Vec<(usize, Step)> {
    let f = &world.floors[a];
    let margin = HALF_W + 4.0;
    let mut out = Vec::new();
    for (b, g) in world.floors.iter().enumerate() {
        let up = f.y - g.y;
        if b == a || !(JUMP_MIN_UP..=JUMP_MAX_UP).contains(&up) {
            continue;
        }
        let (a_lo, a_hi) = (f.x0 + HALF_W, f.x1 - HALF_W);
        let (b_lo, b_hi) = (g.x0 + margin, g.x1 - margin);
        if a_lo > a_hi || b_lo > b_hi {
            continue;
        }
        let (lo, hi) = (a_lo.max(b_lo), a_hi.min(b_hi));
        let (take_off, land_x) = if lo <= hi {
            let x = want_x.clamp(lo, hi);
            (x, x)
        } else if b_lo > a_hi {
            (a_hi, b_lo)
        } else {
            (a_lo, b_hi)
        };
        if (take_off - land_x).abs() <= JUMP_REACH_X {
            out.push((
                b,
                Step::Jump {
                    take_off,
                    land_x,
                    y: g.y,
                },
            ));
        }
    }
    for x in [f.x0 - 24.0, f.x1 + 24.0] {
        if let Some(y) = world.floor_below(x, f.y + 1.0)
            && let Some(b) = floor_at(world, x, y)
            && b != a
        {
            out.push((b, Step::Walk { to_x: x }));
        }
    }
    out
}

/// The first move on the shortest way from floor `from` to floor `to`, if there is one.
fn route(world: &World, from: usize, to: usize, want_x: f32) -> Option<Step> {
    let mut first: Vec<Option<Step>> = vec![None; world.floors.len()];
    let mut seen = vec![false; world.floors.len()];
    seen[from] = true;
    let mut queue = std::collections::VecDeque::from([from]);
    while let Some(a) = queue.pop_front() {
        for (b, step) in moves(world, a, want_x) {
            if seen[b] {
                continue;
            }
            seen[b] = true;
            first[b] = if a == from { Some(step) } else { first[a] };
            if b == to {
                return first[b];
            }
            queue.push_back(b);
        }
    }
    None
}

/// Sends one pet after one object: along her floor, then window to window (jumping up, or
/// walking off edges to come down), and when there is no way she waits underneath, hopping.
pub fn chase(pet: &mut Pet, o: &Object, world: &World, dt: f32) -> Chase {
    if o.kind.toy() {
        return play_with(pet, o, world);
    }
    // Caught it in mid-hop: the jump adds to her reach.
    if pet.is_hopping()
        && o.motion != Motion::Held
        && (o.x - pet.x).abs() <= EAT_REACH_X
        && pet.y - o.y <= EAT_REACH_UP
    {
        return Chase::Ate;
    }
    if !pet.free_to_chase() {
        return Chase::Going;
    }
    // Being teased (held out of reach, or kept moving away) wears her patience down.
    let teased = o.motion == Motion::Held || o.speed > TEASE_SPEED;
    if teased && pet.stew(dt) {
        pet.get_angry();
        return Chase::Going;
    }
    let span = floor_span(world, pet.x, pet.y);
    let near = |x: f32| x.clamp(span.0 + HALF_W, span.1 - HALF_W);
    let obj_floor = world.floor_below(o.x, o.y).unwrap_or(world.ground());
    let (here, target) = (
        floor_at(world, pet.x, pet.y),
        floor_at(world, o.x, obj_floor),
    );
    if here.is_some() && here == target {
        // The same floor: run to it, then take it or hop for it.
        if (o.x - pet.x).abs() > EAT_REACH_X {
            pet.chase_to(near(o.x));
            return Chase::Going;
        }
        if o.motion != Motion::Held && pet.y - o.y <= EAT_REACH_UP {
            return Chase::Ate;
        }
        pet.face(o.x);
        pet.hop();
        if !teased && pet.stew(dt) {
            pet.get_angry();
        }
        return Chase::Going;
    }
    let step = match (here, target) {
        (Some(h), Some(t)) => route(world, h, t, o.x),
        _ => None,
    };
    match step {
        Some(Step::Jump {
            take_off,
            land_x,
            y,
        }) => {
            if (pet.x - take_off).abs() > 6.0 {
                pet.chase_to(take_off);
            } else {
                pet.jump_to(y, land_x);
            }
        }
        Some(Step::Walk { to_x }) => pet.chase_to(to_x),
        None => {
            // No way up from here: wait underneath it and hop, getting crosser.
            if (near(o.x) - pet.x).abs() > EAT_REACH_X {
                pet.chase_to(near(o.x));
            } else {
                pet.face(o.x);
                pet.hop();
            }
            if !teased && pet.stew(dt) {
                pet.get_angry();
            }
        }
    }
    Chase::Going
}

/// Playing with a toy: run to it, swat it when it is calm and close, lose interest after a few.
fn play_with(pet: &mut Pet, o: &Object, world: &World) -> Chase {
    if !pet.free_to_chase() || !pet.wants_to_play() || o.motion == Motion::Held {
        return Chase::Going;
    }
    let span = floor_span(world, pet.x, pet.y);
    let near = |x: f32| x.clamp(span.0 + HALF_W, span.1 - HALF_W);
    let obj_floor = world.floor_below(o.x, o.y).unwrap_or(world.ground());
    let (here, target) = (
        floor_at(world, pet.x, pet.y),
        floor_at(world, o.x, obj_floor),
    );
    if here.is_some() && here == target {
        if (o.x - pet.x).abs() > BAT_REACH_X {
            pet.chase_to(near(o.x));
        } else if pet.bat_ready() && o.speed < TOY_CALM_SPEED {
            return Chase::Bat;
        } else {
            pet.face(o.x);
        }
        return Chase::Going;
    }
    match match (here, target) {
        (Some(h), Some(t)) => route(world, h, t, o.x),
        _ => None,
    } {
        Some(Step::Jump {
            take_off,
            land_x,
            y,
        }) => {
            if (pet.x - take_off).abs() > 6.0 {
                pet.chase_to(take_off);
            } else {
                pet.jump_to(y, land_x);
            }
        }
        Some(Step::Walk { to_x }) => pet.chase_to(to_x),
        None => {
            if (near(o.x) - pet.x).abs() > EAT_REACH_X {
                pet.chase_to(near(o.x));
            }
        }
    }
    Chase::Going
}

#[derive(Default)]
pub struct Objects {
    pub items: Vec<Object>,
    /// A little state for varying swats without a random-number dependency.
    swats: u32,
}

impl Objects {
    pub fn step(&mut self, dt: f32, world: &World, px: f32) {
        for o in &mut self.items {
            o.speed = (o.x - o.last.0).hypot(o.y - o.last.1) / dt.max(1e-3);
            o.step(dt, world, px);
            o.last = (o.x, o.y);
            let anchor = o.anchor(px);
            if let Some(r) = &mut o.rope {
                r.step(anchor, dt, world, px);
            }
        }
    }

    /// Sends the closest free pet after the first edible object. Returns which pet (an index
    /// into `pets`) ate it, if one did; the object is gone then.
    pub fn chase(&mut self, pets: &mut [&mut Pet], world: &World, dt: f32) -> Option<usize> {
        let i = self.items.iter().position(|o| o.kind.edible())?;
        let o = self.items[i].clone();
        let who = (0..pets.len())
            .filter(|&k| pets[k].free_to_chase())
            .min_by(|&a, &b| {
                let d = |p: &Pet| (p.x - o.x).abs() + 2.0 * (p.y - o.y).abs();
                d(pets[a]).total_cmp(&d(pets[b]))
            })?;
        if chase(pets[who], &o, world, dt) == Chase::Ate {
            self.items.remove(i);
            pets[who].eat();
            return Some(who);
        }
        None
    }
}

impl Objects {
    /// Sends the closest bored-free pet to play with the first toy; returns which pet (an index
    /// into `pets`) swatted it this frame, if one did. The toy then flies off.
    pub fn play(&mut self, pets: &mut [&mut Pet], world: &World, dt: f32) -> Option<usize> {
        let _ = dt;
        let i = self.items.iter().position(|o| o.kind.toy())?;
        let o = self.items[i].clone();
        let who = (0..pets.len())
            .filter(|&k| pets[k].free_to_chase() && pets[k].wants_to_play())
            .min_by(|&a, &b| {
                let d = |p: &Pet| (p.x - o.x).abs() + 2.0 * (p.y - o.y).abs();
                d(pets[a]).total_cmp(&d(pets[b]))
            })?;
        if chase(pets[who], &o, world, dt) == Chase::Bat {
            // away from her, a little differently each time
            self.swats = self.swats.wrapping_mul(1_103_515_245).wrapping_add(12_345);
            let r1 = ((self.swats >> 16) & 0xff) as f32 / 255.0;
            let r2 = ((self.swats >> 8) & 0xff) as f32 / 255.0;
            let dir = if o.x >= pets[who].x { 1.0 } else { -1.0 };
            pets[who].bat(o.x);
            self.items[i].nudge(dir * (240.0 + 200.0 * r1), -(320.0 + 220.0 * r2));
            if let Some(r) = &mut self.items[i].rope {
                r.unravel(UNRAVEL_PER_SWAT);
            }
            return Some(who);
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pet::State;
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

    fn window(x: f32, y: f32, w: f32) -> Rect {
        Rect { x, y, w, h: 400.0 }
    }

    fn settle(o: &mut Object, w: &World, secs: f32) {
        for _ in 0..(secs * 60.0) as usize {
            o.step(1.0 / 60.0, w, 1.0);
        }
    }

    /// Runs a pet after one cake until it is eaten; returns the seconds it took.
    fn run_chase(w: &World, pet: &mut Pet, cake: Object, max_secs: f32) -> Option<f32> {
        let mut objects = Objects {
            items: vec![cake],
            ..Default::default()
        };
        let mut rng = StdRng::seed_from_u64(3);
        for i in 0..(max_secs * 60.0) as usize {
            objects.step(1.0 / 60.0, w, 1.0);
            pet.step(1.0 / 60.0, w, &mut rng);
            if objects.chase(&mut [&mut *pet], w, 1.0 / 60.0).is_some() {
                return Some(i as f32 / 60.0);
            }
        }
        None
    }

    /// Settles a yarn ball and its thread on the floor.
    fn settle_with_rope(o: &mut Object, w: &World, secs: f32) {
        for _ in 0..(secs * 60.0) as usize {
            o.step(1.0 / 60.0, w, 1.0);
            let a = o.anchor(1.0);
            if let Some(r) = &mut o.rope {
                r.step(a, 1.0 / 60.0, w, 1.0);
            }
        }
    }

    #[test]
    fn yarn_is_a_toy_not_food_and_has_a_thread() {
        let y = Object::new(Kind::Yarn, 300.0, 300.0);
        assert!(y.kind.toy() && !y.kind.edible());
        assert!(y.rope.is_some());
        assert!(Object::new(Kind::Cake, 1.0, 1.0).rope.is_none());
    }

    #[test]
    fn a_thrown_yarn_ball_rolls_to_a_stop_on_the_ground() {
        let w = world(&[]);
        let mut o = Object::new(Kind::Yarn, 400.0, 1032.0);
        o.motion = Motion::Resting;
        o.grab();
        o.throw(500.0, 0.0);
        let mut rolled = false;
        for _ in 0..(8.0 * 60.0) as usize {
            o.step(1.0 / 60.0, &w, 1.0);
            rolled |= o.motion == Motion::Rolling;
        }
        assert!(rolled, "it should roll, not just stop");
        assert_eq!(o.motion, Motion::Resting);
        assert!(o.x > 500.0 && o.x < 1920.0);
    }

    #[test]
    fn a_yarn_ball_rolls_off_the_edge_of_a_window_and_falls_to_the_ground() {
        let w = world(&[window(300.0, 700.0, 400.0)]);
        let mut o = Object::new(Kind::Yarn, 640.0, 700.0);
        o.motion = Motion::Resting;
        o.nudge(300.0, 0.0);
        settle(&mut o, &w, 6.0);
        assert!(o.x > 700.0, "it should have left the window: {}", o.x);
        assert!(
            (o.y - 1032.0).abs() < 0.5,
            "it should end on the ground: {}",
            o.y
        );
    }

    #[test]
    fn the_thread_lies_on_the_floor_and_stays_tied_to_the_ball() {
        let w = world(&[]);
        let mut o = Object::new(Kind::Yarn, 600.0, 1032.0);
        o.motion = Motion::Resting;
        settle_with_rope(&mut o, &w, 4.0);
        let pts = o.rope.as_ref().unwrap().points();
        let (ax, ay) = o.anchor(1.0);
        assert_eq!(pts[0], (ax, ay));
        for &(x, y) in pts {
            assert!(x.is_finite() && y.is_finite());
            assert!(y <= 1032.0 + 0.5, "the thread sank below the floor: {y}");
        }
        assert!(
            pts.iter().skip(1).all(|p| p.1 > 1032.0 - 6.0),
            "it should lie flat"
        );
        let reach = pts
            .iter()
            .map(|p| (p.0 - ax).hypot(p.1 - ay))
            .fold(0.0_f32, f32::max);
        assert!(
            reach < pts.len() as f32 * ROPE_SEG + 1.0,
            "stretched to {reach}"
        );
    }

    #[test]
    fn the_thread_drapes_over_a_window_edge() {
        let w = world(&[window(300.0, 700.0, 400.0)]);
        // the ball rests near the right edge of the window top: the thread hangs off the end
        let mut o = Object::new(Kind::Yarn, 690.0, 700.0);
        o.motion = Motion::Resting;
        o.rope = Some(Rope::trailing(690.0, 700.0, 1.0));
        settle_with_rope(&mut o, &w, 5.0);
        let pts = o.rope.as_ref().unwrap().points();
        assert!(
            pts.iter().any(|p| p.1 > 705.0),
            "some of it should hang down over the edge: {pts:?}"
        );
        assert!(
            pts.iter().any(|p| (p.1 - 700.0).abs() < 6.0),
            "some of it should lie on the window"
        );
    }

    #[test]
    fn she_bats_the_yarn_about_then_gets_bored_for_a_while() {
        let w = world(&[]);
        let mut pet = Pet::new(300.0, 1032.0);
        let mut ball = Object::new(Kind::Yarn, 900.0, 1032.0);
        ball.motion = Motion::Resting;
        let mut objects = Objects {
            items: vec![ball],
            ..Default::default()
        };
        let mut rng = StdRng::seed_from_u64(3);
        let mut bats = Vec::new();
        for i in 0..(75.0 * 60.0) as usize {
            objects.step(1.0 / 60.0, &w, 1.0);
            pet.step(1.0 / 60.0, &w, &mut rng);
            if objects.play(&mut [&mut pet], &w, 1.0 / 60.0).is_some() {
                bats.push(i as f32 / 60.0);
            }
            assert!(
                (0.0..=1920.0).contains(&objects.items[0].x),
                "the ball left the screen"
            );
        }
        assert!(
            bats.len() >= 8,
            "she should keep playing: {} bats",
            bats.len()
        );
        // after seven bats there is a long pause before the eighth
        assert!(
            bats[7] - bats[6] > 20.0,
            "bored for a while: {:?}",
            &bats[..8]
        );
    }

    #[test]
    fn yarn_is_never_eaten_even_when_she_stands_on_it() {
        let w = world(&[]);
        let mut pet = Pet::new(700.0, 1032.0);
        let mut ball = Object::new(Kind::Yarn, 700.0, 1032.0);
        ball.motion = Motion::Resting;
        let mut objects = Objects {
            items: vec![ball],
            ..Default::default()
        };
        let mut rng = StdRng::seed_from_u64(3);
        for _ in 0..600 {
            objects.step(1.0 / 60.0, &w, 1.0);
            pet.step(1.0 / 60.0, &w, &mut rng);
            assert!(objects.chase(&mut [&mut pet], &w, 1.0 / 60.0).is_none());
        }
        assert_eq!(objects.items.len(), 1);
    }

    #[test]
    fn a_bouncing_yarn_ball_spins_and_a_resting_one_does_not() {
        let w = world(&[]);
        let mut o = Object::new(Kind::Yarn, 300.0, 400.0);
        o.vx = 300.0;
        let mut quarters = std::collections::HashSet::new();
        for _ in 0..(1.5 * 60.0) as usize {
            o.step(1.0 / 60.0, &w, 1.0);
            quarters.insert(o.quarter());
        }
        assert!(o.angle.abs() > 3.0, "it should have turned: {}", o.angle);
        assert!(
            quarters.len() >= 3,
            "it should pass through several quarter turns: {quarters:?}"
        );
        settle(&mut o, &w, 10.0);
        let a = o.angle;
        settle(&mut o, &w, 1.0);
        assert_eq!(o.angle, a, "a ball at rest holds still");
        let mut cake = Object::new(Kind::Cake, 300.0, 400.0);
        cake.vx = 300.0;
        settle(&mut cake, &w, 3.0);
        assert_eq!(cake.angle, 0.0, "a cake does not spin");
    }

    #[test]
    fn a_swat_sends_the_ball_up_and_each_swat_unwinds_more_thread() {
        let w = world(&[]);
        let mut pet = Pet::new(300.0, 1032.0);
        let mut ball = Object::new(Kind::Yarn, 900.0, 1032.0);
        ball.motion = Motion::Resting;
        let mut objects = Objects {
            items: vec![ball],
            ..Default::default()
        };
        let mut rng = StdRng::seed_from_u64(3);
        let start = objects.items[0].rope.as_ref().unwrap().points().len();
        let mut swats = 0;
        for _ in 0..(40.0 * 60.0) as usize {
            objects.step(1.0 / 60.0, &w, 1.0);
            pet.step(1.0 / 60.0, &w, &mut rng);
            if objects.play(&mut [&mut pet], &w, 1.0 / 60.0).is_some() {
                swats += 1;
                assert!(
                    objects.items[0].vy <= -320.0,
                    "swatted up: {}",
                    objects.items[0].vy
                );
                let len = objects.items[0].rope.as_ref().unwrap().points().len();
                assert_eq!(
                    len,
                    (start + swats * UNRAVEL_PER_SWAT).min(ROPE_MAX),
                    "after {swats} swats"
                );
            }
        }
        assert!(swats >= 5, "only {swats} swats");
    }

    #[test]
    fn a_dropped_cake_falls_bounces_and_rests_on_the_ground() {
        let w = world(&[]);
        let mut o = Object::new(Kind::Cake, 500.0, 300.0);
        settle(&mut o, &w, 4.0);
        assert_eq!(o.motion, Motion::Resting);
        assert!((o.y - 1032.0).abs() < 0.5);
    }

    #[test]
    fn a_held_cake_stays_put_and_a_throw_stays_on_screen() {
        let w = world(&[]);
        let mut o = Object::new(Kind::Cake, 500.0, 300.0);
        o.grab();
        settle(&mut o, &w, 1.0);
        assert_eq!((o.x, o.y), (500.0, 300.0));
        o.drag_to(900.0, 400.0);
        o.throw(5000.0, -600.0);
        for _ in 0..600 {
            o.step(1.0 / 60.0, &w, 1.0);
            assert!((0.0..=1920.0).contains(&o.x), "off screen at {}", o.x);
        }
        assert_eq!(o.motion, Motion::Resting);
    }

    #[test]
    fn a_cake_on_a_window_falls_when_the_window_goes() {
        let with = world(&[window(400.0, 600.0, 500.0)]);
        let mut o = Object::new(Kind::Cake, 600.0, 400.0);
        settle(&mut o, &with, 3.0);
        assert!(
            (o.y - 600.0).abs() < 0.5,
            "should sit on the window top: {}",
            o.y
        );
        settle(&mut o, &world(&[]), 3.0);
        assert!((o.y - 1032.0).abs() < 0.5);
    }

    #[test]
    fn she_runs_to_a_cake_on_the_floor_and_eats_it() {
        let w = world(&[]);
        let mut pet = Pet::new(300.0, 1032.0);
        let mut cake = Object::new(Kind::Cake, 700.0, 1032.0);
        cake.motion = Motion::Resting;
        let t = run_chase(&w, &mut pet, cake, 30.0).expect("never ate it");
        assert!(t < 12.0, "took {t} s: she should run, not stroll");
    }

    #[test]
    fn she_hops_under_a_cake_held_out_of_reach_and_never_takes_it() {
        let w = world(&[]);
        let mut pet = Pet::new(300.0, 1032.0);
        let mut cake = Object::new(Kind::Cake, 400.0, 900.0);
        cake.grab();
        let mut objects = Objects {
            items: vec![cake],
            ..Default::default()
        };
        let mut rng = StdRng::seed_from_u64(3);
        let mut hopped = false;
        for _ in 0..(20.0 * 60.0) as usize {
            pet.step(1.0 / 60.0, &w, &mut rng);
            assert!(objects.chase(&mut [&mut pet], &w, 1.0 / 60.0).is_none());
            hopped |= pet.is_hopping();
        }
        assert!(hopped, "she should have hopped for it");
        assert!(
            (pet.x - 400.0).abs() < 40.0,
            "she should be underneath at {}",
            pet.x
        );
        assert_eq!(objects.items.len(), 1);
    }

    #[test]
    fn she_jumps_up_onto_a_window_for_a_cake_resting_on_it() {
        let w = world(&[window(300.0, 800.0, 600.0)]);
        let mut pet = Pet::new(500.0, 1032.0);
        let mut cake = Object::new(Kind::Cake, 700.0, 800.0);
        cake.motion = Motion::Resting;
        let t = run_chase(&w, &mut pet, cake, 60.0).expect("never got up to it");
        assert!(
            (pet.y - 800.0).abs() < 1.0,
            "she ate it from y {} ({t} s)",
            pet.y
        );
    }

    #[test]
    fn she_climbs_from_window_to_window_when_one_jump_is_too_high() {
        // the ground is 382 px below the upper window: too high for one jump, fine in two
        let w = world(&[window(600.0, 650.0, 600.0), window(100.0, 900.0, 600.0)]);
        let mut pet = Pet::new(300.0, 1032.0);
        let mut cake = Object::new(Kind::Cake, 900.0, 650.0);
        cake.motion = Motion::Resting;
        let t = run_chase(&w, &mut pet, cake, 120.0).expect("never climbed up to it");
        assert!(
            (pet.y - 650.0).abs() < 1.0,
            "ate it from y {} ({t} s)",
            pet.y
        );
    }

    #[test]
    fn a_window_with_no_way_up_gets_a_hopping_pet_underneath() {
        let w = world(&[window(500.0, 300.0, 600.0)]);
        let mut pet = Pet::new(300.0, 1032.0);
        let mut cake = Object::new(Kind::Cake, 800.0, 300.0);
        cake.motion = Motion::Resting;
        let mut objects = Objects {
            items: vec![cake],
            ..Default::default()
        };
        let mut rng = StdRng::seed_from_u64(3);
        let mut hopped = false;
        for _ in 0..(40.0 * 60.0) as usize {
            pet.step(1.0 / 60.0, &w, &mut rng);
            assert!(objects.chase(&mut [&mut pet], &w, 1.0 / 60.0).is_none());
            hopped |= pet.is_hopping();
        }
        assert!(hopped);
        assert!(
            pet.y > 1032.0 - 60.0,
            "she never left the ground for good: {}",
            pet.y
        );
    }

    /// Runs a pet after a cake for `secs`, calling `each(frame, &mut objects)` before every frame;
    /// returns whether she got angry at any point.
    fn angry_within(
        w: &World,
        pet: &mut Pet,
        cake: Object,
        secs: f32,
        mut each: impl FnMut(usize, &mut Objects),
    ) -> bool {
        let mut objects = Objects {
            items: vec![cake],
            ..Default::default()
        };
        let mut rng = StdRng::seed_from_u64(3);
        for i in 0..(secs * 60.0) as usize {
            each(i, &mut objects);
            objects.step(1.0 / 60.0, w, 1.0);
            pet.step(1.0 / 60.0, w, &mut rng);
            let _ = objects.chase(&mut [&mut *pet], w, 1.0 / 60.0);
            if pet.state == State::Angry {
                return true;
            }
        }
        false
    }

    #[test]
    fn she_gets_angry_when_the_cake_is_out_of_reach_for_good() {
        let w = world(&[window(500.0, 300.0, 600.0)]);
        let mut pet = Pet::new(300.0, 1032.0);
        let mut cake = Object::new(Kind::Cake, 800.0, 300.0);
        cake.motion = Motion::Resting;
        assert!(angry_within(&w, &mut pet, cake, 30.0, |_, _| {}));
    }

    #[test]
    fn she_gets_angry_when_you_keep_holding_it_away_or_moving_it() {
        let w = world(&[]);
        // held up high above her
        let mut pet = Pet::new(300.0, 1032.0);
        let mut cake = Object::new(Kind::Cake, 400.0, 800.0);
        cake.grab();
        assert!(angry_within(&w, &mut pet, cake, 30.0, |_, _| {}));
        // dragged along the floor just ahead of her, faster than she can run
        let mut pet = Pet::new(300.0, 1032.0);
        let mut cake = Object::new(Kind::Cake, 700.0, 1032.0);
        cake.grab();
        let got = angry_within(&w, &mut pet, cake, 40.0, |i, o| {
            o.items[0].drag_to(700.0 + (i as f32 * 3.0) % 900.0, 1032.0);
        });
        assert!(got, "she never lost her temper while it was dragged away");
    }

    #[test]
    fn she_can_catch_a_cake_just_above_her_with_a_hop() {
        let w = world(&[]);
        let mut pet = Pet::new(500.0, 1032.0);
        let mut cake = Object::new(Kind::Cake, 500.0, 1032.0 - 80.0);
        cake.motion = Motion::Resting; // floating there, as if tossed up
        assert_eq!(chase(&mut pet, &cake, &w, 1.0 / 60.0), Chase::Going);
        pet.hop();
        let mut rng = StdRng::seed_from_u64(1);
        let mut caught = false;
        for _ in 0..40 {
            pet.step(1.0 / 60.0, &w, &mut rng);
            caught |= chase(&mut pet, &cake, &w, 1.0 / 60.0) == Chase::Ate;
        }
        assert!(caught, "the hop should have got it");
    }

    #[test]
    fn she_stays_calm_when_she_can_simply_get_it() {
        let w = world(&[]);
        let mut pet = Pet::new(300.0, 1032.0);
        let mut cake = Object::new(Kind::Cake, 900.0, 1032.0);
        cake.motion = Motion::Resting;
        assert!(!angry_within(&w, &mut pet, cake, 30.0, |_, _| {}));
    }

    #[test]
    fn she_walks_off_a_window_edge_for_a_cake_on_the_ground() {
        let w = world(&[window(300.0, 800.0, 600.0)]);
        let mut pet = Pet::new(700.0, 800.0);
        let mut cake = Object::new(Kind::Cake, 1300.0, 1032.0);
        cake.motion = Motion::Resting;
        let t = run_chase(&w, &mut pet, cake, 90.0).expect("never came down for it");
        assert!(
            (pet.y - 1032.0).abs() < 1.0,
            "ate it from y {} ({t} s)",
            pet.y
        );
    }

    #[test]
    fn the_closer_of_two_pets_goes_for_it() {
        let w = world(&[]);
        let mut near = Pet::new(600.0, 1032.0);
        let mut far = Pet::new(1500.0, 1032.0);
        let mut cake = Object::new(Kind::Cake, 700.0, 1032.0);
        cake.motion = Motion::Resting;
        let mut objects = Objects {
            items: vec![cake],
            ..Default::default()
        };
        let mut rng = StdRng::seed_from_u64(3);
        let mut ate = None;
        for _ in 0..(20.0 * 60.0) as usize {
            near.step(1.0 / 60.0, &w, &mut rng);
            far.step(1.0 / 60.0, &w, &mut rng);
            if let Some(i) = objects.chase(&mut [&mut near, &mut far], &w, 1.0 / 60.0) {
                ate = Some(i);
                break;
            }
        }
        assert_eq!(ate, Some(0));
    }
}
