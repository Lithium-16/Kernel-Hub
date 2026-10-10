// Party Games: the big screen. Shows the room and the game; the phones do the playing.
(() => {
  'use strict';
  const KEY = new URLSearchParams(location.search).get('key') || '';
  const GLYPH = { quip: 'Q', bluff: 'B', shirt: 'T', drama: 'D' };
  // Timer speeds the host can pick (the server's TIMERS).
  const TIMERS = { fast: 'Fast', normal: 'Normal', relaxed: 'Relaxed', extra: 'Extra time' };
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
    const face = p.pfp
      ? `<img src="/pfp/${esc(p.pfp)}.webp" alt="">`
      : esc((p.name || '?')[0].toUpperCase());
    return `<span class="av ${size} c${p.color}${p.pfp ? ' pic' : ''}${off ? ' off' : ''}" aria-hidden="true">${face}${extra}</span>`;
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
  // A room opened from the share link shows the address it was opened at.
  const link = () => st.link || (st.room.guest ? location.origin : '');
  const shortLink = () => link().replace(/^https?:\/\//, '');

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
          ${link() ? `<div class="url">${esc(shortLink())}</div>` : `<div class="url none">Press Share in Kernel to get a link for your friends.</div>`}
          <div class="tiles">${[...r.code].map((c) => `<span>${esc(c)}</span>`).join('')}</div>
          ${game ? `<div class="picking"><span class="sub">${vip ? `${esc(vip)} picks` : 'Up next · move the mouse for host controls'}</span><span class="gname" data-game="${esc(game.key)}">${esc(game.title)}</span></div><p class="sub" style="font-size:18px">${game.min}–${game.max} players${esc(need)} · timers: ${esc(TIMERS[r.timer] || 'Normal')}</p>` : ''}
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

  // The how-to-play card before a game (the first time it's played tonight).
  function introScene(intro) {
    const how = (window.PartyHowTo || {})[intro.game] || { steps: [], score: '' };
    const g = st.room.games.find((x) => x.key === intro.game) || { title: '' };
    return `<div class="scene" data-game="${esc(intro.game)}">${bar(g.title, GLYPH[intro.game] || '★')}<span class="tag">How to play</span>
      <h2 class="big" style="font-size:72px">${esc(g.title)}</h2>
      <ol class="howto big-steps">${how.steps.map((x) => `<li>${esc(x)}</li>`).join('')}</ol>
      <p class="sub howscore"><b>Scoring:</b> ${esc(how.score)}</p>${ring()}</div>`;
  }

  // -- Drama Club ------------------------------------------------------------------------
  const PART = { beginning: 'The beginning', middle: '', ending: 'The ending' };
  function drama(v) {
    const D = window.PartyDraw;
    const S = window.PartyScenes;
    const T = 'Drama Club';
    const step = `Step ${v.step} of ${v.steps.length} · ${v.steps[v.step - 1]}`;
    const head = (tag) =>
      `<div class="scene" data-game="drama">${bar(T, 'D')}<span class="tag">${tag}</span>`;
    const theme = v.theme
      ? `<span class="dtheme">${esc(v.theme)}</span>${v.problem ? `<p class="dproblem">The problem: <b>${esc(v.problem)}</b></p>` : ''}`
      : '';
    const work = (title, sub, extra = '') =>
      `${head(step)}<h2 class="big" style="font-size:64px">${title}</h2>${theme}<p class="sub">${sub}</p>${extra}${doneRow(v.waiting)}${ring()}</div>`;
    if (v.phase === 'pitch')
      return work(
        "What's tonight's story about?",
        'Write a theme on your phone. Then everyone votes.',
      );
    if (v.phase === 'pitch_vote')
      return `${head(step)}<div class="dthemes">${v.themes.map((t) => `<div class="opt">${esc(t.text)}${t.problem ? `<small>${esc(t.problem)}</small>` : ''}</div>`).join('')}</div><p class="sub">Vote on your phone (not for your own).</p>${ring()}</div>`;
    if (v.phase === 'create')
      return work(
        'Everyone invents a character',
        "Name, looks and personality. Don't draw them: someone else will!",
      );
    if (v.phase === 'draw')
      return work(
        "Draw someone else's character",
        'Neutral, flustered, sad and angry. Nobody sees the drawings until the show!',
      );
    if (v.phase === 'headline') {
      const r = v.relay;
      const lines = v.outline
        .map(
          (x, i) =>
            `<li class="${i === v.outline.length - 1 ? 'new' : ''}">${av(x.by, 'sm')}<span><small>Chapter ${x.number}</small>${esc(x.headline)}</span></li>`,
        )
        .join('');
      return `${head(step)}<h2 class="big" style="font-size:52px">The story so far</h2>${theme}
        <ol class="doutline">${lines}<li class="next">${av(r.writer, 'sm')}<span><small>Chapter ${r.number} of ${r.of}</small><b>${esc(nameOf(r.writer))}</b> is writing what happens next…</span></li></ol>${ring()}</div>`;
    }
    if (v.phase === 'write')
      return work(
        'One story, one chapter each',
        'Everyone writes their chapter at the same time, following the outline from start to end.',
        `<div class="dstarring">Starring ${(v.cast || []).map((c) => `<b>${esc(c.name)}</b>`).join(' · ')}</div>`,
      );
    if (v.phase === 'show') {
      const ch = v.chapter;
      const hl = ch.headline ? `<p class="sub dhl">${esc(ch.headline)}</p>` : '';
      const card =
        ch.index === 0
          ? `<span class="tag">Tonight's novel</span><h2 class="big">${esc(v.theme)}</h2><p class="sub dchap">Chapter 1 · The beginning</p>${hl}`
          : `<span class="tag">${esc(v.theme)}</span><h2 class="big">Chapter ${ch.index + 1}</h2>${PART[ch.part] ? `<p class="sub dchap">${PART[ch.part]}</p>` : ''}${hl}`;
      return `<div class="scene vn" data-game="drama"><div class="vnstage">${S.svg(ch.bg, 'vnbg')}
        ${ch.cast.map((_, k) => `<canvas class="sprite ${spot(k, ch.cast.length)}" width="400" height="400" id="sp${k}"${crowd(k, ch.cast.length)}></canvas>`).join('')}
        <div class="vnbox" id="vnbox" hidden><div class="plate" id="vnplate"></div><p id="vntext"></p></div>
        <div class="vnchap" aria-hidden="true">Ch. ${ch.index + 1} of ${ch.of}</div>
        <div class="vnnext" aria-hidden="true">Click or press Space ▸</div>
        <div class="vncard" id="vncard">${card}</div>
      </div></div>`;
    }
    if (v.phase === 'credits') {
      const cr = v.credits;
      return `<div class="scene vn" data-game="drama"><div class="vncredroll"><div class="roll">
        <span class="tag">The end</span><h2 class="big">${esc(v.theme)}</h2>
        <h3>Starring</h3>
        <div class="dcastwall">${cr.characters.map((c) => `<div>${D.artCanvas(c.face, '')}<b>${esc(c.name)}</b><small>invented by ${esc(nameOf(c.creator))}<br>drawn by ${esc(nameOf(c.artist))}</small></div>`).join('')}</div>
        <h3>Written by</h3>
        <p class="sub">${cr.writers.map((w, i) => `Chapter ${i + 1}: <b>${esc(nameOf(w))}</b>`).join(' · ')}</p>
      </div></div></div>`;
    }
    if (v.phase === 'vote')
      return `${head(step)}<h2 class="big" style="font-size:52px">Vote: best chapter and best drawing</h2>
        <div class="dcastwall">${(v.gallery || []).map((c) => `<div>${D.artCanvas(c.face, '')}<b>${esc(c.name)}</b><small>drawn by ${esc(nameOf(c.artist))}</small></div>`).join('')}</div>${doneRow(v.waiting)}${ring()}</div>`;
    return scores(v, T);
  }

  // Where each character stands: one in the middle, two left and right, a crowd spread across.
  const spot = (k, n) => (n === 1 ? 'c' : n === 2 ? (k ? 'r' : 'l') : 'n');
  function crowd(k, n) {
    if (n <= 2) return '';
    const w = Math.max(210, Math.min(380, 1180 / n));
    const gap = (1280 - n * w) / (n + 1);
    return ` style="left:${Math.round(gap + k * (w + gap))}px;width:${Math.round(w)}px;height:${Math.round(w)}px"`;
  }
  // The visual-novel player for one chapter. The host clicks through it: line -1 is the
  // chapter's title card, then each click shows the next line (typed out, the speaker's sprite in
  // the line's mood). Text between *asterisks* is an action, shown in its own style.
  let vnTimers = [];
  let vn = null; // {ch, line, cur: moods}
  function stopVN() {
    for (const t of vnTimers) clearTimeout(t);
    vnTimers = [];
  }
  /** A line split into plain text and *actions*, without the asterisks. */
  function parts(text) {
    const out = [];
    const re = /\*([^*]+)\*/g;
    let last = 0;
    for (const m of text.matchAll(re)) {
      if (m.index > last) out.push({ t: text.slice(last, m.index), act: false });
      out.push({ t: m[1], act: true });
      last = m.index + m[0].length;
    }
    if (last < text.length) out.push({ t: text.slice(last), act: false });
    return out;
  }
  /** The first n letters of a line, as HTML with the actions styled. */
  function richUpTo(segs, n) {
    let left = n;
    let html = '';
    for (const sg of segs) {
      if (left <= 0) break;
      const t = sg.t.slice(0, left);
      left -= t.length;
      html += sg.act ? `<em class="act">${esc(t)}</em>` : esc(t);
    }
    return html;
  }
  function playVN(ch) {
    vn = { ch, line: -2, cur: ch.cast.map(() => 'neutral') };
    vnLine(ch.line);
  }
  function vnLine(line) {
    if (!vn || line === vn.line) return;
    const D = window.PartyDraw;
    const { ch } = vn;
    stopVN();
    const at = (ms, f) => vnTimers.push(setTimeout(f, ms));
    const faces = ch.cast.map((c) => c.faces || {});
    // everyone's mood as of this line (the page may have been reloaded mid-chapter)
    const cur = ch.cast.map(() => 'neutral');
    for (const ln of ch.lines.slice(0, Math.max(0, line + 1)))
      if (ln.who !== 2 && cur[ln.who] !== undefined) cur[ln.who] = ln.emotion;
    const moved =
      vn.line >= -1 &&
      line >= 0 &&
      ch.lines[line] &&
      ch.lines[line].who !== 2 &&
      vn.cur[ch.lines[line].who] !== cur[ch.lines[line].who];
    vn.line = line;
    vn.cur = cur;
    ch.cast.forEach((_, k) => {
      const c = $(`sp${k}`);
      if (c) D.render(c, faces[k][cur[k]] || faces[k].neutral || []);
    });
    const card = $('vncard');
    const box = $('vnbox');
    if (line < 0) {
      card?.classList.remove('gone');
      if (box) box.hidden = true;
      return;
    }
    card?.classList.add('gone');
    ch.cast.forEach((_, k) => $(`sp${k}`)?.classList.add('in'));
    const ln = ch.lines[line];
    if (!ln || !box) return;
    box.hidden = false;
    const narr = ln.who === 2;
    const segs = parts(ln.text);
    const len = segs.reduce((n, sg) => n + sg.t.length, 0);
    box.classList.toggle('narrator', narr);
    box.classList.toggle('long', len > 120);
    box.classList.toggle('longer', len > 240);
    $('vnplate').textContent = narr ? '' : ch.cast[ln.who].name;
    ch.cast.forEach((_, k) => {
      const c = $(`sp${k}`);
      if (!c) return;
      c.classList.toggle('dim', !narr && k !== ln.who);
      if (!narr && k === ln.who) {
        c.classList.remove('pop');
        void c.offsetWidth;
        c.classList.add('pop');
      }
    });
    if (moved) S().whoosh();
    const text = $('vntext');
    if (matchMedia('(prefers-reduced-motion: reduce)').matches) {
      text.innerHTML = richUpTo(segs, len);
      return;
    }
    let n = 0;
    const voice = narr ? 1 : ln.who * 2;
    const flat = segs.map((sg) => sg.t).join('');
    const type = () => {
      if (!text.isConnected) return;
      n++;
      text.innerHTML = richUpTo(segs, n);
      if (n % 2 === 1 && flat[n - 1] !== ' ') S().blip(voice);
      if (n < len) at(22, type);
    };
    type();
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
      h.shirt && h.shirt.scene
        ? `<div class="fhit"><div class="dmini">${window.PartyScenes.svg(h.shirt.scene.bg)}${h.shirt.scene.faces.map((f) => D.artCanvas(f, '')).join('')}</div><div><q>${esc(h.shirt.scene.line || h.text)}</q><small>${esc(h.name)} · ${esc(h.title)}, ${esc(h.date)}</small></div></div>`
        : h.shirt
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
      <div class="credit" style="left:auto;right:24px">Join with room code ${esc(st.room.code)}${link() ? ` at ${esc(shortLink())}` : ''}</div></div>`;
  }

  function results(v) {
    const s = v.standings || [];
    const winner = v.winner ? nameOf(v.winner) : '';
    const order = [s[1], s[0], s[2]];
    const hit = (v.hits || [])[0];
    return `<div class="scene" data-game="none">${bar('Party Night', '★')}
      <h2 class="big" style="font-size:60px;text-align:center">${winner ? `${esc(winner)} wins ${esc(v.title)}!` : `${esc(v.title)} is over`}</h2>
      ${hit ? `<div class="hitline">${hit.kind === 'shirt' ? 'Winning shirt' : hit.kind === 'scene' ? 'Winning chapter' : 'Best of the game'}: <q>${esc(hit.text)}</q> · ${esc(hit.name)}</div>` : ''}
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
    if (r.state === 'playing' && r.intro) {
      html = introScene(r.intro);
      scene = `intro:${r.intro.game}`;
    } else if (r.state === 'playing' && v && v.game) {
      html =
        v.game === 'drama'
          ? drama(v)
          : v.game === 'shirt'
            ? shirtGame(v)
            : v.game === 'quip'
              ? quip(v)
              : bluff(v);
      scene = [
        v.game,
        v.phase,
        v.round,
        v.number,
        v.matchup && v.matchup.number,
        v.battle && v.battle.shirts.map((x) => x.id).join(','),
        v.chapter && v.chapter.index,
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
    const endsIn = r.intro
      ? r.intro.ends_in
      : v && typeof v.ends_in === 'number'
        ? v.ends_in
        : null;
    if (endsIn !== null) {
      if (scene !== lastScene || total === null) total = Math.max(endsIn, 1);
      deadline = performance.now() / 1000 + endsIn;
    } else {
      deadline = null;
      total = null;
    }
    cue(scene, r, v);
    // The story playing (and the credits rolling) runs on its own timers: redrawing it for an
    // unrelated room change, like someone reconnecting, would bring back the title card and wipe
    // the cast. Only a new chapter redraws it.
    const playing = v && v.game === 'drama' && (v.phase === 'show' || v.phase === 'credits');
    if (playing && scene === lastScene) lastContent = content;
    if (content !== lastContent || scene !== lastScene) {
      const entering = scene !== lastScene;
      $('tvc').innerHTML = html;
      if (window.PartyDraw) window.PartyDraw.paint($('tvc'));
      const sc = $('tvc').firstElementChild;
      $('tvc').dataset.game = sc.dataset.game || 'none';
      if (entering) sc.classList.add('enter');
      if (entering && scene === 'results') confetti();
      if (entering) stopVN();
      if (entering && v && v.game === 'drama' && v.phase === 'show') playVN(v.chapter);
      lastContent = content;
      lastScene = scene;
    }
    if (v && v.game === 'drama' && v.phase === 'show' && scene === lastScene)
      vnLine(v.chapter.line);
    paintTimer();
    controls();
  }

  let lastTick = null;
  function paintTimer() {
    const ring = $('ring');
    if (!ring || deadline === null) return;
    const left = Math.max(0, deadline - performance.now() / 1000);
    ring.style.strokeDashoffset = String(276.5 * (1 - left / total));
    const secs = Math.ceil(left);
    $('left').textContent = String(secs);
    if (secs >= 1 && secs <= 5 && secs !== lastTick && total > 8) S().tick(secs === 1);
    lastTick = secs;
  }

  // -- sound: cues for what's on screen (sound.js makes the noises) ---------------------
  const S = () => window.PartySound || new Proxy({}, { get: () => () => {} });
  let cueScene = '';
  let cueWaiting = null;
  function cue(scene, r, v) {
    const sound = S();
    sound.track(
      r.state === 'playing' ? (r.intro ? r.intro.game : v && v.game) || 'lobby' : 'lobby',
    );
    const waiting = v && Array.isArray(v.waiting) ? v.waiting.length : null;
    if (scene !== cueScene) {
      cueScene = scene;
      lastTick = null;
      const phase = (v && v.phase) || '';
      if (scene === 'results') sound.fanfare();
      else if (scene === 'lobby' || scene === 'fame') {
        /* the music is enough */
      } else if (/reveal|show/.test(phase)) sound.reveal();
      else sound.phase();
      if (scene === 'results' && v && v.badges && v.badges.length)
        v.badges.forEach((_, i) => setTimeout(() => S().chime(), 1200 + i * 800));
    } else if (waiting !== null && cueWaiting !== null && waiting < cueWaiting) sound.ding();
    cueWaiting = waiting;
  }
  // Browsers only play sound after a click or key press.
  const chip = document.createElement('button');
  chip.type = 'button';
  chip.className = 'soundchip';
  chip.textContent = 'Click anywhere for sound';
  document.body.append(chip);
  const wake = () => {
    S().unlock();
    setTimeout(() => {
      if (S().ready()) chip.remove();
    }, 300);
  };
  addEventListener('pointerdown', wake);
  addEventListener('keydown', (e) => {
    wake();
    if (e.key === 'm' || e.key === 'M') {
      const off = S().prefs.sound || S().prefs.music;
      S().set('sound', !off);
      S().set('music', !off);
      controls();
    }
    if ([' ', 'Enter', 'ArrowRight'].includes(e.key) && vnOn()) {
      e.preventDefault();
      nextLine();
    }
  });
  // Drama Club's show: the host clicks (or presses Space) through it, line by line.
  const vnOn = () => st && st.view && st.view.game === 'drama' && st.view.phase === 'show';
  function nextLine() {
    if (ws && ws.readyState === 1) ws.send(JSON.stringify({ type: 'next' }));
  }
  $('tvc').addEventListener('click', (e) => {
    if (vnOn() && !e.target.closest('button')) nextLine();
  });
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
    rows.push(
      `<span class="lbl">Timers</span>${Object.entries(TIMERS)
        .map(([k, label]) => btn(label + (r.timer === k ? ' ✓' : ''), { type: 'timer', timer: k }))
        .join('')}`,
    );
    if (r.guest) rows.push(btn('Close this room', { type: 'close' }, 'kick'));
    const P = S().prefs || {};
    rows.push(
      `<button type="button" data-snd="sound" aria-pressed="${!!P.sound}">Sound ${P.sound ? 'on' : 'off'}</button><button type="button" data-snd="music" aria-pressed="${!!P.music}">Music ${P.music ? 'on' : 'off'}</button>`,
    );
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
  // The buttons on the closed screen: never leave the big screen at a dead end.
  $('tvc').addEventListener('click', async (e) => {
    const b = e.target.closest('[data-go]');
    if (!b) return;
    if (b.dataset.go === 'join') {
      location.href = '/';
      return;
    }
    try {
      const r = await fetch('/host/new', { method: 'POST' });
      const out = await r.json();
      if (r.ok && out.url) location.href = out.url;
      else b.textContent = out.error || 'Hosting is not available';
    } catch {
      b.textContent = 'Hosting is not available';
    }
  });
  $('ctl').addEventListener('click', (e) => {
    const t = e.target.closest('[data-snd]');
    if (t) {
      S().set(t.dataset.snd, !S().prefs[t.dataset.snd]);
      return controls();
    }
    const b = e.target.closest('[data-cmd]');
    if (!b || !ws || ws.readyState !== 1) return;
    ctlMsg = '';
    ws.send(b.dataset.cmd);
  });

  // -- connection ------------------------------------------------------------------------
  let ws = null;
  let backoff = 500;
  let closed = false; // a room opened from the share link was closed: don't reconnect
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
      } else if (msg.type === 'closed') {
        closed = true;
        $('ctl').innerHTML = '';
        $('tvc').innerHTML =
          '<div class="scene" data-game="none"><div class="join"><div class="left"><h2 class="big" style="font-size:76px">This room has closed</h2><p class="sub">Host a new one, or go back to the join page.</p><div class="gone"><button type="button" data-go="host">Host a new room</button><button type="button" data-go="join">Join a game</button></div></div></div></div>';
      } else if (msg.type === 'error') {
        ctlMsg = msg.message;
        controls();
        $('ctl').classList.add('show');
      }
    };
    ws.onclose = () => {
      if (closed) return;
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
