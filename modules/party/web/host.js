// Party Games: the big screen. Shows the room and the game; the phones do the playing.
(() => {
  'use strict';
  const KEY = new URLSearchParams(location.search).get('key') || '';
  const GLYPH = { quip: 'Q', bluff: 'B', shirt: 'T' };
  const $ = (id) => document.getElementById(id);
  const esc = (s) =>
    String(s ?? '').replace(
      /[&<>"']/g,
      (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' })[c],
    );
  const fmt = (n) => Number(n || 0).toLocaleString('en-US');

  let st = null;
  let lastScene = '';
  let lastContent = '';
  let deadline = null;
  let total = null;
  let ctlMsg = '';
  let showFame = false; // the lobby takes turns with the hall of fame
  const CROWN =
    '<svg class="crown" viewBox="0 0 34 22" aria-hidden="true"><path d="M2 20 L4 5 L11 12 L17 2 L23 12 L30 5 L32 20 Z" fill="var(--gold)" stroke="var(--ink)" stroke-width="2.5" stroke-linejoin="round"/></svg>';

  // -- layout: a 1280x720 stage scaled to fit the window --------------------------------
  const fit = () =>
    $('tvc').style.setProperty('--s', String(Math.min(innerWidth / 1280, innerHeight / 720)));
  addEventListener('resize', fit);
  fit();

  // -- helpers ---------------------------------------------------------------------------
  const player = (pid) => st.room.players.find((p) => p.pid === pid) || { name: '?', color: 0 };
  const nameOf = (pid) => player(pid).name;
  const av = (pid, size = '', extra = '', off = false) => {
    const p = player(pid);
    return `<span class="av ${size} c${p.color}${off ? ' off' : ''}" aria-hidden="true">${esc((p.name || '?')[0].toUpperCase())}${extra}</span>`;
  };
  const bar = (title, glyph) =>
    `<div class="tvbar"><div class="logo"><i>${glyph}</i>${esc(title)}</div><div class="code">Room <b>${esc(st.room.code)}</b></div></div>`;
  const ring = () =>
    `<div class="timer"><svg viewBox="0 0 100 100"><circle class="bgc" cx="50" cy="50" r="44"/><circle class="fgc" id="ring" cx="50" cy="50" r="44" stroke-dasharray="276.5" stroke-dashoffset="0"/></svg><span id="left"></span></div>`;
  const doneRow = (waiting) => {
    const w = new Set(waiting || []);
    const players = st.room.players.filter((p) => p.playing);
    return `<div class="doneRow">${players.map((p) => av(p.pid, '', w.has(p.pid) ? '' : '<span class="tick"></span>', w.has(p.pid))).join('')}</div>`;
  };
  const shortLink = () => (st.link || '').replace(/^https?:\/\//, '');

  // -- screens ---------------------------------------------------------------------------
  function lobby() {
    const r = st.room;
    const slots = r.players.map(
      (p) =>
        `<div class="slot ${p.connected ? '' : 'off'}">${av(p.pid, 'sm', p.champ ? CROWN : '')}<div>${esc(p.name)}<small>${p.bot ? '<b class="bot">BOT</b> ' : ''}${p.champ ? 'Champion · ' : ''}${p.pid === r.vip ? 'VIP' : p.connected ? (p.night ? `${fmt(p.night)} tonight` : 'joined') : 'away'}</small></div></div>`,
    );
    for (let i = r.players.length; i < r.max_players; i++)
      slots.push(`<div class="slot empty">Open seat</div>`);
    const game = r.games.find((g) => g.key === r.choice) || r.games[0];
    const vip = r.vip ? nameOf(r.vip) : '';
    const need =
      game && r.players.filter((p) => p.connected).length < game.min
        ? ` · needs ${game.min}+ players`
        : '';
    return `<div class="scene" data-game="none">${bar('Party Night', '★')}
      <div class="join">
        <div class="left">
          <h2 class="big" style="font-size:76px">Grab your phone and join!</h2>
          ${st.link ? `<div class="url">${esc(shortLink())}</div>` : `<div class="url none">Press Share in Kernel to get a link for your friends.</div>`}
          <div class="tiles">${[...r.code].map((c) => `<span>${esc(c)}</span>`).join('')}</div>
          ${game ? `<div class="picking"><span class="sub">${vip ? `${esc(vip)} picks` : 'Up next · move the mouse for host controls'}</span><span class="gname" data-game="${esc(game.key)}">${esc(game.title)}</span></div><p class="sub" style="font-size:18px">${game.min}–${game.max} players${esc(need)}</p>` : ''}
        </div>
        <div class="slots">${slots.join('')}</div>
      </div>
    </div>`;
  }

  function scores(v, title) {
    const rows = v.standings || [];
    const max = Math.max(1, ...rows.map((s) => s.score));
    return `<div class="scene" data-game="${v.game}">${bar(title, GLYPH[v.game])}<span class="tag">${v.phase === 'over' ? 'Final scores' : 'Scores'}</span>
      <div class="board">${rows
        .map(
          (
            s,
            k,
          ) => `<div class="brow"><span class="rk">${1 + rows.filter((r) => r.score > s.score).length}</span>${av(s.pid)}<span class="nm">${esc(nameOf(s.pid))}</span>
        <span class="track"><i class="c${player(s.pid).color}" style="--w:${Math.round((s.score / max) * 100)}%;animation-delay:${k * 80}ms"></i></span>
        <span class="num">${fmt(s.score)}${s.gained ? `<small>+${fmt(s.gained)}</small>` : ''}</span></div>`,
        )
        .join('')}</div></div>`;
  }

  function quip(v) {
    const T = 'Quip Clash';
    const dbl = v.mult > 1 ? ' · double points' : '';
    if (v.phase === 'write') {
      return `<div class="scene" data-game="quip">${bar(T, 'Q')}<span class="tag">Round ${v.round}${dbl} · write</span>
        <h2 class="big" style="font-size:92px;max-width:14ch">Answer your two prompts on your phone!</h2>
        <p class="sub">Keep it short. Funnier beats true.</p>${doneRow(v.waiting)}${ring()}</div>`;
    }
    if (v.phase === 'vote' || v.phase === 'reveal') {
      const m = v.matchup;
      const reveal = v.phase === 'reveal';
      const side = (k) => {
        const text = m.answers[k];
        const win = reveal && m.winner === k;
        const total = reveal ? m.voters[0].length + m.voters[1].length : 0;
        const w = total ? Math.round((m.voters[k].length / total) * 100) : 0;
        return `<div class="ans ${win ? 'win' : ''}"><span class="letter">${'AB'[k]}</span>${reveal && m.sweep === k ? `<span class="sweep">Clean sweep! +${fmt(250 * v.mult)}</span>` : ''}
          <div class="txt ${text ? '' : 'empty'}">${text ? esc(text) : '(no answer)'}</div>
          ${
            reveal
              ? `<div class="bar"><i style="--w:${w}%"></i></div>
          <div class="voters">${m.voters[k].map((p) => av(p, 'xs')).join('')}</div>
          <div class="who">${av(m.authors[k], 'sm')}${esc(nameOf(m.authors[k]))}<span class="pts">+${fmt(m.points[k])}</span></div>`
              : ''
          }</div>`;
      };
      const note =
        reveal && m.outcome === 'jinx'
          ? '<div class="stamp jinx">JINX! Same answer, no points</div>'
          : reveal && m.outcome === 'forfeit'
            ? `<div class="stamp jinx">No contest: ${esc(nameOf(m.authors[m.winner]))} wins by default</div>`
            : '';
      return `<div class="scene" data-game="quip">${bar(T, 'Q')}<span class="tag">Round ${v.round}${dbl} · matchup ${m.number} of ${m.of}</span>
        <div class="bubble">${esc(m.prompt)}</div>${note}
        <div class="vs">${side(0)}<div class="vsx">VS</div>${side(1)}</div>
        ${reveal ? '' : `<p class="sub" style="margin-bottom:56px">Vote on your phone for the funnier answer.</p>${ring()}`}</div>`;
    }
    if (v.phase === 'final_write') {
      return `<div class="scene" data-game="quip">${bar(T, 'Q')}<span class="tag">Final round · everyone answers</span>
        <div class="bubble">${esc(v.final.prompt)}</div>${doneRow(v.waiting)}${ring()}</div>`;
    }
    if (v.phase === 'final_vote' || v.phase === 'final_reveal') {
      const reveal = v.phase === 'final_reveal';
      const answers = v.final.answers;
      const top = reveal && answers[0] ? answers[0].voters.length : -1;
      return `<div class="scene" data-game="quip">${bar(T, 'Q')}<span class="tag">Final round · ${reveal ? 'results' : `everyone gets ${v.final.votes_each} votes`}</span>
        <div class="bubble" style="font-size:38px">${esc(v.final.prompt)}</div>
        <div class="finals">${answers
          .map(
            (
              a,
            ) => `<div class="ans ${reveal && a.voters.length === top && top > 0 ? 'top' : ''}"><div class="txt">${esc(a.text)}</div>
          ${reveal ? `<div class="voters">${a.voters.map((p) => av(p, 'xs')).join('')}</div><div class="who">${av(a.by, 'xs')}${esc(nameOf(a.by))}<span class="pts">+${fmt(a.points)}</span></div>` : ''}</div>`,
          )
          .join('')}</div>
        ${reveal ? '' : ring()}</div>`;
    }
    return scores(v, T);
  }

  function bluff(v) {
    const T = 'Bluff Buffet';
    const tag = `${v.final ? 'Final question' : `Question ${v.number} of ${v.of}`}${v.mult > 1 ? ` · ×${v.mult} points` : ''}`;
    const fact = (size, fill, delay = 0) => {
      const [a, b] = v.question.split('___');
      const blank = fill
        ? `<span class="blank later" style="--d:${delay}s"><span class="q">?????</span><span class="a">${esc(fill)}</span></span>`
        : '<span class="blank">?????</span>';
      return `<p class="fact" style="font-size:${size}px">${esc(a)}${blank}${esc(b ?? '')}</p>`;
    };
    if (v.phase === 'lie') {
      return `<div class="scene" data-game="bluff">${bar(T, 'B')}<span class="tag">${tag} · write a lie</span>${fact(56)}
        <p class="sub">It's true. Write something believable enough to fool your friends.</p>${doneRow(v.waiting)}${ring()}</div>`;
    }
    if (v.phase === 'pick') {
      return `<div class="scene" data-game="bluff">${bar(T, 'B')}<span class="tag">${tag} · find the truth</span>${fact(42)}
        <div class="opts">${v.options.map((o) => `<div class="opt">${esc(o)}</div>`).join('')}</div>
        <p class="sub">Pick the real answer on your phone. You can't pick your own lie.</p>${ring()}</div>`;
    }
    if (v.phase === 'reveal') {
      const step = 2.5;
      const truthAt =
        Math.max(
          0,
          v.reveal.findIndex((o) => o.truth),
        ) * step;
      return `<div class="scene" data-game="bluff">${bar(T, 'B')}<span class="tag">${tag} · reveal</span>${fact(42, v.answer, truthAt)}
        <div class="opts">${v.reveal
          .map(
            (
              o,
              i,
            ) => `<div class="opt show ${o.truth ? 'truth' : ''}" style="animation-delay:${(i * step).toFixed(1)}s">
          <span class="flag ${o.truth ? '' : 'lie'}">${o.truth ? 'THE TRUTH' : o.house ? 'HOUSE LIE' : 'LIE'}</span>${esc(o.text)}${o.likes ? `<span class="hearts">♥ ${o.likes}</span>` : ''}
          <div class="by">${o.truth ? 'Found by' : o.house ? 'Our lie · fooled' : `By <b>${esc(o.authors.map(nameOf).join(' & '))}</b> · fooled`} ${o.pickers.map((p) => av(p, 'xs')).join('') || '<span>nobody</span>'}</div></div>`,
          )
          .join('')}</div></div>`;
    }
    return scores(v, T);
  }

  function shirtGame(v) {
    const D = window.PartyDraw;
    const T = 'Shirt Showdown';
    const fin = v.phase.startsWith('final');
    const tag = fin ? 'The final' : `Round ${v.round} of ${v.rounds}`;
    const head = (t) =>
      `<div class="scene" data-game="shirt">${bar(T, 'T')}<span class="tag">${t}</span>`;
    const who = (pid) => (pid ? esc(nameOf(pid)) : 'nobody');
    if (v.phase === 'draw' || v.phase === 'slogan') {
      const draw = v.phase === 'draw';
      const waiting = new Set(v.waiting || []);
      const counts = st.room.players
        .filter((p) => p.playing)
        .map(
          (p) =>
            `<div class="who">${av(p.pid, '', waiting.has(p.pid) ? '' : '<span class="tick"></span>')}${v.counts[p.pid] || 0}</div>`,
        )
        .join('');
      return `${head(`${tag} · ${draw ? 'draw' : 'write'}`)}
        <h2 class="big" style="font-size:84px;max-width:15ch">${draw ? 'Draw designs on your phone!' : 'Write slogans on your phone!'}</h2>
        <p class="sub">${draw ? 'As many as you like. Everything goes in a shared pool.' : 'Short and punchy. Anyone might end up wearing it.'}</p>
        <div class="counts">${counts}</div>${ring()}</div>`;
    }
    if (v.phase === 'assemble') {
      return `${head(`${tag} · make shirts`)}
        <h2 class="big" style="font-size:92px;max-width:14ch">Make your shirt!</h2>
        <p class="sub">Pick a design and a slogan from the pool. Choose wisely.</p>${doneRow(v.waiting)}${ring()}</div>`;
    }
    if (v.battle) {
      const b = v.battle;
      const reveal = v.phase.endsWith('reveal');
      const side = (k) => {
        const x = b.shirts[k];
        const role = fin
          ? `Round ${k + 1} winner`
          : k
            ? 'Challenger'
            : `Champion${b.streak && !reveal ? ` · ${b.streak} win streak` : ''}`;
        const cls = reveal ? (b.winner === k ? 'win' : 'lose') : '';
        return `<div class="side ${cls}"><span class="role">${role}</span>${D.shirt(x, 'mid')}
          ${reveal ? `<div class="count">${b.votes[k]} vote${b.votes[k] === 1 ? '' : 's'}</div><div class="voters">${b.voters[k].map((p) => av(p, 'xs')).join('')}</div><div class="credits">Art by <b>${who(x.artist)}</b> · Slogan by <b>${who(x.writer)}</b></div>` : ''}</div>`;
      };
      const stamp = fin ? 'WINNING SHIRT!' : b.winner === 0 ? 'DEFENDED!' : 'NEW CHAMPION!';
      return `${head(fin ? 'The final · round winners face off' : `${tag} · challenger ${b.number} of ${b.of}`)}
        <div class="battle">${side(0)}<div class="vsx">VS</div>${side(1)}</div>
        ${reveal ? `<div class="stamp" style="font-size:50px;top:330px;bottom:auto">${stamp}</div>` : `<p class="sub" style="margin-bottom:30px">Vote on your phone. You can't vote for a shirt you helped make.</p>${ring()}`}</div>`;
    }
    if (v.phase === 'round_over') {
      const w = v.winner;
      return `${head(`${tag} · winner`)}
        ${w ? `<div class="battle" style="grid-template-columns:1fr"><div class="side win">${D.shirt(w, 'big')}<div class="credits">Art by <b>${who(w.artist)}</b> · Slogan by <b>${who(w.writer)}</b>${v.streak ? ` · survived ${v.streak} challenge${v.streak === 1 ? '' : 's'}` : ''}</div></div></div>` : '<h2 class="big" style="font-size:60px">No shirts this round!</h2>'}</div>`;
    }
    return scores(v, T);
  }

  function fameScene() {
    const f = st.fame;
    const D = window.PartyDraw;
    const champName = f.champion ? f.champion.name : '';
    const board = f.board.length ? f.board : f.all_time;
    const hit = (h) =>
      h.shirt
        ? `<div class="fhit">${D.shirt(h.shirt, 'sm')}<div><b>Winning shirt</b><small>${esc(h.name)} · ${esc(h.title)}, ${esc(h.date)}</small></div></div>`
        : `<div class="fhit"><div><q>${esc(h.text)}</q><small>${esc(h.name)} · ${h.kind === 'lie' ? `fooled ${h.votes}` : `${h.votes} of ${h.of} votes`} · ${esc(h.title)}, ${esc(h.date)}</small></div></div>`;
    return `<div class="scene" data-game="none">${bar('Hall of Fame', '★')}
      <div class="fame">
        <div class="panel"><h3>${f.board.length ? esc(f.season) : 'All time'} <small>${f.board.length ? 'this season' : ''}</small></h3>
          ${board
            .slice(0, 6)
            .map(
              (r, k) =>
                `<div class="rv"><span class="rk">${k + 1}</span><div>${esc(r.name)}${r.name === champName ? ' ' + CROWN.replace('class="crown"', 'class="crown inline"') : ''}<small>${r.wins} win${r.wins === 1 ? '' : 's'} · ${r.games} game${r.games === 1 ? '' : 's'}</small></div><span class="sc">${fmt(r.points)}</span></div>`,
            )
            .join('')}</div>
        <div class="panel"><h3>Rivalries <small>head to head</small></h3>
          ${f.rivalries.length ? f.rivalries.map((x) => `<div class="rv"><span></span><div>${esc(x.a)} vs ${esc(x.b)}</div><span class="sc">${x.a_won}–${x.b_won}</span></div>`).join('') : '<p class="sub" style="font-size:18px">Play a few games to start some rivalries.</p>'}
          <h3 style="margin-top:16px">Champions</h3>
          ${
            f.champions.length
              ? f.champions
                  .slice(0, 3)
                  .map(
                    (c) =>
                      `<div class="rv"><span></span><div>${esc(c.name)}<small>${esc(c.label)}</small></div><span class="sc">${fmt(c.points)}</span></div>`,
                  )
                  .join('')
              : `<p class="sub" style="font-size:18px">${champName ? `${esc(champName)} leads ${esc(f.season)} so far.` : 'The first season is under way.'}</p>`
          }</div>
        <div class="panel"><h3>Greatest hits</h3>
          ${f.hits.length ? f.hits.slice(0, 3).map(hit).join('') : '<p class="sub" style="font-size:18px">The best answers and shirts end up here.</p>'}</div>
      </div>
      <div class="credit" style="left:auto;right:24px">Join with room code ${esc(st.room.code)}${st.link ? ` at ${esc(shortLink())}` : ''}</div></div>`;
  }

  function results(v) {
    const s = v.standings || [];
    const winner = v.winner ? nameOf(v.winner) : '';
    const order = [s[1], s[0], s[2]];
    const hit = (v.hits || [])[0];
    return `<div class="scene" data-game="none">${bar('Party Night', '★')}
      <h2 class="big" style="font-size:60px;text-align:center">${winner ? `${esc(winner)} wins ${esc(v.title)}!` : `${esc(v.title)} is over`}</h2>
      ${hit ? `<div class="hitline">${hit.kind === 'shirt' ? 'Winning shirt' : 'Best of the game'}: <q>${esc(hit.text)}</q> · ${esc(hit.name)}</div>` : ''}
      <div class="podium">${order.map((x, k) => (x ? `<div class="pod p${[2, 1, 3][k]}">${av(x.pid)}<div class="name">${esc(nameOf(x.pid))}</div><div class="blk">${fmt(x.score)}</div></div>` : '<div></div>')).join('')}</div>
      ${
        (v.badges || []).length
          ? `<div class="toasts">${v.badges
              .slice(0, 3)
              .map(
                (b, i) =>
                  `<div class="toast" style="animation-delay:${1.2 + i * 0.8}s"><span class="badge">★</span><div>${esc(b.name)}: ${esc(b.title)}<small>${esc(b.about)}</small></div></div>`,
              )
              .join('')}</div>`
          : ''
      }
      <canvas class="confetti" id="confetti" width="1280" height="720"></canvas></div>`;
  }

  // -- render ----------------------------------------------------------------------------
  function render() {
    const r = st.room;
    const v = st.view;
    let html;
    let scene;
    if (r.state === 'playing' && v && v.game) {
      html = v.game === 'shirt' ? shirtGame(v) : v.game === 'quip' ? quip(v) : bluff(v);
      scene = [
        v.game,
        v.phase,
        v.round,
        v.number,
        v.matchup && v.matchup.number,
        v.battle && v.battle.shirts.map((x) => x.id).join(','),
      ].join(':');
    } else if (r.state === 'results' && v) {
      html = results(v);
      scene = 'results';
    } else if (showFame && st.fame && st.fame.games) {
      html = fameScene();
      scene = 'fame';
    } else {
      html = lobby();
      scene = 'lobby';
    }
    const { ends_in: _e, ...rest } = v || {};
    const content = JSON.stringify([r, rest, st.link, scene === 'fame' ? st.fame : null]);
    if (v && typeof v.ends_in === 'number') {
      if (scene !== lastScene || total === null) total = Math.max(v.ends_in, 1);
      deadline = performance.now() / 1000 + v.ends_in;
    } else {
      deadline = null;
      total = null;
    }
    if (content !== lastContent || scene !== lastScene) {
      const entering = scene !== lastScene;
      $('tvc').innerHTML = html;
      if (window.PartyDraw) window.PartyDraw.paint($('tvc'));
      const sc = $('tvc').firstElementChild;
      $('tvc').dataset.game = sc.dataset.game || 'none';
      if (entering) sc.classList.add('enter');
      if (entering && scene === 'results') confetti();
      lastContent = content;
      lastScene = scene;
    }
    paintTimer();
    controls();
  }

  function paintTimer() {
    const ring = $('ring');
    if (!ring || deadline === null) return;
    const left = Math.max(0, deadline - performance.now() / 1000);
    ring.style.strokeDashoffset = String(276.5 * (1 - left / total));
    $('left').textContent = String(Math.ceil(left));
  }
  setInterval(paintTimer, 250);
  // In the lobby, show the hall of fame for 10 seconds out of every 25.
  setInterval(() => {
    const t = Math.floor(Date.now() / 1000) % 25;
    const fame = t >= 15;
    if (fame !== showFame) {
      showFame = fame;
      if (st && st.room.state === 'lobby') render();
    }
  }, 500);

  function confetti() {
    if (matchMedia('(prefers-reduced-motion: reduce)').matches) return;
    const c = $('confetti');
    if (!c) return;
    const g = c.getContext('2d');
    const cs = getComputedStyle(document.documentElement);
    const cols = ['--quip', '--bluff', '--gold', '--danger', '--good'].map((x) =>
      cs.getPropertyValue(x).trim(),
    );
    const bits = Array.from({ length: 160 }, () => ({
      x: 760 + (Math.random() - 0.5) * 300,
      y: 300,
      vx: (Math.random() - 0.5) * 18,
      vy: -Math.random() * 16 - 6,
      r: Math.random() * 6.3,
      s: 8 + Math.random() * 10,
      c: cols[Math.floor(Math.random() * cols.length)],
    }));
    let f = 0;
    (function frame() {
      if (!document.body.contains(c)) return;
      g.clearRect(0, 0, 1280, 720);
      for (const b of bits) {
        b.x += b.vx;
        b.y += b.vy;
        b.vy += 0.45;
        b.vx *= 0.99;
        b.r += 0.15;
        g.save();
        g.translate(b.x, b.y);
        g.rotate(b.r);
        g.fillStyle = b.c;
        g.fillRect(-b.s / 2, -b.s / 4, b.s, b.s / 2);
        g.restore();
      }
      if (++f < 170) requestAnimationFrame(frame);
      else g.clearRect(0, 0, 1280, 720);
    })();
  }

  // -- host controls (show on mouse move; hidden on stream otherwise) ---------------------
  function controls() {
    if (!st) return;
    const r = st.room;
    const btn = (label, cmd, cls = '') =>
      `<button type="button" class="${cls}" data-cmd='${esc(JSON.stringify(cmd))}'>${esc(label)}</button>`;
    const rows = [];
    if (r.state === 'playing')
      rows.push(btn('Skip', { type: 'skip' }) + btn('End game', { type: 'end' }));
    else {
      rows.push(
        r.games
          .map((g) =>
            btn(g.title + (g.key === r.choice ? ' ✓' : ''), { type: 'choose', game: g.key }),
          )
          .join('') + btn('Start', { type: 'start', game: r.choice }),
      );
      const bots = r.players.filter((p) => p.bot).length;
      rows.push(
        (r.players.length < r.max_players ? btn('Add bot', { type: 'add_bot' }, 'bot') : '') +
          (bots ? btn(`Remove bots (${bots})`, { type: 'remove_bots' }, 'kick') : ''),
      );
      if (r.state === 'results') rows.push(btn('Back to lobby', { type: 'lobby' }));
    }
    if (r.players.length)
      rows.push(
        r.players.map((p) => btn(`Kick ${p.name}`, { type: 'kick', pid: p.pid }, 'kick')).join(''),
      );
    $('ctl').innerHTML =
      (ctlMsg ? `<div class="msg">${esc(ctlMsg)}</div>` : '') +
      rows.map((x) => `<div class="row">${x}</div>`).join('');
  }
  let hideT = null;
  addEventListener('mousemove', () => {
    $('ctl').classList.add('show');
    clearTimeout(hideT);
    hideT = setTimeout(() => $('ctl').classList.remove('show'), 3000);
  });
  $('ctl').addEventListener('click', (e) => {
    const b = e.target.closest('[data-cmd]');
    if (!b || !ws || ws.readyState !== 1) return;
    ctlMsg = '';
    ws.send(b.dataset.cmd);
  });

  // -- connection ------------------------------------------------------------------------
  let ws = null;
  let backoff = 500;
  function connect() {
    const proto = location.protocol === 'https:' ? 'wss' : 'ws';
    ws = new WebSocket(`${proto}://${location.host}/ws?role=host&key=${encodeURIComponent(KEY)}`);
    ws.onopen = () => {
      backoff = 500;
      $('offline').hidden = true;
    };
    ws.onmessage = (e) => {
      let msg;
      try {
        msg = JSON.parse(e.data);
      } catch {
        return;
      }
      if (msg.type === 'state') {
        st = msg;
        render();
      } else if (msg.type === 'error') {
        ctlMsg = msg.message;
        controls();
        $('ctl').classList.add('show');
      }
    };
    ws.onclose = () => {
      $('offline').hidden = false;
      setTimeout(connect, backoff);
      backoff = Math.min(backoff * 2, 5000);
    };
  }
  connect();
  try {
    navigator.wakeLock?.request('screen').catch(() => {});
  } catch {
    /* not supported */
  }
})();
