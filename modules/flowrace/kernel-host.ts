// Kernel's host for Flow Race. Copied into the game's server/ folder by the flowrace module
// and run with Node instead of server/main.ts. It drives the same GameServer the standalone
// server does (through server/core's ports), and differs in four ways:
//
// - players and ratings are kept in a JSON file (DATA_DIR), so they survive restarts;
// - it listens on HOST (127.0.0.1 by default): friends reach it through Tailscale, not the LAN;
// - it caps open connections (MAX_CONNECTIONS), since it may face the internet;
// - GET /kernel/status reports players online and recent matches to the Kernel module.
//
// It only uses what server/main.ts uses. If the game's API changes so this no longer starts,
// the module falls back to server/main.ts (with in-memory ratings) and says so.
import { randomBytes, randomUUID } from 'node:crypto';
import { mkdirSync, readFileSync, renameSync, writeFileSync } from 'node:fs';
import { readFile } from 'node:fs/promises';
import { createServer } from 'node:http';
import { extname, join, normalize, sep } from 'node:path';
import { fileURLToPath } from 'node:url';
import { Worker } from 'node:worker_threads';
import { WebSocket, WebSocketServer } from 'ws';
import type { Board } from '../shared/board.ts';
import type { LeaderboardEntry, MatchResult, ServerMessage } from '../shared/protocol.ts';
import { GameServer } from './core/game-server.ts';
import type { Auth, ConnId, Identity, PlayerStore, Profile } from './core/ports.ts';
import { PuzzlePool } from './core/puzzle-pool.ts';
import { START_RATING } from './core/rating.ts';
import { mulberry32 } from './core/rng.ts';

const PORT = Number(process.env.PORT ?? 8095);
const HOST = process.env.HOST ?? '127.0.0.1';
const DATA_DIR = process.env.DATA_DIR ?? fileURLToPath(new URL('../../data/', import.meta.url));
const MAX_CONNECTIONS = Number(process.env.MAX_CONNECTIONS ?? 64);
const PUBLIC_DIR = fileURLToPath(new URL('../public/', import.meta.url));
const TYPES: Record<string, string> = {
  '.html': 'text/html; charset=utf-8',
  '.js': 'text/javascript; charset=utf-8',
  '.css': 'text/css; charset=utf-8',
  '.map': 'application/json',
  '.svg': 'image/svg+xml',
};
const MAX_MSGS_PER_SEC = 60;
const KEEP_RECENT = 50;

// -- saved players --------------------------------------------------------------------------

interface Saved {
  tokens: Record<string, string>;
  profiles: Record<string, Profile & { name: string }>;
  recent: MatchResult[];
  matches: number;
}

class FileState {
  data: Saved = { tokens: {}, profiles: {}, recent: [], matches: 0 };
  private readonly file: string;
  private timer: NodeJS.Timeout | null = null;

  constructor(dir: string) {
    mkdirSync(dir, { recursive: true });
    this.file = join(dir, 'flowrace.json');
    try {
      this.data = { ...this.data, ...JSON.parse(readFileSync(this.file, 'utf8')) };
    } catch {
      // first start, or an unreadable file: start fresh (the old one stays for a look)
    }
  }

  /** Saves shortly after a change, batching bursts (a match result touches several players). */
  changed(): void {
    this.timer ??= setTimeout(() => this.flush(), 500);
  }

  flush(): void {
    if (this.timer) clearTimeout(this.timer);
    this.timer = null;
    const tmp = `${this.file}.tmp`;
    writeFileSync(tmp, JSON.stringify(this.data));
    renameSync(tmp, this.file);
  }
}

const state = new FileState(DATA_DIR);

/** Guests, remembered: a returning browser keeps its name, rating and place on the board. */
const auth: Auth = {
  async identify(hello: { token?: string; name: string }): Promise<Identity> {
    const known = hello.token ? state.data.tokens[hello.token] : undefined;
    if (known && hello.token) return { playerId: known, token: hello.token, name: hello.name };
    const token = randomBytes(24).toString('base64url');
    const playerId = randomUUID();
    state.data.tokens[token] = playerId;
    state.changed();
    return { playerId, token, name: hello.name };
  },
};

const ranked = () =>
  Object.entries(state.data.profiles)
    .filter(([, p]) => p.games > 0)
    .sort(([, a], [, b]) => b.rating - a.rating || b.games - a.games);

const store: PlayerStore = {
  async getProfile(playerId) {
    const p = state.data.profiles[playerId];
    return p ? { rating: p.rating, games: p.games } : { rating: START_RATING, games: 0 };
  },
  async saveProfile(playerId, profile) {
    state.data.profiles[playerId] = { ...profile };
    state.changed();
  },
  async recordMatch(result) {
    state.data.recent.push(result);
    if (state.data.recent.length > KEEP_RECENT) state.data.recent.shift();
    state.data.matches += 1;
    state.changed();
  },
  async leaderboard(limit): Promise<LeaderboardEntry[]> {
    return ranked()
      .slice(0, limit)
      .map(([id, p], i) => ({ rank: i + 1, id, name: p.name, rating: p.rating, games: p.games }));
  },
  async rankOf(playerId) {
    const i = ranked().findIndex(([id]) => id === playerId);
    return i === -1 ? null : i + 1;
  },
};

