import { useCallback, useEffect, useState } from 'react';
import { stateTone } from '../lib/format.ts';
import type { Module, NodeClient } from '../lib/node.ts';
import { Icon } from './Icon.tsx';

interface Listed {
  id: string;
  name: string;
  icon: string;
  enabled: boolean;
  state: string | null;
}

const UPDATE_TEXT: Record<string, string> = {
  off: 'Updates are off on this node (no [update] repo in node.toml).',
  checking: 'Checking GitHub…',
  downloading: 'Downloading the new build…',
  installing: 'Installing; it restarts and comes back in a minute.',
  failed: 'The last update step failed.',
  current: 'Up to date.',
  none: 'No builds found on GitHub yet.',
  unknown: 'Not checked yet.',
};

/** Settings > the home node: updates (now or automatically) and which modules run. */
export function NodeControls({
  client,
  node,
  nodeName,
  online,
}: {
  client: NodeClient;
  nodeName: string;
  /** The node's own module from the catalog (status: build, update, auto_install, …). */
  node: Module | undefined;
  online: boolean;
}) {
  const status = (node?.status ?? {}) as Record<string, unknown>;
  const has = (action: string) => node?.actions.some((a) => a.id === action) ?? false;
  const [busy, setBusy] = useState('');
  const [message, setMessage] = useState('');
  const [modules, setModules] = useState<Listed[] | null>(null);
  const [pending, setPending] = useState<{ id: string; enabled: boolean } | null>(null);

  const canList = online && has('modules.list');
  const loadModules = useCallback(async () => {
    const r = await client.invoke('node', 'modules.list');
    if (r.ok) setModules((r.result as { modules: Listed[] }).modules);
  }, [client]);
  useEffect(() => {
    if (canList) void loadModules();
  }, [canList, loadModules]);

  const run = async (label: string, action: string, params: Record<string, unknown> = {}) => {
    setBusy(label);
    setMessage('');
    const r = await client.invoke('node', action, params);
    setBusy('');
    const said = (r.result as { message?: string } | undefined)?.message;
    setMessage(r.ok ? (said ?? '') : (r.error?.message ?? 'failed'));
    return r.ok;
  };

  if (!node) {
    return null;
  }
  const word = String(status.update ?? 'unknown');
  const build = status.build as number | undefined;
  const latest = status.latest_build as number | null | undefined;
  const available = word === 'available' && latest;

  return (
    <div className="form" style={{ marginTop: 24 }}>
      <div style={{ fontWeight: 600, fontSize: 14 }}>Node updates and modules</div>
      <div className="field">
        <span>
          Kernel on {nodeName}: build {build ?? '?'}
          {available ? (
            <b style={{ color: 'var(--accent)' }}> · build {latest} is out</b>
          ) : (
            <span className="muted"> · {UPDATE_TEXT[word] ?? word}</span>
          )}
        </span>
        <span style={{ display: 'flex', gap: 6, flexWrap: 'wrap' }}>
          <button
            type="button"
            className="btn"
            disabled={!online || !!busy || word === 'off'}
            onClick={() => void run('checking', 'update.check')}
          >
            <Icon name="refresh-cw" />
            Check now
          </button>
          <button
            type="button"
            className={`btn${available ? ' primary' : ''}`}
            disabled={!online || !available || !!busy}
            onClick={() => void run('updating', 'update.install')}
          >
            <Icon name="download" />
            {busy === 'updating' ? 'Updating…' : 'Update now'}
          </button>
        </span>
        {status.error ? (
          <span className="hint c-bad selectable">{String(status.error)}</span>
        ) : null}
        {status.last_update_error ? (
          <span className="hint c-bad selectable">
            Last update: {String(status.last_update_error)}
          </span>
        ) : null}
      </div>

      {has('update.auto') ? (
        <label className="field" style={{ flexDirection: 'row', alignItems: 'center', gap: 8 }}>
          <input
            type="checkbox"
            checked={status.auto_install === true}
            disabled={!online || !!busy}
            onChange={(e) => void run('saving', 'update.auto', { enabled: e.target.checked })}
            style={{ width: 16, height: 16 }}
          />
          <span>Install updates automatically</span>
          <span className="hint">
            it checks GitHub every few hours and restarts itself on new builds
          </span>
        </label>
      ) : (
        <span className="hint">
          Update the node once to get the auto-update switch and module switches here.
        </span>
      )}

      {canList && modules ? (
        <div className="field">
          <span>Modules</span>
          <div>
            {modules
              .filter((m) => m.id !== 'hello' || m.enabled)
              .map((m) => {
                const asked = pending?.id === m.id ? pending : null;
                return (
                  <div key={m.id} className="trow" style={{ border: '1px solid var(--line)' }}>
                    <Icon name={m.icon} />
                    <span className="grow">{m.name}</span>
                    {asked ? (
                      <>
                        <span className="muted">
                          {asked.enabled ? 'Turn on' : 'Turn off'}? Kernel restarts (a few seconds).
                        </span>
                        <button
                          type="button"
                          className="btn primary"
                          disabled={!!busy}
                          onClick={async () => {
                            const ok = await run('switching', 'modules.enable', {
                              module: m.id,
                              enabled: asked.enabled,
                            });
                            setPending(null);
                            if (ok) void loadModules();
                          }}
                        >
                          Apply
                        </button>
                        <button type="button" className="btn" onClick={() => setPending(null)}>
                          Cancel
                        </button>
                      </>
                    ) : (
                      <>
                        <span className="muted">
                          {m.enabled ? (
                            <>
                              <span className={`sq ${stateTone(m.state ?? 'starting')}`} />{' '}
                              {m.state ?? 'starting'}
                            </>
                          ) : (
                            'off'
                          )}
                        </span>
                        <label
                          style={{ display: 'flex', alignItems: 'center', gap: 6 }}
                          title={m.enabled ? 'Turn off' : 'Turn on'}
                        >
                          <input
                            type="checkbox"
                            aria-label={`${m.name} on`}
                            checked={m.enabled}
                            disabled={!online || !!busy}
                            onChange={(e) => setPending({ id: m.id, enabled: e.target.checked })}
                            style={{ width: 16, height: 16 }}
                          />
                        </label>
                      </>
                    )}
                  </div>
                );
              })}
          </div>
          <span className="hint">
            New modules arrive with updates switched off; turn them on here. Each one installs what
            it needs by itself the first time.
          </span>
        </div>
      ) : null}
      {busy || message ? (
        <span className="hint selectable">{busy ? `${busy}…` : message}</span>
      ) : null}
    </div>
  );
}
