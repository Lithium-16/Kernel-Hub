import {
  type ActivityEntry,
  type ErrorInfo,
  Message,
  type ModuleInfo,
  type NodeEvent,
  PROTOCOL_VERSION,
} from '@kernel/protocol';
import type { z } from 'zod';

export type Module = z.infer<typeof ModuleInfo>;
export type Activity = z.infer<typeof ActivityEntry>;
export type KernelEvent = z.infer<typeof NodeEvent>;
export type NodeError = z.infer<typeof ErrorInfo>;
export type ActionResult = { ok: boolean; result?: unknown; error?: NodeError };
type Msg = z.infer<typeof Message>;
type Body<T extends Msg['type']> = Extract<Msg, { type: T }>['body'];

export type Conn = 'idle' | 'connecting' | 'online' | 'offline' | 'unauthorized';

export interface Snapshot {
  conn: Conn;
  node: { id: string; name: string; version: string } | null;
  modules: Module[];
  error: string | null;
}

export interface ClientOptions {
  url: string;
  token: string;
  client?: { name: string; version: string };
  pollMs?: number;
  WebSocketImpl?: new (url: string) => WebSocket;
}

interface Pending {
  resolve: (m: Msg) => void;
  reject: (e: Error) => void;
  timer: ReturnType<typeof setTimeout>;
}

const REQUEST_TIMEOUT_MS = 15_000;
// Actions carry their own timeouts on the node (a delayed stop can take 30+ minutes).
const ACTION_TIMEOUT_MS = 45 * 60_000;
const RETRY_MAX_MS = 15_000;

export function now(): string {
  return `${new Date().toISOString().slice(0, 19)}Z`;
}

function newId(): string {
  return crypto.randomUUID();
}

/** One connection to one node: hello, then catalog polling, actions and activity. Reconnects. */
export class NodeClient {
  private ws: WebSocket | null = null;
  private pending = new Map<string, Pending>();
  private listeners = new Set<() => void>();
  private eventListeners = new Set<(e: KernelEvent) => void>();
  private snap: Snapshot = { conn: 'idle', node: null, modules: [], error: null };
  private retryMs = 1000;
  private retryTimer: ReturnType<typeof setTimeout> | null = null;
  private pollTimer: ReturnType<typeof setInterval> | null = null;
  private stopped = true;

  constructor(private readonly opts: ClientOptions) {}

  subscribe = (fn: () => void): (() => void) => {
    this.listeners.add(fn);
    return () => this.listeners.delete(fn);
  };

  getSnapshot = (): Snapshot => this.snap;

  /** Events the node pushes as they happen (a crash, a player joining). */
  onEvent = (fn: (e: KernelEvent) => void): (() => void) => {
    this.eventListeners.add(fn);
    return () => this.eventListeners.delete(fn);
  };

  start(): void {
    if (!this.stopped) return;
    this.stopped = false;
    this.open();
  }

  stop(): void {
    this.stopped = true;
    this.clearTimers();
    this.ws?.close();
    this.ws = null;
    this.failPending('disconnected');
    this.set({ conn: 'idle' });
  }

  async refresh(): Promise<void> {
    const m = await this.request('catalog.get', {});
    if (m.type === 'catalog') this.set({ modules: m.body.modules });
  }

  async invoke(
    module: string,
    action: string,
    params: Record<string, unknown> = {},
  ): Promise<ActionResult> {
    let reply: Msg;
    try {
      reply = await this.request(
        'action.invoke',
        {
          module,
          action,
          params,
          actor: { kind: 'user', ref: this.opts.client?.name ?? 'desktop' },
        },
        ACTION_TIMEOUT_MS,
      );
    } catch (e) {
      return { ok: false, error: { code: 'offline', message: (e as Error).message } };
    }
    void this.refresh().catch(() => {});
    if (reply.type === 'action.result') return reply.body;
    if (reply.type === 'error') return { ok: false, error: reply.body };
    return { ok: false, error: { code: 'internal', message: `unexpected reply ${reply.type}` } };
  }

  async activity(limit = 100, module?: string): Promise<Activity[]> {
    const m = await this.request('activity.query', module ? { limit, module } : { limit });
    if (m.type === 'activity') return m.body.entries;
    throw new Error(m.type === 'error' ? m.body.message : `unexpected reply ${m.type}`);
  }

