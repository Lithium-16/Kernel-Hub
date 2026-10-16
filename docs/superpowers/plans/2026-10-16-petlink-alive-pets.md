# PetLink Alive Pets Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Pets visit each other on their own every few minutes, play fetch, hide-and-seek and rock paper scissors when together, and peek works without sending a visiting pet home.

**Architecture:** Games are pure local logic (`games.rs`) run by the visited machine alone, next to the existing `Social` (bump/headpat) on `mine` and `visitor.pet`; no network traffic. Automatic visits reuse the existing `visit.start`/`visit.end` messages through the same `start_visit` path, with one new optional `auto` field. Peek-while-visiting reuses the existing `peek.request`/`peek.result` flow and the host's in-place `Job::Peek` handling.

**Tech Stack:** Rust (eframe/egui overlay, `rand`), crate `crates/petlink`; tests with `cargo test --release -p petlink`.

**Spec:** `docs/superpowers/specs/2026-10-16-petlink-alive-pets-design.md`

## Global Constraints

- No relay change and no new message kind: only `VisitStart` gains an optional `auto` field (`#[serde(default, skip_serializing_if = "std::ops::Not::not")]`), so older clients still parse it.
- Automatic visits average about 3 minutes apart (random 120 to 240 s), each lasting 60 to 120 s; the numbers live in named constants in one place.
- Games last well under a minute, play only while both pets are on the same screen, never block call-home, peek, mail or a visit ending.
- A peek is only ever answered with a screenshot when "Let friend peek" is on; that check stays exactly where it is (`peek_possible`).
- Tray switch "Let pets wander", on by default, saved in config (`#[serde(default = "default_true")] pub wander: bool`).
- The peek magnifier stays the code-drawn one in `app.rs` (`State::Peek`); game props are code-drawn too (ball, three hands).
- Existing tests keep passing; no new dependencies.

## Review Focus

- Both sides start an automatic visit in the same second: only one pet may end up away (the other cancels its walk).
- The friend's machine is away, hidden behind a fullscreen app, or has "Let pets wander" off: the pet must come straight back, with no ghost.
- The link drops, or the visit ends, in the middle of a game: both pets are released and the ball / hidden pet is gone.
- Peek is requested while the pet is visiting and the friend has peeking off: she stays visiting and the denial message shows.
- A peek or a game starting while a mail delivery visitor is on screen: games wait, peek and mail behave as today.

---

### Task 1: Games core and rock paper scissors

**Files:**
- Create: `crates/petlink/src/games.rs`
- Modify: `crates/petlink/src/main.rs` (add `mod games;`), `crates/petlink/src/pet.rs` (make `show_emote` public), `crates/petlink/src/social.rs` (`busy` no longer `#[cfg(test)]`), `crates/petlink/src/app.rs` (own a `Games`, update it in `step_social`, draw props)

**Interfaces:**
- Produces, in `games.rs`:
  - `pub enum Hand { Rock, Paper, Scissors }` (`Clone, Copy, PartialEq, Debug`)
  - `pub fn rps_winner(a: Hand, b: Hand) -> Option<bool>`: `Some(true)` when `a` wins, `Some(false)` when `b` wins, `None` on a tie.
  - `pub enum Who { A, B }` and `pub enum Prop { Ball { x: f32, y: f32 }, Hand { who: Who, hand: Hand, x: f32, y: f32 } }`
  - `pub struct Games`, `Games::new(k: f32, rng: &mut impl Rng) -> Games` (`k` divides the waits, as in `Social`)
  - `Games::update(&mut self, dt: f32, world: &World, a: &mut Pet, b: &mut Pet, px: f32, rng: &mut impl Rng, hearts: &mut Hearts, social_busy: bool)`
  - `Games::cancel(&mut self, a: &mut Pet, b: &mut Pet)` releases both pets and drops all props
  - `Games::busy(&self) -> bool`, `Games::props(&self) -> Vec<Prop>`, `Games::hidden(&self) -> Option<Who>` (used by Task 3)
