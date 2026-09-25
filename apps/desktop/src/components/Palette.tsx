import { useEffect, useMemo, useRef, useState } from 'react';
import { type Action, coerce, defaults, hasParams, needsConfirm } from '../lib/actions.ts';
import { summary } from '../lib/format.ts';
import type { ActionResult, Module, NodeClient } from '../lib/node.ts';
import { Icon } from './Icon.tsx';
import type { View } from './Sidebar.tsx';

type Item =
  | { kind: 'view'; key: string; label: string; icon: string; view: View }
  | { kind: 'action'; key: string; label: string; icon: string; module: Module; action: Action };

interface Props {
  client: NodeClient;
  modules: Module[];
  /** In-app palette: also offers navigation. The Alt+Space window only runs actions. */
  onNavigate?: (v: View) => void;
  onClose: () => void;
}

function items(modules: Module[], withViews: boolean): Item[] {
  const out: Item[] = [];
  for (const m of modules) {
    if (m.state !== 'running') continue;
    for (const a of m.actions) {
      if (a.id === 'console.tail') continue;
      out.push({
        kind: 'action',
        key: `${m.id}/${a.id}`,
        label: `${m.name}: ${a.label}`,
        icon: a.icon ?? m.icon,
        module: m,
        action: a,
      });
    }
  }
  if (withViews) {
    for (const m of modules) {
      out.push({
        kind: 'view',
        key: `view/${m.id}`,
        label: `Open ${m.name}`,
        icon: m.icon,
        view: { kind: 'module', id: m.id },
      });
    }
    out.push({
      kind: 'view',
      key: 'view/activity',
      label: 'Open Activity',
      icon: 'list-clock',
      view: { kind: 'activity' },
    });
    out.push({
      kind: 'view',
      key: 'view/settings',
      label: 'Open Settings',
      icon: 'settings-2',
      view: { kind: 'settings' },
    });
  }
  return out;
}

export function matches(label: string, query: string): boolean {
  const l = label.toLowerCase();
  return query
    .toLowerCase()
    .split(/\s+/)
    .filter(Boolean)
    .every((word) => l.includes(word));
}