  async events(limit = 100, module?: string): Promise<KernelEvent[]> {
    const m = await this.request('events.query', module ? { limit, module } : { limit });
    if (m.type === 'events') return m.body.events;
    throw new Error(m.type === 'error' ? m.body.message : `unexpected reply ${m.type}`);
  }

  request<T extends Msg['type']>(
    type: T,
    body: Body<T>,
    timeoutMs = REQUEST_TIMEOUT_MS,
  ): Promise<Msg> {
    const ws = this.ws;
    if (!ws || ws.readyState !== 1) return Promise.reject(new Error('not connected'));
    const id = newId();
    const envelope = { v: PROTOCOL_VERSION, id, ts: now(), type, body };
    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => {
        this.pending.delete(id);
        reject(new Error(`${type} timed out`));
      }, timeoutMs);
      this.pending.set(id, { resolve, reject, timer });
      ws.send(JSON.stringify(envelope));
    });
  }

  private set(patch: Partial<Snapshot>): void {
    this.snap = { ...this.snap, ...patch };
    for (const fn of this.listeners) fn();
  }

  private open(): void {
    this.set({ conn: 'connecting', error: null });
    const Impl = this.opts.WebSocketImpl ?? WebSocket;
    let ws: WebSocket;
    try {
      ws = new Impl(this.opts.url);
    } catch (e) {
      this.set({ conn: 'offline', error: (e as Error).message });
      this.scheduleRetry();
      return;
    }
    this.ws = ws;
    ws.onopen = () => void this.handshake(ws);
    ws.onmessage = (ev: MessageEvent) => this.receive(ev.data);
    ws.onclose = () => {
      if (this.ws !== ws) return;
      this.ws = null;
      this.clearTimers();
      this.failPending('connection closed');
      if (this.stopped || this.snap.conn === 'unauthorized') return;
      this.set({ conn: 'offline', error: this.snap.error ?? "can't reach the node" });
      this.scheduleRetry();
    };
  }

  private async handshake(ws: WebSocket): Promise<void> {
    try {
      const reply = await this.request('hello', {
        client: this.opts.client ?? { name: 'desktop', version: '0.1.0' },
        token: this.opts.token,
      });
      if (reply.type === 'welcome') {
        this.retryMs = 1000;
        this.set({ conn: 'online', node: reply.body.node, error: null });
        await this.refresh();
        this.pollTimer = setInterval(
          () => void this.refresh().catch(() => {}),
          this.opts.pollMs ?? 2000,
        );
      } else {
        const message = reply.type === 'error' ? reply.body.message : 'unexpected reply to hello';
        this.set({ conn: 'unauthorized', error: message });
        ws.close();
      }
    } catch (e) {
      this.set({ error: (e as Error).message });
      ws.close();
    }
  }

  private receive(data: unknown): void {
    if (typeof data !== 'string') return;
    let raw: unknown;
    try {
      raw = JSON.parse(data);
    } catch {
      return;
    }
    const parsed = Message.safeParse(raw);
    if (!parsed.success) return;
    if (parsed.data.type === 'event') {
      for (const fn of this.eventListeners) fn(parsed.data.body);
      return;
    }
    if (!parsed.data.re) return;
    const p = this.pending.get(parsed.data.re);
    if (!p) return;
    this.pending.delete(parsed.data.re);
    clearTimeout(p.timer);
    p.resolve(parsed.data);
  }

  private scheduleRetry(): void {
    if (this.stopped) return;
    this.retryTimer = setTimeout(() => this.open(), this.retryMs);
    this.retryMs = Math.min(this.retryMs * 2, RETRY_MAX_MS);
  }

  private clearTimers(): void {
    if (this.retryTimer) clearTimeout(this.retryTimer);
    if (this.pollTimer) clearInterval(this.pollTimer);
    this.retryTimer = null;
    this.pollTimer = null;
  }

  private failPending(reason: string): void {
    for (const p of this.pending.values()) {
      clearTimeout(p.timer);
      p.reject(new Error(reason));
    }
    this.pending.clear();
  }
}
