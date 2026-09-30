import { useEffect, useState } from 'react';
import { label } from '../lib/format.ts';
import { edits, type SettingField, toText } from '../lib/module-settings.ts';
import type { NodeClient } from '../lib/node.ts';
import { Icon } from './Icon.tsx';

/** A module's settings on its node, edited in place. Saving restarts the module. */
export function ModuleSettings({
  client,
  module,
  onClose,
}: {
  client: NodeClient;
  module: string;
  onClose: () => void;
}) {
  const [fields, setFields] = useState<SettingField[] | null>(null);
  const [texts, setTexts] = useState<Record<string, string | boolean>>({});
  const [message, setMessage] = useState<{ ok: boolean; text: string } | null>(null);
  const [saving, setSaving] = useState(false);

  const load = async () => {
    const r = await client.invoke('node', 'settings.get', { module });
    if (r.ok) {
      setFields((r.result as { fields: SettingField[] }).fields);
      setTexts({});
    } else setMessage({ ok: false, text: r.error?.message ?? "couldn't load settings" });
  };

  // Reload only when the module changes.
  useEffect(() => {
    setFields(null);
    setMessage(null);
    void load();
  }, [module]);

  const save = async () => {
    if (!fields) return;
    const r = edits(fields, texts);
    if ('error' in r) {
      setMessage({ ok: false, text: r.error });
      return;
    }
    if (!Object.keys(r.values).length) {
      setMessage({ ok: true, text: 'Nothing changed.' });
      return;
    }
    setSaving(true);
    const res = await client.invoke('node', 'settings.set', {
      module,
      values: JSON.stringify(r.values),
    });
    setSaving(false);
    if (res.ok) {
      setMessage({ ok: true, text: 'Saved. The module is restarting with the new settings.' });
      await load();
    } else setMessage({ ok: false, text: res.error?.message ?? 'failed' });
  };

  return (
    <section className="panel" style={{ flexShrink: 0 }}>
      <div className="ph">
        <Icon name="sliders-horizontal" />
        <span className="t">Settings</span>
        <span className="muted">on this node · saving restarts the module</span>
        <span style={{ flex: 1 }} />
        <button
          type="button"
          className="btn primary"
          disabled={!fields || saving}
          onClick={() => void save()}
        >
          <Icon name={saving ? 'loader-circle' : 'save'} className={saving ? 'i spin' : 'i'} />
          Save
        </button>
        <button type="button" className="btn" onClick={onClose}>
          Close
        </button>
      </div>
      {message ? (
        <div className={`banner ${message.ok ? '' : 'bad'}`} style={{ margin: 12 }}>
          {message.text}
        </div>
      ) : null}
      {fields?.length === 0 ? <div className="empty">This module has no settings.</div> : null}
      <div style={{ display: 'grid', gap: 14, padding: 16, maxWidth: 640 }}>
        {(fields ?? []).map((f) => {
          const current = f.key in texts ? texts[f.key] : toText(f);
          const set = (v: string | boolean) => setTexts((t) => ({ ...t, [f.key]: v }));
          return (
            <label key={f.key} className="field">
              <span>
                {label(f.key)}
                {f.changed ? <span className="muted"> · changed on this PC</span> : null}
              </span>
              {f.type === 'bool' ? (
                <input
                  type="checkbox"
                  checked={Boolean(current)}
                  disabled={f.locked}
                  onChange={(e) => set(e.target.checked)}
                  style={{ width: 16, height: 16 }}
                />
              ) : f.type === 'list' || f.type === 'other' ? (
                <textarea
                  className="input mono"
                  rows={Math.min(6, Math.max(2, String(current).length / 60))}
                  value={String(current)}
                  disabled={f.locked}
                  onChange={(e) => set(e.target.value)}
                  style={{ height: 'auto', padding: 8 }}
                />
              ) : (
                <input
                  className="input"
                  type={f.secret ? 'password' : 'text'}
                  inputMode={f.type === 'int' || f.type === 'float' ? 'decimal' : undefined}
                  value={String(current)}
                  placeholder={f.secret ? 'not set' : undefined}
                  disabled={f.locked}
                  onChange={(e) => set(e.target.value)}
                />
              )}
              {f.note ? <span className="hint">{f.note}</span> : null}
              {f.locked ? (
                <span className="hint">
                  This one starts a program, so it can only be changed on the PC itself.
                </span>
              ) : null}
            </label>
          );
        })}
      </div>
    </section>
  );
}
