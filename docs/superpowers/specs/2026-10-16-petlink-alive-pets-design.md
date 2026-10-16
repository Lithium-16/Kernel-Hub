# PetLink: pets that feel alive together

Status: draft for the owner's approval. Calls marked **(my call)** are the designer's, not the owner's.

## Intent

Today a visit is something the owner starts by hand, and once their pet has walked through the door the owner is left with an empty screen while the friend's pet just wanders about. The owner wants the two pets to feel alive and to have a life together: they wander over to each other on their own, and when they meet they play games with each other. The owner and their friend are spectators who enjoy watching; the pets are the ones doing things.

Success is the owner looking up from work, finding their friend's pet on their desktop doing something with their own pet, and smiling.

## What gets built

### 1. Pets visit each other on their own

- Every few minutes (the owner's words: "every few minutes, more alive") a pet may wander through the door to the friend's desktop without anyone clicking anything. **(my call)** Aim for an average of about 3 minutes between starts, and for a visit lasting 1 to 2 minutes, but make both easy to tune in one place.
- Either side can start one. Both machines must agree on a single start: if both start at once, it is one shared visit, not two.
- The existing manual "send to visit" and "call home" stay and behave as today. A manual call-home also ends an automatic visit.
- An automatic visit starts only when both owners are online, neither is idle/away, and neither has a fullscreen app in front (the same condition that already hides the pets). Nothing may steal focus, input or the cursor.
- **(my call)** A tray switch "Let pets wander" (on by default) turns the automatic visits off on that machine; when off, that machine neither starts nor accepts automatic visits.

### 2. Games the pets play together

Three games, played only while both pets are on the same screen. The owners never choose or control anything; the pets pick when to play and what to do.

- **Fetch:** a ball is thrown and one pet runs after it and brings it back.
- **Hide-and-seek:** one pet hides behind a window edge or the door while the other looks for it, then they swap or celebrate.
- **Rock paper scissors:** each pet makes its own choice at random, they reveal at the same moment, and the result is shown (winner cheers, loser pouts, a tie repeats once).

Rules:

- Both screens must show the same game with the same outcome. How the two clients stay in step (one side decides and tells the other, or both exchange their choices) is left to the builder, but a cheating or out-of-sync client must never be able to do more than make a game look odd.
- Games start and end by themselves, last well under a minute, and are separated by the plain wandering, bumping and headpats that already exist. They never block a manual call-home, a peek, mail delivery or the visit ending.
- Games play on the desktop ground line the pets already use, with the same drawing approach as the existing pet poses and emotes. New poses need art; where art does not exist yet the builder uses simple code-drawn props (a ball, three hand shapes) the way the peek magnifier is drawn today, and the owner can swap in real art later.

### 3. Peek works while the pet is visiting

- If the owner's pet is at the friend's desktop (a manual or an automatic visit) and the owner chooses "peek", the pet stays where she is. She does not walk to the door, and she does not go home afterwards. The friend's screenshot arrives as today, in the existing viewer window.
- On the friend's screen, the visiting pet raises the magnifying glass in place, holds the pose for the same time as today, and the screenshot is taken at the end of it. The magnifier is the one already used for peeking: a code-drawn round glass with a brown handle (`State::Peek` drawing in `app.rs`). The owner believes they uploaded a magnifier sprite; none exists in the repository's `assets` folder, so the existing drawn one is reused and the owner can supply art later.
- The friend's "Let friend peek" setting still decides: if it is off, the answer is a denial, exactly as today, and nothing is taken. Nothing in this design may weaken that consent.
- A peek from home keeps working as now (pet walks to the door, comes back when the answer arrives).
- If the peek fails or is denied while she is visiting, she stays visiting and the existing denial message is shown.

## Goals

- Pets spend real time together without any clicks.
- Peek is no longer a round trip home when she is already there.
- The pets look different from one minute to the next.

## Non-goals

- Owners controlling the games, choosing moves, or scoring.
- Chat, shared drawing, or any other friend-to-friend feature.
- More than two pets.
- Changes to mail.

## Anti-goals

- A game or visit that interrupts work: stealing focus, covering what the owner clicks, or playing sound.
- A second copy of a pet on either screen (the earlier duplicate-pet bug must not return, including when both sides start a visit at once or a visit ends in the middle of a game).
- Any screenshot taken without the friend's peek setting being on.
- Ghosts left behind when a connection drops mid-game; the pets must always get home.

## Constraints

- Rust app in `crates/petlink`; the relay is the Python module in `modules/petlink`. New friend-to-friend message kinds must be added to the relay's allowed (`ROUTED`) set, so the relay has to be redeployed for the games and automatic visits to work.
- Older PetLink versions ignore message kinds they do not know (they log and skip them), so the new version must keep working next to an older one: no games, no automatic visits, and nothing breaks. The existing `visit.start`/`visit.end` and peek messages keep their meaning.
- New fields on existing messages must be optional, as the bow colour on `visit.start` already is.
- Releases reach friends through the repository's `node-build-N` self-update.
- The app already has tests for pets, social behaviour and the protocol; new behaviour gets tests in the same style, and the existing tests keep passing.

## Left to the builder

- Message names and shapes for starting/ending a game, and which side decides outcomes.
- How an automatic visit is negotiated so both sides agree.
- Poses, timings and the exact tuning of how often things happen.
- Whether the three games land together or one after the other.
