// Party Games: drawing on phones and showing drawings and shirts anywhere.
//
// A drawing is a list of strokes on a 400x400 grid:
//   {c: color, w: width, p: [x0, y0, x1, y1, ...]}   a pen line (c: ink index or "#rrggbb", w: 1-40)
//   {e: true, w: width, p: [...]}                     an eraser line
//   {fill: color}                                     paints the whole background (older drawings)
//   {ff: color, x, y}                                 a paint-bucket fill from that point
//   a line with s: true                               straight segments (the line and box tools)
// partydraw.py checks every drawing on the server with the same rules.
(() => {
  'use strict';
  const SIZE = 400;
  const INK = [
    '#1a1230',
    '#ffffff',
    '#ff5a5f',
    '#ffb100',
    '#2ec27e',
    '#3d7bff',
    '#a259ff',
    '#ff7fb0',
  ];
  const INK_NAMES = ['Ink', 'White', 'Red', 'Orange', 'Green', 'Blue', 'Purple', 'Pink'];
  const MIN_W = 1;
  const MAX_W = 40;
  const MAX_POINTS = 7900; // the server allows 8000
  // Shirts come in the same colors as the inks, with slogan text that reads on each.
  const SHIRT_TEXT = [
    '#ffffff',
    '#1a1230',
    '#ffffff',
    '#1a1230',
    '#1a1230',
    '#ffffff',
    '#ffffff',
    '#1a1230',
  ];
  const SHIRTS = INK.map((bg, i) => ({ bg, fg: SHIRT_TEXT[i], name: INK_NAMES[i] }));
  const esc = (s) =>
    String(s ?? '').replace(
      /[&<>"']/g,
      (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' })[c],
    );
  const colorOf = (c) => (typeof c === 'number' ? INK[c] || INK[0] : c);

  // -- drawing strokes ---------------------------------------------------------------------

  const rgb = (c) => {
    const h = colorOf(c);
    return [1, 3, 5].map((i) => Number.parseInt(h.slice(i, i + 2), 16));
  };
  /**
   * Paint-bucket fill: colors the area around (x, y) that looks like the pixel there, stopping
   * at lines. Anti-aliased line edges count as part of the area, so no pale halo is left.
   */
  function floodFill(ctx, s, k) {
    const { width: w, height: h } = ctx.canvas;
    const x0 = Math.min(w - 1, Math.max(0, Math.floor(s.x * k)));
    const y0 = Math.min(h - 1, Math.max(0, Math.floor(s.y * k)));
    const img = ctx.getImageData(0, 0, w, h);
    const d = img.data;
    const at = (y0 * w + x0) * 4;
    const t = [d[at], d[at + 1], d[at + 2], d[at + 3]];
    const [r, g, b] = rgb(s.ff);
    if (t[3] === 255 && Math.abs(t[0] - r) + Math.abs(t[1] - g) + Math.abs(t[2] - b) < 8) return;
    const same = (i) =>
      t[3] < 8
        ? d[i + 3] < 140 // an empty spot: fill everything mostly empty
        : Math.abs(d[i] - t[0]) +
            Math.abs(d[i + 1] - t[1]) +
            Math.abs(d[i + 2] - t[2]) +
            Math.abs(d[i + 3] - t[3]) <
          90;
    const seen = new Uint8Array(w * h);
    const stack = [x0, y0];
    while (stack.length) {
      const y = stack.pop();
      let x = stack.pop();
      while (x > 0 && !seen[y * w + x - 1] && same((y * w + x - 1) * 4)) x--;
      let up = false;
      let down = false;
      for (; x < w; x++) {
        const p = y * w + x;
        if (seen[p] || !same(p * 4)) break;
        seen[p] = 1;
        d[p * 4] = r;
        d[p * 4 + 1] = g;
        d[p * 4 + 2] = b;
        d[p * 4 + 3] = 255;
        if (y > 0) {
          const q = p - w;
          const ok = !seen[q] && same(q * 4);
          if (ok && !up) stack.push(x, y - 1);
          up = ok;
        }
        if (y < h - 1) {
          const q = p + w;
          const ok = !seen[q] && same(q * 4);
          if (ok && !down) stack.push(x, y + 1);
          down = ok;
        }
      }
    }
    ctx.putImageData(img, 0, 0);
  }

  /** One stroke, smoothed: a quadratic curve through the midpoints between samples. */
  function drawStroke(ctx, s, k) {
    if (s.ff !== undefined) {
      floodFill(ctx, s, k);
      return;
    }
    if (s.fill !== undefined) {
      ctx.globalCompositeOperation = 'source-over';
      ctx.fillStyle = colorOf(s.fill);
      ctx.fillRect(0, 0, ctx.canvas.width, ctx.canvas.height);
      return;
    }
    if (s.clear) {
      ctx.clearRect(0, 0, ctx.canvas.width, ctx.canvas.height);
      return;
    }
    const p = s.p;
    ctx.globalCompositeOperation = s.e ? 'destination-out' : 'source-over';
    ctx.strokeStyle = ctx.fillStyle = s.e ? '#000' : colorOf(s.c);
    ctx.lineWidth = Math.max(1, s.w * k);
    ctx.lineCap = 'round';
    ctx.lineJoin = 'round';
    if (p.length <= 4) {
      // a dot (or a tiny tap with two samples)
      ctx.beginPath();
      ctx.arc(p[0] * k, p[1] * k, ctx.lineWidth / 2, 0, Math.PI * 2);
      ctx.fill();
      if (p.length === 2) return;
    }
    ctx.beginPath();
    ctx.moveTo(p[0] * k, p[1] * k);
    if (s.s) {
      for (let i = 2; i < p.length; i += 2) ctx.lineTo(p[i] * k, p[i + 1] * k);
      ctx.stroke();
      return;
    }
    for (let i = 2; i < p.length - 2; i += 2) {
      const mx = ((p[i] + p[i + 2]) / 2) * k;
      const my = ((p[i + 1] + p[i + 3]) / 2) * k;
      ctx.quadraticCurveTo(p[i] * k, p[i + 1] * k, mx, my);
    }
    ctx.lineTo(p[p.length - 2] * k, p[p.length - 1] * k);
    ctx.stroke();
  }

  /** Draws a whole drawing onto a canvas of any size (the 400 grid is scaled to fit). */
  function render(canvas, strokes) {
    const ctx = canvas.getContext('2d');
    const k = canvas.width / SIZE;
    ctx.clearRect(0, 0, canvas.width, canvas.height);
    for (const s of strokes || []) drawStroke(ctx, s, k);
    ctx.globalCompositeOperation = 'source-over';
  }

  // Canvases in HTML built as strings: register the strokes, then paint() once inserted.
  const art = new Map();
  let nextArt = 0;
  function artCanvas(strokes, cls = 'art') {
    const id = `a${nextArt++}`;
    art.set(id, strokes || []);
    return `<canvas class="${cls}" width="400" height="400" data-art="${id}"></canvas>`;
  }
  function paint(root) {
    for (const c of root.querySelectorAll('canvas[data-art]')) render(c, art.get(c.dataset.art));
    art.clear();
  }

  const SHIRT_PATH =
    'M60 18 L95 4 Q120 22 145 4 L180 18 L232 62 L204 98 L182 84 L182 236 L58 236 L58 84 L36 98 L8 62 Z';
  /** A shirt in its color, with its drawing printed on the chest and the slogan under it. */
  function shirt(view, size = '') {
    const col = SHIRTS[view.color] || SHIRTS[0];
    return `<div class="shirt ${size}" style="--shirt:${col.bg};--shirt-fg:${col.fg}">
      <svg viewBox="0 0 240 240" aria-hidden="true"><path d="${SHIRT_PATH}"/></svg>
      <div class="print">${view.strokes && view.strokes.length ? artCanvas(view.strokes) : ''}</div>
      <div class="slogan">${esc(view.slogan || '')}</div></div>`;
  }

  // -- the drawing studio --------------------------------------------------------------------

  /** A stroke as plain data, in the shape the server accepts. */
  function plain(s) {
    if (s.fill !== undefined) return { fill: s.fill };
    if (s.ff !== undefined) return { ff: s.ff, x: s.x, y: s.y };
    const out = s.e ? { e: true, w: s.w, p: s.p.slice() } : { c: s.c, w: s.w, p: s.p.slice() };
    if (s.s) out.s = true;
    return out;
  }

  const ICONS = {
    pen: '<path d="M4 20l4-1 11-11-3-3L5 16l-1 4z"/><path d="M14 6l3 3"/>',
    eraser: '<path d="M8 20h12"/><path d="M5 15l9-9 5 5-9 9H8l-3-3z"/><path d="M9 11l5 5"/>',
    fill: '<path d="M5 12l7-7 7 7-7 7-7-7z"/><path d="M5 12h14"/><path d="M20 16s2 2.5 2 4a2 2 0 0 1-4 0c0-1.5 2-4 2-4z"/>',
    line: '<path d="M5 19L19 5"/>',
    box: '<rect x="4.5" y="6.5" width="15" height="11" rx="1"/>',
    circle: '<circle cx="12" cy="12" r="7.5"/>',
    undo: '<path d="M9 7L4 12l5 5"/><path d="M4 12h10a6 6 0 0 1 0 12h-2"/>',
    redo: '<path d="M15 7l5 5-5 5"/><path d="M20 12H10a6 6 0 0 0 0 12h2"/>',
    clear: '<path d="M4 7h16"/><path d="M9 7V4h6v3"/><path d="M6 7l1 13h10l1-13"/>',
  };
  const icon = (name) =>
    `<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">${ICONS[name]}</svg>`;
  const RECENT_KEY = 'party-colors';
  const loadRecent = () => {
    try {
      const v = JSON.parse(localStorage.getItem(RECENT_KEY) || '[]');
      return Array.isArray(v) ? v.filter((c) => /^#[0-9a-f]{6}$/i.test(c)).slice(0, 4) : [];
    } catch {
      return [];
    }
  };
  const saveRecent = (list) => {
    try {
      localStorage.setItem(RECENT_KEY, JSON.stringify(list));
    } catch {
      /* private mode */
    }
  };

  /**
   * A drawing studio in `root`: the pad, tools (pen, eraser, fill, undo, redo, clear), quick
   * colors plus a full color picker and recent colors, a pen size slider with a live preview,
   * and an ink meter. Returns {strokes(), empty(), load(strokes), destroy()}.
   * opts.sprite: no background fill (a character that stands on a scene).
   * opts.guide: strokes shown faded under the drawing (e.g. the neutral face) to trace over.
   * opts.start: strokes to begin with (a drawing already sent, to redo it).
   */
  function Studio(root, opts = {}) {
    let tool = 'pen';
    let color = 0; // an ink index or "#rrggbb"
    let size = 8;
    let recent = loadRecent();
    const history = []; // strokes, fills and clear markers, in order
    const undone = [];
    let current = null;
    let queued = false;

    root.innerHTML = `<div class="studio">
      <div class="padwrap"><canvas class="pad" aria-label="Drawing pad"></canvas><div class="meter" title="Ink left"><i></i></div></div>
      <div class="tbar" role="toolbar" aria-label="Drawing tools">
        <button type="button" data-tool="pen" aria-pressed="true" aria-label="Pen">${icon('pen')}<span>Pen</span></button>
        <button type="button" data-tool="eraser" aria-pressed="false" aria-label="Eraser">${icon('eraser')}<span>Eraser</span></button>
        <button type="button" data-tool="fill" aria-pressed="false" aria-label="Fill: tap an area to color it">${icon('fill')}<span>Fill</span></button>
        <button type="button" data-tool="line" aria-pressed="false" aria-label="Straight line">${icon('line')}<span>Line</span></button>
        <button type="button" data-tool="box" aria-pressed="false" aria-label="Box">${icon('box')}<span>Box</span></button>
        <button type="button" data-tool="circle" aria-pressed="false" aria-label="Circle">${icon('circle')}<span>Circle</span></button>
        <button type="button" data-do="undo" aria-label="Undo">${icon('undo')}<span>Undo</span></button>
        <button type="button" data-do="redo" aria-label="Redo">${icon('redo')}<span>Redo</span></button>
        <button type="button" data-do="clear" aria-label="Clear the drawing">${icon('clear')}<span>Clear</span></button>
      </div>
      <div class="colors" role="group" aria-label="Colors"></div>
      <div class="sizer"><span class="sizebox"><span class="sizedot" aria-hidden="true"></span></span>
        <input type="range" min="${MIN_W}" max="${MAX_W}" value="${size}" aria-label="Pen size">
        <span class="sizenum"></span></div>
    </div>`;
    const canvas = root.querySelector('canvas');
    const ctx = canvas.getContext('2d');
    const cache = document.createElement('canvas'); // everything already drawn
    const cctx = cache.getContext('2d');
    const range = root.querySelector('input[type=range]');
    const guide = opts.guide && opts.guide.length ? document.createElement('canvas') : null;

    const live = () => {
      let from = 0;
      history.forEach((s, i) => {
        if (s.clear) from = i + 1;
      });
      return history.slice(from);
    };
    const points = () =>
      live().reduce((n, s) => n + (s.p ? s.p.length / 2 : 0), 0) +
      (current ? current.p.length / 2 : 0);
    const k = () => canvas.width / SIZE;

    function rebuild() {
      cctx.globalCompositeOperation = 'source-over';
      cctx.clearRect(0, 0, cache.width, cache.height);
      for (const s of history) drawStroke(cctx, s, k());
      cctx.globalCompositeOperation = 'source-over';
      frame();
    }
    function frame() {
      queued = false;
      ctx.globalCompositeOperation = 'source-over';
      ctx.clearRect(0, 0, canvas.width, canvas.height);
      if (guide) {
        ctx.globalAlpha = 0.22;
        ctx.drawImage(guide, 0, 0);
        ctx.globalAlpha = 1;
      }
      ctx.drawImage(cache, 0, 0);
      if (current) drawStroke(ctx, current, k());
      ctx.globalCompositeOperation = 'source-over';
      root.querySelector('.meter i').style.width =
        `${Math.min(100, (points() / MAX_POINTS) * 100)}%`;
    }
    const later = () => {
      if (!queued) {
        queued = true;
        requestAnimationFrame(frame);
      }
    };
    function resize() {
      const px = Math.round(canvas.clientWidth * Math.min(2, window.devicePixelRatio || 1));
      if (!px || canvas.width === px) return;
      canvas.width = canvas.height = cache.width = cache.height = px;
      if (guide) {
        guide.width = guide.height = px;
        render(guide, opts.guide);
      }
      rebuild();
      syncTools();
    }

    function syncTools() {
      for (const b of root.querySelectorAll('[data-tool]'))
        b.setAttribute('aria-pressed', String(b.dataset.tool === tool));
      for (const b of root.querySelectorAll('[data-color]'))
        b.setAttribute(
          'aria-pressed',
          String(tool !== 'eraser' && String(color) === b.dataset.color),
        );
      root.querySelector('[data-do="undo"]').disabled = !history.length;
      root.querySelector('[data-do="redo"]').disabled = !undone.length;
      root.querySelector('[data-do="clear"]').disabled = !live().length;
      const dot = root.querySelector('.sizedot');
      const px = Math.max(3, Math.round((size * canvas.clientWidth) / SIZE));
      dot.style.width = dot.style.height = `${px}px`;
      dot.style.background = tool === 'eraser' ? 'transparent' : colorOf(color);
      dot.classList.toggle('erase', tool === 'eraser');
      root.querySelector('.sizenum').textContent = String(size);
    }
    function colorsRow() {
      const sw = (c, label) =>
        `<button type="button" data-color="${esc(String(c))}" style="--sw:${esc(colorOf(c))}" aria-label="${esc(label)}"></button>`;
      root.querySelector('.colors').innerHTML =
        INK.map((_, i) => sw(i, INK_NAMES[i])).join('') +
        recent.map((c) => sw(c, `Recent color ${c}`)).join('') +
        `<label class="custom" aria-label="Pick any color"><input type="color" value="${esc(typeof color === 'string' ? color : '#ff5a5f')}"></label>`;
      syncTools();
    }

    const at = (e) => {
      const r = canvas.getBoundingClientRect();
      const x = Math.round(((e.clientX - r.left) / r.width) * SIZE);
      const y = Math.round(((e.clientY - r.top) / r.height) * SIZE);
      return [Math.max(0, Math.min(SIZE, x)), Math.max(0, Math.min(SIZE, y))];
    };
    // Shapes: drag from one corner to the other; the stroke is rebuilt on every move.
    let origin = null;
    function shape(x, y) {
      const [x0, y0] = origin;
      if (tool === 'line') return [x0, y0, x, y];
      if (tool === 'box') return [x0, y0, x, y0, x, y, x0, y, x0, y0];
      const cx = (x0 + x) / 2;
      const cy = (y0 + y) / 2;
      const rx = Math.abs(x - x0) / 2;
      const ry = Math.abs(y - y0) / 2;
      const out = [];
      for (let i = 0; i <= 48; i++) {
        const a = (i / 48) * Math.PI * 2;
        out.push(Math.round(cx + rx * Math.cos(a)), Math.round(cy + ry * Math.sin(a)));
      }
      return out.map((v) => Math.max(0, Math.min(SIZE, v)));
    }
    canvas.addEventListener('pointerdown', (e) => {
      if (current || points() >= MAX_POINTS) return;
      e.preventDefault();
      if (tool === 'fill') {
        const [x, y] = at(e);
        history.push({ ff: color, x, y });
        undone.length = 0;
        drawStroke(cctx, history[history.length - 1], k());
        frame();
        syncTools();
        return;
      }
      canvas.setPointerCapture(e.pointerId);
      if (tool === 'line' || tool === 'box' || tool === 'circle') {
        origin = at(e);
        current = { c: color, w: size, p: [...origin, ...origin] };
        if (tool !== 'circle') current.s = true;
      } else
        current =
          tool === 'eraser' ? { e: true, w: size, p: at(e) } : { c: color, w: size, p: at(e) };
      later();
    });
    canvas.addEventListener('pointermove', (e) => {
      if (!current) return;
      if (origin) {
        current.p = shape(...at(e));
        return later();
      }
      const samples = e.getCoalescedEvents ? e.getCoalescedEvents() : [];
      for (const s of samples.length ? samples : [e]) {
        const [x, y] = at(s);
        const n = current.p.length;
        if (Math.hypot(x - current.p[n - 2], y - current.p[n - 1]) < 1.5) continue;
        if (points() >= MAX_POINTS) break;
        current.p.push(x, y);
      }
      later();
    });
    const end = () => {
      if (!current) return;
      origin = null;
      history.push(current);
      undone.length = 0;
      drawStroke(cctx, current, k());
      cctx.globalCompositeOperation = 'source-over';
      current = null;
      frame();
      syncTools();
    };
    canvas.addEventListener('pointerup', end);
    canvas.addEventListener('pointercancel', end);

    function act(what) {
      if (what === 'undo' && history.length) undone.push(history.pop());
      else if (what === 'redo' && undone.length) history.push(undone.pop());
      else if (what === 'clear' && live().length) {
        history.push({ clear: true });
        undone.length = 0;
      } else return;
      rebuild();
      syncTools();
    }
    root.addEventListener('click', (e) => {
      const b = e.target.closest('button');
      if (!b || b.disabled) return;
      if (b.dataset.tool) tool = b.dataset.tool;
      else if (b.dataset.do) return act(b.dataset.do);
      else if (b.dataset.color) {
        const c = b.dataset.color;
        color = /^\d+$/.test(c) ? Number(c) : c;
        if (tool === 'eraser') tool = 'pen';
      }
      syncTools();
    });
    root.addEventListener('input', (e) => {
      if (e.target === range) {
        size = Number(range.value);
        syncTools();
      } else if (e.target.type === 'color') {
        color = e.target.value.toLowerCase();
        if (tool === 'eraser') tool = 'pen';
        syncTools();
      }
    });
    root.addEventListener('change', (e) => {
      if (e.target.type !== 'color') return;
      const c = e.target.value.toLowerCase();
      if (!INK.includes(c)) {
        recent = [c, ...recent.filter((x) => x !== c)].slice(0, 4);
        saveRecent(recent);
        colorsRow();
      }
    });
    const keys = (e) => {
      if (!root.isConnected) return removeEventListener('keydown', keys);
      if (!(e.ctrlKey || e.metaKey) || e.key.toLowerCase() !== 'z') return;
      e.preventDefault();
      act(e.shiftKey ? 'redo' : 'undo');
    };
    addEventListener('keydown', keys);
    const ro = new ResizeObserver(resize);
    ro.observe(canvas);
    colorsRow();
    const copy = (list) => (list || []).map(plain);
    if (opts.start && opts.start.length) history.push(...copy(opts.start));
    resize();

    return {
      /** Replaces the drawing with these strokes (undoable as one clear). */
      load(list) {
        if (live().length) history.push({ clear: true });
        history.push(...copy(list));
        undone.length = 0;
        rebuild();
        syncTools();
      },
      /** What gets sent: everything since the last clear, as plain data. */
      strokes: () => live().map(plain),
      empty: () => live().every((s) => s.e),
      destroy() {
        ro.disconnect();
        removeEventListener('keydown', keys);
      },
    };
  }

  window.PartyDraw = {
    SIZE,
    INK,
    INK_NAMES,
    MIN_W,
    MAX_W,
    SHIRTS,
    render,
    artCanvas,
    paint,
    shirt,
    Studio,
  };
})();
