import type { ActionSpec, ParamSpec } from '@kernel/protocol';
import type { z } from 'zod';

export type Action = z.infer<typeof ActionSpec>;
export type Param = z.infer<typeof ParamSpec>;

export function hasParams(a: Action): boolean {
  return Object.keys(a.params).length > 0;
}

/** Actions that can't run without input (a player name, a message) live in the More menu. */
export function needsInput(a: Action): boolean {
  return Object.values(a.params).some((p) => p.default === undefined && p.type !== 'bool');
}

/** Actions a human should confirm before running, matching the assistant's approval tiers. */
export function needsConfirm(a: Action): boolean {
  return a.ai !== 'safe';
}

export function defaults(a: Action): Record<string, string | boolean> {
  const out: Record<string, string | boolean> = {};
  for (const [name, p] of Object.entries(a.params)) {
    if (p.type === 'bool') out[name] = p.default === true;
    else if (p.default !== undefined) out[name] = String(p.default);
    else if (p.type === 'enum') out[name] = p.options?.[0] ?? '';
    else out[name] = '';
  }
  return out;
}

/** Turn form strings into typed params. Returns an error message instead when a value is bad. */
export function coerce(
  a: Action,
  raw: Record<string, string | boolean>,
): { params: Record<string, unknown> } | { error: string } {
  const params: Record<string, unknown> = {};
  for (const [name, p] of Object.entries(a.params)) {
    const v = raw[name];
    const required = p.default === undefined;
    if (p.type === 'bool') {
      params[name] = v === true;
      continue;
    }
    const text = typeof v === 'string' ? v.trim() : '';
    if (text === '') {
      if (required) return { error: `${name} is required` };
      continue;
    }
    if (p.type === 'int' || p.type === 'float') {
      const n = Number(text);
      if (!Number.isFinite(n) || (p.type === 'int' && !Number.isInteger(n))) {
        return { error: `${name} must be ${p.type === 'int' ? 'a whole number' : 'a number'}` };
      }
      params[name] = n;
    } else {
      params[name] = text;
    }
  }
  return { params };
}
