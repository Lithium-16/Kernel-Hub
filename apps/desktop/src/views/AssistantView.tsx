import { useEffect, useRef, useState, useSyncExternalStore } from 'react';
import { Icon } from '../components/Icon.tsx';
import { TitleBar } from '../components/TitleBar.tsx';
import { type ChatMessage, chatStore, type Provider } from '../lib/chat.ts';
import { summary } from '../lib/format.ts';
import type { Module, NodeClient } from '../lib/node.ts';

const PROVIDERS: { id: Provider; name: string; hint: string }[] = [
  { id: 'auto', name: 'Auto', hint: 'the local model first, Claude if it fails' },
  { id: 'local', name: 'Local', hint: 'only the model on your node (free)' },
  { id: 'claude', name: 'Claude', hint: 'Claude, within your monthly budget' },
];

const IDEAS = [
  'Is the Minecraft server up?',
  'How hot is the GPU right now?',
  "What's using my VRAM?",
  'Free up VRAM',
];

export function AssistantView({
  client,
  modules,
  online,
}: {
  client: NodeClient;
  modules: Module[];
  online: boolean;
}) {
  const chat = useSyncExternalStore(chatStore.subscribe, chatStore.getSnapshot);
  const [draft, setDraft] = useState('');
  const [started, setStarted] = useState<number | null>(null);
  const [now, setNow] = useState(Date.now());
  const bottom = useRef<HTMLDivElement>(null);

  // Scroll to the newest message.
  useEffect(
    () => bottom.current?.scrollIntoView({ block: 'end' }),
    [chat.messages.length, chat.busy],
  );

  useEffect(() => {
    if (!chat.busy) {
      setStarted(null);
      return;
    }
    setStarted(Date.now());
    const t = setInterval(() => setNow(Date.now()), 500);
    return () => clearInterval(t);
  }, [chat.busy]);

  const send = (text: string) => {
    if (!online) return;
    setDraft('');
    void chatStore.send(text, (t, conv, provider) => client.chat(t, conv, provider));
  };

  const name = (moduleId: string, actionId: string) => {
    const m = modules.find((x) => x.id === moduleId);
    return {
      module: m?.name ?? moduleId,
      action: m?.actions.find((a) => a.id === actionId)?.label ?? actionId,
    };
  };

  const approve = async (msg: Extract<ChatMessage, { role: 'assistant' }>, index: number) => {
    const a = msg.reply.approvals[index];
    if (!a) return;
    chatStore.markApproved(msg.id, index, 'running…');
    const r = await client.invoke(a.module, a.action, a.params);
    chatStore.markApproved(
      msg.id,
      index,
      r.ok ? `done: ${summary(r.result)}` : `failed: ${r.error?.message ?? 'error'}`,
    );
  };

  return (
    <div className="main">
      <TitleBar
        title="Assistant"
        meta={
          <span style={{ display: 'inline-flex', gap: 4, alignItems: 'center' }}>
            {PROVIDERS.map((p) => (
              <button
                key={p.id}
                type="button"
                title={p.hint}
                className={`btn${chat.provider === p.id ? ' primary' : ''}`}
                onClick={() => chatStore.setProvider(p.id)}
              >
                {p.name}
              </button>
            ))}
            <button
              type="button"
              className="btn"
              title="Start a new conversation"
              onClick={() => chatStore.clear()}
            >
              <Icon name="eraser" />
              New
            </button>
          </span>
        }
      />
      <div className="content chat">
        {!online ? (
          <div className="banner">Connect to your node to talk to the assistant.</div>
        ) : null}
        {chat.messages.length === 0 ? (
          <div className="empty">
            <Icon name="sparkles" size={22} className="i accent" />
            <div>Ask Kernel to check on things or press buttons for you.</div>
            <div style={{ display: 'flex', gap: 6, flexWrap: 'wrap', justifyContent: 'center' }}>
              {IDEAS.map((idea) => (
                <button
                  key={idea}
                  type="button"
                  className="btn"
                  disabled={!online}
                  onClick={() => send(idea)}
                >
                  {idea}
                </button>
              ))}
            </div>
            <div className="muted" style={{ fontSize: 12 }}>
              It can only use your modules' buttons. Anything that normally asks first waits for
              your Approve.
            </div>
          </div>
        ) : null}
        {chat.messages.map((m) =>
          m.role === 'user' ? (
            <div key={m.id} className="bubble user selectable">
              {m.text}
            </div>
          ) : m.role === 'error' ? (
            <div key={m.id} className="banner bad selectable">
              <Icon name="circle-alert" />
              {m.text}
            </div>
          ) : (
            <div key={m.id} className="bubble bot">
              <div className="selectable" style={{ whiteSpace: 'pre-wrap' }}>
                {m.reply.text}
              </div>
              {m.reply.steps.length ? (
                <div className="steps">
                  {m.reply.steps.map((s, i) => {
                    const n = name(s.module, s.action);
                    return (
                      <div key={i} className="step" title={s.summary}>
                        <Icon
                          name={s.ok ? 'check' : 'clock'}
                          size={12}
                          className={s.ok ? 'i c-ok' : 'i c-warn'}
                        />
                        <span>
                          {n.module} · {n.action}
                        </span>
                        <span className="muted grow">{s.summary}</span>
                      </div>
                    );
                  })}
                </div>
              ) : null}
              {m.reply.approvals.map((a, i) => {
                const n = name(a.module, a.action);
                const outcome = m.approved[i];
                return (
                  <div key={i} className="approval">
                    <span className="grow">
                      Wants to run{' '}
                      <b>
                        {n.module} · {a.label}
                      </b>
                      {Object.keys(a.params).length ? (
                        <span className="muted"> {JSON.stringify(a.params)}</span>
                      ) : null}
                    </span>
                    {outcome ? (
                      <span className="muted selectable">{outcome}</span>
                    ) : (
                      <button
                        type="button"
                        className="btn primary"
                        onClick={() => void approve(m, i)}
                      >
                        <Icon name="check" />
                        Approve
                      </button>
                    )}
                  </div>
                );
              })}
              <div className="meta muted">
                {m.reply.provider === 'claude' ? 'Claude' : 'Local'} · {m.reply.model}
                {m.reply.cost_usd > 0 ? ` · $${m.reply.cost_usd.toFixed(4)}` : ''}
                {m.reply.route ? ` · ${m.reply.route}` : ''}
              </div>
            </div>
          ),
        )}
        {chat.busy ? (
          <div className="bubble bot muted">
            <Icon name="loader-circle" className="i spin" /> Thinking…
            {started && now - started > 8000
              ? ` ${Math.round((now - started) / 1000)}s (loading the model can take a bit)`
              : ''}
          </div>
        ) : null}
        <div ref={bottom} />
      </div>
      <form
        className="composer"
        onSubmit={(e) => {
          e.preventDefault();
          send(draft);
        }}
      >
        <textarea
          className="input"
          rows={2}
          value={draft}
          placeholder={
            online ? 'Ask Kernel… (Enter to send, Shift+Enter for a new line)' : 'Not connected'
          }
          disabled={!online}
          onChange={(e) => setDraft(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === 'Enter' && !e.shiftKey) {
              e.preventDefault();
              send(draft);
            }
          }}
        />
        <button
          type="submit"
          className="btn primary"
          disabled={!online || chat.busy || !draft.trim()}
        >
          <Icon name="send" />
          Send
        </button>
      </form>
    </div>
  );
}
