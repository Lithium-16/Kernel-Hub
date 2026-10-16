//! The overlay: a transparent, click-through window over the primary monitor with the pets,
//! the door and the parcels drawn on it.

use crate::config::Config;
use crate::mailui::{self, Compose, MailAction};
use crate::net::{Net, NetCmd, NetEvent};
use crate::pet::{Event, Pet, State};
use crate::protocol::{Envelope, FileRef, Msg, Person, PresenceState, TouchKind, new_id};
use crate::sprites::Sprites;
use crate::tray::{Tray, TrayAction};
use crate::win;
use crate::world::{Rect, World};
use egui::{
    Align2, Color32, ColorImage, FontId, Pos2, Stroke, TextureHandle, TextureOptions, Vec2,
};
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::PathBuf;
use std::time::{Duration, Instant};

const IDLE_AWAY_SECS: u64 = 300;
const DOOR_W: f32 = 20.0;
const DOOR_H: f32 = 28.0;
const BUBBLE_S: u64 = 4;

/// The pet's pictures as textures (cheap to clone).
#[derive(Clone)]
pub struct Tex {
    walk: Vec<TextureHandle>,
    idle: TextureHandle,
    sit: TextureHandle,
    sleep: TextureHandle,
    carry: TextureHandle,
    parcel: TextureHandle,
}

impl Tex {
    fn new(ctx: &egui::Context, name: &str, s: &Sprites) -> Tex {
        let load = |n: &str, img: &image::RgbaImage| {
            ctx.load_texture(
                format!("{name}-{n}"),
                ColorImage::from_rgba_unmultiplied(
                    [img.width() as usize, img.height() as usize],
                    img.as_raw(),
                ),
                TextureOptions::NEAREST,
            )
        };
        Tex {
            walk: s.walk.iter().map(|f| load("walk", f)).collect(),
            idle: load("idle", &s.idle),
            sit: load("sit", &s.sit),
            sleep: load("sleep", &s.sleep),
            carry: load("carry", &s.carry),
            parcel: load("parcel", &s.parcel),
        }
    }
}

/// Received mail waiting by the door.
pub struct Parcel {
    pub mail: Envelope,
    pub x: f32,
}

/// What a visiting pet is here to do.
#[derive(Clone)]
enum Job {
    Deliver(Envelope),
    Peek(String),
    Visit,
}

#[derive(Clone, Copy, PartialEq)]
enum Stage {
    Entering,
    Working,
    Leaving,
}

struct Visitor {
    pet: Pet,
    job: Job,
    stage: Stage,
    /// Set while the pet poses with the magnifier for this request.
    peek_id: Option<String>,
}

#[derive(Clone, Copy, PartialEq)]
enum Target {
    Mine,
    Visitor,
    Door,
    Parcel(usize),
    /// A letter of mine that failed to go out.
    Outgoing(usize),
}

struct DragState {
    target: Target,
    start: Pos2,
    grab_off: Vec2,
    grabbed: bool,
    last: Vec<(Instant, Pos2)>,
}

/// Why my pet is walking to the door.
#[derive(Clone, Copy, PartialEq)]
enum Trip {
    None,
    Visit,
    #[allow(dead_code)]
    Peek,
    Mail,
}

/// A letter of mine on its way out: files are uploaded first, then the mail is sent.
struct Outgoing {
    mail: Envelope,
    pending: HashSet<String>,
    failed: HashSet<String>,
    sources: HashMap<String, (String, PathBuf)>,
    handed_in: bool,
    sent: bool,
}

pub struct PetApp {
    config: Config,
    data_dir: PathBuf,
    net: Net,
    tray: Tray,
    me: Option<Person>,
    friend: Option<Person>,
    friend_online: bool,
    friend_away: bool,
    friend_peek: bool,
    my_peek: bool,
    mine: Pet,
    my_tex: Tex,
    my_hue: u16,
    my_tex_dirty: bool,
    friend_tex: Option<Tex>,
    friend_hue: u16,
    visitor: Option<Visitor>,
    jobs: VecDeque<Job>,
    my_pet_away: bool,
    mine_back_at: Option<Instant>,
    trip: Trip,
    parcels: Vec<Parcel>,
    outgoing: Vec<Outgoing>,
    carrying: Option<String>,
    world: World,
    world_refreshed: Instant,
    passthrough: bool,
    drag: Option<DragState>,
    door_menu: Option<Pos2>,
    bubbles: Vec<(String, Instant)>,
    away: bool,
    hidden: bool,
    #[cfg(debug_assertions)]
    autotest_done: HashSet<String>,
    last_screen: Rect,
    ppp: f32,
    windows: mailui::Windows,
    ctx: egui::Context,
}

fn world_now(ppp: f32) -> World {
    let r = |r: Rect| Rect {
        x: r.x / ppp,
        y: r.y / ppp,
        w: r.w / ppp,
        h: r.h / ppp,
    };
    let windows: Vec<Rect> = win::top_windows().into_iter().map(r).collect();
    World::build(
        r(win::screen()),
        win::taskbar().map(r),
        &windows,
        win::tray_left() / ppp,
    )
}

fn inside(r: Rect, p: Pos2) -> bool {
    r.contains(p.x, p.y)
}

fn to_egui(r: Rect) -> egui::Rect {
    egui::Rect::from_min_size(Pos2::new(r.x, r.y), Vec2::new(r.w, r.h))
}

