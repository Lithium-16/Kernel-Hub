import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { NodeClient } from '../src/lib/node.ts';

type Env = {
  v: number;
  id: string;
  re?: string;
  ts: string;
  type: string;
  body: Record<string, unknown>;
};

const MODULE = {
  id: 'hello',
  name: 'Hello',
  icon: 'hand',
  version: '0.1.0',
  state: 'running',
  actions: [{ id: 'greet.say', label: 'Say hello', ai: 'safe', params: {} }],
  status: { greetings: 0 },
};

const EVENT = {
  id: 'e1',
  ts: '2026-09-25T00:00:00Z',
  node_id: 'pluto',
  module: 'minecraft',
  kind: 'player.joined',
  level: 'info',
  message: 'Steve joined',
  data: { player: 'Steve' },
};

/** A fake kerneld on the other end of a fake WebSocket. */
class FakeSocket {
  static last: FakeSocket | null = null;
  static token = 'good-token';
  readyState = 0;
  sent: Env[] = [];
  onopen: (() => void) | null = null;
  onmessage: ((ev: { data: string }) => void) | null = null;
  onclose: (() => void) | null = null;
  onerror: (() => void) | null = null;

  constructor(readonly url: string) {
    FakeSocket.last = this;
    queueMicrotask(() => {
      this.readyState = 1;
      this.onopen?.();
    });
  }

  send(data: string) {
    const msg = JSON.parse(data) as Env;
    this.sent.push(msg);
    const reply = (type: string, body: Record<string, unknown>) =>
      queueMicrotask(() =>
        this.onmessage?.({
          data: JSON.stringify({
            v: 1,
            id: `r-${msg.id}`,
            re: msg.id,
            ts: '2026-09-25T00:00:00Z',
            type,
            body,
          }),
        }),
      );
    switch (msg.type) {
      case 'hello':
        if (msg.body.token === FakeSocket.token) {
          reply('welcome', {
            node: { id: 'pluto', name: 'Pluto', version: '0.1.0' },
            capabilities: ['actions'],
          });
        } else {
          reply('error', { code: 'unauthorized', message: 'invalid token' });
        }
        break;
      case 'catalog.get':
        reply('catalog', { modules: [MODULE] });
        break;
      case 'action.invoke':
        reply('action.result', {
          ok: true,
          result: { message: `hi ${String((msg.body.params as { name?: string }).name)}` },
        });
        break;
      case 'events.query':
        reply('events', { events: [EVENT] });
        break;
      default:
        reply('error', { code: 'bad_request', message: 'nope' });
    }
  }

  close() {
    if (this.readyState === 3) return;
    this.readyState = 3;
    queueMicrotask(() => this.onclose?.());
  }
}

const flush = () => new Promise((r) => setTimeout(r, 0));

function client(token = 'good-token') {
  return new NodeClient({
    url: 'ws://pluto:47800/ws',
    token,
    pollMs: 60_000,
    WebSocketImpl: FakeSocket as unknown as new (url: string) => WebSocket,
  });
}

describe('NodeClient', () => {
  beforeEach(() => {
    FakeSocket.last = null;
  });
  afterEach(() => {
    vi.useRealTimers();
  });

  it('says hello, then loads the catalog', async () => {
    const c = client();
    const seen: string[] = [];
    c.subscribe(() => seen.push(c.getSnapshot().conn));
    c.start();
    await flush();
    await flush();
    const s = c.getSnapshot();
    expect(s.conn).toBe('online');
    expect(s.node?.name).toBe('Pluto');
    expect(s.modules.map((m) => m.id)).toEqual(['hello']);
    expect(seen[0]).toBe('connecting');
    const hello = FakeSocket.last?.sent[0];
    expect(hello?.type).toBe('hello');
    expect(hello?.v).toBe(1);
    expect(hello?.ts).toMatch(/^\d{4}-\d\d-\d\dT\d\d:\d\d:\d\dZ$/);
    c.stop();
    expect(c.getSnapshot().conn).toBe('idle');
  });

  it('invokes actions as the user', async () => {
    const c = client();
    c.start();
    await flush();
    await flush();
    const r = await c.invoke('hello', 'greet.say', { name: 'Pluto' });
    expect(r).toEqual({ ok: true, result: { message: 'hi Pluto' } });
    const sent = FakeSocket.last?.sent.find((m) => m.type === 'action.invoke');
    expect(sent?.body.actor).toEqual({ kind: 'user', ref: 'desktop' });
    c.stop();
  });

  it('stops retrying when the token is refused', async () => {
    const c = client('wrong-token');
    c.start();
    await flush();
    await flush();
    await flush();
    expect(c.getSnapshot().conn).toBe('unauthorized');
    expect(c.getSnapshot().error).toBe('invalid token');
    c.stop();
  });

  it('reports an offline node as a failed action instead of throwing', async () => {
    const c = client();
    const r = await c.invoke('hello', 'greet.say');
    expect(r.ok).toBe(false);
    expect(r.error?.code).toBe('offline');
  });

  it('reconnects with backoff after the connection drops', async () => {
    vi.useFakeTimers();
    const c = client();
    c.start();
    await vi.advanceTimersByTimeAsync(0);
    const first = FakeSocket.last;
    expect(c.getSnapshot().conn).toBe('online');
    first?.close();
    await vi.advanceTimersByTimeAsync(0);
    expect(c.getSnapshot().conn).toBe('offline');
    await vi.advanceTimersByTimeAsync(1000);
    expect(FakeSocket.last).not.toBe(first);
    await vi.advanceTimersByTimeAsync(0);
    expect(c.getSnapshot().conn).toBe('online');
    c.stop();
  });

  it('ignores messages that do not match the protocol', async () => {
    const c = client();
    c.start();
    await flush();
    await flush();
    FakeSocket.last?.onmessage?.({ data: 'not json' });
    FakeSocket.last?.onmessage?.({ data: JSON.stringify({ v: 2, id: 'x', type: 'welcome' }) });
    expect(c.getSnapshot().conn).toBe('online');
    c.stop();
  });

  it('loads events and hears pushed ones', async () => {
    const c = client();
    c.start();
    await flush();
    await flush();
    expect(await c.events(10)).toEqual([EVENT]);
    const heard: string[] = [];
    const off = c.onEvent((e) => heard.push(e.message));
    const push = (id: string) =>
      FakeSocket.last?.onmessage?.({
        data: JSON.stringify({ v: 1, id, ts: '2026-09-25T00:00:01Z', type: 'event', body: EVENT }),
      });
    push('p1');
    off();
    push('p2');
    expect(heard).toEqual(['Steve joined']);
    c.stop();
  });
});
