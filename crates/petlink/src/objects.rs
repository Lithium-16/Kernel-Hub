//! Things on the desktop that pets can go after: a slice of cake you can drag and throw, and a
//! room to add more kinds later. Pure logic (the drawing and the mouse live in `app.rs`).

use crate::pet::{HALF_W, JUMP_MAX_UP, JUMP_MIN_UP, JUMP_REACH_X, Pet, floor_span};
use crate::world::{Rect, World};

const GRAVITY: f32 = 1400.0;
/// An impact faster than this bounces back up by `BOUNCE`; slower ones come to rest.
const BOUNCE_MIN_SPEED: f32 = 120.0;
const BOUNCE: f32 = 0.35;
const BOUNCE_VX_DAMP: f32 = 0.7;
/// A pet can take the treat when it is this close sideways...
const EAT_REACH_X: f32 = 22.0;
/// ...and no higher than this above her feet (she stretches up for it).
const EAT_REACH_UP: f32 = 48.0;
/// A treat dragged faster than this is being kept away from her.
const TEASE_SPEED: f32 = 60.0;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Cake,
}

impl Kind {
    /// Width and height in sprite pixels.
    pub fn size(self) -> (f32, f32) {
        match self {
            Kind::Cake => (16.0, 11.0),
        }
    }

    pub fn edible(self) -> bool {
        match self {
            Kind::Cake => true,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Motion {
    /// In the owner's hand.
    Held,
    Falling,
    Resting,
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
        }
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
                let old_y = self.y;
                self.x += self.vx * dt;
                self.y += self.vy * dt;
                let (left, right) = (world.screen.x + half, world.screen.right() - half);
                if self.x < left || self.x > right {
                    self.x = self.x.clamp(left, right);
                    self.vx = -self.vx * BOUNCE;
                }
                let top = world.screen.y + self.kind.size().1 * px;
                if self.y < top {
                    self.y = top;
                    self.vy = self.vy.abs() * BOUNCE;
                }
                if self.vy > 0.0
                    && let Some(f) = world.floor_below(self.x, old_y)
                    && self.y >= f
                {
                    self.y = f;
                    if self.vy > BOUNCE_MIN_SPEED {
                        self.vy = -self.vy * BOUNCE;
                        self.vx *= BOUNCE_VX_DAMP;
                    } else {
                        self.vx = 0.0;
                        self.vy = 0.0;
                        self.motion = Motion::Resting;
                    }
                }
            }
        }
    }
}

/// What the chasing pet did this frame.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Chase {
    Going,
    Ate,
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

#[derive(Default)]
pub struct Objects {
    pub items: Vec<Object>,
}

impl Objects {
    pub fn step(&mut self, dt: f32, world: &World, px: f32) {
        for o in &mut self.items {
            o.speed = (o.x - o.last.0).hypot(o.y - o.last.1) / dt.max(1e-3);
            o.step(dt, world, px);
            o.last = (o.x, o.y);
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
        let mut objects = Objects { items: vec![cake] };
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
        let mut objects = Objects { items: vec![cake] };
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
        let mut objects = Objects { items: vec![cake] };
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
        let mut objects = Objects { items: vec![cake] };
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
        let mut objects = Objects { items: vec![cake] };
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