pub fn run(config: Config, data_dir: PathBuf) -> eframe::Result<()> {
    let screen = win::screen();
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("PetLink")
            .with_position([screen.x, screen.y])
            // One pixel short, so Windows never mistakes the overlay for a fullscreen app
            // (which would hide the taskbar when the overlay takes focus).
            .with_inner_size([screen.w, screen.h - 1.0])
            .with_decorations(false)
            .with_transparent(true)
            .with_always_on_top()
            .with_taskbar(false)
            .with_mouse_passthrough(true)
            .with_resizable(false),
        ..Default::default()
    };
    eframe::run_native(
        "PetLink",
        options,
        Box::new(move |cc| Ok(Box::new(PetApp::new(cc, config, data_dir)?))),
    )
}

impl PetApp {
    fn new(
        cc: &eframe::CreationContext<'_>,
        config: Config,
        data_dir: PathBuf,
    ) -> anyhow::Result<Self> {
        let ctx = cc.egui_ctx.clone();
        let net = Net::start(config.clone(), data_dir.clone(), move || {
            ctx.request_repaint()
        });
        let tray = Tray::new(false)?;
        let ppp = cc.egui_ctx.pixels_per_point();
        let world = world_now(ppp);
        let my_hue = config.bow_hue.unwrap_or(0);
        let my_tex = Tex::new(&cc.egui_ctx, "me", &Sprites::load(my_hue)?);
        let mine = Pet::new(world.screen.w * 0.5, world.ground());
        let mut app = PetApp {
            config,
            data_dir,
            net,
            tray,
            me: None,
            friend: None,
            friend_online: false,
            friend_away: false,
            friend_peek: false,
            my_peek: false,
            mine,
            my_tex,
            my_hue,
            my_tex_dirty: false,
            friend_tex: None,
            friend_hue: 0,
            visitor: None,
            jobs: VecDeque::new(),
            my_pet_away: false,
            mine_back_at: None,
            trip: Trip::None,
            parcels: Vec::new(),
            outgoing: Vec::new(),
            carrying: None,
            last_screen: Rect {
                x: 0.0,
                y: 0.0,
                w: 0.0,
                h: 0.0,
            },
            world,
            world_refreshed: Instant::now(),
            passthrough: true,
            drag: None,
            door_menu: None,
            bubbles: Vec::new(),
            away: false,
            hidden: false,
            #[cfg(debug_assertions)]
            autotest_done: HashSet::new(),
            ppp,
            windows: mailui::Windows::default(),
            ctx: cc.egui_ctx.clone(),
        };
        app.load_parcels();
        Ok(app)
    }

    fn parcels_path(&self) -> PathBuf {
        self.data_dir.join("parcels.json")
    }

    /// Unopened letters survive a restart.
    fn load_parcels(&mut self) {
        let mail: Vec<Envelope> = std::fs::read_to_string(self.parcels_path())
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default();
        let door = self.world.door_x;
        self.parcels = mail
            .into_iter()
            .enumerate()
            .map(|(i, mail)| Parcel {
                mail,
                x: door - 70.0 - 24.0 * i as f32,
            })
            .collect();
    }

    fn save_parcels(&self) {
        let mail: Vec<&Envelope> = self.parcels.iter().map(|p| &p.mail).collect();
        if let Err(e) = std::fs::write(
            self.parcels_path(),
            serde_json::to_vec(&mail).unwrap_or_default(),
        ) {
            tracing::warn!("couldn't save parcels: {e}");
        }
    }

    /// Points per sprite pixel.
    fn px(&self) -> f32 {
        f32::from(self.config.scale) / self.ppp
    }

    fn bubble(&mut self, text: impl Into<String>) {
        let text = text.into();
        tracing::info!("bubble: {text}");
        self.bubbles.push((text, Instant::now()));
        if self.bubbles.len() > 3 {
            self.bubbles.remove(0);
        }
    }

    fn friend_name(&self) -> String {
        self.friend
            .as_ref()
            .map_or_else(|| "your friend".into(), |f| f.name.clone())
    }

    fn friend_id(&self) -> String {
        self.friend
            .as_ref()
            .map(|f| f.id.clone())
            .unwrap_or_default()
    }

    fn send_to_friend(&self, msg: Msg) {
        let _ = self
            .net
            .cmds
            .send(NetCmd::Send(Envelope::new(&self.friend_id(), msg)));
    }

    fn send_to_relay(&self, msg: Msg) {
        let _ = self.net.cmds.send(NetCmd::Send(Envelope::new("", msg)));
    }

    fn my_pet_hidden(&self) -> bool {
        self.my_pet_away || self.mine_back_at.is_some()
    }

    fn bring_my_pet_home(&mut self) {
        self.trip = Trip::None;
        if self.my_pet_away {
            self.my_pet_away = false;
            self.mine.enter_from_door(&self.world);
        }
        self.tray.set_visiting(false);
    }

    fn can_interact(&self) -> bool {
        self.friend.is_some() && self.friend_online
    }

    // ------------------------------------------------------------------ network

