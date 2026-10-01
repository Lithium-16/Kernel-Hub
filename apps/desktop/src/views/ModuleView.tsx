import { useCallback, useEffect, useLayoutEffect, useRef, useState } from 'react';
import { Icon } from '../components/Icon.tsx';
import { ModuleLog } from '../components/ModuleLog.tsx';
import { ModuleSettings } from '../components/ModuleSettings.tsx';
import { ParamForm } from '../components/ParamForm.tsx';
import { TitleBar } from '../components/TitleBar.tsx';
import { type Action, hasParams, needsConfirm, needsInput } from '../lib/actions.ts';
import { label, stateTone, summary, value, valueTone } from '../lib/format.ts';
import { layoutStatus, type Section } from '../lib/layout.ts';
import type { ActionResult, Module, NodeClient } from '../lib/node.ts';

/** Modules with both of these get a live console instead of two buttons. */
const CONSOLE_ACTIONS = new Set(['console.tail', 'server.command']);
const CONFIRM_WINDOW_MS = 4000;

interface LastRun {
  action: Action;
  res: ActionResult;
}

export function ModuleView({
  client,
  module,
  nodeName,
}: {
  client: NodeClient;
  module: Module;
  nodeName: string;
}) {
  const running = module.state === 'running';
  const ids = new Set(module.actions.map((a) => a.id));
  const hasConsole = [...CONSOLE_ACTIONS].every((id) => ids.has(id));
  // The Node module's settings and log actions have their own panels below.
  const usable = module.actions.filter(
    (a) =>
      !(hasConsole && CONSOLE_ACTIONS.has(a.id)) &&
      !(module.id === 'node' && (a.id.startsWith('settings.') || a.id === 'logs.tail')),
  );
  const toolbar = usable.filter((a) => !needsInput(a));
  const more = usable.filter(needsInput);
  const [menu, setMenu] = useState<{ x: number; y: number } | null>(null);

  const [form, setForm] = useState<string | null>(null);
  const [armed, setArmed] = useState<string | null>(null);
  const [busy, setBusy] = useState<ReadonlySet<string>>(new Set());
  const [last, setLast] = useState<LastRun | null>(null);
  const [panel, setPanel] = useState<'settings' | 'log' | null>(null);

  // A different module gets a clean slate.
  useEffect(() => {
    setForm(null);
    setArmed(null);
    setLast(null);
    setMenu(null);
    setPanel(null);
  }, [module.id]);

  useEffect(() => {
    if (!armed) return;
    const t = setTimeout(() => setArmed(null), CONFIRM_WINDOW_MS);
    return () => clearTimeout(t);
  }, [armed]);

  const run = async (a: Action, params: Record<string, unknown> = {}) => {
    setArmed(null);
    setBusy((b) => new Set(b).add(a.id));
    const res = await client.invoke(module.id, a.id, params);
    setBusy((b) => {
      const next = new Set(b);
      next.delete(a.id);
      return next;
    });
    setLast({ action: a, res });
    if (res.ok) setForm(null);
  };

  const click = (a: Action) => {
    if (hasParams(a)) {
      setForm(form === a.id ? null : a.id);
    } else if (needsConfirm(a) && armed !== a.id) {
      setArmed(a.id);
    } else {
      void run(a);
    }
  };

  const status = module.status ?? null;
  const { tiles, details, sections } = layoutStatus(status);
  const formAction = usable.find((a) => a.id === form);
  const error = typeof status?.error === 'string' ? status.error : null;
  const stateText = value('state', status?.state ?? module.state);

  return (
    <div className="main">
      <TitleBar title={module.name} meta={`on ${nodeName} · module ${module.version}`} />
      <div className="toolbar">
        {toolbar.map((a) => {
          const isArmed = armed === a.id;
          const isBusy = busy.has(a.id);
          return (
            <button
              key={a.id}
              type="button"
              className={`btn ${isArmed ? 'confirm' : ''}`}
              disabled={!running || isBusy}
              title={a.description ?? a.label}
              onClick={() => click(a)}
            >
              <Icon
                name={isBusy ? 'loader-circle' : (a.icon ?? 'play')}
                className={isBusy ? 'i spin' : 'i'}
              />
              {isArmed ? `Confirm ${a.label.toLowerCase()}` : a.label}
              {hasParams(a) ? <Icon name="chevron-down" size={12} /> : null}
            </button>
          );
        })}
        {more.length ? (
          <>
            <span className="divider" />
            <button
              type="button"
              className="btn"
              disabled={!running}
              aria-haspopup="menu"
              aria-expanded={menu !== null}
              onClick={(e) => {
                const r = e.currentTarget.getBoundingClientRect();
                setMenu(menu ? null : { x: r.left, y: r.bottom + 4 });
              }}
            >
              <Icon name="ellipsis" />
              More
              <Icon name="chevron-down" size={12} />
            </button>
          </>
        ) : null}
        <span style={{ flex: 1 }} />
        {module.id !== 'node' ? (
          <button
            type="button"
            className={`btn${panel === 'settings' ? ' primary' : ''}`}
            title="This module's settings on its node"
            onClick={() => setPanel(panel === 'settings' ? null : 'settings')}
          >
            <Icon name="sliders-horizontal" />
            Settings
          </button>
        ) : null}
        <button
          type="button"
          className={`btn${panel === 'log' ? ' primary' : ''}`}
          title="The end of this module's log"
          onClick={() => setPanel(panel === 'log' ? null : 'log')}
        >
          <Icon name="scroll-text" />
          Log
        </button>
      </div>
      {menu ? (
        <div style={{ position: 'fixed', inset: 0, zIndex: 4 }} onMouseDown={() => setMenu(null)}>
          <div
            className="menu"
            role="menu"
            style={{ left: menu.x, top: menu.y }}
            onMouseDown={(e) => e.stopPropagation()}
          >
            {more.map((a) => (
              <button
                key={a.id}
                type="button"
                role="menuitem"
                className="row"
                onClick={() => {
                  setMenu(null);
                  setForm(a.id);
                }}
              >
                <Icon name={a.icon ?? 'play'} />
                <span className="grow">{a.label}</span>
                {needsConfirm(a) ? <span className="r">confirm</span> : null}
              </button>
            ))}
          </div>
        </div>
      ) : null}
      {formAction ? (
        <ParamForm
          key={formAction.id}
          action={formAction}
          busy={busy.has(formAction.id)}
          onRun={(params) => void run(formAction, params)}
          onCancel={() => setForm(null)}
        />
      ) : null}

      <div className="content">
        {panel === 'settings' ? (
          <ModuleSettings client={client} module={module.id} onClose={() => setPanel(null)} />
        ) : null}
        {panel === 'log' ? (
          <ModuleLog client={client} module={module.id} onClose={() => setPanel(null)} />
        ) : null}
        {!running ? (
          <div className="banner">
            <Icon name="triangle-alert" />
            The module is {module.state} on {nodeName}. Its buttons come back when it is running.
          </div>
        ) : null}
        {error ? (
          <div className="banner bad">
            <Icon name="circle-alert" />
            <span className="selectable">{error}</span>
          </div>
        ) : null}

        {tiles.length ? (
          <div className="tiles">
            {tiles.map((t) => (
              <div key={t.key} className="tile">
                <span className="k">{label(t.name)}</span>
                <span className={`v ${toneClass(t.key, t.value, t.max)}`}>
                  {t.key === 'state' ? <span className={`sq ${stateTone(t.value)}`} /> : null}
                  {value(t.key, t.value)}
                  {t.max !== undefined ? <small>/ {value(t.key, t.max)}</small> : null}
                </span>
              </div>
            ))}
          </div>
        ) : null}

        <div
          className="panels"
          style={
            hasConsole || sections.length || details.length || last
              ? undefined
              : { display: 'none' }
          }
        >
          {hasConsole ? (
            <Console client={client} module={module} running={running} />
          ) : (
            <div className="stack">
              {last ? <ResultPanel last={last} /> : null}
              {details.length ? <DetailsPanel details={details} /> : null}
            </div>
          )}
          <div className="stack">
            {hasConsole && last ? <ResultPanel last={last} /> : null}
            {sections.map((s) => (
              <SectionPanel key={s.key} section={s} />
            ))}
            {hasConsole && details.length ? <DetailsPanel details={details} /> : null}
          </div>
        </div>
      </div>

      <div className="statusbar">
        {last ? (
          <>
            <span className={`sq ${last.res.ok ? 'ok' : 'bad'}`} />
            <span className="grow">
              {last.action.label}:{' '}
              {last.res.ok ? summary(last.res.result) : (last.res.error?.message ?? 'failed')}
            </span>
          </>
        ) : (
          <>
            <span className={`sq ${stateTone(status?.state ?? module.state)}`} />
            <span className="grow">
              {stateText} · live from {nodeName}
            </span>
          </>
        )}
      </div>
    </div>
  );
}

