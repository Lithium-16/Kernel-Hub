import { useState } from 'react';
import { Icon } from '../components/Icon.tsx';
import { TitleBar } from '../components/TitleBar.tsx';
import { stateTone } from '../lib/format.ts';
import type { Snapshot } from '../lib/node.ts';
import { type NodeSettings, validUrl } from '../lib/settings.ts';

const CONN_TEXT = {
  idle: 'Not connected',
  connecting: 'Connecting…',
  online: 'Connected',
  offline: "Can't reach the node",
  unauthorized: 'The node refused the token',
} as const;

interface Props {
  settings: NodeSettings;
  snapshot: Snapshot;
  onSave: (s: NodeSettings) => void;
}

export function SettingsView({ settings, snapshot, onSave }: Props) {
  const [url, setUrl] = useState(settings.url);
  const [token, setToken] = useState(settings.token);
  const [show, setShow] = useState(false);
  const urlOk = validUrl(url);
  const changed = url !== settings.url || token !== settings.token;

  return (
    <div className="main">
      <TitleBar title="Settings" />
      <div className="content">
        <form
          className="form"
          onSubmit={(e) => {
            e.preventDefault();
            if (urlOk && token) onSave({ url: url.trim(), token });
          }}
        >
          <div style={{ fontWeight: 600, fontSize: 14 }}>Home node</div>
          <div className="trow" style={{ border: '1px solid var(--line)', paddingLeft: 12 }}>
            <span className={`sq ${stateTone(snapshot.conn)}`} />
            <span className="grow">
              {CONN_TEXT[snapshot.conn]}
              {snapshot.node ? ` to ${snapshot.node.name} (kerneld ${snapshot.node.version})` : ''}
            </span>
            {snapshot.error && snapshot.conn !== 'online' ? (
              <span className="muted selectable">{snapshot.error}</span>
            ) : null}
          </div>
          <label className="field">
            <span>Address</span>
            <input
              className="input selectable"
              value={url}
              onChange={(e) => setUrl(e.target.value)}
              spellCheck={false}
              placeholder="ws://pluto:47800/ws"
            />
            <span className={`hint ${url && !urlOk ? 'c-bad' : ''}`}>
              {url && !urlOk
                ? 'Use ws://host:port/ws'
                : "Pluto's Tailscale address (100.x.x.x) or name. The node only accepts Tailscale connections."}
            </span>
          </label>
          <label className="field">
            <span>Token</span>
            <span style={{ display: 'flex', gap: 6 }}>
              <input
                className="input selectable"
                style={{ flex: 1 }}
                type={show ? 'text' : 'password'}
                value={token}
                onChange={(e) => setToken(e.target.value)}
                spellCheck={false}
                autoComplete="off"
              />
              <button
                type="button"
                className="btn"
                onClick={() => setShow(!show)}
                aria-label={show ? 'Hide token' : 'Show token'}
              >
                <Icon name={show ? 'eye-off' : 'eye'} />
              </button>
            </span>
            <span className="hint">
              The token from the node's node.toml. Pairing with a code replaces this later.
            </span>
          </label>
          <div style={{ display: 'flex', gap: 8 }}>
            <button type="submit" className="btn primary" disabled={!urlOk || !token || !changed}>
              Save and connect
            </button>
          </div>
        </form>
      </div>
    </div>
  );
}
