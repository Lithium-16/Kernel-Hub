import { useEffect, useMemo, useRef, useState, useSyncExternalStore } from 'react';
import { Palette } from './components/Palette.tsx';
import { Sidebar, type View } from './components/Sidebar.tsx';
import { TitleBar } from './components/TitleBar.tsx';
import { NodeClient } from './lib/node.ts';
import { loadNotifyLevel, shouldNotify, show, title } from './lib/notify.ts';
import { loadSettings, type NodeSettings, saveSettings } from './lib/settings.ts';
import { ActivityView } from './views/ActivityView.tsx';
import { ModuleView } from './views/ModuleView.tsx';
import { SettingsView } from './views/SettingsView.tsx';

export const CLIENT = { name: 'desktop', version: '0.1.0' };

export function App() {
  const [settings, setSettings] = useState<NodeSettings>(loadSettings);
  const client = useMemo(
    () => new NodeClient({ url: settings.url, token: settings.token, client: CLIENT }),
    [settings],
  );
  const snap = useSyncExternalStore(client.subscribe, client.getSnapshot);
  const [view, setView] = useState<View>(() =>
    settings.token ? { kind: 'activity' } : { kind: 'settings' },
  );
  const [palette, setPalette] = useState(false);
  const openedFirst = useRef(false);

  useEffect(() => {
    if (!settings.token) return;
    client.start();
    return () => client.stop();
  }, [client, settings.token]);

  // Windows notifications for events at or above the chosen level.
  const modulesRef = useRef(snap.modules);
  modulesRef.current = snap.modules;
  useEffect(
    () =>
      client.onEvent((e) => {
        if (!shouldNotify(loadNotifyLevel(), e)) return;
        const name = modulesRef.current.find((m) => m.id === e.module)?.name ?? e.module;
        void show(title(e, name), e.message).catch(() => {});
      }),
    [client],
  );

  // Open the first module once the catalog arrives, unless the user already went somewhere.
  useEffect(() => {
    const first = snap.modules[0];
    if (openedFirst.current || !first) return;
    openedFirst.current = true;
    setView((v) => (v.kind === 'activity' ? { kind: 'module', id: first.id } : v));
  }, [snap.modules]);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.ctrlKey && e.key.toLowerCase() === 'k') {
        e.preventDefault();
        setPalette((p) => !p);
      } else if (e.ctrlKey && e.key === ',') {
        e.preventDefault();
        setView({ kind: 'settings' });
      }
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, []);

  const save = (s: NodeSettings) => {
    saveSettings(s);
    openedFirst.current = false;
    setSettings(s);
  };

  const nodeName = snap.node?.name ?? 'the home node';
  let main: React.ReactNode;
  if (view.kind === 'settings') {
    main = <SettingsView settings={settings} snapshot={snap} onSave={save} />;
  } else if (view.kind === 'activity') {
    main = <ActivityView client={client} modules={snap.modules} online={snap.conn === 'online'} />;
  } else {
    const module = snap.modules.find((m) => m.id === view.id);
    main = module ? (
      <ModuleView client={client} module={module} nodeName={nodeName} />
    ) : (
      <div className="main">
        <TitleBar title="Kernel" />
        <div className="empty">
          {snap.conn === 'online'
            ? 'That module is no longer enabled.'
            : `Waiting for ${nodeName}…`}
        </div>
      </div>
    );
  }

  return (
    <div className="app">
      <Sidebar snapshot={snap} view={view} onView={setView} onPalette={() => setPalette(true)} />
      {main}
      {palette ? (
        <div className="palette-backdrop" onMouseDown={() => setPalette(false)}>
          <div onMouseDown={(e) => e.stopPropagation()}>
            <Palette
              client={client}
              modules={snap.modules}
              onNavigate={setView}
              onClose={() => setPalette(false)}
            />
          </div>
        </div>
      ) : null}
    </div>
  );
}
