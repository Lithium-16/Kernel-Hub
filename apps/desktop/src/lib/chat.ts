/** The assistant conversation on this PC: kept across views and restarts (last 60 messages). */
import type { ChatReply } from './node.ts';

export type Provider = 'auto' | 'local' | 'claude';

export type ChatMessage =
  | { id: string; role: 'user'; text: string }
  | { id: string; role: 'assistant'; reply: ChatReply; approved: Record<number, string> }
  | { id: string; role: 'error'; text: string };

export interface ChatState {
  conversation?: string;
  messages: ChatMessage[];
  provider: Provider;
  busy: boolean;
}

const KEY = 'kernel.chat';
const KEEP = 60;

function load(): ChatState {
  try {
    const raw = JSON.parse(localStorage.getItem(KEY) ?? '{}') as Partial<ChatState>;
    return {
      conversation: typeof raw.conversation === 'string' ? raw.conversation : undefined,
      messages: Array.isArray(raw.messages) ? raw.messages.slice(-KEEP) : [],
      provider: raw.provider === 'local' || raw.provider === 'claude' ? raw.provider : 'auto',
      busy: false,
    };
  } catch {
    return { messages: [], provider: 'auto', busy: false };
  }
}

let n = 0;
const id = () => `${Date.now().toString(36)}-${(n++).toString(36)}`;

export class ChatStore {
  private state: ChatState = load();
  private listeners = new Set<() => void>();

  subscribe = (fn: () => void) => {
    this.listeners.add(fn);
    return () => this.listeners.delete(fn);
  };

  getSnapshot = () => this.state;

  private set(patch: Partial<ChatState>) {
    this.state = { ...this.state, ...patch };
    try {
      const { busy: _busy, ...keep } = this.state;
      localStorage.setItem(KEY, JSON.stringify({ ...keep, messages: keep.messages.slice(-KEEP) }));
    } catch {
      // Private mode: it lives until the app closes.
    }
    for (const fn of this.listeners) fn();
  }

  setProvider(provider: Provider) {
    this.set({ provider });
  }

  /** A fresh conversation (the node forgets nothing it logged; the model starts over). */
  clear() {
    this.set({ conversation: undefined, messages: [] });
  }

  async send(
    text: string,
    ask: (text: string, conversation: string | undefined, provider: Provider) => Promise<ChatReply>,
  ) {
    const trimmed = text.trim();
    if (!trimmed || this.state.busy) return;
    this.set({
      messages: [...this.state.messages, { id: id(), role: 'user', text: trimmed }],
      busy: true,
    });
    try {
      const reply = await ask(trimmed, this.state.conversation, this.state.provider);
      this.set({
        conversation: reply.conversation,
        messages: [...this.state.messages, { id: id(), role: 'assistant', reply, approved: {} }],
        busy: false,
      });
    } catch (e) {
      this.set({
        messages: [...this.state.messages, { id: id(), role: 'error', text: (e as Error).message }],
        busy: false,
      });
    }
  }

  /** Record what happened when the person approved (or tried to approve) a request. */
  markApproved(messageId: string, index: number, outcome: string) {
    this.set({
      messages: this.state.messages.map((m) =>
        m.id === messageId && m.role === 'assistant'
          ? { ...m, approved: { ...m.approved, [index]: outcome } }
          : m,
      ),
    });
  }
}

export const chatStore = new ChatStore();
