import { StrictMode, useEffect, useMemo, useState, useSyncExternalStore } from 'react';
import { createRoot } from 'react-dom/client';
import { Palette } from './components/Palette.tsx';
import { applyLook, followLook, loadLook } from './lib/look.ts';
import { NodeClient } from './lib/node.ts';
import { loadSettings } from './lib/settings.ts';
import { win } from './lib/window.ts';
import './styles.css';

applyLook(loadLook());
followLook();

/** The Alt+Space window: its own connection, reset every time it is shown. */
function PaletteWindow() {
  const [settings, setSettings] = useState(loadSettings);
  const client = useMemo(
    () =>
      new NodeClient({
        url: settings.url,
        token: settings.token,
        client: { name: 'palette', version: '0.1.0' },
        pollMs: 5000,
      }),
    [settings],
  );
  const snap = useSyncExternalStore(client.subscribe, client.getSnapshot);
  const [shown, setShown] = useState(0);

  useEffect(() => {
    if (!settings.token) return;
    client.start();
    return () => client.stop();
  }, [client, settings.token]);

  useEffect(() => {
    const onFocus = () => setShown((n) => n + 1);
    const onStorage = () => setSettings(loadSettings());
    window.addEventListener('focus', onFocus);
    window.addEventListener('storage', onStorage);
    return () => {
      window.removeEventListener('focus', onFocus);
      window.removeEventListener('storage', onStorage);
    };
  }, []);

  return (
    <Palette key={shown} client={client} modules={snap.modules} onClose={() => void win.hide()} />
  );
}

const root = document.getElementById('root');
if (root) {
  createRoot(root).render(
    <StrictMode>
      <PaletteWindow />
    </StrictMode>,
  );
}