function SectionPanel({ section }: { section: Section }) {
  return (
    <section className="panel">
      <div className="ph">
        <span className="t">{label(section.key)}</span>
        {Array.isArray(section.value) ? (
          <span className="muted">{section.value.length}</span>
        ) : null}
      </div>
      <ValueBody name={section.key} value={section.value} />
    </section>
  );
}

function DetailsPanel({ details }: { details: [string, unknown][] }) {
  return (
    <section className="panel">
      <div className="ph">
        <span className="t">Details</span>
      </div>
      {details.map(([k, v]) => (
        <div key={k} className="trow">
          <span className="muted" style={{ width: 110, flexShrink: 0 }}>
            {label(k)}
          </span>
          <span className="grow selectable">{value(k, v)}</span>
        </div>
      ))}
    </section>
  );
}

function ResultPanel({ last }: { last: LastRun }) {
  const { action, res } = last;
  return (
    <section className="panel">
      <div className="ph">
        <Icon name={res.ok ? 'check' : 'x'} className={`i ${res.ok ? 'c-ok' : 'c-bad'}`} />
        <span className="t">{action.label}</span>
        <span className="muted">{res.ok ? 'done' : res.error?.code}</span>
      </div>
      {res.ok ? (
        <ValueBody name="result" value={res.result} />
      ) : (
        <div className="trow selectable c-bad">{res.error?.message}</div>
      )}
    </section>
  );
}

