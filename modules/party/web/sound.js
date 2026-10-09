// Party Games: sound for the big screen, all made in the browser with WebAudio (no sound
// files). Short effects for the moments that matter, and a quiet looping tune per game that
// ducks under them. Browsers only allow sound after a click or key press: unlock() on the
// first one. Sound and music can be switched off separately; the choice is remembered.
(() => {
  'use strict';
  const KEY = 'party-sound';
  const prefs = { sound: true, music: true };
  try {
    Object.assign(prefs, JSON.parse(localStorage.getItem(KEY) || '{}'));
  } catch {
    /* private mode: defaults */
  }
  const save = () => {
    try {
      localStorage.setItem(KEY, JSON.stringify(prefs));
    } catch {
      /* private mode */
    }
  };

  let ctx = null;
  let sfx = null;
  let music = null;
  const MUSIC_VOL = 0.16;
  function ensure() {
    if (ctx) return ctx;
    const AC = window.AudioContext || window.webkitAudioContext;
    if (!AC) return null;
    ctx = new AC();
    const master = ctx.createGain();
    master.gain.value = 0.9;
    master.connect(ctx.destination);
    sfx = ctx.createGain();
    sfx.gain.value = prefs.sound ? 1 : 0;
    sfx.connect(master);
    music = ctx.createGain();
    music.gain.value = prefs.music ? MUSIC_VOL : 0;
    music.connect(master);
    return ctx;
  }
  const ready = () => !!ctx && ctx.state === 'running';
  function unlock() {
    if (!ensure()) return;
    if (ctx.state === 'suspended') ctx.resume();
    startMusic();
  }

  // -- building blocks ---------------------------------------------------------------------
  const hz = (semis, base = 440) => base * 2 ** (semis / 12);
  function tone(freq, at, dur, { type = 'triangle', vol = 0.25, to = sfx, glide = 0 } = {}) {
    const o = ctx.createOscillator();
    const g = ctx.createGain();
    o.type = type;
    o.frequency.setValueAtTime(freq, at);
    if (glide) o.frequency.exponentialRampToValueAtTime(freq * glide, at + dur);
    g.gain.setValueAtTime(0.0001, at);
    g.gain.exponentialRampToValueAtTime(vol, at + 0.008);
    g.gain.exponentialRampToValueAtTime(0.0001, at + dur);
    o.connect(g).connect(to);
    o.start(at);
    o.stop(at + dur + 0.02);
  }
  let noiseBuf = null;
  function noise(at, dur, { vol = 0.2, freq = 2000, q = 1, to = sfx, sweep = 0 } = {}) {
    if (!noiseBuf) {
      noiseBuf = ctx.createBuffer(1, ctx.sampleRate, ctx.sampleRate);
      const d = noiseBuf.getChannelData(0);
      for (let i = 0; i < d.length; i++) d[i] = Math.random() * 2 - 1;
    }
    const src = ctx.createBufferSource();
    src.buffer = noiseBuf;
    const f = ctx.createBiquadFilter();
    f.type = 'bandpass';
    f.frequency.setValueAtTime(freq, at);
    if (sweep) f.frequency.exponentialRampToValueAtTime(freq * sweep, at + dur);
    f.Q.value = q;
    const g = ctx.createGain();
    g.gain.setValueAtTime(0.0001, at);
    g.gain.exponentialRampToValueAtTime(vol, at + 0.01);
    g.gain.exponentialRampToValueAtTime(0.0001, at + dur);
    src.connect(f).connect(g).connect(to);
    src.start(at);
    src.stop(at + dur + 0.02);
  }
  /** Music steps aside for a moment so an effect reads clearly. */
  function duck(sec = 0.8) {
    if (!prefs.music) return;
    const t = ctx.currentTime;
    music.gain.cancelScheduledValues(t);
    music.gain.setValueAtTime(music.gain.value, t);
    music.gain.linearRampToValueAtTime(MUSIC_VOL * 0.3, t + 0.05);
    music.gain.linearRampToValueAtTime(MUSIC_VOL, t + sec);
  }
  const play = (f) => {
    if (!ready() || !prefs.sound) return;
    f(ctx.currentTime);
  };

  // -- effects -------------------------------------------------------------------------------
  const C5 = hz(3);
  const fx = {
    /** A new phase starts: a quick rising arpeggio. */
    phase: () =>
      play((t) => {
        duck(0.9);
        [0, 4, 7, 12].forEach((s, i) =>
          tone(C5 * 2 ** (s / 12), t + i * 0.07, 0.22, { type: 'square', vol: 0.07 }),
        );
      }),
    /** The last seconds of a timer. */
    tick: (last = false) =>
      play((t) => tone(last ? 1760 : 1320, t, 0.06, { type: 'sine', vol: last ? 0.18 : 0.1 })),
    /** Before a reveal: a short drum roll, then a sting. */
    reveal: () =>
      play((t) => {
        duck(1.6);
        for (let i = 0; i < 10; i++)
          noise(t + i * 0.055, 0.06, { vol: 0.05 + i * 0.012, freq: 1800, q: 0.7 });
        tone(hz(-2, C5), t + 0.6, 0.5, { type: 'square', vol: 0.09 });
        tone(C5 * 2, t + 0.6, 0.7, { type: 'triangle', vol: 0.16 });
      }),
    /** Someone's answer or vote came in. */
    ding: () =>
      play((t) => {
        tone(1568, t, 0.35, { type: 'sine', vol: 0.12 });
        tone(2349, t, 0.25, { type: 'sine', vol: 0.05 });
      }),
    /** Results and confetti. */
    fanfare: () =>
      play((t) => {
        duck(2.4);
        const notes = [0, 4, 7, 12, 7, 12, 16];
        notes.forEach((s, i) =>
          tone(C5 * 2 ** (s / 12), t + i * 0.11, i === notes.length - 1 ? 0.9 : 0.2, {
            type: 'square',
            vol: 0.08,
          }),
        );
        [0, 4, 7].forEach((s) =>
          tone((C5 * 2 ** (s / 12)) / 2, t + 0.77, 1.1, { type: 'triangle', vol: 0.12 }),
        );
      }),
    /** A badge was earned. */
    chime: () =>
      play((t) => {
        tone(hz(19, C5 / 2), t, 0.5, { type: 'sine', vol: 0.12 });
        tone(hz(24, C5 / 2), t + 0.12, 0.7, { type: 'sine', vol: 0.12 });
      }),
    /** Visual-novel text typing; `voice` shifts the pitch per speaker. */
    blip: (voice = 0) =>
      play((t) =>
        tone(520 + voice * 140 + Math.random() * 30, t, 0.035, { type: 'square', vol: 0.035 }),
      ),
    /** A sprite changes mood. */
    whoosh: () => play((t) => noise(t, 0.25, { vol: 0.08, freq: 500, q: 0.8, sweep: 4 })),
  };

  // -- music ---------------------------------------------------------------------------------
  // Each tune: a base note, tempo, and four chords (semitones from the base, as triads).
  const MAJ = [0, 4, 7];
  const MIN = [0, 3, 7];
  const TUNES = {
    lobby: {
      base: hz(-9, 220),
      bpm: 92,
      chords: [
        [0, MAJ],
        [7, MAJ],
        [9, MIN],
        [5, MAJ],
      ],
    },
    quip: {
      base: hz(-4, 220),
      bpm: 104,
      chords: [
        [0, MAJ],
        [5, MAJ],
        [7, MAJ],
        [5, MAJ],
      ],
    },
    bluff: {
      base: hz(0, 220),
      bpm: 88,
      chords: [
        [0, MIN],
        [8, MAJ],
        [3, MAJ],
        [10, MAJ],
      ],
    },
    shirt: {
      base: hz(-2, 220),
      bpm: 112,
      chords: [
        [0, MAJ],
        [9, MIN],
        [5, MAJ],
        [7, MAJ],
      ],
    },
    drama: {
      base: hz(-6, 220),
      bpm: 78,
      chords: [
        [0, MAJ],
        [9, MIN],
        [4, MIN],
        [5, MAJ],
      ],
    },
  };
  let tune = 'lobby';
  let timer = null;
  let nextAt = 0;
  let step = 0; // eighth notes since the tune started
  function schedule() {
    if (!ready()) return;
    const t = TUNES[tune] || TUNES.lobby;
    const eighth = 60 / t.bpm / 2;
    while (nextAt < ctx.currentTime + 0.25) {
      const bar = Math.floor(step / 8) % 4;
      const [root, shape] = t.chords[bar];
      const beat = step % 8;
      if (beat === 0 || beat === 4)
        tone(t.base * 2 ** ((root - 12) / 12), nextAt, eighth * 3.2, {
          type: 'triangle',
          vol: 0.5,
          to: music,
        });
      const note = shape[[0, 1, 2, 1][beat % 4]] + root + (beat >= 4 ? 12 : 0);
      tone(t.base * 2 ** (note / 12), nextAt, eighth * 0.9, { type: 'sine', vol: 0.22, to: music });
      if (beat % 2 === 1) noise(nextAt, 0.04, { vol: 0.05, freq: 7000, q: 1.5, to: music });
      nextAt += eighth;
      step++;
    }
  }
  function startMusic() {
    if (timer || !ctx) return;
    nextAt = ctx.currentTime + 0.1;
    timer = setInterval(schedule, 60);
  }
  /** Switches the tune, at the start of the next bar. */
  function track(name) {
    const want = TUNES[name] ? name : 'lobby';
    if (want === tune) return;
    tune = want;
    step = Math.ceil(step / 8) * 8;
  }

  function set(kind, on) {
    prefs[kind] = on;
    save();
    if (!ctx) return;
    const g = kind === 'music' ? music : sfx;
    g.gain.cancelScheduledValues(ctx.currentTime);
    g.gain.setValueAtTime(on ? (kind === 'music' ? MUSIC_VOL : 1) : 0, ctx.currentTime);
  }

  window.PartySound = { prefs, unlock, ready, track, set, ...fx };
})();
