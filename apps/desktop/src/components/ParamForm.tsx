import { useState } from 'react';
import { type Action, coerce, defaults } from '../lib/actions.ts';
import { Icon } from './Icon.tsx';

interface Props {
  action: Action;
  busy?: boolean;
  onRun: (params: Record<string, unknown>) => void;
  onCancel: () => void;
}

/** One row of inputs for an action's parameters, typed from its spec. */
export function ParamForm({ action, busy, onRun, onCancel }: Props) {
  const [values, setValues] = useState(() => defaults(action));
  const [error, setError] = useState<string | null>(null);
  const set = (name: string, v: string | boolean) => setValues((old) => ({ ...old, [name]: v }));

  const submit = () => {
    const r = coerce(action, values);
    if ('error' in r) setError(r.error);
    else {
      setError(null);
      onRun(r.params);
    }
  };

  return (
    <form
      className="paramrow"
      onSubmit={(e) => {
        e.preventDefault();
        submit();
      }}
      onKeyDown={(e) => {
        if (e.key === 'Escape') onCancel();
      }}
    >
      <Icon name={action.icon ?? 'play'} />
      <span style={{ fontWeight: 600 }}>{action.label}</span>
      {Object.entries(action.params).map(([name, p], i) => {
        const v = values[name];
        const common = { 'aria-label': p.description ?? name, title: p.description ?? name };
        if (p.type === 'bool') {
          return (
            <label
              key={name}
              className="muted"
              style={{ display: 'flex', gap: 6, alignItems: 'center' }}
            >
              <input
                type="checkbox"
                checked={v === true}
                onChange={(e) => set(name, e.target.checked)}
                {...common}
              />
              {name}
            </label>
          );
        }
        if (p.type === 'enum') {
          return (
            <select
              key={name}
              className="input"
              value={String(v)}
              onChange={(e) => set(name, e.target.value)}
              {...common}
            >
              {(p.options ?? []).map((o) => (
                <option key={o} value={o}>
                  {o}
                </option>
              ))}
            </select>
          );
        }
        const numeric = p.type === 'int' || p.type === 'float';
        return (
          <input
            key={name}
            className="input"
            style={{ flex: numeric ? '0 0 110px' : '1 1 220px' }}
            type={numeric ? 'number' : 'text'}
            min={p.min}
            max={p.max}
            step={p.type === 'float' ? 'any' : 1}
            placeholder={p.description ?? name}
            value={String(v ?? '')}
            onChange={(e) => set(name, e.target.value)}
            autoFocus={i === 0}
            {...common}
          />
        );
      })}
      {error ? <span className="c-bad">{error}</span> : null}
      <span style={{ flex: 1 }} />
      <button type="button" className="act" onClick={onCancel}>
        Cancel
      </button>
      <button
        type="submit"
        className={`btn ${action.ai === 'safe' ? 'primary' : 'confirm'}`}
        disabled={busy}
      >
        {busy ? <Icon name="loader-circle" className="i spin" /> : null}
        Run
      </button>
    </form>
  );
}