/** Render any JSON value: tables for lists of objects, rows for lists and objects. */
function ValueBody({ name, value: v }: { name: string; value: unknown }) {
  if (Array.isArray(v)) {
    if (v.length === 0) return <div className="trow muted">None</div>;
    const objects = v.every((x) => x !== null && typeof x === 'object' && !Array.isArray(x));
    if (objects) {
      const rows = v as Record<string, unknown>[];
      const cols = Object.keys(rows[0] ?? {});
      return (
        <div style={{ overflow: 'auto', maxHeight: 360 }}>
          <table className="table selectable">
            <thead>
              <tr>
                {cols.map((c) => (
                  <th key={c}>{label(c)}</th>
                ))}
              </tr>
            </thead>
            <tbody>
              {rows.map((r, i) => (
                <tr key={i}>
                  {cols.map((c) => (
                    <td
                      key={c}
                      className={c === 'bytes' || c.endsWith('_bytes') ? 'mono muted' : undefined}
                    >
                      {value(c, r[c])}
                    </td>
                  ))}
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      );
    }
    return (
      <div style={{ overflow: 'auto', maxHeight: 360 }}>
        {v.map((x, i) => (
          <div key={i} className="trow selectable">
            {value(name, x)}
          </div>
        ))}
      </div>
    );
  }
  if (v !== null && typeof v === 'object') {
    return (
      <>
        {Object.entries(v as Record<string, unknown>).map(([k, x]) =>
          (Array.isArray(x) && x.length > 0) ||
          (x !== null && typeof x === 'object' && !Array.isArray(x)) ? (
            <div key={k}>
              <div className="trow muted">{label(k)}</div>
              <ValueBody name={k} value={x} />
            </div>
          ) : (
            <div key={k} className="trow">
              <span
                className={`sq ${stateTone(x)}`}
                style={stateTone(x) === 'mute' ? { visibility: 'hidden' } : undefined}
              />
              <span className="muted" style={{ width: 110, flexShrink: 0 }}>
                {label(k)}
              </span>
              <span className="grow selectable">{Array.isArray(x) ? 'None' : value(k, x)}</span>
            </div>
          ),
        )}
      </>
    );
  }
  return <div className="trow selectable">{value(name, v)}</div>;
}

const LOG_LINE =
  /^\[(\d\d:\d\d:\d\d)\] \[[^\]]*?\/(INFO|WARN|ERROR|FATAL|DEBUG)\](?: \[[^\]]*\])?: (.*)$/;

function lineTone(level: string, text: string): string {
  if (level === 'WARN') return 'c-warn';
  if (level === 'ERROR' || level === 'FATAL') return 'c-bad';
  if (/joined the game|left the game/.test(text)) return 'c-acc';
  if (text.startsWith('Done (')) return 'c-ok';
  return '';
}

function Console({
  client,
  module,
  running,
}: {
  client: NodeClient;
  module: Module;
  running: boolean;
}) {
  const [lines, setLines] = useState<string[]>([]);
  const [cmd, setCmd] = useState('');
  const [sending, setSending] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const box = useRef<HTMLDivElement>(null);
  const stick = useRef(true);

  const load = useCallback(async () => {
    const r = await client.invoke(module.id, 'console.tail', { lines: 200 });
    const got = (r.result as { lines?: unknown } | undefined)?.lines;
    if (r.ok && Array.isArray(got)) setLines(got.map(String));
  }, [client, module.id]);

  useEffect(() => {
    if (!running) return;
    void load();
    const t = setInterval(() => void load(), 3000);
    return () => clearInterval(t);
  }, [running, load]);

  useLayoutEffect(() => {
    const el = box.current;
    if (el && stick.current) el.scrollTop = el.scrollHeight;
  }, [lines]);

  const send = async () => {
    const command = cmd.trim();
    if (!command || sending) return;
    setSending(true);
    const r = await client.invoke(module.id, 'server.command', { command });
    setSending(false);
    if (r.ok) {
      setCmd('');
      setError(null);
      stick.current = true;
      await load();
    } else {
      setError(r.error?.message ?? 'failed');
    }
  };

  return (
    <section className="panel">
      <div className="ph">
        <Icon name="terminal" />
        <span className="t">Console</span>
        <span className="muted">{running ? 'live' : 'offline'}</span>
      </div>
      <div
        ref={box}
        className="console"
        onScroll={(e) => {
          const el = e.currentTarget;
          stick.current = el.scrollHeight - el.scrollTop - el.clientHeight < 24;
        }}
      >
        {lines.length === 0 ? (
          <div className="line muted">{running ? 'Waiting for the log…' : 'No output'}</div>
        ) : null}
        {lines.map((line, i) => {
          const m = LOG_LINE.exec(line);
          return (
            <div key={i} className="line">
              {m ? (
                <>
                  <span className="muted">{m[1]}</span>{' '}
                  <span className={lineTone(m[2] ?? '', m[3] ?? '')}>{m[3]}</span>
                </>
              ) : (
                line
              )}
            </div>
          );
        })}
      </div>
      {error ? <div className="trow c-bad selectable">{error}</div> : null}
      <label className="cin">
        <span className="mono c-acc">/</span>
        <input
          type="text"
          placeholder={
            running ? 'say, whitelist add, time set day…' : 'Start the server to send commands'
          }
          aria-label="Server command"
          disabled={!running}
          value={cmd}
          onChange={(e) => setCmd(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === 'Enter') void send();
          }}
        />
        {sending ? (
          <Icon name="loader-circle" className="i spin" />
        ) : (
          <span className="kbd">Enter</span>
        )}
      </label>
    </section>
  );
}

function toneClass(key: string, v: unknown, max: unknown): string {
  const tone = valueTone(key, v, max);
  return tone ? `c-${tone}` : '';
}
