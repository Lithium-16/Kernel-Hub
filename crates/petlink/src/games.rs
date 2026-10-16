//! Games the two pets play on their own when they meet: rock paper scissors, fetch and
//! hide-and-seek. Pure logic, run by the visited machine alone (nothing goes over the wire).

use crate::fx::Hearts;
use crate::pet::{Emote, Pet, State, floor_span};
use crate::world::World;
use rand::Rng;

/// Wait between games, in seconds (divided by `k` in debug testing).
const GAP_MIN_S: f32 = 20.0;
const GAP_MAX_S: f32 = 45.0;
/// Automatic visits: seconds between starts and how long one lasts (min, max).
pub const AUTO_GAP_S: (f32, f32) = (120.0, 240.0);
pub const AUTO_STAY_S: (f32, f32) = (60.0, 120.0);

pub fn auto_gap(rng: &mut impl Rng) -> f32 {
    rng.random_range(AUTO_GAP_S.0..AUTO_GAP_S.1)
}

pub fn auto_stay(rng: &mut impl Rng) -> f32 {
    rng.random_range(AUTO_STAY_S.0..AUTO_STAY_S.1)
}

/// A game that has not finished by now is called off.
const GAME_MAX_S: f32 = 45.0;
/// Rock paper scissors: when the hands are shown and when the round is over.
const RPS_REVEAL_S: f32 = 1.5;
const RPS_END_S: f32 = 3.2;
const FETCH_FLIGHT_S: f32 = 0.8;
const HIDE_MIN_S: f32 = 3.0;
const HIDE_MAX_S: f32 = 6.0;

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Hand {
    Rock,
    Paper,
    Scissors,
}

