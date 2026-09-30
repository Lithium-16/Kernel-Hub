import { useEffect, useState } from 'react';
import type { NodeClient } from '../lib/node.ts';
import { Icon } from './Icon.tsx';

/** The end of a module's log on its node, refreshed every few seconds while open. */
export function ModuleLog({
  client,
  module,
  onClose,
}: {
  client: NodeClient;
  module: string;
  onClose: () => void;
}) {
  const [lines, setLines] = useState<string[] | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let alive = true;
    const load = async () => {
      const r = await client.invoke('node', 'logs.tail', { module, lines: 200 });
      if (!alive) return;
      if (r.ok) {
        setLines((r.result as { lines: string[] }).lines);
        setError(null);
      } else setError(r.error?.message ?? 'failed');
    };
    void load();
    const t = setInterval(() => void load(), 4000);
    return () => {
      alive = false;
      clearInterval(t);
    };
  }, [client, module]);

  return (
    <section className="panel" style={{ flexShrink: 0, maxHeight: 360 }}>
      <div className="ph">
        <Icon name="scroll-text" />
        <span className="t">Log</span>
        <span className="muted">last 200 lines</span>
        <span style={{ flex: 1 }} />
        <button type="button" className="btn" onClick={onClose}>
          Close
        </button>
      </div>
      <pre
        className="mono selectable"
        style={{ margin: 0, padding: 12, overflow: 'auto', fontSize: 12 }}
      >
        {error ?? (lines === null ? 'Loading…' : lines.join('\n') || 'Empty.')}
      </pre>
    </section>
  );
}