    fn on_net_event(&mut self, ev: NetEvent) {
        match ev {
            NetEvent::Connected { me, friends } => {
                tracing::info!("connected as {} with {} friend(s)", me.name, friends.len());
                self.my_peek = me.peek;
                self.tray.set_peek(me.peek);
                let hue = self.config.bow_hue.unwrap_or(me.bow_hue);
                if hue != self.my_hue {
                    self.my_hue = hue;
                    self.my_tex_dirty = true;
                }
                self.friend = friends.into_iter().next();
                if let Some(f) = &self.friend {
                    self.friend_online = f.online;
                    self.friend_peek = f.peek;
                    self.tray.set_friend(&f.name);
                    if self.friend_hue != f.bow_hue {
                        self.friend_hue = f.bow_hue;
                        self.friend_tex = None;
                    }
                }
                self.me = Some(me);
                if self.away {
                    self.send_presence(true);
                }
            }
            NetEvent::Disconnected => {
                tracing::info!("disconnected from the relay");
                self.friend_online = false;
                self.visitor = None;
                self.jobs.clear();
                if self.my_pet_away {
                    self.bubble("Lost the connection; your pet is home");
                }
                self.bring_my_pet_home();
            }
            NetEvent::Message(env) => self.on_message(env),
            NetEvent::Upgrade => self.bubble("Please update PetLink"),
            NetEvent::Uploaded { file_id } => {
                tracing::info!("uploaded {file_id}");
                for o in &mut self.outgoing {
                    o.pending.remove(&file_id);
                }
            }
            NetEvent::UploadFailed { file_id, error } => {
                tracing::warn!("upload {file_id} failed: {error}");
                for o in &mut self.outgoing {
                    if o.pending.remove(&file_id) {
                        o.failed.insert(file_id.clone());
                    }
                }
            }
        }
    }

    fn send_presence(&self, away: bool) {
        let state = if away {
            PresenceState::Away
        } else {
            PresenceState::Online
        };
        self.send_to_relay(Msg::Presence { state });
    }

    /// The friend dropped off: their pet vanishes and mine comes home.
    fn friend_gone(&mut self) {
        self.friend_online = false;
        self.visitor = None;
        self.jobs.clear();
        if self.my_pet_away {
            let msg = format!("{} is offline", self.friend_name());
            self.bubble(msg);
        }
        self.bring_my_pet_home();
    }

    fn on_message(&mut self, env: Envelope) {
        if let Some(f) = &self.friend
            && !env.from.is_empty()
            && env.from != f.id
        {
            tracing::debug!("ignoring {} from a stranger", env.id);
            return;
        }
        tracing::info!(
            "message: {}",
            serde_json::to_string(&env.msg).unwrap_or_default()
        );
        match env.msg.clone() {
            Msg::Presence { state } => {
                self.friend_away = state == PresenceState::Away;
                if let Some(v) = &mut self.visitor {
                    v.pet.set_owner_away(self.friend_away);
                }
                if state == PresenceState::Offline {
                    self.friend_gone();
                } else {
                    self.friend_online = true;
                }
            }
            Msg::PeekAllow { on } => self.friend_peek = on,
            Msg::Mail { .. } => self.jobs.push_back(Job::Deliver(env)),
            Msg::MailRead { .. } => self.mine.pat(),
            Msg::Delivered { mail_id } => {
                self.outgoing.retain(
                    |o| !matches!(&o.mail.msg, Msg::Mail { mail_id: m, .. } if *m == mail_id),
                );
            }
            Msg::PeekRequest { peek_id } => {
                if self.my_peek {
                    self.jobs.push_back(Job::Peek(peek_id));
                } else {
                    self.send_to_friend(Msg::PeekResult {
                        peek_id,
                        file_id: String::new(),
                        denied: true,
                    });
                }
            }
            Msg::VisitStart => {
                let has_visit = self
                    .visitor
                    .as_ref()
                    .is_some_and(|v| matches!(v.job, Job::Visit))
                    || self.jobs.iter().any(|j| matches!(j, Job::Visit));
                if !has_visit {
                    self.jobs.push_back(Job::Visit);
                }
            }
            Msg::VisitEnd => {
                self.jobs.retain(|j| !matches!(j, Job::Visit));
                if let Some(v) = &mut self.visitor
                    && matches!(v.job, Job::Visit)
                {
                    v.stage = Stage::Leaving;
                    v.pet.exit_to_door();
                }
                if self.my_pet_away {
                    self.bring_my_pet_home();
                }
            }
            Msg::Touch { what } => {
                let verb = match what {
                    TouchKind::Pat => "patted",
                    TouchKind::Poke => "poked",
                    TouchKind::Yeet => "yeeted",
                };
                let msg = format!("{} {verb} your pet", self.friend_name());
                self.bubble(msg);
            }
            Msg::Error { code, message, .. } => {
                self.bubble(message);
                if code == "offline" && self.my_pet_away {
                    self.bring_my_pet_home();
                }
            }
            _ => {}
        }
    }

    // ------------------------------------------------------------------ the frame

    fn ensure_textures(&mut self, ctx: &egui::Context) {
        if self.my_tex_dirty {
            self.my_tex_dirty = false;
            if let Ok(s) = Sprites::load(self.my_hue) {
                self.my_tex = Tex::new(ctx, "me", &s);
            }
        }
        if self.friend_tex.is_none()
            && let Ok(s) = Sprites::load(self.friend_hue)
        {
            self.friend_tex = Some(Tex::new(ctx, "friend", &s));
        }
    }

    fn spawn_visitor(&mut self, job: Job) {
        let mut pet = Pet::new(self.world.door_x, self.world.ground());
        pet.enter_from_door(&self.world);
        pet.set_owner_away(self.friend_away);
        self.visitor = Some(Visitor {
            pet,
            job,
            stage: Stage::Entering,
            peek_id: None,
        });
    }