/// `Some(true)` when `a` wins, `Some(false)` when `b` wins, `None` on a tie.
pub fn rps_winner(a: Hand, b: Hand) -> Option<bool> {
    use Hand::*;
    match (a, b) {
        (x, y) if x == y => None,
        (Rock, Scissors) | (Paper, Rock) | (Scissors, Paper) => Some(true),
        _ => Some(false),
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Who {
    A,
    B,
}

impl Who {
    fn other(self) -> Who {
        match self {
            Who::A => Who::B,
            Who::B => Who::A,
        }
    }
}

/// Something to draw besides the pets.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Prop {
    Ball { x: f32, y: f32 },
    Hand { who: Who, hand: Hand, x: f32, y: f32 },
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Game {
    Rps,
    Fetch,
    Hide,
}

const ALL: [Game; 3] = [Game::Rps, Game::Fetch, Game::Hide];

enum Run {
    Rps {
        t: f32,
        round: u8,
        hands: Option<(Hand, Hand)>,
        scored: bool,
    },
    Fetch {
        t: f32,
        stage: u8,
        thrower: Who,
        dir: f32,
        land_x: f32,
        ball: (f32, f32),
    },
    Hide {
        t: f32,
        stage: u8,
        hider: Who,
        spot_x: f32,
        hidden_for: f32,
    },
}

pub struct Games {
    k: f32,
    next_in: f32,
    total: f32,
    run: Option<Run>,
}

fn pet<'p>(who: Who, a: &'p mut Pet, b: &'p mut Pet) -> &'p mut Pet {
    match who {
        Who::A => a,
        Who::B => b,
    }
}

/// Still part of a game (as opposed to grabbed, patted, thrown, put to sleep...).
fn playing(p: &Pet) -> bool {
    matches!(
        p.state,
        State::Idle
            | State::Wander { .. }
            | State::Sit
            | State::Linger
            | State::Approach { .. }
            | State::HeadPatted
            | State::Blush
    )
}

/// Lets a pet go only if the game still has it (never drops one the user is holding).
fn free(p: &mut Pet) {
    if matches!(
        p.state,
        State::Linger | State::Approach { .. } | State::HeadPatted | State::Blush
    ) {
        p.release();
    }
}

impl Games {
    pub fn new(k: f32, rng: &mut impl Rng) -> Games {
        let k = k.max(1.0);
        Games {
            k,
            next_in: rng.random_range(GAP_MIN_S..GAP_MAX_S) / k,
            total: 0.0,
            run: None,
        }
    }

    pub fn busy(&self) -> bool {
        self.run.is_some()
    }

    /// The pet that is hiding right now (not to be drawn).
    pub fn hidden(&self) -> Option<Who> {
        match &self.run {
            Some(Run::Hide {
                stage: 1,
                hider,
                ..
            }) => Some(*hider),
            _ => None,
        }
    }

    pub fn props(&self) -> Vec<Prop> {
        match &self.run {
            Some(Run::Fetch { stage, ball, .. }) if (1..=3).contains(stage) => {
                vec![Prop::Ball {
                    x: ball.0,
                    y: ball.1,
                }]
            }
            _ => Vec::new(),
        }
    }

    /// Hands are placed by the caller from the pets' positions (see `hand_props`).
    pub fn hand_props(&self, a: &Pet, b: &Pet, px: f32) -> Vec<Prop> {
        match &self.run {
            Some(Run::Rps {
                hands: Some((ha, hb)),
                ..
            }) => vec![
                Prop::Hand {
                    who: Who::A,
                    hand: *ha,
                    x: a.x,
                    y: a.y - 75.0 * px,
                },
                Prop::Hand {
                    who: Who::B,
                    hand: *hb,
                    x: b.x,
                    y: b.y - 75.0 * px,
                },
            ],
            _ => Vec::new(),
        }
    }

    /// Ends whatever is going on and sets the pets free.
    pub fn cancel(&mut self, a: &mut Pet, b: &mut Pet) {
        if self.run.take().is_some() {
            free(a);
            free(b);
        }
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
        social_busy: bool,
    ) {
        if self.run.is_none() {
            if social_busy {
                return;
            }
            self.next_in = (self.next_in - dt).max(0.0);
            if self.next_in <= 0.0
                && a.calm()
                && b.calm()
                && (a.y - b.y).abs() < 1.0
                && (a.x - b.x).abs() > 60.0 * px
            {
                let game = ALL[rng.random_range(0..ALL.len())];
                self.begin(game, world, a, b, px, rng);
            }
            return;
        }
        self.total += dt;
        if !playing(a) || !playing(b) || self.total > GAME_MAX_S {
            tracing::info!("game called off");
            self.cancel(a, b);
            self.next_in = rng.random_range(GAP_MIN_S..GAP_MAX_S) / self.k;
            return;
        }
        let done = match self.run.as_mut() {
            Some(run) => step(run, dt, world, a, b, px, rng, hearts),
            None => false,
        };
        if done {
            self.run = None;
            free(a);
            free(b);
            self.next_in = rng.random_range(GAP_MIN_S..GAP_MAX_S) / self.k;
        }
    }

    fn begin(
        &mut self,
        game: Game,
        world: &World,
        a: &mut Pet,
        b: &mut Pet,
        px: f32,
        rng: &mut impl Rng,
    ) {
        tracing::info!("game: {game:?}");
        self.total = 0.0;
        self.run = Some(match game {
            Game::Rps => {
                a.linger(RPS_END_S + 5.0, b.x);
                b.linger(RPS_END_S + 5.0, a.x);
                Run::Rps {
                    t: 0.0,
                    round: 0,
                    hands: None,
                    scored: false,
                }
            }
            Game::Fetch => {
                let thrower = if rng.random_bool(0.5) { Who::A } else { Who::B };
                let span = floor_span(world, a.x, a.y);
                let t_x = pet(thrower, a, b).x;
                let room_right = span.1 - t_x;
                let room_left = t_x - span.0;
                let dir = if room_right >= room_left { 1.0 } else { -1.0 };
                let dist = rng.random_range(150.0..400.0_f32).min(room_right.max(room_left) - 40.0);
                let land_x = (t_x + dir * dist.max(60.0)).clamp(span.0 + 20.0, span.1 - 20.0);
                let ground = pet(thrower, a, b).y;
                pet(thrower, a, b).linger(FETCH_FLIGHT_S + 0.5, land_x);
                pet(thrower.other(), a, b).walk_then_linger(land_x, 0.4, land_x);
                Run::Fetch {
                    t: 0.0,
                    stage: 1,
                    thrower,
                    dir,
                    land_x,
                    ball: (t_x, ground - 40.0 * px),
                }
            }
            Game::Hide => {
                let hider = if rng.random_bool(0.5) { Who::A } else { Who::B };
                let span = floor_span(world, a.x, a.y);
                let seeker_x = pet(hider.other(), a, b).x;
                let spot_x = if seeker_x < (span.0 + span.1) / 2.0 {
                    span.1 - 40.0
                } else {
                    span.0 + 40.0
                };
                pet(hider, a, b).walk_then_linger(spot_x, 60.0, spot_x);
                pet(hider.other(), a, b).linger(30.0, 2.0 * seeker_x - spot_x);
                Run::Hide {
                    t: 0.0,
                    stage: 0,
                    hider,
                    spot_x,
                    hidden_for: rng.random_range(HIDE_MIN_S..HIDE_MAX_S),
                }
            }
        });
    }

    #[cfg(test)]
    fn start(&mut self, game: Game, world: &World, a: &mut Pet, b: &mut Pet, rng: &mut impl Rng) {
        self.begin(game, world, a, b, 1.0, rng);
    }
}

/// Advances one game; true when it is over.
#[allow(clippy::too_many_arguments)]
fn step(
    run: &mut Run,
    dt: f32,
    world: &World,
    a: &mut Pet,
    b: &mut Pet,
    px: f32,
    rng: &mut impl Rng,
    hearts: &mut Hearts,
) -> bool {
    match run {
        Run::Rps {
            t,
            round,
            hands,
            scored,
        } => {
            *t += dt;
            if hands.is_none() && *t >= RPS_REVEAL_S {
                let mut pick = || match rng.random_range(0..3) {
                    0 => Hand::Rock,
                    1 => Hand::Paper,
                    _ => Hand::Scissors,
                };
                *hands = Some((pick(), pick()));
                *scored = false;
            }
            if let Some((ha, hb)) = *hands
                && !*scored
                && *t >= RPS_REVEAL_S + 0.5
            {
                *scored = true;
                match rps_winner(ha, hb) {
                    None if *round == 0 => {
                        *round = 1;
                        *hands = None;
                        *t = 0.0;
                    }
                    None => {}
                    Some(a_wins) => {
                        let (w, l) = if a_wins { (&mut *a, &mut *b) } else { (&mut *b, &mut *a) };
                        w.headpatted();
                        l.show_emote(Emote::Dots);
                        hearts.burst(w.x, w.y - 50.0 * px, 3, rng);
                    }
                }
            }
            *t >= RPS_END_S && *scored
        }
        Run::Fetch {
            t,
            stage,
            thrower,
            dir,
            land_x,
            ball,
        } => {
            *t += dt;
            let (tx, ty) = (pet(*thrower, a, b).x, pet(*thrower, a, b).y);
            match *stage {
                1 => {
                    // in the air
                    let s = (*t / FETCH_FLIGHT_S).min(1.0);
                    ball.0 = tx + (*land_x - tx) * s;
                    ball.1 = ty - 6.0 * px - 90.0 * px * 4.0 * s * (1.0 - s);
                    if s >= 1.0 {
                        *stage = 2;
                    }
                }
                2 => {
                    // the other pet runs to it
                    let f = pet(thrower.other(), a, b);
                    if f.state == State::Linger {
                        let home_x = tx - *dir * 30.0 * px;
                        f.walk_then_linger(home_x, 0.8, tx);
                        *stage = 3;
                    } else if *t > 12.0 {
                        return true;
                    }
                }
                3 => {
                    let f = pet(thrower.other(), a, b);
                    ball.0 = f.x;
                    ball.1 = f.y - 30.0 * px;
                    if f.state == State::Linger {
                        *stage = 4;
                        *t = 0.0;
                        hearts.burst(tx, ty - 50.0 * px, 3, rng);
                        pet(*thrower, a, b).show_emote(Emote::Heart);
                    }
                }
                _ => {
                    if *t > 1.0 {
                        return true;
                    }
                }
            }
            false
        }
        Run::Hide {
            t,
            stage,
            hider,
            spot_x,
            hidden_for,
        } => {
            *t += dt;
            match *stage {
                0 => {
                    if pet(*hider, a, b).state == State::Linger {
                        *stage = 1;
                        *t = 0.0;
                    }
                }
                1 => {
                    if *t >= *hidden_for {
                        let sx = *spot_x;
                        let seeker = pet(hider.other(), a, b);
                        let from_left = seeker.x < sx;
                        seeker.walk_then_linger(sx + if from_left { -40.0 } else { 40.0 } * px, 0.8, sx);
                        *stage = 2;
                        *t = 0.0;
                    }
                }
                2 => {
                    if pet(hider.other(), a, b).state == State::Linger {
                        *stage = 3;
                        *t = 0.0;
                        let (hx, hy) = (pet(*hider, a, b).x, pet(*hider, a, b).y);
                        hearts.burst(hx, hy - 50.0 * px, 3, rng);
                        pet(*hider, a, b).headpatted();
                        pet(hider.other(), a, b).show_emote(Emote::Bang);
                    }
                }
                _ => {
                    if *t > 1.5 {
                        return true;
                    }
                }
            }
            let _ = world;
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::Rect;
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
        g: Games,
        h: Hearts,
        rng: StdRng,
        social_busy: bool,
    }

    impl Rig {
        fn new() -> Rig {
            let mut rng = StdRng::seed_from_u64(5);
            Rig {
                w: world(),
                a: Pet::new(600.0, 1032.0),
                b: Pet::new(900.0, 1032.0),
                g: Games::new(1.0, &mut rng),
                h: Hearts::default(),
                rng,
                social_busy: false,
            }
        }
        fn tick(&mut self, secs: f32) {
            for _ in 0..(secs * 60.0) as usize {
                let dt = 1.0 / 60.0;
                for p in [&mut self.a, &mut self.b] {
                    if p.state == State::Idle {
                        p.t = 0.0;
                    }
                    p.step(dt, &self.w, &mut self.rng);
                }
                self.g.update(
                    dt,
                    &self.w,
                    &mut self.a,
                    &mut self.b,
                    1.0,
                    &mut self.rng,
                    &mut self.h,
                    self.social_busy,
                );
                self.h.update(dt);
            }
        }
        /// Ticks until the game is over; returns how long it took.
        fn play_out(&mut self) -> f32 {
            let mut t = 0.0;
            while self.g.busy() && t < 60.0 {
                self.tick(0.1);
                t += 0.1;
            }
            t
        }
    }

    #[test]
    fn automatic_visit_timings_stay_in_range() {
        let mut rng = StdRng::seed_from_u64(1);
        for _ in 0..200 {
            let g = auto_gap(&mut rng);
            let s = auto_stay(&mut rng);
            assert!((AUTO_GAP_S.0..AUTO_GAP_S.1).contains(&g));
            assert!((AUTO_STAY_S.0..AUTO_STAY_S.1).contains(&s));
        }
    }

    #[test]
    fn rps_winner_table() {
        use Hand::*;
        assert_eq!(rps_winner(Rock, Scissors), Some(true));
        assert_eq!(rps_winner(Paper, Rock), Some(true));
        assert_eq!(rps_winner(Scissors, Paper), Some(true));
        assert_eq!(rps_winner(Rock, Paper), Some(false));
        assert_eq!(rps_winner(Paper, Scissors), Some(false));
        assert_eq!(rps_winner(Scissors, Rock), Some(false));
        for h in [Rock, Paper, Scissors] {
            assert_eq!(rps_winner(h, h), None);
        }
    }

    #[test]
    fn a_game_never_starts_while_social_is_busy() {
        let mut r = Rig::new();
        r.social_busy = true;
        r.tick(300.0);
        assert!(!r.g.busy());
    }

    #[test]
    fn a_game_starts_by_itself_and_both_pets_end_calm() {
        let mut r = Rig::new();
        let mut waited = 0.0;
        while !r.g.busy() && waited < 120.0 {
            r.tick(0.5);
            waited += 0.5;
        }
        assert!(r.g.busy(), "no game within 120 s");
        assert!(r.play_out() < 50.0);
        r.tick(5.0);
        assert!(r.a.calm() && r.b.calm());
        assert!(r.g.props().is_empty());
        assert_eq!(r.g.hidden(), None);
    }

    #[test]
    fn every_game_runs_to_the_end_and_leaves_nothing_behind() {
        for game in ALL {
            let mut r = Rig::new();
            let (w, mut rng) = (world(), StdRng::seed_from_u64(9));
            r.g.start(game, &w, &mut r.a, &mut r.b, &mut rng);
            assert!(r.g.busy(), "{game:?} did not start");
            let took = r.play_out();
            assert!(!r.g.busy(), "{game:?} never finished ({took} s)");
            r.tick(3.0);
            assert!(r.a.calm() && r.b.calm(), "{game:?} left a pet busy");
            assert!(r.g.props().is_empty() && r.g.hidden().is_none());
        }
    }

    #[test]
    fn fetch_ball_stays_on_the_floor_and_inside_the_screen() {
        let mut r = Rig::new();
        let (w, mut rng) = (world(), StdRng::seed_from_u64(9));
        r.g.start(Game::Fetch, &w, &mut r.a, &mut r.b, &mut rng);
        while r.g.busy() {
            for p in r.g.props() {
                if let Prop::Ball { x, y } = p {
                    assert!(y <= 1032.0, "ball below the floor: {y}");
                    assert!((0.0..=1920.0).contains(&x));
                }
            }
            r.tick(0.1);
        }
    }

    #[test]
    fn hide_and_seek_hides_one_pet_for_a_while_then_shows_it() {
        let mut r = Rig::new();
        let (w, mut rng) = (world(), StdRng::seed_from_u64(9));
        r.g.start(Game::Hide, &w, &mut r.a, &mut r.b, &mut rng);
        assert_eq!(r.g.hidden(), None, "hidden before it reached its spot");
        let mut hidden_secs = 0.0;
        while r.g.busy() {
            if r.g.hidden().is_some() {
                hidden_secs += 0.1;
            }
            r.tick(0.1);
        }
        assert!(hidden_secs >= HIDE_MIN_S - 0.2, "hid for {hidden_secs} s");
        assert_eq!(r.g.hidden(), None);
    }

    #[test]
    fn cancel_mid_hide_shows_the_pet_and_frees_both() {
        let mut r = Rig::new();
        let (w, mut rng) = (world(), StdRng::seed_from_u64(9));
        r.g.start(Game::Hide, &w, &mut r.a, &mut r.b, &mut rng);
        while r.g.hidden().is_none() && r.g.busy() {
            r.tick(0.1);
        }
        assert!(r.g.hidden().is_some());
        r.g.cancel(&mut r.a, &mut r.b);
        assert_eq!(r.g.hidden(), None);
        assert!(!r.g.busy());
        r.tick(2.0);
        assert!(r.a.calm() && r.b.calm());
    }

    #[test]
    fn a_pet_picked_up_mid_game_calls_the_game_off_without_dropping_it() {
        let mut r = Rig::new();
        let (w, mut rng) = (world(), StdRng::seed_from_u64(9));
        r.g.start(Game::Rps, &w, &mut r.a, &mut r.b, &mut rng);
        r.tick(0.5);
        r.a.grab();
        r.tick(0.1);
        assert!(!r.g.busy());
        assert_eq!(r.a.state, State::Held, "the game let go of a held pet");
    }
}