- Consumes: `Pet::calm`, `Pet::linger`, `Pet::release`, `Pet::show_emote` (now `pub`), `Pet::headpatted` for the winner's hop, `Pet::blush` is not used.

- [ ] **Step 1: Write the failing tests** in `games.rs` `#[cfg(test)]`, using a rig like `social.rs` (`world()`, two `Pet`s on the ground at x 600 and 900, `StdRng::seed_from_u64(5)`):
  - `rps_winner_table`: all nine pairs, e.g. `rps_winner(Hand::Rock, Hand::Scissors) == Some(true)`, `rps_winner(Hand::Rock, Hand::Paper) == Some(false)`, `rps_winner(Hand::Paper, Hand::Paper) == None`.
  - `a_game_never_starts_while_social_is_busy`: with `social_busy = true`, tick 300 s: `busy()` stays false.
  - `a_game_starts_and_both_pets_end_calm`: with `social_busy = false`, tick until `busy()` is true (within 120 s), then until it is false again (within 60 s); afterwards both pets satisfy `calm()` after 5 more seconds, and `props()` is empty.
  - `cancel_releases_the_pets_and_clears_props`: start a game, call `cancel`, assert `!busy()`, `props().is_empty()`, and both pets `calm()` after 2 s.
- [ ] **Step 2: Run** `cargo test --release -p petlink games` and expect FAIL (module missing).
- [ ] **Step 3: Implement** `games.rs` with a scheduler (`next_in` random 20 to 45 s divided by `k`, counted down only while neither a game nor `social_busy`; when it hits zero and both pets are `calm()`, pick one of the games at random; only RPS exists in this task) and the RPS sequence: both pets `linger` facing each other for about 1.5 s, each picks a random `Hand`, `Prop::Hand` entries show above their heads for 1.5 s, then the winner gets `headpatted()` plus a heart burst via `hearts.burst`, the loser `show_emote(Emote::Dots)`, a tie repeats once, then both are `release`d.
- [ ] **Step 4: Wire into `app.rs`:** add `games: Games` to `PetApp` (created next to `Social::new` in `spawn_visitor` so a fresh visit gets a fresh scheduler, and in the constructor); in `step_social` pass `self.social.busy()` and call `self.games.update(...)` inside the same `Job::Visit` + `Stage::Working` + home condition; call `self.games.cancel(...)` wherever `self.visitor` is set to `None` or the visit ends or `bring_my_pet_home` runs; draw `Prop::Hand` as a small code-drawn rock (filled circle), paper (rounded rectangle) and scissors (two crossed lines) in a new `draw_props` called after the pets in the draw order.
- [ ] **Step 5: Run** `cargo test --release -p petlink` and expect all tests PASS.
- [ ] **Step 6: Commit** `feat(petlink): the pets play rock paper scissors when they meet`.

### Task 2: Fetch

**Files:**
- Modify: `crates/petlink/src/games.rs` (add `Game::Fetch`), `crates/petlink/src/app.rs` (draw `Prop::Ball`)

**Interfaces:**
- Consumes: `Games` from Task 1, `Pet::walk_then_linger(to_x, secs, face_x)`, `floor_span`, `Pet::headpatted`.
- Produces: `Prop::Ball { x, y }` while a ball exists.

- [ ] **Step 1: Write the failing tests** in `games.rs`: `fetch_ball_lands_on_the_floor_inside_the_span` (force the fetch game through a test-only `Games::start(Game::Fetch, ..)`, tick, assert the `Prop::Ball` y never goes below the floor y and x stays inside `floor_span`); `fetch_ends_with_the_ball_gone_and_pets_calm` (full run under 60 s, `props()` empty afterwards, both `calm()`).
- [ ] **Step 2: Run** `cargo test --release -p petlink fetch` and expect FAIL.
- [ ] **Step 3: Implement** the fetch sequence: the thrower stands still 0.5 s, the ball flies in an arc to a random x inside the floor span at 150 to 400 points distance and rests, the other pet `walk_then_linger` to it, the ball then follows that pet's x back to the thrower, a heart burst, and the roles are done; add `Game::Fetch` to the random pick.
- [ ] **Step 4: Draw** `Prop::Ball` as a filled circle in `draw_props`.
- [ ] **Step 5: Run** `cargo test --release -p petlink`, expect PASS, then commit `feat(petlink): the pets play fetch`.

