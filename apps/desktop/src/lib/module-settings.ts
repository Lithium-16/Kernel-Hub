/** Module settings as the node's `settings.get` describes them, and turning edits back into values. */

export interface SettingField {
  key: string;
  type: 'bool' | 'int' | 'float' | 'string' | 'list' | 'other';
  value: unknown;
  default: unknown;
  changed: boolean;
  note?: string | null;
  secret: boolean;
  locked: boolean;
}

export const HIDDEN = '(hidden)';

/** What an input shows for a value: lists as JSON, the rest as text. */
export function toText(f: SettingField, v: unknown = f.value): string | boolean {
  if (f.type === 'bool') return Boolean(v);
  if (f.type === 'list' || f.type === 'other') return JSON.stringify(v);
  return v === null || v === undefined ? '' : String(v);
}

/** An edited input back to a typed value, or an error for the person. */
export function fromText(
  f: SettingField,
  text: string | boolean,
): { value: unknown } | { error: string } {
  if (f.type === 'bool') return { value: Boolean(text) };
  const s = String(text);
  if (f.type === 'int') {
    return /^-?\d+$/.test(s.trim())
      ? { value: Number(s) }
      : { error: `${f.key} must be a whole number` };
  }
  if (f.type === 'float') {
    const n = Number(s);
    return s.trim() !== '' && Number.isFinite(n)
      ? { value: n }
      : { error: `${f.key} must be a number` };
  }
  if (f.type === 'list' || f.type === 'other') {
    try {
      const v = JSON.parse(s) as unknown;
      if (f.type === 'list' && !Array.isArray(v))
        return { error: `${f.key} must be a list, like ["a", "b"]` };
      return { value: v };
    } catch {
      return { error: `${f.key} must be a list, like ["a", "b"]` };
    }
  }
  return { value: s };
}

/** The values to send: only fields the person edited. */
export function edits(
  fields: SettingField[],
  texts: Record<string, string | boolean>,
): { values: Record<string, unknown> } | { error: string } {
  const values: Record<string, unknown> = {};
  for (const f of fields) {
    if (!(f.key in texts) || f.locked) continue;
    const text = texts[f.key] as string | boolean;
    if (text === toText(f)) continue;
    const r = fromText(f, text);
    if ('error' in r) return r;
    values[f.key] = r.value;
  }
  return { values };
}