export function Palette({ client, modules, onNavigate, onClose }: Props) {
  const [query, setQuery] = useState('');
  const [sel, setSel] = useState(0);
  const [armed, setArmed] = useState<string | null>(null);
  const [paramsFor, setParamsFor] = useState<Item | null>(null);
  const [values, setValues] = useState<Record<string, string | boolean>>({});
  const [running, setRunning] = useState<string | null>(null);
  const [result, setResult] = useState<{ label: string; res: ActionResult } | null>(null);
  const input = useRef<HTMLInputElement>(null);

  const all = useMemo(() => items(modules, Boolean(onNavigate)), [modules, onNavigate]);
  const list = useMemo(() => all.filter((i) => matches(i.label, query)).slice(0, 50), [all, query]);
  const current = list[Math.min(sel, list.length - 1)];

  useEffect(() => {
    input.current?.focus();
  }, []);

  const run = async (item: Extract<Item, { kind: 'action' }>, params: Record<string, unknown>) => {
    setRunning(item.key);
    setArmed(null);
    const res = await client.invoke(item.module.id, item.action.id, params);
    setRunning(null);
    setResult({ label: item.label, res });
    if (res.ok) {
      setParamsFor(null);
      setQuery('');
      input.current?.focus();
    }
  };

  const choose = (item: Item | undefined) => {
    if (!item || running) return;
    if (item.kind === 'view') {
      onNavigate?.(item.view);
      onClose();
      return;
    }
    if (hasParams(item.action)) {
      setParamsFor(item);
      setValues(defaults(item.action));
      return;
    }
    if (needsConfirm(item.action) && armed !== item.key) {
      setArmed(item.key);
      return;
    }
    void run(item, {});
  };

  const submitParams = () => {
    if (paramsFor?.kind !== 'action') return;
    const r = coerce(paramsFor.action, values);
    if ('error' in r)
      setResult({
        label: paramsFor.label,
        res: { ok: false, error: { code: 'invalid_params', message: r.error } },
      });
    else void run(paramsFor, r.params);
  };

  const onKey = (e: React.KeyboardEvent) => {
    if (e.key === 'Escape') {
      e.preventDefault();
      if (paramsFor) setParamsFor(null);
      else onClose();
    } else if (paramsFor) {
      if (e.key === 'Enter') {
        e.preventDefault();
        submitParams();
      }
    } else if (e.key === 'ArrowDown') {
      e.preventDefault();
      setSel((s) => Math.min(s + 1, list.length - 1));
      setArmed(null);
    } else if (e.key === 'ArrowUp') {
      e.preventDefault();
      setSel((s) => Math.max(s - 1, 0));
      setArmed(null);
    } else if (e.key === 'Enter') {
      e.preventDefault();
      choose(current);
    }
  };

  let foot: React.ReactNode = (
    <>
      <span className="grow">{list.length ? `${list.length} results` : 'No matches'}</span>
      <span className="kbd">Enter</span> run <span className="kbd">Esc</span> close
    </>
  );
  if (running) {
    foot = (
      <>
        <Icon name="loader-circle" className="i spin" />
        <span className="grow">Running…</span>
      </>
    );
  } else if (armed && current?.key === armed) {
    foot = (
      <span className="grow c-warn">
        {current.label} needs confirming. Press Enter again to run it.
      </span>
    );
  } else if (result) {
    foot = (
      <>
        <span className={`sq ${result.res.ok ? 'ok' : 'bad'}`} />
        <span className="grow selectable">
          {result.label}: {result.res.ok ? summary(result.res.result) : result.res.error?.message}
        </span>
      </>
    );
  }

  return (
    <div className="palette" role="dialog" aria-label="Command palette" onKeyDown={onKey}>
      <div className="palette-input">
        <Icon name="search" size={16} />
        {paramsFor?.kind === 'action' ? (
          <>
            <span style={{ whiteSpace: 'nowrap', fontSize: 15 }}>{paramsFor.label}</span>
            {Object.entries(paramsFor.action.params).map(([name, p], i) =>
              p.type === 'bool' ? (
                <label
                  key={name}
                  className="muted"
                  style={{ display: 'flex', gap: 6, alignItems: 'center' }}
                >
                  <input
                    type="checkbox"
                    checked={values[name] === true}
                    onChange={(e) => setValues((v) => ({ ...v, [name]: e.target.checked }))}
                  />
                  {name}
                </label>
              ) : (
                <input
                  key={name}
                  placeholder={p.description ?? name}
                  aria-label={p.description ?? name}
                  value={String(values[name] ?? '')}
                  onChange={(e) => setValues((v) => ({ ...v, [name]: e.target.value }))}
                  autoFocus={i === 0}
                />
              ),
            )}
          </>
        ) : (
          <input
            ref={input}
            placeholder="Run anything…"
            aria-label="Search actions"
            value={query}
            onChange={(e) => {
              setQuery(e.target.value);
              setSel(0);
              setArmed(null);
            }}
          />
        )}
      </div>
      {paramsFor ? null : (
        <div className="palette-list" role="listbox">
          {list.map((item, i) => (
            <div
              key={item.key}
              role="option"
              tabIndex={-1}
              aria-selected={item === current}
              className={`palette-item ${item === current ? 'on' : ''}`}
              onMouseMove={() => setSel(i)}
              onClick={() => choose(item)}
              onKeyDown={() => {}}
            >
              <Icon name={item.icon} />
              <span className="grow">{item.label}</span>
              {item.kind === 'action' && needsConfirm(item.action) ? (
                <span className="muted">confirm</span>
              ) : null}
              {item.kind === 'action' && hasParams(item.action) ? (
                <Icon name="chevron-right" size={12} />
              ) : null}
            </div>
          ))}
          {list.length === 0 ? (
            <div className="palette-item muted">
              {modules.length
                ? 'Nothing matches'
                : 'Not connected to a node. Open Kernel to set it up.'}
            </div>
          ) : null}
        </div>
      )}
      <div className="palette-foot">{foot}</div>
    </div>
  );
}
