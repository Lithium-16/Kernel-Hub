// Kernel's host for OpenFork. Copied into the game's server/ folder by the openfork module and
// run with Node instead of server/main.ts. It drives the same GameServer the standalone server
// does (through server/core's ports), and differs in four ways:
//
// - guest identities and match history are kept in a JSON file (DATA_DIR), so a server
//   restart (or an update) doesn't forget who is who;
// - it listens on HOST (127.0.0.1 by default): friends reach it through Tailscale, not the LAN;
// - it caps open connections (MAX_CONNECTIONS), since it may face the internet;
// - GET /kernel/status reports players online and recent matches to the Kernel module.
//
// It only uses what server/main.ts uses. If the game's API changes so this no longer starts,
// the module falls back to server/main.ts and says so.
import { randomBytes, randomUUID } from 'node:crypto';
import { mkdirSync, readdirSync, readFileSync, renameSync, writeFileSync } from 'node:fs';
import { readFile } from 'node:fs/promises';
import { createServer } from 'node:http';
import { extname, join, normalize, sep } from 'node:path';
import { fileURLToPath } from 'node:url';
import { WebSocket, WebSocketServer } from 'ws';
import type { GameMap } from '../shared/map.ts';
import type { ServerMessage } from '../shared/protocol.ts';
import { TICK_MS } from '../shared/rules.ts';
import { GameServer } from './core/game-server.ts';
import type { Auth, ConnId, Identity, MatchResult } from './core/ports.ts';

const PORT = Number(process.env.PORT ?? 8096);
const HOST = process.env.HOST ?? '127.0.0.1';
const DATA_DIR = process.env.DATA_DIR ?? fileURLToPath(new URL('../../data/', import.meta.url));
const MAX_CONNECTIONS = Number(process.env.MAX_CONNECTIONS ?? 64);
const PUBLIC_DIR = fileURLToPath(new URL('../public/', import.meta.url));
const TYPES: Record<string, string> = {
  '.html': 'text/html; charset=utf-8',
  '.js': 'text/javascript; charset=utf-8',
  '.css': 'text/css; charset=utf-8',
  '.json': 'application/json',
  '.map': 'application/json',
  '.png': 'image/png',
  '.svg': 'image/svg+xml',
};
const MAX_MSGS_PER_SEC = 60;
const KEEP_RECENT = 50;

// -- saved players and matches --------------------------------------------------------------

interface Saved {
  tokens: Record<string, string>;
  names: Record<string, string>;
  wins: Record<string, number>;
  recent: MatchResult[];
  matches: number;
}

class FileState {
  data: Saved = { tokens: {}, names: {}, wins: {}, recent: [], matches: 0 };
  private readonly file: string;
  private timer: NodeJS.Timeout | null = null;

  constructor(dir: string) {
    mkdirSync(dir, { recursive: true });
    this.file = join(dir, 'openfork.json');
    try {
      this.data = { ...this.data, ...JSON.parse(readFileSync(this.file, 'utf8')) };
    } catch {
      // first start, or an unreadable file: start fresh (the old one stays for a look)
    }
  }

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

/** Guests, remembered: a returning browser comes back as the same person, even after a restart. */
const auth: Auth = {
  async identify(hello: { token?: string; name: string }): Promise<Identity> {
    const known = hello.token ? state.data.tokens[hello.token] : undefined;
    if (known && hello.token) {
      state.data.names[known] = hello.name;
      state.changed();
      return { id: known, token: hello.token, name: hello.name };
    }
    const token = randomBytes(24).toString('base64url');
    const id = randomUUID();
    state.data.tokens[token] = id;
    state.data.names[id] = hello.name;
    state.changed();
    return { id, token, name: hello.name };
  },
};

const matches = {
  recordMatch(result: MatchResult): void {
    state.data.recent.push(result);
    if (state.data.recent.length > KEEP_RECENT) state.data.recent.shift();
    state.data.matches += 1;
    const human = result.players.find((p) => p.name === result.winner && p.human);
    if (human) state.data.wins[human.name] = (state.data.wins[human.name] ?? 0) + 1;
    state.changed();
  },
};

// -- the game -------------------------------------------------------------------------------

const maps = new Map<string, GameMap>();
for (const f of readdirSync(join(PUBLIC_DIR, 'maps')).filter((f) => f.endsWith('.json'))) {
  const map: GameMap = JSON.parse(readFileSync(join(PUBLIC_DIR, 'maps', f), 'utf8'));
  maps.set(map.id, map);
}

const sockets = new Map<ConnId, WebSocket>();
const game = new GameServer({
  transport: {
    send(conn: ConnId, msg: ServerMessage) {
      const ws = sockets.get(conn);
      if (ws?.readyState === WebSocket.OPEN) ws.send(JSON.stringify(msg));
    },
  },
  auth,
  clock: { now: () => Date.now() },
  maps,
  matches,
  log: (m) => console.log(m),
});
const started = Date.now();

function status() {
  const last = state.data.recent.at(-1);
  return {
    online: sockets.size,
    players: Object.keys(state.data.names).length,
    matches: state.data.matches,
    uptime_s: Math.round((Date.now() - started) / 1000),
    last_match: last
      ? {
          mode: last.map,
          players: last.players.filter((p) => p.human).map((p) => p.name),
          winner: last.winner,
          minutes: Math.round((last.endedAt - last.startedAt) / 60_000),
        }
      : null,
    top: Object.entries(state.data.wins)
      .sort(([, a], [, b]) => b - a)
      .slice(0, 5)
      .map(([name, wins]) => ({ name, wins })),
  };
}

const http = createServer(async (req, res) => {
  const url = new URL(req.url ?? '/', 'http://localhost');
  if (url.pathname === '/health') {
    res.writeHead(200, { 'content-type': 'text/plain' }).end('ok');
    return;
  }
  if (url.pathname === '/kernel/status') {
    // Only for the module on this PC; requests through Tailscale are marked by tailscaled.
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

setInterval(() => game.tick(), TICK_MS);

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

http.listen(PORT, HOST, () => console.log(`OpenFork (Kernel host) on http://${HOST}:${PORT}`));
