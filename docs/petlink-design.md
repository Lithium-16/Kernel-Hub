# PetLink — design

A linked desktop pet for me and one friend (a small group later). Each of us has a pixel
catgirl living on our own taskbar. A little door on the taskbar leads to the other person's
screen: the pet carries mail through it, peeks at the other screen with a magnifying glass
(only when allowed), or goes over to visit.

Status: design approved in chat on 2026-10-15; this file is the written spec for review.

## 1. Goals and non-goals

**Goals (v1)**
- Two people, each running `petlink.exe` on Windows, linked through a relay on Pluto.
- The friend installs one exe and nothing else (no Tailscale, no account).
- Mail: notes, images and files, delivered by the pet, queued while the other side is offline.
- Peek: a single snapshot of the other screen, only while the other side's toggle is on.
- Pet life: presence (sleeps when its owner is away), walking on the taskbar and window tops,
  pats/pokes, yeeting.
- The client updates itself.

**Non-goals (v1)**
- Live screen streaming, voice, clipboard sync, scheduled mail, accessories, scrapbook,
  "point here", "peek back" (all in the v2 backlog, section 9).
- macOS/Linux clients. Multi-monitor (v1 uses the primary monitor's taskbar only).
- More than one friend. The protocol is group-ready (section 4), the UI is not.

## 2. Pieces

| Piece | Where | What |
|---|---|---|
| `modules/petlink` | Kernel-Hub module, runs on Pluto under `kerneld` | The relay: WebSocket hub, token auth, mail queue, file store, presence. Python + aiohttp, like `party`. |
| `crates/petlink` | Rust workspace crate, builds `petlink.exe` | The client: overlay window with the pets, taskbar door, tray menu, mail window, self-update. |
| `crates/petlink/assets` | in the repo | Sprites: `walk.png`/`walk.json` (8 directions + 8-frame walk east, 64×64 cells), `idle.png`, `sit.png`, `sleep.png`, `carry.png` (holding the parcel), `parcel.png` (the parcel on its own, 15×12, for mail waiting by the door). |

```
 my PC                          Pluto                          friend's PC
┌──────────────┐  wss (Funnel)  ┌────────────────────┐  wss  ┌──────────────┐
│ petlink.exe  │◄──────────────►│ kerneld            │◄─────►│ petlink.exe  │
│ pet + door   │                │  └ modules/petlink │       │ pet + door   │
└──────┬───────┘                │    relay :8098     │       └──────┬───────┘
       │ self-update            └────────────────────┘              │
       └──────────────► GitHub Releases (node-build-N) ◄────────────┘
```

## 3. Relay module (`modules/petlink`)

- **Manifest** `module.toml` like `party`: `runtime = "python"`, `autostart`, `port = 8098`.
- **Listening:** only `127.0.0.1:8098`. Reached from outside through **Tailscale Funnel**,
  using the same share pattern as `flowrace`/`party` (`share.start` / `share.stop` actions,
  `ai = "never"` for start).
  - All three Funnel ports (443, 8443, 10000) are already used by flowrace, openfork and party.
    PetLink is shared on a path instead: `tailscale funnel --bg --https=10000
    --set-path=/petlink http://127.0.0.1:8098`. The client URL is
    `wss://pluto.<tailnet>.ts.net:10000/petlink/ws`.
  - Must check: party's `share.stop` turns off only its own path, not the whole port.
    If it resets port 10000, fix party's stop to use its path too.
- **Users and tokens:** `users` table (`id`, `name`, `bow_hue`, `token_hash`). Tokens are
  32 random bytes, shown once, stored as SHA-256. Actions: `user.add` (returns the token and a
  ready-to-send config snippet, `ai = "never"`), `user.remove` (`ai = "never"`).
- **Endpoints**
  - `GET /ws` — WebSocket. First message must be `hello {token, client_version}`; anything
    else, or a bad token, closes with code 4401. One live socket per user (a new one replaces
    the old one).
  - `PUT /files/{id}` and `GET /files/{id}` — file bodies for mail, with the same token as a
    bearer header. Upload limit 100 MB; a download is allowed only for the recipient.
- **Storage:** SQLite in the module's data folder; files in `data/files/`. A file is deleted
  once its recipient has downloaded it, or after 7 days. Undelivered notes also expire after
  30 days.
- **Rate limits:** per user, at most 30 messages/min and 200 MB of uploads per hour.
- **Status for the Kernel app:** who's online, mail queued, storage used, share link.

## 4. Protocol

JSON text frames over the WebSocket. Every message:
`{ "v": 1, "id": "<ulid>", "kind": "...", "from": "<user id>", "to": "<user id>", ... }`.
The relay fills in `from` from the authenticated socket (clients can't spoof it) and routes
by `to`. Addressing by user id is what makes a group possible later.

| kind | direction | payload | relay behaviour |
|---|---|---|---|
| `hello` | client → relay | `token`, `client_version` | auth; replies `welcome {me, friends[{id,name,bow_hue,online}]}` |
| `presence` | both | `state: online/away/offline` | broadcast to friends; `offline` is sent by the relay when a socket drops |
| `mail` | client → client | `mail_id`, `text?`, `files[{file_id,name,size,mime}]` | stored until delivered, then pushed; sender gets `delivered` |
| `mail.read` | client → client | `mail_id` | forwarded (the sender's pet can react) |
| `peek.request` | client → client | `peek_id` | forwarded only if the target is online |
| `peek.result` | client → client | `peek_id`, `file_id` or `denied` | forwarded |
| `peek.allow` | client → relay | `on: bool` | stored and broadcast, so the requester's UI greys out when it's off |
| `visit.start` / `visit.end` | client → client | — | forwarded; the relay sends `visit.end` itself if either side drops |
| `touch` | client → client | `what: pat/poke/yeet` | forwarded, never stored |

The `peek.request` gate is enforced by the **target's client** (it checks its own toggle);
the relay also refuses to forward one while the stored toggle is off, so a hacked client
can't skip it.

## 5. Client (`crates/petlink`, `petlink.exe`)

**Windows-only code** is behind `#[cfg(windows)]`, so the crate still builds and passes
clippy on CI's Ubuntu runner (the pure logic is tested there).

### 5.1 Overlay
- A borderless, transparent, always-on-top, tool-window (no taskbar button) layered window
  covering the primary monitor. It's click-through everywhere except the pixels of the pets,
  the door and the parcels (per-frame hit test against the sprite's alpha).
- Rendering: `winit` + `softbuffer`, pixel art scaled ×2 (setting: 1–4) with nearest-neighbour.
- Hidden while a fullscreen app is in front (the taskbar is hidden then anyway).

### 5.2 World
- **Floors:** the taskbar's top edge (from `SHAppBarMessage(ABM_GETTASKBARPOS)`), plus the top
  edges of visible, non-minimised top-level windows (`EnumWindows` + DWM extended frame
  bounds), refreshed every 250 ms. A pet stands on the nearest floor below it.
- **The door:** drawn on the taskbar's top edge, just left of the tray. It glows when the
  friend is online and is dark when they're offline or the relay is unreachable. Parcels
  waiting to be opened sit next to it.

### 5.3 Pet behaviour (a small state machine per pet)

| State | Sprite | Notes |
|---|---|---|
| idle | `idle.png`, 1 px breathing bob | default |
| wander | walk row (mirrored for west) | to a random point on its floor, sometimes jumps up to a window top |
| sit | `sit.png` | perched on a window top for a while |
| fall | idle frame, rotating | when its floor disappears (window moved/closed/minimised); lands with a squash |
| yeeted | idle frame, spinning, physics | dragged and thrown: velocity from the last mouse moves, gravity, bounces off screen edges, lands dizzy (stars) |
| patted | idle + squash hop + heart | click without dragging |
| sleep | `sleep.png` + floating "z" | the **owner** is away: no input for 5 min (`GetLastInputInfo`), or offline |
| carry | `carry.png` while walking | taking mail to the door |
| peek | idle + magnifier prop drawn over the hand | on the other screen, for ~2 s while the snapshot is taken |

- **My pet** lives on my screen. **The friend's pet** appears on my screen only while it's
  visiting, delivering mail, or peeking, entering and leaving through my door.
- Pats, pokes and yeets work on whatever pet is on my screen. If it's the friend's visiting pet,
  a `touch` goes to them and they get a small notice ("Pattles yeeted your pet").
- **Bow colour:** each user has a `bow_hue`. On load, pixels with saturation ≥ 0.6 and hue
  within 340°–15° (the red bows and chest ribbon) are hue-shifted to it. Tried on the walk
  sheet: eyes, skin and the inner ears stay untouched.

### 5.4 Mail
- **Sending:** drop text, an image or files onto my pet, or tray → "Write letter" (a small egui
  window: a text box, attachments, Send). Files are uploaded with `PUT /files/{id}` first;
  then the `mail` message is sent. My pet picks up the parcel, walks to the door and goes in.
- **Receiving:** the friend's pet comes out of my door carrying the parcel, puts it down by
  the door (`parcel.png`) and goes back. Clicking the parcel opens the mail window: the note,
  images shown inline, and files listed with "Save" (to `Downloads\PetLink\`).
- Files are **never opened automatically**. `.exe .bat .cmd .ps1 .vbs .js .msi .scr .lnk`
  show a warning before saving, and saved files keep the Mark-of-the-Web
  (`Zone.Identifier`), so Windows treats them as downloaded.
- Sent while the friend is offline: my pet still walks into the door; the relay holds it.

### 5.5 Peek
- Tray toggle **"Let <friend> peek"**, **off by default**. While it's on, the tray icon has a
  coloured dot.
- Clicking my door opens its menu; "Peek" is greyed out when their toggle is off or they're
  offline. My pet goes through my door, comes out of theirs, holds up the magnifier for
  ~2 s, and their client grabs one screenshot of the primary monitor (DXGI/GDI), downscaled to
  at most 1920 px wide, JPEG q80, uploaded as a file. My pet comes home and a lens window
  opens: the image, with drag to pan and wheel to zoom, and "Save".
- The magnifier pose always plays before the capture, and the capture can't be hidden.

### 5.6 Visit
- Door menu → "Send to visit". My pet goes over and wanders their screen as a visitor; my door
  shows it's away. "Call home" (or either side going offline) brings it back.

### 5.7 Tray and settings
- Tray: Write letter, Let <friend> peek, Send to visit / Call home, Bow colour, Pet size,
  Start with Windows, Check for updates, Quit.
- Settings in `%APPDATA%\PetLink\config.toml`: `relay_url`, `token`, `scale`. The first run
  without a config asks for a pasted invite (the snippet from `user.add`).

## 6. Updates and releases

- CI builds `petlink.exe` on `windows-latest` and attaches `petlink-windows-x64.exe` +
  `.sha256` to the same `node-build-N` release that already carries the node and the desktop
  app.
- The client checks GitHub Releases on start and every 6 hours, using the same rule as
  `crates/node/src/update.rs`: the newest non-draft, non-prerelease `node-build-N` that has both
  assets. It downloads, verifies the SHA-256, renames the running exe to `petlink.old.exe`,
  moves the new one in place, restarts, and deletes the old one on the next start.
- Trust: the same as the node — updates come only from this repo's releases over HTTPS.
  Pluto is not in the update path, so a compromised relay can't push code to the friend's PC.
  The exe is unsigned (like the rest of Kernel for now): the first run needs SmartScreen's
  "More info → Run anyway".
- Protocol `v` stays at 1 within compatible changes. A client with an unsupported `v` gets
  `error {code: "upgrade"}` and its pet shows a "please update" bubble.

## 7. Errors and edge cases

- **Relay unreachable:** reconnect with backoff (1 s doubling to 60 s, with jitter); the door
  goes dark; outgoing mail is kept on disk and sent on reconnect.
- **Upload fails mid-way:** the mail isn't sent; the parcel stays by my door with a red mark
  and a "Retry" click.
- **Friend offline during peek/visit:** the menu items are greyed out; if they drop mid-way,
  my pet comes home and a bubble says so.
- **Display changes** (resolution, DPI, taskbar moved/auto-hidden): the overlay is resized and
  floors recomputed on `WM_DISPLAYCHANGE`/`WM_DPICHANGED`; an auto-hidden taskbar uses the
  screen's bottom edge as the floor.
- **Disk:** the relay refuses uploads when its file store passes 2 GB.
- Everything is logged: the relay to the module's stderr log, the client to
  `%APPDATA%\PetLink\logs\`.

## 8. Testing

- **Relay** (pytest, run by CI's existing module job): auth (bad token, missing hello),
  routing by `to`, mail queued while offline and delivered on connect, files only downloadable
  by the recipient and deleted after download, `peek.request` refused while the toggle is off,
  rate limits, and `visit.end` sent when a socket drops.
- **Client logic** (cargo test, both OSes): the pet state machine (falls when its floor goes,
  sleeps when away), bow hue mask (snapshot of the recoloured walk sheet), protocol
  (de)serialisation against shared JSON fixtures, the update picker, and the dangerous-file
  check.
- **Manual:** two clients on one PC against a local relay (`--relay ws://127.0.0.1:8098/ws`,
  two config folders), then the real thing with the friend over Funnel.

## 9. v2 backlog

Point here (click a spot on a snapshot and the pet points at it on their screen), peek back,
clipboard throw hotkey, voice notes, scheduled mail, accessories earned by mail count, a mail
scrapbook, more friends (one door per friend), multi-monitor, a doodle pad.

Two pets together (when visiting):

- bump into each other and both blush
- holding hands while walking (needs sprites)
- headpats for each other, with hearts
- napping together when both owners are away
- sitting together on a window top
- waiting at the door when the friend comes online
- a "chu" goodbye with a heart when a visit ends
- heart-sealed letters
- matching accessories (half-heart charms)
- a shared umbrella when it rains for either of them