// -- the game -------------------------------------------------------------------------------

const t0 = performance.now();
const pool = new PuzzlePool(mulberry32((Math.random() * 2 ** 32) >>> 0));
pool.refill(10_000);
console.log(`Puzzle pool ready in ${Math.round(performance.now() - t0)}ms`);

const sockets = new Map<ConnId, WebSocket>();
const game = new GameServer({
  transport: {
    send(conn: ConnId, msg: ServerMessage) {
      const ws = sockets.get(conn);
      if (ws?.readyState === WebSocket.OPEN) ws.send(JSON.stringify(msg));
    },
  },
  auth,
  store,
  clock: { now: () => Date.now() },
  puzzles: pool,
  log: (m) => console.log(m),
});
const started = Date.now();

function status() {
  const last = state.data.recent.at(-1);
  const winner = last?.standings.find((s) => s.id === last.winnerId);
  return {
    online: sockets.size,
    players: Object.keys(state.data.profiles).length,
    matches: state.data.matches,
    uptime_s: Math.round((Date.now() - started) / 1000),
    last_match: last
      ? {
          mode: last.options.mode,
          players: last.standings.map((s) => s.name),
          winner: winner?.name ?? null,
          ranked: last.ranked,
        }
      : null,
    top: ranked()
      .slice(0, 5)
      .map(([, p]) => ({ name: p.name, rating: p.rating, games: p.games })),
  };
}

const http = createServer(async (req, res) => {
  const url = new URL(req.url ?? '/', 'http://localhost');
  if (url.pathname === '/health') {
    res.writeHead(200, { 'content-type': 'text/plain' }).end('ok');
    return;
  }
  if (url.pathname === '/kernel/status') {
    // Only for the module on this PC; through Tailscale the request comes from tailscaled,
    // which marks it.
    const proxied = req.headers['tailscale-user-login'] || req.headers['tailscale-funnel-request'];
    if (proxied || req.socket.remoteAddress?.replace('::ffff:', '') !== '127.0.0.1') {
      res.writeHead(404).end();
      return;
    }
    res.writeHead(200, { 'content-type': 'application/json' }).end(JSON.stringify(status()));
    return;
  }
  try {
    const rel = url.pathname === '/' ? 'index.html' : decodeURIComponent(url.pathname.slice(1));
    const file = normalize(join(PUBLIC_DIR, rel));
    if (!file.startsWith(PUBLIC_DIR.endsWith(sep) ? PUBLIC_DIR : PUBLIC_DIR + sep)) {
      res.writeHead(403).end();
      return;
    }
    const body = await readFile(file);
    res.writeHead(200, {
      'content-type': TYPES[extname(file)] ?? 'application/octet-stream',
      'cache-control': 'no-cache',
    });
    res.end(body);
  } catch {
    res.writeHead(404, { 'content-type': 'text/plain' }).end('Not found');
  }
});

const wss = new WebSocketServer({ server: http, path: '/ws', maxPayload: 32 * 1024 });
const alive = new WeakSet<WebSocket>();

wss.on('connection', (ws) => {
  if (sockets.size >= MAX_CONNECTIONS) {
    ws.close(1013, 'server full');
    return;
  }
  const conn = randomUUID();
  sockets.set(conn, ws);
  alive.add(ws);
  game.handleConnect(conn);

  let windowStart = Date.now();
  let count = 0;
  ws.on('pong', () => alive.add(ws));
  ws.on('message', (data) => {
    const now = Date.now();
    if (now - windowStart >= 1000) {
      windowStart = now;
      count = 0;
    }
    if (++count > MAX_MSGS_PER_SEC) {
      ws.close(1008, 'rate limit');
      return;
    }
    let raw: unknown;
    try {
      raw = JSON.parse(data.toString());
    } catch {
      return;
    }
    game.handleMessage(conn, raw).catch((e) => console.error('handleMessage failed', e));
  });
  ws.on('close', () => {
    sockets.delete(conn);
    game.handleDisconnect(conn);
  });
});

setInterval(() => {
  for (const ws of wss.clients) {
    if (!alive.has(ws)) {
      ws.terminate();
      continue;
    }
    alive.delete(ws);
    ws.ping();
  }
}, 15_000);

setInterval(() => game.tick(), 100);

const generator = new Worker(new URL('./puzzle-worker.ts', import.meta.url));
let generating = false;
const feedPool = () => {
  if (generating) return;
  const size = pool.wanted();
  if (size === null) return;
  generating = true;
  generator.postMessage(size);
};
generator.on('message', (board: Board) => {
  pool.add(board);
  generating = false;
  feedPool();
});
generator.on('error', (e) => {
  console.error('puzzle worker failed', e);
  generating = false;
});
setInterval(feedPool, 250);

const stop = () => {
  state.flush();
  process.exit(0);
};
process.on('SIGINT', stop);
process.on('SIGTERM', stop);
// The module stops us by closing stdin (works the same on Windows, which has no SIGTERM).
if (process.env.STOP_ON_STDIN_CLOSE === '1') {
  process.stdin.on('end', stop);
  process.stdin.resume();
}

http.listen(PORT, HOST, () => console.log(`Flow Race (Kernel host) on http://${HOST}:${PORT}`));
