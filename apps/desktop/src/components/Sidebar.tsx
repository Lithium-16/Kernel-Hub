import type React from 'react';
import { stateTone, value } from '../lib/format.ts';
import { moduleTint } from '../lib/look.ts';
import type { Module, Snapshot } from '../lib/node.ts';
import { Icon } from './Icon.tsx';
import { Logo } from './Logo.tsx';

export type View =
  | { kind: 'module'; id: string }
  | { kind: 'activity' }
  | { kind: 'assistant' }
  | { kind: 'settings' };

/** "1/20" for modules that report players, otherwise their state. */
function moduleMeta(m: Module): { tone: string; text: string } {
  const s = m.status ?? {};
  if (m.state !== 'running') return { tone: stateTone(m.state), text: m.state };
  const state = s.state ?? m.state;
  if (typeof s.players_online === 'number' && s.state === 'running') {
    const max = typeof s.players_max === 'number' ? `/${s.players_max}` : '';
    return { tone: 'ok', text: `${s.players_online}${max}` };
  }
  return { tone: stateTone(state), text: value('state', state).toLowerCase() };
}

interface Props {
  snapshot: Snapshot;
  view: View;
  onView: (v: View) => void;
  onPalette: () => void;
  /** A newer build of this app, found on startup. */
  updateBuild?: number | null;
}

export function Sidebar({ snapshot, view, onView, onPalette, updateBuild }: Props) {
  const on = (v: View) =>
    v.kind === view.kind && (v.kind !== 'module' || (view.kind === 'module' && view.id === v.id));
  const nodeTone = stateTone(snapshot.conn);
  // The node has a newer build waiting (and won't install it by itself).
  const nodeStatus = (snapshot.modules.find((m) => m.id === 'node')?.status ?? {}) as Record<
    string,
    unknown
  >;
  const nodeUpdate =
    nodeStatus.update === 'available' && nodeStatus.auto_install !== true
      ? (nodeStatus.latest_build as number)
      : null;
  return (
    <aside className="sidebar">
      <div className="brand" data-tauri-drag-region>
        <Logo />
        <span style={{ flex: 1 }} data-tauri-drag-region>
          Kernel
        </span>
      </div>
      <button type="button" className="search" onClick={onPalette}>
        <Icon name="search" />
        <span style={{ flex: 1 }}>Run anything…</span>
        <span className="kbd">Ctrl</span>
        <span className="kbd">K</span>
      </button>
      <button
        type="button"
        className={`row ${on({ kind: 'activity' }) ? 'on' : ''}`}
        onClick={() => onView({ kind: 'activity' })}
      >
        <Icon name="list-clock" />
        <span className="grow">Activity</span>
      </button>
      <button
        type="button"
        className={`row ${on({ kind: 'assistant' }) ? 'on' : ''}`}
        onClick={() => onView({ kind: 'assistant' })}
      >
        <Icon name="sparkles" />
        <span className="grow">Assistant</span>
      </button>

      <div className="sect">Modules</div>
      {snapshot.modules.map((m) => {
        const meta = moduleMeta(m);
        const v: View = { kind: 'module', id: m.id };
        return (
          <button
            key={m.id}
            type="button"
            className={`row ${on(v) ? 'on' : ''}`}
            onClick={() => onView(v)}
            style={{ '--tint': moduleTint(m.id) } as React.CSSProperties}
          >
            <Icon name={m.icon} className="i mod" />
            <span className="grow">{m.name}</span>
            <span className="r">
              <span className={`sq ${meta.tone}`} />
              {meta.text}
            </span>
          </button>
        );
      })}
      {snapshot.modules.length === 0 ? (
        <div className="row muted" style={{ cursor: 'default' }}>
          {snapshot.conn === 'online' ? 'No modules enabled' : 'Not connected'}
        </div>
      ) : null}

      <div className="sect">Nodes</div>
      <button type="button" className="row" onClick={() => onView({ kind: 'settings' })}>
        <span style={{ width: 14, display: 'flex', justifyContent: 'center' }}>
          <span className={`sq ${nodeTone}`} />
        </span>
        <span className="grow">{snapshot.node?.name ?? 'Home node'}</span>
        <span className="r">{snapshot.conn === 'online' ? 'home' : snapshot.conn}</span>
      </button>

      <span style={{ flex: 1 }} />
      {nodeUpdate ? (
        <button
          type="button"
          className="row"
          title="Update it from Settings"
          onClick={() => onView({ kind: 'settings' })}
        >
          <Icon name="server" className="i accent" />
          <span className="grow">Update {snapshot.node?.name ?? 'node'}</span>
          <span className="r">build {nodeUpdate}</span>
        </button>
      ) : null}
      {updateBuild ? (
        <button
          type="button"
          className="row"
          title="Install it from Settings > App updates"
          onClick={() => onView({ kind: 'settings' })}
        >
          <Icon name="download" className="i accent" />
          <span className="grow">Update available</span>
          <span className="r">build {updateBuild}</span>
        </button>
      ) : null}
      <button
        type="button"
        className={`row ${on({ kind: 'settings' }) ? 'on' : ''}`}
        onClick={() => onView({ kind: 'settings' })}
      >
        <Icon name="settings-2" />
        <span className="grow">Settings</span>
        <span className="kbd">Ctrl</span>
        <span className="kbd">,</span>
      </button>
    </aside>
  );
}