### Task 3: Hide-and-seek

**Files:**
- Modify: `crates/petlink/src/games.rs` (add `Game::Hide`), `crates/petlink/src/app.rs` (skip drawing the hidden pet and its emote)

**Interfaces:**
- Consumes: `Games::hidden() -> Option<Who>` declared in Task 1 (returns `None` until now).
- Produces: `Games::hidden()` returns `Some(Who::A | Who::B)` while that pet is hiding.

- [ ] **Step 1: Write the failing tests:** `hide_and_seek_hides_one_pet_then_shows_it_again` (while the game runs `hidden()` is `Some(..)` for at least 3 s and `None` before and after); `cancel_during_hide_shows_the_pet_again` (`cancel` mid-hide gives `hidden() == None`).
- [ ] **Step 2: Run** `cargo test --release -p petlink hide` and expect FAIL.
- [ ] **Step 3: Implement** the sequence: the hider `walk_then_linger`s to the far end of its floor span and becomes hidden (not drawn) for 4 to 8 s, the seeker walks toward the spot, finds it, both get a heart burst, the hider reappears, everything is released; add `Game::Hide` to the random pick.
- [ ] **Step 4: In `app.rs` draw order,** skip `draw_pet` and `draw_emote` for the pet `games.hidden()` names (A is `mine`, B is the visitor); a hidden pet must still be saveable from a call-home (it simply returns, `cancel` runs).
- [ ] **Step 5: Run** `cargo test --release -p petlink`, expect PASS, commit `feat(petlink): the pets play hide-and-seek`.

### Task 4: Automatic visits

**Files:**
- Modify: `crates/petlink/src/protocol.rs` (`VisitStart { hue, auto }`), `crates/petlink/src/config.rs` (`wander: bool`), `crates/petlink/src/tray.rs` (`TrayAction::ToggleWander`, check item "Let pets wander", `Cmd::Wander(bool)`, `Tray::set_wander(&self, on: bool)`), `crates/petlink/src/app.rs`

**Interfaces:**
- Consumes: `start_visit`, `Trip`, `my_pet_away`, `can_interact`, `bring_my_pet_home`, `Msg::VisitStart { hue }` from the earlier bow-colour fix.
- Produces: `const AUTO_VISIT_GAP_S: std::ops::Range<f32> = 120.0..240.0`, `const AUTO_VISIT_STAY_S: std::ops::Range<f32> = 60.0..120.0`, `fn wander_ok(&self) -> bool` on `PetApp` (online, not away, not fullscreen, no visitor present, `config.wander`), fields `next_auto_in: f32`, `auto_trip: bool`, `auto_leave_at: Option<Instant>`.

