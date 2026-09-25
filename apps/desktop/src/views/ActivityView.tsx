import { useEffect, useState } from 'react';
import { TitleBar } from '../components/TitleBar.tsx';
import { when } from '../lib/format.ts';
import type { Activity, Module, NodeClient } from '../lib/node.ts';

const RESULT_TONE = { ok: 'c-ok', error: 'c-bad', denied: 'c-warn' } as const;

export function ActivityView({
  client,
  modules,
  online,
}: {
  client: NodeClient;
  modules: Module[];
  online: boolean;
}) {
  const [entries, setEntries] = useState<Activity[] | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!online) return;
    let alive = true;
    const load = () =>
      client
        .activity(200)
        .then((e) => {
          if (alive) {
            setEntries(e);
            setError(null);
          }
        })
        .catch((e: Error) => alive && setError(e.message));
    void load();
    const t = setInterval(load, 5000);
    return () => {
      alive = false;
      clearInterval(t);
    };
  }, [client, online]);

  const actionLabel = (moduleId: string, actionId: string) => {
    const m = modules.find((x) => x.id === moduleId);
    const a = m?.actions.find((x) => x.id === actionId);
    return { module: m?.name ?? moduleId, action: a?.label ?? actionId };
  };

  return (
    <div className="main">
      <TitleBar title="Activity" meta="every action, who ran it, and how it went" />
      <div className="content" style={{ padding: 0 }}>
        {error ? (
          <div className="banner bad" style={{ margin: 16 }}>
            {error}
          </div>
        ) : null}
        {!online ? <div className="empty">Connect to your home node to see activity.</div> : null}
        {online && entries?.length === 0 ? <div className="empty">Nothing has run yet.</div> : null}
        {online && entries?.length ? (
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
