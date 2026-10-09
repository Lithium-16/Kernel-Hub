// Party Games: the phone controller. One WebSocket; the server says what to show.
(() => {
  'use strict';
  const $ = (id) => document.getElementById(id);
  const esc = (s) =>
    String(s ?? '').replace(
      /[&<>"']/g,
      (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' })[c],
    );
  const fmt = (n) => Number(n || 0).toLocaleString('en-US');
  const ORD = ['1st', '2nd', '3rd', '4th', '5th', '6th', '7th', '8th'];
  const SAFETY = [
    'A spreadsheet that just says “no”',
    'Gary',
    'Free trial, expires immediately',
    'Three raccoons in a trench coat',
    'An unskippable ad',
    "Grandma's group chat",
    'Soup, but angry',
    'A strongly worded email',
  ];
  const pick = (list) => list[Math.floor(Math.random() * list.length)];

  const store = {
    get() {
      try {
        return JSON.parse(localStorage.getItem('party-me') || '{}');
      } catch {
        return {};
      }
    },
    set(v) {
      try {
        localStorage.setItem('party-me', JSON.stringify(v));
      } catch {
        /* private mode */
      }
    },
  };
  const urlCode = (new URLSearchParams(location.search).get('code') || '')
    .toUpperCase()
    .slice(0, 4);

  let me = store.get();
  let pid = null;
  let st = null;
  let joinErr = '';
  let kicked = false;
  let canHost = false; // anyone on this link may open a room of their own
  let lastKey = '';
  let deadline = null;
  let total = null;
  let ws = null;
  let autoJoining = false;
  let pinFor = null; // a name whose PIN this phone needs
  let pinDigits = '';
  let pinErr = '';
  let pad = null; // the drawing pad, while drawing
  let fpicks = []; // Quip Clash last round: the answers picked so far
  let fmost = 3;
  let hand = null; // Shirt Showdown: the hand being assembled, and the picks
  let sel = null;
  // Drama Club: the mood being drawn, the stage picked, and the script being written
  let dEmo = null;
  let dBg = null;
  let dLines = null; // {key, lines: [{who, emotion, text}]}
  let dJob = null;
  let dMost = 6;

  $('ph').innerHTML =
    `<div id="top"></div><main class="phmain" id="main"></main><div class="phfoot" id="foot" hidden></div>`;

  // -- helpers ---------------------------------------------------------------------------
  const player = (id) =>
    (st && st.room.players.find((p) => p.pid === id)) || { name: '?', color: 0 };
  const av = (id, size = 'sm') => {
    const p = player(id);
    return `<span class="av ${size} c${p.color}" aria-hidden="true">${esc((p.name || '?')[0].toUpperCase())}</span>`;
  };
  const wait = (title, note) =>
    `<div class="wait"><div class="face" aria-hidden="true"><svg viewBox="0 0 70 40"><ellipse cx="18" cy="20" rx="14" ry="17" fill="var(--bg)"/><ellipse cx="52" cy="20" rx="14" ry="17" fill="var(--bg)"/><circle cx="22" cy="12" r="6" fill="var(--fg)"/><circle cx="56" cy="12" r="6" fill="var(--fg)"/></svg></div><h2 class="ptitle">${title}</h2><p class="pnote">${note}</p></div>`;
  const isVip = () => st && pid && st.room.vip === pid;
  const send = (msg) => {
    if (ws && ws.readyState === 1) ws.send(JSON.stringify(msg));
  };
  const buzz = () => {
    try {
      navigator.vibrate && navigator.vibrate(25);
    } catch {
      /* not supported */
    }
  };
  const skipBtn = () =>
    isVip()
      ? `<button class="ghost" type="button" data-send='{"type":"skip"}'>Skip ahead (VIP)</button>`
      : '';
  const factParts = (q) => {
    const [a, b] = q.split('___');
    return `${esc(a)}<u>_____</u>${esc(b ?? '')}`;
  };
  // Your place counts only people with more points, so ties share a place.
  const standing = (v) => {
    const s = v.standings || [];
    const mine = s.find((x) => x.pid === pid);
    if (!mine) return { k: -1, me: null, ahead: null, tied: [] };
    const higher = s.filter((x) => x.score > mine.score);
    return {
      k: higher.length,
      me: mine,
      ahead: higher.length ? higher[higher.length - 1] : null,
      tied: s.filter((x) => x.pid !== pid && x.score === mine.score),
    };
  };

  // -- screens: each returns {key, main, foot, game, input} ------------------------------
  function pinScreen() {
    const keys = ['1', '2', '3', '4', '5', '6', '7', '8', '9', '', '0', '⌫'];
    return {
      key: `pin:${pinFor}:${pinDigits.length}:${pinErr}`,
      main: `<span class="kicker">Welcome back</span><h2 class="ptitle">Hi ${esc(pinFor)}! Type your PIN</h2>
        <p class="pnote">This phone is new to your profile. Your PIN brings your points, badges and rivalries along.</p>
        <div class="dots">${[0, 1, 2, 3].map((k) => `<i class="${k < pinDigits.length ? 'on' : ''}"></i>`).join('')}</div>
        ${pinErr ? `<div class="err" role="alert">${esc(pinErr)}</div>` : ''}
        <div class="keypad">${keys.map((k) => (k ? `<button type="button" data-key="${k}" aria-label="${k === '⌫' ? 'Delete' : k}">${k}</button>` : '<span></span>')).join('')}</div>`,
      foot: `<button class="big-btn" type="button" data-act="pinjoin" ${pinDigits.length === 4 ? '' : 'disabled'}>Sign in</button><button class="ghost" type="button" data-act="pinback">Use another name</button>`,
    };
  }

  function profileCard() {
    const pr = st && st.profile;
    if (!pr) return '';
    return `<div class="card profile"><div class="prow2">${av(pid, '')}<div><b>${esc(pr.name)}</b><span class="pnote">Playing since ${esc(pr.since)}</span></div></div>
      <div class="stat"><div><b>${fmt(pr.points)}</b>points</div><div><b>${fmt(pr.wins)}</b>wins</div><div><b>${fmt(pr.games)}</b>games</div></div>
      ${pr.season_points ? `<span class="pnote">${fmt(pr.season_points)} points this season</span>` : ''}
      ${pr.badges.length ? `<div class="minis">${pr.badges.map((b) => `<span class="chip badge-chip" title="${esc(b.about)}">${esc(b.title)}</span>`).join('')}</div>` : ''}
      ${pr.rivals.map((x) => `<span class="pnote">vs ${esc(x.name)}: ${x.won} won, ${x.lost} lost</span>`).join('')}
      ${pr.best ? `<span class="pnote">Your greatest hit: “${esc(pr.best.text)}”</span>` : ''}
      ${pr.pin ? '' : `<div class="field"><label for="f-pin">Set a PIN to play as ${esc(pr.name)} on other phones</label><div class="pinrow"><input id="f-pin" inputmode="numeric" pattern="[0-9]*" maxlength="4" autocomplete="off" placeholder="4 digits"><button class="ghost" type="button" data-act="setpin">Save</button></div></div>`}
    </div>`;
  }

  function joinScreen() {
    if (pinFor) return pinScreen();
    if (autoJoining)
      return { key: 'rejoin', main: wait('Reconnecting…', 'Getting you back into the room.') };
    const code = me.code && !urlCode ? me.code : urlCode;
    return {
      key: 'join:' + joinErr + kicked + canHost,
      main: `<span class="kicker">Party Night</span><h2 class="ptitle">${kicked ? 'You were removed from the room' : 'Join the game'}</h2>
        ${kicked ? `<p class="pnote">Ask the host if that was a mistake.</p>` : ''}
        <div class="field"><label for="f-code">Room code</label><input id="f-code" class="codein" maxlength="4" autocomplete="off" autocapitalize="characters" spellcheck="false" enterkeyhint="next" value="${esc(code)}"></div>
        <div class="field"><label for="f-name">Your name</label><input id="f-name" maxlength="16" autocomplete="nickname" enterkeyhint="go" value="${esc(me.name || '')}"></div>
        ${joinErr ? `<div class="err" role="alert">${esc(joinErr)}</div>` : `<p class="pnote">Your phone remembers you, so you can reconnect if it locks.</p>`}`,
      foot: `<button class="big-btn" type="button" data-act="join">Join game</button>${canHost ? '<button class="ghost" type="button" data-act="host">Host a game on this screen</button>' : ''}`,
      input: true,
    };
  }

  function lobbyScreen() {
    const r = st.room;
    const chips = r.players
      .map(
        (p) =>
          `<span class="chip ${p.connected ? '' : 'off'}">${av(p.pid, '')}${esc(p.name)}${p.bot ? '<b class="bot">BOT</b>' : ''}</span>`,
      )
      .join('');
    if (!isVip()) {
      return {
        key: 'lobby:' + JSON.stringify([r.players, st.profile]) + r.vip + r.choice,
        main:
          wait("You're in!", `${esc(player(r.vip).name)} picks the game. Look at the big screen.`) +
          `<div class="plist">${chips}</div>${profileCard()}`,
        keep: true,
      };
    }
    const game = r.games.find((g) => g.key === r.choice) || r.games[0];
    const here = r.players.filter((p) => p.connected).length;
    const short = game && here < game.min;
    const blurb = {
      quip: 'Funny answers, head-to-head votes',
      bluff: 'Fake answers, find the truth',
      shirt: 'Draw, write slogans, make shirts, battle',
      drama: 'Draw a cast, write a scene, watch the drama',
    };
    return {
      key: 'lobby-vip:' + JSON.stringify([r.players, st.profile]) + r.choice,
      main: `<span class="kicker">You're the VIP</span><h2 class="ptitle">Pick a game, then start when everybody's in.</h2>
        <div class="gpick">${r.games.map((g) => `<button type="button" data-game="${esc(g.key)}" data-choose="${esc(g.key)}" aria-pressed="${g.key === r.choice}"><b>${esc(g.title)}</b><span>${esc(blurb[g.key] || '')} · ${g.min}–${g.max} players</span></button>`).join('')}</div>
        <p class="pnote">${here} of ${r.max_players} players are in.</p><div class="plist">${chips}</div>${profileCard()}`,
      foot: `<button class="big-btn" type="button" data-act="start" ${short ? 'disabled' : ''}>${short ? `Need ${game.min} players` : "Everybody's in"}</button>`,
      game: r.choice,
      keep: true,
    };
  }

  function resultsScreen(v) {
    const { k, me: mine, tied } = standing(v);
    const myHit = (v.hits || []).find((h) => h.pid === pid);
    const myBadges = (v.badges || []).filter((b) => b.pid === pid);
    return {
      key: 'results:' + v.title + JSON.stringify(v.standings) + isVip(),
      main: `<span class="kicker">${esc(v.title)} · final results</span>
        <h2 class="ptitle">${k === 0 && mine && mine.score > 0 ? (tied.length ? 'You tied for the win!' : 'You won!') : k >= 0 ? `You finished ${ORD[k]}${tied.length ? ' (tied)' : ''}` : 'Game over'}</h2>
        ${mine ? `<div class="card hot"><span class="pnote">Your points</span><span class="gain">${fmt(mine.score)}</span></div>` : ''}
        ${myHit ? `<div class="card"><b>${myHit.kind === 'shirt' ? 'Your shirt won!' : myHit.kind === 'scene' ? 'Your scene won!' : 'Best of the game'}</b><span class="pnote">“${esc(myHit.text)}”${myHit.kind === 'shirt' ? '' : ` (${myHit.votes} of ${myHit.of})`}</span></div>` : ''}
        ${myBadges.map((b) => `<div class="card hot"><div class="prow2"><span class="badge">★</span><div><b>New badge: ${esc(b.title)}</b><span class="pnote">${esc(b.about)}</span></div></div></div>`).join('')}
        ${isVip() ? '' : `<p class="pnote">${esc(player(st.room.vip).name)} picks what's next.</p>`}`,
      foot: isVip()
        ? `<button class="big-btn" type="button" data-act="again">Play ${esc(v.title)} again</button><button class="ghost" type="button" data-send='{"type":"lobby"}'>Back to lobby</button>`
        : '<button class="ghost" type="button" data-act="leave">Leave room</button>',
    };
  }

  function scoresScreen(v) {
    const { k, me: mine, ahead, tied } = standing(v);
    return {
      key: `scores:${v.game}:${v.phase}:${v.round || v.number}`,
      main: `<span class="kicker">${v.phase === 'over' ? 'Final scores' : 'Scores'}</span><h2 class="ptitle">You're in ${ORD[k] || 'the game'}${tied.length ? ' (tied)' : ''}</h2>
        <div class="card"><span class="pnote">${ahead ? `${fmt(ahead.score - mine.score)} points behind ${esc(player(ahead.pid).name)}` : tied.length ? `Tied for the lead with ${esc(tied.map((x) => player(x.pid).name).join(', '))}` : "Everyone's chasing you"}</span>
        <span class="gain">${fmt(mine ? mine.score : 0)}</span>${mine && mine.gained ? `<span class="pnote">+${fmt(mine.gained)} this round</span>` : ''}</div>`,
      foot: skipBtn(),
    };
  }

  function quipScreen(v) {
    const base = `quip:${v.phase}:${v.round}`;
    if (v.phase === 'write') {
      if (!v.todo)
        return {
          key: base + ':done',
          main: wait(
            'Both answers in!',
            "Look at the big screen. Voting starts when everyone's done.",
          ),
          foot: skipBtn(),
        };
      return {
        key: base + ':' + v.todo.prompt,
        main: `<span class="kicker">Prompt ${v.todo.number} of ${v.todo.of}</span><h2 class="ptitle">${esc(v.todo.prompt)}</h2>
          <div class="field"><label for="f-ans">Your answer</label><textarea id="f-ans" maxlength="80" enterkeyhint="send" placeholder="Type something funny"></textarea><span class="count" id="cnt">0/80</span></div>`,
        foot: `<button class="big-btn" type="button" data-act="answer">Send</button><button class="ghost" type="button" data-act="safety">Give me a safety quip</button>`,
        input: true,
      };
    }
    if (v.phase === 'vote') {
      const m = v.matchup;
      if (!v.can_vote)
        return {
          key: base + ':mine:' + m.prompt,
          main: wait('Your answer is up!', 'Everyone else is voting. Look at the big screen.'),
          foot: skipBtn(),
        };
      if (v.voted !== null && v.voted !== undefined)
        return {
          key: base + ':voted:' + m.prompt,
          main: wait('Vote locked in', 'Watch the big screen for the reveal.'),
          foot: skipBtn(),
        };
      return {
        key: base + ':vote:' + m.prompt,
        main: `<span class="kicker">Vote for the funnier one</span><h2 class="ptitle">${esc(m.prompt)}</h2>
          ${m.answers.map((a, k) => `<button class="choice" type="button" data-send='${esc(JSON.stringify({ type: 'vote', choice: k }))}'><span class="l">${'AB'[k]}</span>${a ? esc(a) : '(no answer)'}</button>`).join('')}`,
        input: true,
      };
    }
    if (v.phase === 'reveal') {
      return {
        key: base + ':reveal:' + v.number,
        main: v.mine
          ? `<span class="kicker">Your matchup</span><h2 class="ptitle">${v.outcome === 'jinx' ? 'Jinx! You both wrote the same thing.' : v.outcome === 'forfeit' ? (v.won ? 'You win by default!' : 'No answer, no points.') : v.sweep_by === pid ? 'Clean sweep!' : v.won ? 'You won the matchup!' : v.gain ? 'Close one!' : 'Ouch.'}</h2><div class="card hot"><span class="pnote">You got</span><span class="gain">+${fmt(v.gain)}</span></div>`
          : wait(
              'Look up!',
              v.outcome === 'vote' ? 'Here come the votes.' : 'No vote needed for this one.',
            ),
        foot: skipBtn(),
      };
    }
    if (v.phase === 'final_write') {
      if (v.answered)
        return {
          key: base + ':fdone',
          main: wait('Answer in!', "Everyone's answering the same prompt this time."),
          foot: skipBtn(),
        };
      return {
        key: base + ':fwrite',
        main: `<span class="kicker">Final round · everyone answers</span><h2 class="ptitle">${esc(v.prompt)}</h2>
          <div class="field"><label for="f-ans">Your answer</label><textarea id="f-ans" maxlength="80" enterkeyhint="send" placeholder="Make it count"></textarea><span class="count" id="cnt">0/80</span></div>`,
        foot: `<button class="big-btn" type="button" data-act="answer">Send</button><button class="ghost" type="button" data-act="safety">Give me a safety quip</button>`,
        input: true,
      };
    }
    if (v.phase === 'final_vote') {
      if (v.voted)
        return {
          key: base + ':fvoted',
          main: wait('Votes locked in', 'Results on the big screen soon.'),
          foot: skipBtn(),
        };
      const most = Math.min(v.votes_each, v.choices.length);
      return {
        key: base + ':fvote',
        main: `<span class="kicker">Final round · you have ${most} votes</span><h2 class="ptitle">${esc(v.prompt)}</h2>
          <p class="pnote">Tap up to ${most} answers you love, then lock them in.</p>
          ${v.choices.map((c, k) => `<button class="choice" type="button" data-fvote="${esc(c.id)}" aria-pressed="false"><span class="l">${'ABCDEFGH'[k]}</span>${esc(c.text)}</button>`).join('')}`,
        foot: `<button class="big-btn" type="button" data-act="fvote" disabled>Pick up to ${most}</button>`,
        after: () => {
          fpicks = [];
          fmost = most;
        },
        input: true,
      };
    }
    if (v.phase === 'final_reveal') {
      return {
        key: base + ':freveal',
        main: `<span class="kicker">Final round</span><h2 class="ptitle">${v.gain ? 'People loved it!' : 'Tough crowd.'}</h2><div class="card hot"><span class="pnote">You got</span><span class="gain">+${fmt(v.gain)}</span></div>`,
        foot: skipBtn(),
      };
    }
    return scoresScreen(v);
  }

  function bluffScreen(v) {
    const base = `bluff:${v.phase}:${v.number}`;
    const tag = `${v.final ? 'Final question' : `Question ${v.number} of ${v.of}`}${v.mult > 1 ? ` · ×${v.mult} points` : ''}`;
    if (v.phase === 'lie') {
      if (v.lied)
        return {
          key: base + ':done',
          main: wait('Lie submitted', `“${esc(v.lied)}”. Let's see who falls for it.`),
          foot: skipBtn(),
        };
      return {
        key: base + ':write',
        main: `<span class="kicker">${tag} · write a lie</span><h2 class="ptitle">${factParts(v.question)}</h2>
          <div class="field"><label for="f-lie">Your lie</label><input id="f-lie" maxlength="40" enterkeyhint="send" autocomplete="off" placeholder="Something believable"></div>`,
        foot: `<button class="big-btn" type="button" data-act="lie">Send lie</button>${
          (v.suggestions || []).length
            ? `<details class="liefor"><summary>Lie for me</summary><div>${v.suggestions.map((t) => `<button class="choice" type="button" data-send='${esc(JSON.stringify({ type: 'lie', text: t }))}'>${esc(t)}</button>`).join('')}</div></details>`
            : ''
        }`,
        input: true,
      };
    }
    if (v.phase === 'pick') {
      if (v.picked !== null && v.picked !== undefined)
        return {
          key: base + ':picked',
          main: `${wait('Answer locked in', 'Did you find the truth? Look up!')}${
            v.options.some((o) => !o.mine)
              ? `<p class="pnote">Tap ♥ on the lies you loved.</p><div class="likes">${v.options
                  .map((o, k) =>
                    o.mine
                      ? ''
                      : `<button class="like" type="button" data-like="${k}" aria-pressed="${!!o.liked}"><span class="h">♥</span>${esc(o.text)}</button>`,
                  )
                  .join('')}</div>`
              : ''
          }`,
          foot: skipBtn(),
        };
      return {
        key: base + ':pick',
        main: `<span class="kicker">${tag} · find the truth</span><h2 class="ptitle">${factParts(v.question)}</h2>
          ${v.options.map((o, k) => `<button class="choice" type="button" ${o.mine ? 'disabled' : ''} data-send='${esc(JSON.stringify({ type: 'pick', choice: k }))}'><span class="l">${'ABCDEFGHI'[k]}</span>${esc(o.text)}${o.mine ? '<span class="mine">your lie</span>' : ''}</button>`).join('')}`,
        input: true,
      };
    }
    if (v.phase === 'reveal') {
      const fooled = (v.fooled || []).map((p) => player(p).name);
      return {
        key: base + ':reveal',
        main: `<span class="kicker">How you did</span><h2 class="ptitle">${v.found ? 'You found the truth!' : v.picked ? 'You got fooled!' : 'No answer this time'}</h2>
          <div class="card ${v.found ? 'hot' : ''}"><b>${v.found ? `It really was ${esc(v.answer)}` : v.picked ? `You picked “${esc(v.picked)}”` : 'Time ran out'}</b></div>
          ${v.house ? '<p class="pnote">That one was a house lie: nobody scores from it.</p>' : ''}
          ${v.my_lie ? `<div class="card ${fooled.length ? 'hot' : ''}"><b>Your lie: “${esc(v.my_lie)}”</b><span class="pnote">${fooled.length ? `Fooled ${esc(fooled.join(', '))}` : 'Nobody fell for it'}${v.likes ? ` · ♥ ${v.likes}` : ''}</span></div>` : ''}
          <div class="card"><span class="pnote">This question</span><span class="gain">+${fmt(v.gain)}</span></div>`,
        foot: skipBtn(),
      };
    }
    return scoresScreen(v);
  }

  // -- Drama Club ------------------------------------------------------------------------
  const MOODS = { neutral: 'Neutral', flustered: 'Flustered', sad: 'Sad', angry: 'Angry' };
  function castCards(cast) {
    return `<div class="dcast">${cast
      .map(
        (c, k) =>
          `<div class="card"><span class="kicker">${k ? 'Character B' : 'Character A'}</span><b>${esc(c.name)}</b><span class="pnote">${esc(c.bio || 'No bio: make something up!')}</span></div>`,
      )
      .join('')}</div>`;
  }
  /** The script editor: who says each line, how they feel, and what they say. */
  function lineRows(job, most) {
    const names = [job.cast[0].name, job.cast[1].name, 'Narrator'];
    return `${dLines.lines
      .map(
        (ln, i) => `<div class="dline">
          <div class="who">${names.map((n, w) => `<button type="button" data-lwho="${i}:${w}" aria-pressed="${ln.who === w}">${esc(n)}</button>`).join('')}</div>
          ${
            ln.who === 2
              ? ''
              : `<div class="moods">${Object.entries(MOODS)
                  .map(
                    ([k, label]) =>
                      `<button type="button" data-lemo="${i}:${k}" aria-pressed="${ln.emotion === k}">${label}</button>`,
                  )
                  .join('')}</div>`
          }
          <div class="row"><input data-ltext="${i}" maxlength="80" autocomplete="off" placeholder="${ln.who === 2 ? 'What happens…' : 'What they say…'}" value="${esc(ln.text)}">${dLines.lines.length > 1 ? `<button type="button" class="x" data-ldel="${i}" aria-label="Remove line">×</button>` : ''}</div>
        </div>`,
      )
      .join('')}
      ${dLines.lines.length < most ? '<button class="ghost" type="button" data-act="addline">+ Add a line</button>' : ''}`;
  }
  function redrawLines(job, most) {
    const box = $('dlines');
    if (box) box.innerHTML = lineRows(job, most);
  }
  function dramaScreen(v) {
    const D = window.PartyDraw;
    const S = window.PartyScenes;
    const base = `drama:${v.phase}:${v.round}`;
    const tag = `Drama Club${v.theme ? ` · ${v.theme}` : ''}`;
    if (v.phase === 'pitch') {
      if (v.mine)
        return {
          key: base + ':done',
          main: wait('Pitched!', `“${esc(v.mine)}”. Waiting for the others.`),
          foot: skipBtn(),
        };
      return {
        key: base,
        main: `<span class="kicker">Drama Club · pitch a theme</span><h2 class="ptitle">What's today's story about?</h2>
          <p class="pnote">Idea: ${esc(v.idea || 'anything dramatic')}</p>
          <div class="field"><label for="f-theme">Your theme</label><input id="f-theme" maxlength="50" enterkeyhint="send" autocomplete="off" placeholder="A haunted bakery"></div>`,
        foot: '<button class="big-btn" type="button" data-act="theme">Pitch it</button>',
        input: true,
      };
    }
    if (v.phase === 'pitch_vote') {
      if (v.voted || !v.themes.length)
        return {
          key: base + ':voted',
          main: wait('Vote in', 'Look at the big screen.'),
          foot: skipBtn(),
        };
      return {
        key: base,
        main: `<span class="kicker">Drama Club · pick the theme</span><h2 class="ptitle">Which story should we tell?</h2>
          ${v.themes.map((t) => `<button class="choice" type="button" data-send='${esc(JSON.stringify({ type: 'vote', choice: t.pid }))}'>${esc(t.text)}</button>`).join('')}`,
        input: true,
      };
    }
    if (v.phase === 'cast') {
      const c = v.character;
      if (!c.name)
        return {
          key: base + ':name',
          main: `<span class="kicker">${esc(tag)}</span><h2 class="ptitle">Create a character</h2>
            <p class="pnote">Only you will see what they look like until the show. Others only get the name and bio.</p>
            <div class="field"><label for="f-cname">Name</label><input id="f-cname" maxlength="18" autocomplete="off" placeholder="Vlad"></div>
            <div class="field"><label for="f-cbio">One-line bio</label><input id="f-cbio" maxlength="50" autocomplete="off" placeholder="a nervous vampire who runs the bakery"></div>`,
          foot: '<button class="big-btn" type="button" data-act="castname">Next: draw them</button>',
          input: true,
        };
      const sent = Object.keys(c.faces);
      const all = Object.keys(MOODS).every((k) => sent.includes(k));
      // dEmo: the mood being drawn, or '__done' once all four are in and the player is happy
      if (dEmo === '__done' && !all) dEmo = null;
      if (dEmo !== '__done' && !(dEmo in MOODS))
        dEmo = Object.keys(MOODS).find((k) => !sent.includes(k)) || 'neutral';
      const tabs = `<div class="moods tabs">${Object.entries(MOODS)
        .map(
          ([k, label]) =>
            `<button type="button" data-dtab="${k}" aria-pressed="${k === dEmo}">${sent.includes(k) ? '✓ ' : ''}${label}</button>`,
        )
        .join('')}</div>`;
      if (all && dEmo === '__done')
        return {
          key: base + ':all',
          main: wait('Your cast is ready!', 'Nobody has seen them yet. Wait for the show!'),
          foot: skipBtn(),
        };
      const neutral = c.faces.neutral;
      return {
        key: `${base}:${dEmo}:${sent.join(',')}`,
        main: `<span class="kicker">${esc(c.name)} · draw them ${MOODS[dEmo].toLowerCase()}</span>${tabs}
          <p class="pnote">${dEmo === 'neutral' ? `Full character, standing. No background: ${esc(v.theme || 'the scene')} goes behind them.` : 'The neutral drawing is faded underneath: tap Start from neutral, then change the face.'}</p>
          <div id="studio"></div>`,
        flow: true,
        foot: `<div class="footrow"><button class="big-btn" type="button" data-act="face">${all ? 'Save' : `Save ${MOODS[dEmo].toLowerCase()}`}</button>${dEmo !== 'neutral' && neutral ? '<button class="ghost" type="button" data-act="fromneutral">Start from neutral</button>' : all ? '<button class="ghost" type="button" data-act="castdone">Done</button>' : ''}</div>`,
        after: () => {
          pad = D.Studio($('studio'), {
            sprite: true,
            guide: dEmo !== 'neutral' ? neutral : null,
            start: c.faces[dEmo],
          });
        },
        input: true,
      };
    }
    if (v.phase === 'script' || v.phase === 'twist') {
      const job = v.job;
      if (!job || job.done)
        return {
          key: base + ':done',
          main: wait(
            job ? 'Sent!' : 'Sit tight',
            'Waiting for the others. No peeking at the big screen yet!',
          ),
          foot: skipBtn(),
        };
      const twist = v.phase === 'twist';
      const lkey = `${base}:${job.premise}`;
      if (!dLines || dLines.key !== lkey)
        dLines = { key: lkey, lines: [{ who: twist ? 2 : 0, emotion: 'neutral', text: '' }] };
      const names = [job.cast[0].name, job.cast[1].name, 'Narrator'];
      const sofar = twist
        ? `<div class="card dsofar">${job.lines.map((ln) => `<p><b>${esc(names[ln.who])}</b>${ln.who === 2 ? '' : ` <i>(${MOODS[ln.emotion].toLowerCase()})</i>`}: ${esc(ln.text)}</p>`).join('')}</div>`
        : '';
      if (!twist && !v.backgrounds.includes(dBg)) dBg = v.backgrounds[0];
      const head = twist
        ? `<h2 class="ptitle">How does it end?</h2>
          <div class="dstage">${S.svg(job.bg, '', true)}<span>${esc(S.LABELS[job.bg] || '')}</span></div>
          <p class="pnote">${esc(job.premise)}</p>${sofar}`
        : `<h2 class="ptitle">Write the scene</h2>
          <p class="pnote">You only know these two by name and bio. ${v.has_twist ? 'Set it up and build the drama, then <b>stop on a cliffhanger</b>: someone else writes the ending.' : 'Give it a beginning, a middle and an end.'}</p>
          ${castCards(job.cast)}
          <h3 class="dhead">Where?</h3>
          <div class="bgpick row">${v.backgrounds.map((b) => `<button type="button" data-bg="${b}" aria-pressed="${b === dBg}" aria-label="${esc(S.LABELS[b])}">${S.svg(b, '', true)}<span>${esc(S.LABELS[b])}</span></button>`).join('')}</div>
          <div class="field"><label for="f-premise">What's happening? (the scene's title)</label><input id="f-premise" maxlength="70" autocomplete="off" placeholder="Locked in the bakery at midnight with one cupcake left"></div>
          <h3 class="dhead">The script</h3>`;
      return {
        key: lkey,
        main: `<span class="kicker">${esc(tag)} · ${twist ? 'write the twist' : 'write the scene'}</span>
          ${head}
          <div id="dlines">${lineRows(job, v.max_lines)}</div>`,
        foot: `<button class="big-btn" type="button" data-act="${twist ? 'sendtwist' : 'sendscript'}">${twist ? 'Send the twist' : 'Send the script'}</button>`,
        after: () => {
          dJob = job;
          dMost = v.max_lines;
        },
        keep: true,
        input: true,
      };
    }
    if (v.phase === 'show')
      return {
        key: `${base}:show:${v.number}`,
        main: wait(
          `Scene ${v.number} of ${v.of}`,
          `${esc(v.premise)}${v.yours ? '<br>You helped make this one!' : ''}`,
        ),
        foot: skipBtn(),
      };
    if (v.phase === 'vote') {
      const vs = v.voted || {};
      if (vs.scene !== null && vs.scene !== undefined && vs.character)
        return {
          key: base + ':voted',
          main: wait('Votes in!', 'Look at the big screen.'),
          foot: skipBtn(),
        };
      return {
        key: `${base}:${vs.scene}:${vs.character}`,
        main: `<span class="kicker">Drama Club · vote</span><h2 class="ptitle">Best scene and best character</h2>
          <h3 class="dhead">Best scene</h3>
          ${v.scenes.map((sc) => `<button class="choice" type="button" aria-pressed="${vs.scene === sc.index}" data-send='${esc(JSON.stringify({ type: 'vote', scene: sc.index }))}'><span class="dthumb">${S.svg(sc.bg, '', true)}</span>${esc(sc.premise)}<small>${esc(sc.cast.join(' & '))}</small></button>`).join('')}
          <h3 class="dhead">Best character</h3>
          <div class="dgallery">${v.characters.map((c) => `<button type="button" aria-pressed="${vs.character === c.pid}" data-send='${esc(JSON.stringify({ type: 'vote', character: c.pid }))}'>${D.artCanvas(c.face, '')}<span>${esc(c.name)}</span></button>`).join('')}</div>`,
        input: true,
      };
    }
    return scoresScreen(v);
  }

  function shirtScreen(v) {
    const D = window.PartyDraw;
    const base = `shirt:${v.phase}:${v.round}`;
    const tag = `Round ${v.round}`;
    if (v.phase === 'draw') {
      if (v.done)
        return {
          key: base + ':done',
          main: wait('Designs in!', `${v.count} of yours in the pool. Waiting for the others.`),
          foot: skipBtn(),
        };
      return {
        key: `${base}:${v.count}`,
        main: `<span class="kicker">${tag} · draw designs · ${v.count} of ${v.max} sent</span>
          <p class="pnote">Idea: ${esc(v.idea || 'anything!')}</p>
          <div id="studio"></div>`,
        flow: true,
        foot: `<div class="footrow"><button class="big-btn" type="button" data-act="senddraw" ${v.count >= v.max ? 'disabled' : ''}>Send drawing</button><button class="ghost" type="button" data-send='{"type":"done"}'>Done</button></div>`,
        after: () => {
          pad = D.Studio($('studio'));
        },
        input: true,
      };
    }
    if (v.phase === 'slogan') {
      if (v.done)
        return {
          key: base + ':done',
          main: wait('Slogans in!', 'Waiting for the others.'),
          foot: skipBtn(),
        };
      return {
        key: `${base}:${v.mine.length}`,
        main: `<span class="kicker">${tag} · write slogans</span><h2 class="ptitle">Write slogans for shirts</h2>
          <p class="pnote">Idea: ${esc(v.idea || 'anything!')}</p>
          <div class="field"><label for="f-slogan">Your slogan</label><input id="f-slogan" maxlength="40" enterkeyhint="send" autocomplete="off" placeholder="Short and punchy"></div>
          ${v.mine.length ? `<div class="minis">${v.mine.map((t) => `<span class="chip">${esc(t)}</span>`).join('')}</div>` : ''}`,
        foot: `<button class="big-btn" type="button" data-act="slogan" ${v.mine.length >= v.max ? 'disabled' : ''}>Add slogan</button><button class="ghost" type="button" data-send='{"type":"done"}'>I'm done writing</button>`,
        input: true,
      };
    }
    if (v.phase === 'assemble') {
      if (v.made)
        return {
          key: base + ':made',
          main: wait('Shirt printed!', 'Get ready to vote on the big screen.'),
          foot: skipBtn(),
        };
      const key = `${base}:${v.drawings.map((d) => d.id).join(',')}|${v.slogans.map((x) => x.id).join(',')}|${v.rerolls.join(',')}`;
      const reroll = (what, label) =>
        v.rerolls.includes(what)
          ? `<button class="ghost" type="button" data-send='{"type":"reroll","what":"${what}"}'>${label} (1 left)</button>`
          : '';
      return {
        key,
        main: `<span class="kicker">${tag} · make your shirt</span><h2 class="ptitle">Pick a design, a slogan and a color</h2>
          <div class="preview" id="preview"></div>
          ${v.drawings.length ? `<div class="hand">${v.drawings.map((d) => `<button type="button" data-pickd="${d.id}" aria-label="Design">${D.artCanvas(d.strokes, '')}</button>`).join('')}</div>${reroll('drawings', 'New designs')}` : '<p class="pnote">No designs left: this one is all slogan.</p>'}
          ${v.slogans.map((x) => `<button class="choice" type="button" data-picks="${x.id}">${esc(x.text)}</button>`).join('')}${reroll('slogans', 'New slogans')}
          <div class="swatches">${D.SHIRTS.map((c, i) => `<button type="button" data-color="${i}" style="--sw:${c.bg}" aria-label="${c.name} shirt"></button>`).join('')}</div>
          `,
        foot: `<button class="big-btn" type="button" data-act="shirt">Print my shirt</button>`,
        after: () => {
          hand = v;
          sel = { drawing: v.drawings[0]?.id ?? null, slogan: v.slogans[0]?.id ?? null, color: 1 };
          preview();
        },
        input: true,
      };
    }
    if (v.phase === 'vote' || v.phase === 'final_vote') {
      const fin = v.phase === 'final_vote';
      const ids = v.shirts.map((x) => x.id).join(',');
      if (!v.can_vote.length)
        return {
          key: `${base}:${ids}:out`,
          main: wait("Your shirt's up!", 'You helped make one of these, so sit this vote out.'),
          foot: skipBtn(),
        };
      if (v.voted)
        return {
          key: `${base}:${ids}:voted`,
          main: wait('Vote locked in', 'Watch the big screen!'),
          foot: skipBtn(),
        };
      return {
        key: `${base}:${ids}`,
        main: `<span class="kicker">${fin ? 'The final' : `${tag} · showdown`}</span><h2 class="ptitle">Which shirt would you wear?</h2>
          <div class="shirtvote">${v.shirts.map((x, k) => `<button type="button" ${v.can_vote.includes(x.id) ? '' : 'disabled'} data-send='${esc(JSON.stringify({ type: 'vote', choice: x.id }))}'>${D.shirt(x, 'sm')}${v.can_vote.includes(x.id) ? (k ? 'Right' : 'Left') : 'Yours'}</button>`).join('')}</div>`,
        input: true,
      };
    }
    if (v.phase === 'reveal' || v.phase === 'final_reveal') {
      return {
        key: `${base}:${(v.battle || []).join(',')}`,
        main: v.gain
          ? `<span class="kicker">Votes are in</span><h2 class="ptitle">Your work scored!</h2><div class="card hot"><span class="pnote">You got</span><span class="gain">+${fmt(v.gain)}</span></div>`
          : wait('Look up!', 'Who wins this one?'),
        foot: skipBtn(),
      };
    }
    if (v.phase === 'round_over') {
      return {
        key: base + ':over',
        main: v.mine
          ? `<span class="kicker">${tag} winner</span><h2 class="ptitle">Your work won the round!</h2><div class="card hot"><span class="pnote">Bonus</span><span class="gain">+${fmt(v.gain)}</span></div>`
          : v.made
            ? wait(
                'The shirt you put together won!',
                'The points go to whoever drew and wrote it. Nice eye!',
              )
            : wait(`${tag} is over`, 'Check out the winning shirt on the big screen.'),
        foot: skipBtn(),
      };
    }
    return scoresScreen(v);
  }

  function preview() {
    const D = window.PartyDraw;
    const d = hand.drawings.find((x) => x.id === sel.drawing);
    const t = hand.slogans.find((x) => x.id === sel.slogan);
    $('preview').innerHTML = D.shirt(
      { color: sel.color, strokes: d ? d.strokes : [], slogan: t ? t.text : '' },
      'pv',
    );
    D.paint($('preview'));
    for (const b of document.querySelectorAll('[data-pickd]'))
      b.setAttribute('aria-pressed', String(Number(b.dataset.pickd) === sel.drawing));
    for (const b of document.querySelectorAll('[data-picks]'))
      b.setAttribute('aria-pressed', String(Number(b.dataset.picks) === sel.slogan));
    for (const b of document.querySelectorAll('[data-color]'))
      b.setAttribute('aria-pressed', String(Number(b.dataset.color) === sel.color));
  }

  function screen() {
    if (!pid || !st) return joinScreen();
    const v = st.view;
    if (st.room.state === 'playing' && v) {
      if (v.phase === 'next_game')
        return {
          key: 'next',
          main: wait(
            "You're in for the next game",
            "This one's already going. Watch the big screen!",
          ),
        };
      if (v.game === 'shirt') return shirtScreen(v);
      if (v.game === 'drama') return dramaScreen(v);
      return v.game === 'quip' ? quipScreen(v) : bluffScreen(v);
    }
    if (st.room.state === 'results' && v) return resultsScreen(v);
    return lobbyScreen();
  }

  // -- render ----------------------------------------------------------------------------
  function header() {
    if (!pid || !st) {
      $('top').innerHTML = '';
      return;
    }
    const p = player(pid);
    const v = st.view;
    const pts = st.room.state === 'playing' ? p.score : p.night;
    const timed =
      st.room.state === 'playing' && v && typeof v.ends_in === 'number' && v.ends_in > 0;
    $('top').innerHTML =
      `<div class="phtop">${av(pid)}<div class="who">${esc(p.name)}<small>Room ${esc(st.room.code)}${isVip() ? ' · VIP' : ''}</small></div>
      <button class="leave" type="button" data-act="leave" aria-label="Leave this room">Leave</button>
      <div class="pt">${fmt(pts)} pts${timed ? `<span class="mring"><svg viewBox="0 0 40 40"><circle class="bgc" cx="20" cy="20" r="16"/><circle class="fgc" id="ring" cx="20" cy="20" r="16" stroke-dasharray="100.5" stroke-dashoffset="0"/></svg><span id="left"></span></span>` : ''}</div></div>`;
  }

  function render() {
    const s = screen();
    const v = st && st.view;
    const game = s.game || (st && st.room.state === 'playing' && v ? v.game : 'none');
    $('ph').dataset.game = game;
    header();
    if (v && typeof v.ends_in === 'number') {
      if (s.key !== lastKey || total === null) total = Math.max(v.ends_in, 1);
      deadline = performance.now() / 1000 + v.ends_in;
    } else {
      deadline = null;
      total = null;
    }
    if (s.key !== lastKey) {
      if (pad) pad.destroy();
      pad = null;
      const sameScreen = s.keep && lastKey.split(':')[0] === s.key.split(':')[0];
      const typed = sameScreen
        ? [...$('main').querySelectorAll('input[id]')].map((x) => [
            x.id,
            x.value,
            x === document.activeElement,
          ])
        : [];
      $('main').innerHTML = s.main;
      split($('main'), !s.flow);
      for (const [id, value, focused] of typed) {
        const x = $(id);
        if (!x) continue;
        x.value = value;
        if (focused) x.focus();
      }
      if (window.PartyDraw) window.PartyDraw.paint($('main'));
      $('main').classList.remove('enter');
      if (!sameScreen) {
        void $('main').offsetWidth; // restart the entrance animation
        $('main').classList.add('enter');
      }
      $('foot').innerHTML = s.foot || '';
      $('foot').hidden = !s.foot;
      $('foot').classList.toggle('flow', !!s.flow);
      if (s.after) s.after();
      if (s.input && pid) buzz();
      lastKey = s.key;
    }
    paintTimer();
    wakeLock(st && st.room.state === 'playing');
  }

  // On a PC the screen is wide: the question (kicker, title, a note, the shirt preview) goes in
  // a column on the left and everything to answer with on the right. On phones both wrappers
  // are display: contents, so nothing moves.
  function split(main, allowed) {
    const kids = [...main.children];
    let n = 0;
    while (n < kids.length && kids[n].matches('.kicker, .ptitle, .pnote, .preview')) n++;
    const ok = allowed && n > 0 && n < kids.length && kids[0].matches('.kicker, .ptitle');
    main.classList.toggle('split', ok);
    if (!ok) return;
    const q = document.createElement('div');
    const a = document.createElement('div');
    q.className = 'phq';
    a.className = 'pha';
    q.append(...kids.slice(0, n));
    a.append(...kids.slice(n));
    main.append(q, a);
  }

  function paintTimer() {
    const ring = $('ring');
    if (!ring || deadline === null) return;
    const left = Math.max(0, deadline - performance.now() / 1000);
    ring.style.strokeDashoffset = String(100.5 * (1 - left / total));
    $('left').textContent = String(Math.ceil(left));
  }
  setInterval(paintTimer, 250);

  function showErr(text) {
    let e = $('main').querySelector('.err');
    if (!e) {
      e = document.createElement('div');
      e.className = 'err';
      e.setAttribute('role', 'alert');
      ($('main').querySelector('.pha') || $('main')).append(e);
    }
    e.textContent = text;
    buzz();
  }

  // -- actions ---------------------------------------------------------------------------
  // Opens a room of our own and turns this screen into its big screen.
  async function hostRoom() {
    try {
      const r = await fetch('/host/new', { method: 'POST' });
      const out = await r.json();
      if (!r.ok || !out.url) throw new Error(out.error || 'Hosting is not available right now.');
      location.href = out.url;
    } catch (e) {
      joinErr = e.message || 'Hosting is not available right now.';
      lastKey = '';
      render();
    }
  }

  function join() {
    const code = ($('f-code').value || '').trim().toUpperCase();
    const name = ($('f-name').value || '').trim();
    if (code.length !== 4) {
      joinErr = 'The room code is the 4 letters on the big screen.';
      lastKey = '';
      render();
      return;
    }
    if (!name) {
      joinErr = 'Type your name.';
      lastKey = '';
      render();
      return;
    }
    me = { ...me, code, name };
    kicked = false;
    send({
      type: 'join',
      code,
      name,
      token: me.token && me.tokenCode === code ? me.token : undefined,
      device: me.device,
    });
  }

  function act(a) {
    if (a === 'theme') {
      const t = ($('f-theme').value || '').trim();
      if (!t) return showErr('Write a theme first.');
      return send({ type: 'theme', text: t });
    }
    if (a === 'castname') {
      const name = ($('f-cname').value || '').trim();
      if (!name) return showErr('Give your character a name.');
      dEmo = 'neutral';
      return send({ type: 'character', name, bio: ($('f-cbio').value || '').trim() });
    }
    if (a === 'face') {
      if (!pad || pad.empty()) return showErr('Draw something first.');
      const v = st.view;
      send({ type: 'face', emotion: dEmo, strokes: pad.strokes() });
      const sent = new Set([...Object.keys(v.character.faces), dEmo]);
      dEmo = Object.keys(MOODS).find((k) => !sent.has(k)) || '__done';
      return;
    }
    if (a === 'fromneutral') {
      const n = st.view.character.faces.neutral;
      if (pad && n) pad.load(n);
      return;
    }
    if (a === 'castdone') {
      dEmo = '__done';
      lastKey = '';
      return render();
    }
    if (a === 'addline' && dLines && dJob) {
      const last = dLines.lines[dLines.lines.length - 1];
      dLines.lines.push({ who: last.who === 0 ? 1 : 0, emotion: 'neutral', text: '' });
      return redrawLines(dJob, dMost);
    }
    if ((a === 'sendscript' || a === 'sendtwist') && dLines) {
      const lines = dLines.lines.map((ln) => ({ ...ln, text: ln.text.trim() }));
      if (lines.some((ln) => !ln.text))
        return showErr('A line is empty: write something or remove it.');
      if (a === 'sendtwist') return send({ type: 'twist', lines });
      const premise = ($('f-premise').value || '').trim();
      if (!premise) return showErr("Give the scene a title: what's happening, in one line.");
      return send({ type: 'script', bg: dBg, premise, lines });
    }
    if (a === 'join') return join();
    if (a === 'leave') {
      const mid = st && st.room.state === 'playing' && st.view && st.view.game;
      if (mid && !confirm('Leave this game? Your seat stays empty until it ends.')) return;
      return send({ type: 'leave' });
    }
    if (a === 'host') return hostRoom();
    if (a === 'pinjoin') {
      pinErr = '';
      return send({ type: 'join', code: me.code, name: pinFor, device: me.device, pin: pinDigits });
    }
    if (a === 'pinback') {
      pinFor = null;
      pinDigits = '';
      pinErr = '';
      lastKey = '';
      return render();
    }
    if (a === 'setpin') {
      const pin = ($('f-pin').value || '').trim();
      if (!/^[0-9]{4}$/.test(pin)) return showErr('A PIN is 4 digits.');
      return send({ type: 'set_pin', pin });
    }
    if (a === 'start') return send({ type: 'start', game: st.room.choice });
    if (a === 'again') return send({ type: 'start', game: st.view.game });
    if (a === 'answer' || a === 'safety') {
      const t = a === 'safety' ? pick(SAFETY) : ($('f-ans').value || '').trim();
      if (!t) return showErr('Type an answer first, or tap Give me a safety quip.');
      return send({ type: 'answer', text: t });
    }
    if (a === 'fvote') {
      if (!fpicks.length) return showErr('Tap at least one answer.');
      return send({ type: 'vote', choices: fpicks });
    }
    if (a === 'senddraw') {
      if (!pad || pad.empty()) return showErr('Draw something first.');
      return send({ type: 'drawing', strokes: pad.strokes() });
    }
    if (a === 'slogan') {
      const t = ($('f-slogan').value || '').trim();
      if (!t) return showErr('Write a slogan first.');
      return send({ type: 'slogan', text: t });
    }
    if (a === 'shirt') {
      if (sel.drawing === null && sel.slogan === null) return showErr('Pick a design or a slogan.');
      return send({ type: 'shirt', drawing: sel.drawing, slogan: sel.slogan, color: sel.color });
    }
    if (a === 'lie') {
      const t = ($('f-lie').value || '').trim();
      if (!t) return showErr('Write a lie first, or tap Lie for me.');
      return send({ type: 'lie', text: t });
    }
  }

  document.addEventListener('click', (e) => {
    const b = e.target.closest('button');
    if (!b || b.disabled || b.closest('.studio')) return;
    if (b.dataset.act) act(b.dataset.act);
    else if (b.dataset.pickd && sel) {
      sel.drawing = Number(b.dataset.pickd);
      preview();
    } else if (b.dataset.picks && sel) {
      sel.slogan = Number(b.dataset.picks);
      preview();
    } else if (b.dataset.color && sel) {
      sel.color = Number(b.dataset.color);
      preview();
    } else if (b.dataset.choose) send({ type: 'choose', game: b.dataset.choose });
    else if (b.dataset.dtab) {
      dEmo = b.dataset.dtab;
      lastKey = '';
      render();
    } else if (b.dataset.bg) {
      dBg = b.dataset.bg;
      for (const x of document.querySelectorAll('[data-bg]'))
        x.setAttribute('aria-pressed', String(x.dataset.bg === dBg));
    } else if ((b.dataset.lwho || b.dataset.lemo || b.dataset.ldel) && dLines && dJob) {
      if (b.dataset.lwho) {
        const [i, w] = b.dataset.lwho.split(':').map(Number);
        dLines.lines[i].who = w;
      } else if (b.dataset.lemo) {
        const [i, e] = b.dataset.lemo.split(':');
        dLines.lines[Number(i)].emotion = e;
      } else dLines.lines.splice(Number(b.dataset.ldel), 1);
      redrawLines(dJob, dMost);
    } else if (b.dataset.fvote) {
      const id = b.dataset.fvote;
      if (fpicks.includes(id)) fpicks = fpicks.filter((x) => x !== id);
      else if (fpicks.length < fmost) fpicks.push(id);
      else return showErr(`You only have ${fmost} votes. Tap one to take it back.`);
      buzz();
      for (const x of document.querySelectorAll('[data-fvote]'))
        x.setAttribute('aria-pressed', String(fpicks.includes(x.dataset.fvote)));
      const lock = document.querySelector('[data-act="fvote"]');
      lock.disabled = !fpicks.length;
      lock.textContent = fpicks.length
        ? `Lock in ${fpicks.length} vote${fpicks.length === 1 ? '' : 's'}`
        : `Pick up to ${fmost}`;
    } else if (b.dataset.key) {
      pinDigits =
        b.dataset.key === '⌫' ? pinDigits.slice(0, -1) : (pinDigits + b.dataset.key).slice(0, 4);
      buzz();
      render();
    } else if (b.dataset.like) {
      buzz();
      b.setAttribute('aria-pressed', String(b.getAttribute('aria-pressed') !== 'true'));
      send({ type: 'like', choice: Number(b.dataset.like) });
    } else if (b.dataset.send) {
      buzz();
      send(JSON.parse(b.dataset.send));
    }
  });
  document.addEventListener('input', (e) => {
    if (e.target.dataset.ltext !== undefined && dLines) {
      const ln = dLines.lines[Number(e.target.dataset.ltext)];
      if (ln) ln.text = e.target.value;
    }
    if (e.target.id === 'f-ans') $('cnt').textContent = `${e.target.value.length}/80`;
    if (e.target.id === 'f-code') e.target.value = e.target.value.toUpperCase();
  });
  document.addEventListener('keydown', (e) => {
    if (e.key !== 'Enter' || e.shiftKey) return;
    const id = e.target.id;
    if (id === 'f-code') {
      e.preventDefault();
      $('f-name').focus();
    } else if (id === 'f-name') {
      e.preventDefault();
      join();
    } else if (id === 'f-ans') {
      e.preventDefault();
      act('answer');
    } else if (id === 'f-lie') {
      e.preventDefault();
      act('lie');
    } else if (id === 'f-slogan') {
      e.preventDefault();
      act('slogan');
    }
  });
  // Keep the field you're typing in visible above the on-screen keyboard.
  document.addEventListener('focusin', (e) => {
    if (e.target.matches('input, textarea'))
      setTimeout(() => e.target.scrollIntoView({ block: 'center', behavior: 'smooth' }), 250);
  });

  // -- screen wake lock while playing ------------------------------------------------------
  let lock = null;
  async function wakeLock(on) {
    try {
      if (on && !lock && navigator.wakeLock && document.visibilityState === 'visible') {
        lock = await navigator.wakeLock.request('screen');
        lock.addEventListener('release', () => {
          lock = null;
        });
      } else if (!on && lock) {
        await lock.release();
        lock = null;
      }
    } catch {
      lock = null;
    }
  }

  // -- connection: reconnects by itself (phones lock, switch apps, lose signal) -------------
  let backoff = 400;
  let offlineT = null;
  function connect() {
    if (ws && ws.readyState <= 1) return;
    const proto = location.protocol === 'https:' ? 'wss' : 'ws';
    ws = new WebSocket(`${proto}://${location.host}/ws`);
    ws.onopen = () => {
      backoff = 400;
      clearTimeout(offlineT);
      $('offline').hidden = true;
      // Rejoin the last room only if it was recent: an old session must not drop someone into
      // a finished game's results.
      const fresh = me.at && Date.now() - me.at < 4 * 3600 * 1000;
      if (!pid && me.token && me.tokenCode && fresh && (!urlCode || urlCode === me.tokenCode)) {
        autoJoining = true;
        lastKey = '';
        render();
        send({
          type: 'join',
          code: me.tokenCode,
          name: me.name,
          token: me.token,
          device: me.device,
        });
      } else if (pid) {
        pid = null; // the server forgot this socket: join again with the token
        autoJoining = true;
        send({
          type: 'join',
          code: me.tokenCode,
          name: me.name,
          token: me.token,
          device: me.device,
        });
      }
    };
    ws.onmessage = (e) => {
      let msg;
      try {
        msg = JSON.parse(e.data);
      } catch {
        return;
      }
      if (msg.type === 'joined') {
        autoJoining = false;
        if (msg.pid) {
          pid = msg.pid;
          me = {
            ...me,
            token: msg.token,
            tokenCode: me.code || me.tokenCode,
            at: Date.now(),
            name: msg.name,
            device: msg.device || me.device,
          };
          store.set(me);
          joinErr = '';
          pinFor = null;
          pinDigits = '';
          pinErr = '';
        } else {
          pid = null;
          lastKey = '';
          render();
        }
      } else if (msg.type === 'hello') {
        if (canHost !== !!msg.hosting) {
          canHost = !!msg.hosting;
          if (!pid) render();
        }
      } else if (msg.type === 'left') {
        me = { name: me.name, device: me.device };
        store.set(me);
        pid = null;
        st = null;
        joinErr = '';
        lastKey = '';
        render();
      } else if (msg.type === 'closed') {
        me = { name: me.name, device: me.device };
        store.set(me);
        pid = null;
        st = null;
        joinErr = 'That room has closed. Join another with its code, or host your own.';
        lastKey = '';
        render();
      } else if (msg.type === 'kicked') {
        me = { name: me.name, device: me.device };
        store.set(me);
        pid = null;
        st = null;
        kicked = true;
        lastKey = '';
        render();
      } else if (msg.type === 'error') {
        if (!pid) {
          // A saved room that has closed: forget it and ask for the new code.
          if (msg.code === 'pin_needed') {
            pinFor = msg.name || me.name;
            pinDigits = '';
            pinErr = '';
          } else if (msg.code === 'pin_wrong' && pinFor) {
            pinErr = msg.message;
            pinDigits = '';
          } else if (autoJoining) {
            me = { name: me.name, device: me.device };
            store.set(me);
            joinErr = 'That room has closed. Type the code on the big screen.';
          } else joinErr = msg.message;
          autoJoining = false;
          lastKey = '';
          render();
        } else showErr(msg.message);
      } else if (msg.type === 'state') {
        st = msg;
        if (msg.you) pid = msg.you;
        render();
      }
    };
    ws.onclose = () => {
      ws = null;
      clearTimeout(offlineT);
      offlineT = setTimeout(() => {
        $('offline').hidden = false;
      }, 1500);
      setTimeout(connect, backoff);
      backoff = Math.min(backoff * 2, 5000);
    };
  }
  document.addEventListener('visibilitychange', () => {
    if (document.visibilityState === 'visible') {
      connect();
      wakeLock(st && st.room.state === 'playing');
    }
  });
  render();
  connect();
})();
