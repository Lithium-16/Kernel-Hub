import { useEffect, useState } from 'react';
import {
  appBuild,
  checkForUpdate,
  installUpdate,
  loadAutoUpdate,
  saveAutoUpdate,
} from '../lib/app-update.ts';
import { Icon } from './Icon.tsx';

type State =
  | { kind: 'idle' }
  | { kind: 'checking' }
  | { kind: 'current' }
  | { kind: 'available'; build: number }
  | { kind: 'installing'; build: number }
  | { kind: 'error'; message: string };

/** Settings > App updates: this app's build, and the newest one on GitHub. */
export function AppUpdates() {
  const [build, setBuild] = useState<number | null>(null);
  const [state, setState] = useState<State>({ kind: 'idle' });
  const [auto, setAuto] = useState(loadAutoUpdate);

  useEffect(() => {
    void appBuild().then(setBuild, () => setBuild(null));
  }, []);

  const check = async () => {
    setState({ kind: 'checking' });
    try {
      const a = await checkForUpdate();
      setState(a ? { kind: 'available', build: a.build } : { kind: 'current' });
    } catch (e) {
      setState({ kind: 'error', message: String((e as Error).message ?? e) });
    }
  };

  const install = async (target: number) => {
    setState({ kind: 'installing', build: target });
    try {
      await installUpdate();
    } catch (e) {
      setState({ kind: 'error', message: String((e as Error).message ?? e) });
    }
  };

  const text = {
    idle: '',
    checking: 'Checking GitHub…',
    current: "You're on the newest build.",
    available: state.kind === 'available' ? `Build ${state.build} is out.` : '',
    installing:
      state.kind === 'installing'
        ? `Installing build ${state.build}. Kernel closes and opens again in a few seconds.`
        : '',
    error: state.kind === 'error' ? state.message : '',
  }[state.kind];

  return (
    <div className="form" style={{ marginTop: 24 }}>
      <div style={{ fontWeight: 600, fontSize: 14 }}>App updates</div>
      <div className="field">
        <span>
          Kernel desktop{' '}
          {build === null
            ? '(browser preview)'
            : build === 0
              ? '(development build)'
              : `build ${build}`}
        </span>
        <span style={{ display: 'flex', gap: 6 }}>
          <button
            type="button"
            className="btn"
            disabled={state.kind === 'checking' || state.kind === 'installing'}
            onClick={() => void check()}
          >
            <Icon
              name={state.kind === 'checking' ? 'loader-circle' : 'refresh-cw'}
              className={state.kind === 'checking' ? 'i spin' : 'i'}
            />
            Check for updates
          </button>
          {state.kind === 'available' ? (
            <button type="button" className="btn primary" onClick={() => void install(state.build)}>
              <Icon name="download" />
              Install build {state.build}
            </button>
          ) : null}
        </span>
        {text ? (
          <span className={`hint ${state.kind === 'error' ? 'c-bad' : ''}`}>{text}</span>
        ) : null}
        <label style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
          <input
            type="checkbox"
            checked={auto}
            onChange={(e) => {
              setAuto(e.target.checked);
              saveAutoUpdate(e.target.checked);
            }}
            style={{ width: 16, height: 16 }}
          />
          <span>Install app updates automatically</span>
        </label>
        <span className="hint">
          Comes from the same GitHub releases as the node's updates, checked with SHA-256. With
          automatic updates on, the app installs a new build when it finds one (at start and every
          six hours) and reopens by itself.
        </span>
      </div>
    </div>
  );
}
