import { readdirSync, readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { describe, expect, it } from 'vitest';
import { z } from 'zod';
import { Message } from '../src/index.ts';

const dir = fileURLToPath(new URL('../fixtures/', import.meta.url));
const fixtures = readdirSync(dir).filter((f) => f.endsWith('.json'));

describe('fixtures', () => {
  it('covers every message type', () => {
    const types = new Set(
      fixtures.map((f) => (JSON.parse(readFileSync(dir + f, 'utf8')) as { type: string }).type),
    );
    const all = Message.options.map((o) => o.shape.type.value);
    expect([...types].sort()).toEqual([...all].sort());
  });

  it.each(fixtures)('%s is a valid message', (f) => {
    const raw = JSON.parse(readFileSync(dir + f, 'utf8'));
    const parsed = Message.parse(raw);
    expect(parsed).toEqual(raw);
    expect(f.startsWith(parsed.type)).toBe(true);
  });
});

describe('validation', () => {
  const base = { v: 1, id: 'x', ts: '2026-09-24T20:11:02Z' };

  it('rejects an unknown message type', () => {
    expect(Message.safeParse({ ...base, type: 'nope', body: {} }).success).toBe(false);
  });

  it('rejects the wrong protocol version', () => {
    const r = Message.safeParse({ ...base, v: 2, type: 'catalog.get', body: {} });
    expect(r.success).toBe(false);
  });

  it('rejects an empty token', () => {
    const r = Message.safeParse({
      ...base,
      type: 'hello',
      body: { client: { name: 'a', version: '1' }, token: '' },
    });
    expect(r.success).toBe(false);
  });

  it('rejects malformed action ids', () => {
    const catalog = {
      ...base,
      type: 'catalog',
      body: {
        modules: [
          {
            id: 'hello',
            name: 'Hello',
            icon: 'hand',
            version: '0.1.0',
            state: 'running',
            status: null,
            actions: [{ id: 'NoDots', label: 'x', ai: 'safe', params: {} }],
          },
        ],
      },
    };
    expect(Message.safeParse(catalog).success).toBe(false);
  });

  it('caps activity query limits', () => {
    const r = Message.safeParse({ ...base, type: 'activity.query', body: { limit: 5000 } });
    expect(r.success).toBe(false);
  });
});

describe('json schema', () => {
  it('exports a schema that lists every message', () => {
    const schema = z.toJSONSchema(Message) as { oneOf?: unknown[]; anyOf?: unknown[] };
    const variants = schema.oneOf ?? schema.anyOf ?? [];
    expect(variants).toHaveLength(Message.options.length);
  });
});
