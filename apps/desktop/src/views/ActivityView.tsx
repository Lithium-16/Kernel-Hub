import { useEffect, useState } from 'react';
import { TitleBar } from '../components/TitleBar.tsx';
import { when } from '../lib/format.ts';
import type { Activity, KernelEvent, Module, NodeClient } from '../lib/node.ts';

const RESULT_TONE = { ok: 'c-ok', error: 'c-bad', denied: 'c-warn' } as const;
const LEVEL_TONE = { info: 'c-ok', warn: 'c-warn', error: 'c-bad' } as const;

type Tab = 'events' | 'actions';

export function ActivityView({
  client,
  modules,
  online,
}: {
  client: NodeClient;
  modules: Module[];
  online: boolean;
}) {
  const [tab, setTab] = useState<Tab>('events');
  const [entries, setEntries] = useState<Activity[] | null>(null);
  const [events, setEvents] = useState<KernelEvent[] | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!online) return;
    let alive = true;
    const load = () =>
      Promise.all([client.activity(200), client.events(200)])
        .then(([a, e]) => {
          if (alive) {
            setEntries(a);
            setEvents(e);
            setError(null);
          }
        })
        .catch((e: Error) => alive && setError(e.message));
    void load();
    const t = setInterval(load, 15_000);
    // New events arrive as they happen; the interval only catches up after reconnects.
    const off = client.onEvent((e) =>
      setEvents((prev) => [e, ...(prev ?? []).filter((x) => x.id !== e.id)].slice(0, 200)),
    );
    return () => {
      alive = false;
      clearInterval(t);
      off();
    };
  }, [client, online]);

  const moduleName = (id: string) => modules.find((x) => x.id === id)?.name ?? id;
  const actionLabel = (moduleId: string, actionId: string) => {
    const m = modules.find((x) => x.id === moduleId);
    const a = m?.actions.find((x) => x.id === actionId);
    return { module: m?.name ?? moduleId, action: a?.label ?? actionId };
  };

  const tabs = (
    <span style={{ display: 'inline-flex', gap: 4, marginLeft: 8 }}>
      {(['events', 'actions'] as const).map((t) => (
        <button
          key={t}
          type="button"
          className={`btn${tab === t ? ' primary' : ''}`}
          onClick={() => setTab(t)}
        >
          {t === 'events' ? 'Events' : 'Actions'}
        </button>
      ))}
    </span>
  );

  return (
    <div className="main">
      <TitleBar
        title="Activity"
        meta={
          <>
            {tab === 'events'
              ? 'what happened on its own'
              : 'every action, who ran it, and how it went'}
            {tabs}
          </>
        }
      />
      <div className="content" style={{ padding: 0 }}>
        {error ? (
          <div className="banner bad" style={{ margin: 16 }}>
            {error}
          </div>
        ) : null}
        {!online ? <div className="empty">Connect to your home node to see activity.</div> : null}
        {online && tab === 'events' && events?.length === 0 ? (
          <div className="empty">Nothing has happened yet.</div>
        ) : null}
        {online && tab === 'events' && events?.length ? (
          <table className="table selectable">
            <thead>
              <tr>
                <th>When</th>
                <th>Module</th>
                <th>What</th>
                <th>Level</th>
              </tr>
            </thead>
            <tbody>
              {events.map((e) => (
                <tr key={e.id}>
                  <td className="muted">{when(e.ts)}</td>
                  <td>{moduleName(e.module)}</td>
                  <td>
                    {e.message}
                    <span className="muted mono"> · {e.kind}</span>
                  </td>
                  <td className={LEVEL_TONE[e.level]}>{e.level}</td>
                </tr>
              ))}
            </tbody>
          </table>
        ) : null}
        {online && tab === 'actions' && entries?.length === 0 ? (
          <div className="empty">Nothing has run yet.</div>
        ) : null}
        {online && tab === 'actions' && entries?.length ? (
          <table className="table selectable">
            <thead>
              <tr>
                <th>When</th>
                <th>Who</th>
                <th>Module</th>
                <th>Action</th>
                <th>Result</th>
                <th style={{ textAlign: 'right' }}>Took</th>
              </tr>
            </thead>
            <tbody>
              {entries.map((e) => {
                const names = actionLabel(e.module, e.action);
                return (
                  <tr key={e.id}>
                    <td className="muted">{when(e.ts)}</td>
                    <td>
                      {e.actor.kind}
                      {e.actor.ref ? <span className="muted"> · {e.actor.ref}</span> : null}
                    </td>
                    <td>{names.module}</td>
                    <td>{names.action}</td>
                    <td className={RESULT_TONE[e.result]}>
                      {e.result}
                      {e.error_code ? (
                        <span className="muted"> · {e.error_code.replace(/_/g, ' ')}</span>
                      ) : null}
                    </td>
                    <td className="mono muted" style={{ textAlign: 'right' }}>
                      {e.duration_ms < 1000
                        ? `${e.duration_ms} ms`
                        : `${(e.duration_ms / 1000).toFixed(1)} s`}
                    </td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        ) : null}
      </div>
    </div>
  );
}