- [ ] **Step 1: Write the failing tests** for the pure parts: in `protocol.rs`, `visit_start_auto_round_trip` (serialises with `auto: true`, an old-format JSON without `auto` still parses with `auto == false`, and `skip_serializing_if` leaves it out when false); in `config.rs`, `wander_defaults_on_and_is_kept` (like the existing peek default test); in `app.rs` tests, a pure helper `fn auto_gap(rng) -> f32` returns values inside `AUTO_VISIT_GAP_S`.
- [ ] **Step 2: Run** `cargo test --release -p petlink` and expect FAIL.
- [ ] **Step 3: Implement the field, config switch and tray item** (mirror how "Let friend peek" is added: `TrayAction::ToggleWander`, saved to config, pushed to the tray on connect).
- [ ] **Step 4: Implement the timer in `logic`:** count `next_auto_in` down while `wander_ok()` and `trip == Trip::None` and the pet is at home; at zero set `auto_trip = true` and call `start_visit()`; when the pet arrives (`Trip::Visit` branch of `on_my_pet_at_door`) send `VisitStart { hue, auto: self.auto_trip }` and set `auto_leave_at` to now plus a random `AUTO_VISIT_STAY_S`; when it passes, call `start_visit()` (which sends `VisitEnd` and brings her home); reset `next_auto_in` with `auto_gap` after every visit ends.
- [ ] **Step 5: Handle the receiving side:** in `Msg::VisitStart { auto: true, .. }`, if `self.hidden || self.away || !self.config.wander`, answer `Msg::VisitEnd` immediately and do not queue the visit; and if my own pet is still walking to the door on an automatic trip (`self.trip == Trip::Visit && self.auto_trip`), cancel it (`self.trip = Trip::None`, `self.auto_trip = false`, `self.mine.release()`) so only one pet ends up away.
- [ ] **Step 6: Clear `auto_trip` and `auto_leave_at`** in `bring_my_pet_home`, so a manual call-home or a dropped link also ends the automatic visit.
- [ ] **Step 7: Run** `cargo test --release -p petlink`, expect PASS, commit `feat(petlink): pets visit each other on their own`.

### Task 5: Peek while visiting

**Files:**
- Modify: `crates/petlink/src/app.rs`

**Interfaces:**
- Consumes: `start_peek`, `peek_pending`, `Msg::PeekRequest`, the door menu code (`Some(1) => self.start_peek()` and its enable condition), the host's existing in-place peek (`pump_jobs`, `Job::Peek`, `step_visitor`).
- Produces: field `peek_from_visit: bool`, set when a peek is asked while `my_pet_away` and `trip == Trip::None`.

- [ ] **Step 1: Write the failing tests** by extracting the decision into a pure function `fn peek_plan(away: bool, peek_pending: bool, friend_peek: bool, can_interact: bool) -> PeekPlan` with `enum PeekPlan { WalkFromHome, AskInPlace, No }` and test: home plus allowed gives `WalkFromHome`; away plus allowed gives `AskInPlace`; away with a peek already pending gives `No`; friend peeking off gives `No`.
- [ ] **Step 2: Run** `cargo test --release -p petlink peek_plan` and expect FAIL.
- [ ] **Step 3: Implement `start_peek`** with `peek_plan`: `AskInPlace` creates the `peek_id`, sets `peek_pending` with the existing 30 s deadline, sets `peek_from_visit = true`, and sends `PeekRequest`; the pet is not moved. Extend the door menu enable condition for "peek" with the same function.
- [ ] **Step 4: Change the result and timeout paths:** in the `PeekResult` handler and the deadline check in `pump_captures`, call `bring_my_pet_home()` only when `peek_from_visit` is false; always clear `peek_pending` and reset `peek_from_visit`; the denial and timeout bubbles stay as they are.
- [ ] **Step 5: Check by hand on the host side** that a `Job::Peek` arriving while the visitor is `Job::Visit` + `Stage::Working` uses the in-place branch of `pump_jobs` and that after `Event::PeekDone` the visitor stays (`step_visitor` only sets `Stage::Leaving` for `Job::Peek`); add a test only if that needs a change.
- [ ] **Step 6: Run** `cargo test --release -p petlink`, expect PASS, commit `feat(petlink): peek without sending the visiting pet home`.

### Task 6: Release

- [ ] **Step 1:** `cargo build --release -p petlink` and `cargo test --release -p petlink` both clean; open a PR to `main`, wait for CI, merge when green (the repository's CI publishes the `node-build-N` release that friends self-update from).
- [ ] **Step 2:** Swap the new exe into the owner's `Downloads\PetLink(1).exe` with the invite trailer kept (as done earlier), start it, and watch the log for a visit, a game and one peek while visiting.