    /// Turns queued jobs into a visiting pet; a pet that is already visiting takes them in place.
    fn pump_jobs(&mut self) {
        while let Some(job) = self.jobs.front().cloned() {
            match &mut self.visitor {
                None => {
                    self.jobs.pop_front();
                    self.spawn_visitor(job);
                    return;
                }
                Some(v) if matches!(v.job, Job::Visit) && v.stage == Stage::Working => {
                    match job {
                        Job::Deliver(mail) => self.parcels.push(Parcel { mail, x: v.pet.x }),
                        Job::Peek(id) => {
                            v.pet.peek();
                            v.peek_id = Some(id);
                        }
                        Job::Visit => {}
                    }
                    self.jobs.pop_front();
                    self.save_parcels();
                }
                Some(_) => return,
            }
        }
    }

    fn step_visitor(&mut self, dt: f32, rng: &mut impl rand::Rng) {
        let Some(v) = &mut self.visitor else { return };
        let ev = v.pet.step(dt, &self.world, rng);
        match v.stage {
            Stage::Entering if v.pet.state != State::Enter => {
                v.stage = Stage::Working;
                match v.job.clone() {
                    Job::Deliver(mail) => {
                        self.parcels.push(Parcel { mail, x: v.pet.x });
                        v.stage = Stage::Leaving;
                        v.pet.exit_to_door();
                        self.save_parcels();
                    }
                    Job::Peek(id) => {
                        v.pet.peek();
                        v.peek_id = Some(id);
                    }
                    Job::Visit => {}
                }
            }
            Stage::Working => {
                if ev == Some(Event::PeekDone)
                    && let Some(id) = v.peek_id.take()
                {
                    let leave = matches!(v.job, Job::Peek(_));
                    if leave {
                        v.stage = Stage::Leaving;
                        v.pet.exit_to_door();
                    }
                    self.on_peek_done(id);
                }
            }
            Stage::Leaving => {
                if ev == Some(Event::ReachedDoor) {
                    self.visitor = None;
                } else if !matches!(
                    v.pet.state,
                    State::Exit | State::Fall | State::Thrown | State::Held
                ) {
                    v.pet.exit_to_door(); // knocked off course (patted, dizzy): keep going
                }
            }
            Stage::Entering => {}
        }
    }

    /// Placeholder until the capture is wired in.
    fn on_peek_done(&mut self, _peek_id: String) {}

    fn step_mine(&mut self, dt: f32, rng: &mut impl rand::Rng) {
        if let Some(t) = self.mine_back_at
            && Instant::now() >= t
        {
            self.mine_back_at = None;
            self.mine.enter_from_door(&self.world);
        }
        if self.my_pet_hidden() {
            return;
        }
        if self.trip != Trip::None
            && !matches!(
                self.mine.state,
                State::Exit | State::Carry | State::Fall | State::Thrown | State::Held
            )
        {
            // knocked off course (patted, dizzy): keep going
            if self.trip == Trip::Mail {
                self.mine.carry_to_door();
            } else {
                self.mine.exit_to_door();
            }
        }
        if self.mine.step(dt, &self.world, rng) == Some(Event::ReachedDoor) {
            self.on_my_pet_at_door();
        }
    }

    fn on_my_pet_at_door(&mut self) {
        match std::mem::replace(&mut self.trip, Trip::None) {
            Trip::Visit => {
                self.my_pet_away = true;
                self.send_to_friend(Msg::VisitStart);
                self.tray.set_visiting(true);
            }
            Trip::Peek => self.on_my_pet_at_door_for_peek(),
            Trip::Mail => {
                // Goes in through the door; the mail leaves once its files are up.
                if let Some(id) = self.carrying.take() {
                    self.hand_in(&id);
                }
                self.mine_back_at = Some(Instant::now() + Duration::from_millis(1500));
            }
            Trip::None => {}
        }
    }

    fn on_my_pet_at_door_for_peek(&mut self) {}

    fn start_visit(&mut self) {
        if self.my_pet_away {
            self.send_to_friend(Msg::VisitEnd);
            self.bring_my_pet_home();
        } else if self.can_interact() && self.trip == Trip::None && !self.my_pet_hidden() {
            self.trip = Trip::Visit;
            self.mine.exit_to_door();
        }
    }

