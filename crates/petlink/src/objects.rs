//! Things on the desktop that pets can go after: a slice of cake you can drag and throw, and a
//! room to add more kinds later. Pure logic (the drawing and the mouse live in `app.rs`).

use crate::pet::{HALF_W, JUMP_MAX_UP, JUMP_MIN_UP, Pet, floor_span};
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

/// Sends one pet after one object: along her floor, up onto a window with a jump, down off an
/// edge, or (when it is out of reach above her) hopping underneath it.
pub fn chase(pet: &mut Pet, o: &Object, world: &World) -> Chase {
    if !pet.free_to_chase() {
        return Chase::Going;
    }
    let span = floor_span(world, pet.x, pet.y);
    let near = |x: f32| x.clamp(span.0 + HALF_W, span.1 - HALF_W);
    let obj_floor = world.floor_below(o.x, o.y).unwrap_or(world.ground());
    if (obj_floor - pet.y).abs() < 3.0 {
        // The same floor: run to it, then take it or hop for it.
        let dx = o.x - pet.x;
        if dx.abs() > EAT_REACH_X {
            pet.chase_to(near(o.x));
            return Chase::Going;
        }
        let up = pet.y - o.y;
        if o.motion != Motion::Held && up <= EAT_REACH_UP {
            return Chase::Ate;
        }
        pet.face(o.x);
        pet.hop();
    } else if obj_floor < pet.y - 3.0 {
        // On a higher window: jump up if she can, otherwise wait underneath and hop.
        let up = pet.y - obj_floor;
        let Some(f) = world
            .floors
            .iter()
            .find(|f| (f.y - obj_floor).abs() < 1.0 && o.x >= f.x0 && o.x < f.x1)
        else {
            pet.chase_to(near(o.x));
            return Chase::Going;
        };
        let land_x = o.x.clamp(f.x0 + HALF_W + 4.0, f.x1 - HALF_W - 4.0);
        let take_off = near(land_x);
        if (JUMP_MIN_UP..=JUMP_MAX_UP).contains(&up) && (take_off - land_x).abs() < 1.0 {
            pet.go_jump(obj_floor, land_x);
        } else if (take_off - pet.x).abs() > EAT_REACH_X {
            pet.chase_to(take_off);
        } else {
            pet.hop();
        }
    } else {
        // On a lower floor: walk off the nearer edge and drop down to it.
        let edge = if o.x <= span.0 {
            span.0 - 24.0
        } else if o.x >= span.1 {
            span.1 + 24.0
        } else if o.x - span.0 < span.1 - o.x {
            span.0 - 24.0
        } else {
            span.1 + 24.0
        };
        pet.chase_to(edge);
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
            o.step(dt, world, px);
        }
    }

    /// Sends the closest free pet after the first edible object. Returns which pet (an index
    /// into `pets`) ate it, if one did; the object is gone then.
    pub fn chase(&mut self, pets: &mut [&mut Pet], world: &World) -> Option<usize> {
        let i = self.items.iter().position(|o| o.kind.edible())?;
        let o = self.items[i].clone();
        let who = (0..pets.len())
            .filter(|&k| pets[k].free_to_chase())
            .min_by(|&a, &b| {
                let d = |p: &Pet| (p.x - o.x).abs() + 2.0 * (p.y - o.y).abs();
                d(pets[a]).total_cmp(&d(pets[b]))
            })?;
        if chase(pets[who], &o, world) == Chase::Ate {
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
            if objects.chase(&mut [&mut *pet], w).is_some() {
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
            assert!(objects.chase(&mut [&mut pet], &w).is_none());
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
            if let Some(i) = objects.chase(&mut [&mut near, &mut far], &w) {
                ate = Some(i);
                break;
            }
        }
        assert_eq!(ate, Some(0));
    }
}