    fn on_tray(&mut self, ctx: &egui::Context, a: TrayAction) {
        match a {
            TrayAction::WriteLetter => self.open_compose(),
            TrayAction::TogglePeek => {
                self.my_peek = !self.my_peek;
                self.tray.set_peek(self.my_peek);
                self.send_to_relay(Msg::PeekAllow { on: self.my_peek });
            }
            TrayAction::Visit => self.start_visit(),
            TrayAction::BowColour(h) => {
                self.config.bow_hue = Some(h);
                self.save_config();
                self.my_hue = h;
                self.my_tex_dirty = true;
            }
            TrayAction::Scale(n) => {
                self.config.scale = n;
                self.save_config();
            }
            TrayAction::Autostart => {}
            TrayAction::CheckUpdates => self.check_updates(),
            TrayAction::Quit => {
                let _ = self.net.cmds.send(NetCmd::Shutdown);
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
    }

    fn save_config(&self) {
        if let Err(e) = self.config.save(&self.data_dir) {
            tracing::warn!("couldn't save settings: {e:#}");
        }
    }

    /// Debug builds only: PETLINK_AUTOTEST=a,b,c fires each step once when it can, so two
    /// clients can be exercised from logs without clicking.
    #[cfg(debug_assertions)]
    fn autotest(&mut self) {
        let Some(list) = std::env::var_os("PETLINK_AUTOTEST") else {
            return;
        };
        for step in list.to_string_lossy().split(',') {
            if self.autotest_done.contains(step) {
                continue;
            }
            let ready = match step {
                "peek-on" => self.me.is_some(),
                "send-mail" => {
                    self.can_interact() && self.trip == Trip::None && !self.my_pet_hidden()
                }
                "open-mail" => !self.parcels.is_empty(),
                _ => false,
            };
            if !ready {
                continue;
            }
            tracing::info!("autotest: {step}");
            self.autotest_done.insert(step.to_owned());
            match step {
                "peek-on" if !self.my_peek => {
                    self.my_peek = true;
                    self.tray.set_peek(true);
                    self.send_to_relay(Msg::PeekAllow { on: true });
                }
                "send-mail" => {
                    let dir = self.data_dir.join("autotest");
                    let _ = std::fs::create_dir_all(&dir);
                    let png = dir.join("autotest.png");
                    let img = image::RgbaImage::from_fn(96, 64, |x, y| {
                        image::Rgba([(x * 2) as u8, (y * 4) as u8, 160, 255])
                    });
                    let _ = img.save(&png);
                    let bat = dir.join("autotest.bat");
                    let _ = std::fs::write(&bat, "@echo hello from autotest\r\n");
                    self.send_letter("hello from autotest".into(), vec![png, bat]);
                }
                "open-mail" => {
                    self.windows.autosave = true;
                    let ctx = self.ctx.clone();
                    self.open_parcel(&ctx, 0);
                }
                _ => {}
            }
        }
    }

    fn open_compose(&mut self) {
        self.windows.compose.get_or_insert_with(Compose::default);
    }

    fn hand_in(&mut self, mail_id: &str) {
        for o in &mut self.outgoing {
            if matches!(&o.mail.msg, Msg::Mail { mail_id: m, .. } if m == mail_id) {
                o.handed_in = true;
            }
        }
    }

    /// Reads the files, starts their uploads and sends my pet to the door with the parcel.
    fn send_letter(&mut self, text: String, paths: Vec<PathBuf>) {
        let to = self.friend_id();
        let mail_id = new_id();
        let mut refs = Vec::new();
        let mut o = Outgoing {
            mail: Envelope::new(
                &to,
                Msg::Mail {
                    mail_id: mail_id.clone(),
                    text: String::new(),
                    files: vec![],
                },
            ),
            pending: HashSet::new(),
            failed: HashSet::new(),
            sources: HashMap::new(),
            handed_in: false,
            sent: false,
        };
        for p in paths {
            let name = p
                .file_name()
                .map_or_else(|| "file".into(), |n| n.to_string_lossy().into_owned());
            let bytes = match std::fs::read(&p) {
                Ok(b) => b,
                Err(e) => {
                    self.bubble(format!("Could not read {name}: {e}"));
                    return;
                }
            };
            let file_id = format!("f_{}", ulid::Ulid::generate());
            let mime = mime_of(&name).to_owned();
            refs.push(FileRef {
                file_id: file_id.clone(),
                name: name.clone(),
                size: bytes.len() as u64,
                mime,
            });
            o.pending.insert(file_id.clone());
            o.sources.insert(file_id.clone(), (name.clone(), p));
            let _ = self.net.cmds.send(NetCmd::Upload {
                file_id,
                to: to.clone(),
                name,
                bytes,
            });
        }
        o.mail.msg = Msg::Mail {
            mail_id: mail_id.clone(),
            text,
            files: refs,
        };
        tracing::info!("sending letter {mail_id}");
        let can_carry = self.trip == Trip::None && !self.my_pet_hidden();
        o.handed_in = !can_carry;
        self.outgoing.push(o);
        if can_carry {
            self.carrying = Some(mail_id);
            self.trip = Trip::Mail;
            self.mine.carry_to_door();
        }
    }

    /// Sends every letter whose files are all up and that has been handed in at the door.
    fn pump_outgoing(&mut self) {
        let mut sends = Vec::new();
        for o in &mut self.outgoing {
            if o.handed_in && !o.sent && o.pending.is_empty() && o.failed.is_empty() {
                o.sent = true;
                sends.push(o.mail.clone());
            }
        }
        for m in sends {
            let _ = self.net.cmds.send(NetCmd::Send(m));
        }
    }

    fn retry_outgoing(&mut self, i: usize) {
        let to = self.friend_id();
        let Some(o) = self.outgoing.get_mut(i) else {
            return;
        };
        for id in std::mem::take(&mut o.failed) {
            let Some((name, path)) = o.sources.get(&id).cloned() else {
                continue;
            };
            match std::fs::read(&path) {
                Ok(bytes) => {
                    o.pending.insert(id.clone());
                    let _ = self.net.cmds.send(NetCmd::Upload {
                        file_id: id,
                        to: to.clone(),
                        name,
                        bytes,
                    });
                }
                Err(_) => {
                    o.failed.insert(id);
                }
            }
        }
    }

    fn check_updates(&mut self) {}

    fn door_rect(&self) -> Rect {
        let px = self.px();
        Rect {
            x: self.world.door_x - DOOR_W * px / 2.0,
            y: self.world.ground() - DOOR_H * px,
            w: DOOR_W * px,
            h: DOOR_H * px,
        }
    }

    fn parcel_rect(&self, x: f32) -> Rect {
        let px = self.px();
        Rect {
            x: x - 7.5 * px,
            y: self.world.ground() - 12.0 * px,
            w: 15.0 * px,
            h: 12.0 * px,
        }
    }

    fn target_at(&self, p: Pos2) -> Option<Target> {
        let px = self.px();
        if !self.my_pet_hidden() && inside(self.mine.hitbox(px), p) {
            return Some(Target::Mine);
        }
        if self
            .visitor
            .as_ref()
            .is_some_and(|v| inside(v.pet.hitbox(px), p))
        {
            return Some(Target::Visitor);
        }
        if let Some(i) = self
            .parcels
            .iter()
            .rposition(|pc| inside(self.parcel_rect(pc.x), p))
        {
            return Some(Target::Parcel(i));
        }
        if let Some(i) = self.outgoing.iter().position(|o| {
            !o.failed.is_empty() && inside(self.parcel_rect(self.failed_x(&o.mail)), p)
        }) {
            return Some(Target::Outgoing(i));
        }
        inside(self.door_rect(), p).then_some(Target::Door)
    }

    /// Where a failed letter of mine sits: left of the door.
    fn failed_x(&self, mail: &Envelope) -> f32 {
        let i = self
            .outgoing
            .iter()
            .position(|o| o.mail.id == mail.id)
            .unwrap_or(0);
        self.world.door_x - 30.0 - 22.0 * i as f32
    }

    fn cursor_pt(&self) -> Pos2 {
        let (x, y) = win::cursor();
        Pos2::new(x / self.ppp, y / self.ppp)
    }

    /// The pointer part of the frame: dragging, flinging, patting, clicking the door.
    fn handle_pointer(&mut self, ctx: &egui::Context) {
        let (pressed, down) =
            ctx.input(|i| (i.pointer.primary_pressed(), i.pointer.primary_down()));
        let cur = self.cursor_pt();
        let now = Instant::now();
        if self.door_menu.is_none()
            && self.drag.is_none()
            && pressed
            && !self.hidden
            && let Some(target) = self.target_at(cur)
        {
            let feet = match target {
                Target::Mine => Some(Pos2::new(self.mine.x, self.mine.y)),
                Target::Visitor => self.visitor.as_ref().map(|v| Pos2::new(v.pet.x, v.pet.y)),
                _ => None,
            };
            self.drag = Some(DragState {
                target,
                start: cur,
                grab_off: feet.map_or(Vec2::ZERO, |f| cur - f),
                grabbed: false,
                last: Vec::new(),
            });
        }
        let Some(mut d) = self.drag.take() else {
            return;
        };
        d.last.push((now, cur));
        d.last
            .retain(|(t, _)| now.duration_since(*t) <= Duration::from_millis(80));
        let pet = match d.target {
            Target::Mine => Some(&mut self.mine),
            Target::Visitor => self.visitor.as_mut().map(|v| &mut v.pet),
            _ => None,
        };
        if let Some(pet) = pet {
            if !d.grabbed && cur.distance(d.start) > 4.0 {
                pet.grab();
                d.grabbed = true;
            }
            if d.grabbed {
                pet.drag_to(cur.x - d.grab_off.x, cur.y - d.grab_off.y);
            }
        }
        if down {
            self.drag = Some(d);
            return;
        }
        // Released.
        if d.grabbed {
            let (vx, vy) = match (d.last.first(), d.last.last()) {
                (Some((t0, p0)), Some((t1, p1))) if t1 > t0 => {
                    let dt = t1.duration_since(*t0).as_secs_f32().max(0.016);
                    ((p1.x - p0.x) / dt, (p1.y - p0.y) / dt)
                }
                _ => (0.0, 0.0),
            };
            match d.target {
                Target::Mine => self.mine.throw(vx, vy),
                Target::Visitor => {
                    if let Some(v) = &mut self.visitor {
                        v.pet.throw(vx, vy);
                    }
                    self.send_to_friend(Msg::Touch {
                        what: TouchKind::Yeet,
                    });
                }
                _ => {}
            }
            return;
        }
        match d.target {
            Target::Mine => self.mine.pat(),
            Target::Visitor => {
                if let Some(v) = &mut self.visitor {
                    v.pet.pat();
                }
                self.send_to_friend(Msg::Touch {
                    what: TouchKind::Pat,
                });
            }
            Target::Door => {
                let door = self.door_rect();
                self.door_menu = Some(Pos2::new(
                    (door.x - 50.0).clamp(4.0, self.world.screen.w - 150.0),
                    door.y - 112.0,
                ));
            }
            Target::Parcel(i) => self.open_parcel(ctx, i),
            Target::Outgoing(i) => self.retry_outgoing(i),
        }
    }

    fn open_parcel(&mut self, ctx: &egui::Context, i: usize) {
        if i >= self.parcels.len() {
            return;
        }
        let p = self.parcels.remove(i);
        self.save_parcels();
        tracing::info!("opening a letter");
        self.send_to_friend(Msg::MailRead {
            mail_id: match &p.mail.msg {
                Msg::Mail { mail_id, .. } => mail_id.clone(),
                _ => String::new(),
            },
        });
        let from = self.friend_name();
        self.windows.open_reading(ctx, &self.config, p.mail, from);
    }

    fn door_menu_ui(&mut self, ctx: &egui::Context) {
        let Some(pos) = self.door_menu else { return };
        let mut action = None;
        let can = self.can_interact();
        let peek_ok = can && self.friend_peek && !self.my_pet_away;
        let away = self.my_pet_away;
        let resp = egui::Area::new(egui::Id::new("door_menu"))
            .fixed_pos(pos)
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    if ui.button("Write letter…").clicked() {
                        action = Some(0);
                    }
                    if ui.add_enabled(peek_ok, egui::Button::new("Peek")).clicked() {
                        action = Some(1);
                    }
                    let label = if away { "Call home" } else { "Send to visit" };
                    if ui
                        .add_enabled(can || away, egui::Button::new(label))
                        .clicked()
                    {
                        action = Some(2);
                    }
                });
            });
        let outside = ctx.input(|i| {
            i.pointer.primary_pressed()
                && i.pointer
                    .interact_pos()
                    .is_some_and(|p| !resp.response.rect.contains(p))
        });
        if action.is_some() || outside {
            self.door_menu = None;
        }
        match action {
            Some(0) => self.open_compose(),
            Some(1) => self.start_peek(),
            Some(2) => self.start_visit(),
            _ => {}
        }
    }

    fn start_peek(&mut self) {}

    // ------------------------------------------------------------------ drawing

    fn draw_pet(&self, ui: &egui::Ui, pet: &Pet, tex: &Tex, carrying: bool) {
        let px = self.px();
        let t = ui.input(|i| i.time) as f32;
        let (img, walking) = match pet.state {
            State::Carry => (&tex.carry, false),
            State::Enter | State::Exit if carrying => (&tex.carry, false),
            State::Wander { .. } | State::Enter | State::Exit => (&tex.walk[pet.frame % 8], true),
            State::Sit => (&tex.sit, false),
            State::Sleep => (&tex.sleep, false),
            _ => (&tex.idle, false),
        };
        let _ = walking;
        let (w, mut h) = (32.0 * px, 48.0 * px);
        let mut bob = 0.0;
        match pet.state {
            State::Patted => h *= 1.0 + 0.15 * (t * 20.0).sin(),
            State::Idle => bob = if (t * 2.5).sin() > 0.0 { px } else { 0.0 },
            _ => {}
        }
        let rect =
            egui::Rect::from_min_size(Pos2::new(pet.x - w / 2.0, pet.y - h + bob), Vec2::new(w, h));
        let uv = if pet.facing_left {
            egui::Rect::from_min_max(Pos2::new(1.0, 0.0), Pos2::new(0.0, 1.0))
        } else {
            egui::Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0))
        };
        let mut image = egui::Image::new((img.id(), img.size_vec2())).uv(uv);
        if matches!(pet.state, State::Thrown | State::Fall) {
            image = image.rotate(pet.angle, Vec2::splat(0.5));
        }
        image.paint_at(ui, rect);
        let p = ui.painter();
        let head = Pos2::new(pet.x, pet.y - 40.0 * px);
        match pet.state {
            State::Sleep => {
                let rise = (t * 12.0) % 30.0;
                p.text(
                    head + Vec2::new(14.0 * px, -rise),
                    Align2::CENTER_CENTER,
                    "z",
                    FontId::proportional(14.0),
                    Color32::from_white_alpha(((1.0 - rise / 30.0) * 255.0) as u8),
                );
            }
            State::Patted => {
                let c = Pos2::new(pet.x, pet.y - h - 10.0);
                let red = Color32::from_rgb(230, 50, 70);
                p.circle_filled(c + Vec2::new(-3.5, 0.0), 4.5, red);
                p.circle_filled(c + Vec2::new(3.5, 0.0), 4.5, red);
                p.add(egui::Shape::convex_polygon(
                    vec![
                        c + Vec2::new(-7.5, 1.5),
                        c + Vec2::new(7.5, 1.5),
                        c + Vec2::new(0.0, 10.0),
                    ],
                    red,
                    Stroke::NONE,
                ));
            }
            State::Dizzy => {
                for k in 0..3 {
                    let a = t * 6.0 + k as f32 * std::f32::consts::TAU / 3.0;
                    p.circle_filled(
                        head + Vec2::new(a.cos() * 10.0 * px, -8.0 * px + a.sin() * 3.0 * px),
                        3.0,
                        Color32::from_rgb(255, 220, 60),
                    );
                }
            }
            State::Peek => {
                let dir = if pet.facing_left { -1.0 } else { 1.0 };
                let hand = Pos2::new(pet.x + dir * 12.0 * px, pet.y - 22.0 * px);
                let lens = hand + Vec2::new(dir * 6.0, -6.0);
                p.circle_stroke(
                    lens,
                    7.0,
                    Stroke::new(2.0, Color32::from_rgb(230, 230, 240)),
                );
                p.line_segment(
                    [hand, lens + Vec2::new(-dir * 5.0, 5.0)],
                    Stroke::new(2.0, Color32::from_rgb(150, 100, 60)),
                );
            }
            _ => {}
        }
    }

    fn draw_door(&self, ui: &egui::Ui) {
        let p = ui.painter();
        let px = self.px();
        let door = to_egui(self.door_rect());
        if self.friend_online {
            p.rect_filled(
                door.expand(5.0 * px),
                4.0,
                Color32::from_rgba_unmultiplied(255, 214, 110, 90),
            );
            p.rect_filled(door, 2.0, Color32::from_rgb(150, 100, 50));
            p.rect_filled(door.shrink(3.0 * px), 1.0, Color32::from_rgb(255, 226, 140));
        } else {
            p.rect_filled(door, 2.0, Color32::from_rgb(70, 45, 25));
        }
        p.rect_stroke(
            door,
            2.0,
            Stroke::new(1.0, Color32::from_rgb(40, 25, 12)),
            egui::StrokeKind::Inside,
        );
        p.circle_filled(
            Pos2::new(door.right() - 4.0 * px, door.center().y + 2.0 * px),
            1.6 * px,
            Color32::from_rgb(250, 210, 40),
        );
    }

    fn draw_bubbles(&self, ui: &egui::Ui) {
        let p = ui.painter();
        let door = self.door_rect();
        let mut y = door.y - 14.0;
        for (text, _) in self.bubbles.iter().rev() {
            let galley = p.layout_no_wrap(text.clone(), FontId::proportional(14.0), Color32::WHITE);
            let r = Align2::CENTER_BOTTOM
                .anchor_size(Pos2::new(door.x + door.w / 2.0, y), galley.size())
                .expand2(Vec2::new(8.0, 4.0));
            let r = r.translate(Vec2::new(
                (self.world.screen.right() - 6.0 - r.right()).min(0.0),
                0.0,
            ));
            p.rect_filled(r, 6.0, Color32::from_black_alpha(210));
            p.galley(r.min + Vec2::new(8.0, 4.0), galley, Color32::WHITE);
            y = r.top() - 4.0;
        }
    }

    fn draw(&self, ui: &egui::Ui) {
        self.draw_door(ui);
        let px = self.px();
        for pc in &self.parcels {
            let r = to_egui(self.parcel_rect(pc.x));
            let t = &self.my_tex.parcel;
            egui::Image::new((t.id(), t.size_vec2())).paint_at(ui, r);
            let _ = px;
        }
        if !self.my_pet_hidden() {
            self.draw_pet(ui, &self.mine, &self.my_tex, false);
        }
        if let (Some(v), Some(tex)) = (&self.visitor, &self.friend_tex) {
            self.draw_pet(ui, &v.pet, tex, matches!(v.job, Job::Deliver(_)));
        }
        for o in self.outgoing.iter().filter(|o| !o.failed.is_empty()) {
            let r = to_egui(self.parcel_rect(self.failed_x(&o.mail)));
            let t = &self.my_tex.parcel;
            egui::Image::new((t.id(), t.size_vec2())).paint_at(ui, r);
            ui.painter().text(
                r.center_top() + Vec2::new(0.0, -6.0),
                Align2::CENTER_CENTER,
                "!",
                FontId::proportional(18.0),
                Color32::from_rgb(235, 50, 50),
            );
        }
        self.draw_bubbles(ui);
    }
}

fn mime_of(name: &str) -> &'static str {
    match name
        .rsplit('.')
        .next()
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        _ => "application/octet-stream",
    }
}

impl eframe::App for PetApp {
    fn clear_color(&self, _: &egui::Visuals) -> [f32; 4] {
        [0.0, 0.0, 0.0, 0.0]
    }

    /// Everything that isn't painting.
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.ppp = ctx.pixels_per_point();
        self.ensure_textures(ctx);
        while let Ok(ev) = self.net.events.try_recv() {
            self.on_net_event(ev);
        }
        while let Some(a) = self.tray.poll() {
            self.on_tray(ctx, a);
        }
        if self.world_refreshed.elapsed() >= Duration::from_millis(250) {
            self.world_refreshed = Instant::now();
            self.world = world_now(self.ppp);
            if self.world.screen != self.last_screen {
                self.last_screen = self.world.screen;
                ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(Pos2::ZERO));
                ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(Vec2::new(
                    self.world.screen.w,
                    self.world.screen.h - 1.0 / self.ppp,
                )));
            }
            let hidden = win::foreground_is_fullscreen();
            if hidden != self.hidden {
                tracing::info!("fullscreen app in front: {hidden}");
            }
            self.hidden = hidden;
        }
        let away = win::idle_seconds() >= IDLE_AWAY_SECS;
        if away != self.away {
            self.away = away;
            self.send_presence(away);
            self.mine.set_owner_away(away);
        }
        let dt = ctx.input(|i| i.stable_dt).min(0.05);
        let mut rng = rand::rng();
        #[cfg(debug_assertions)]
        self.autotest();
        self.pump_jobs();
        self.pump_outgoing();
        self.step_mine(dt, &mut rng);
        self.step_visitor(dt, &mut rng);
        self.bubbles
            .retain(|(_, t)| t.elapsed() < Duration::from_secs(BUBBLE_S));

        // Only the pets, the door and the parcels catch the mouse; the rest clicks through.
        let cur = self.cursor_pt();
        let over = !self.hidden
            && (self.drag.is_some() || self.door_menu.is_some() || self.target_at(cur).is_some());
        if over == self.passthrough {
            ctx.send_viewport_cmd(egui::ViewportCommand::MousePassthrough(!over));
            self.passthrough = !over;
        }
        ctx.request_repaint_after(Duration::from_millis(16));
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        if !self.hidden {
            self.handle_pointer(&ctx);
            egui::CentralPanel::default()
                .frame(egui::Frame::NONE)
                .show(ui, |ui| self.draw(ui));
            self.door_menu_ui(&ctx);
        }
        if let Some(MailAction::Send { text, files }) =
            self.windows.show(&ctx, &self.net, &self.config)
        {
            self.send_letter(text, files);
        }
        // Files dropped on my pet become a letter.
        let dropped: Vec<PathBuf> = ctx.input(|i| {
            i.raw
                .dropped_files
                .iter()
                .filter_map(|f| f.path.clone())
                .collect()
        });
        if !dropped.is_empty() && self.target_at(self.cursor_pt()) == Some(Target::Mine) {
            self.windows
                .compose
                .get_or_insert_with(Compose::default)
                .add_files(dropped);
        }
    }
}
