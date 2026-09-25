import { describe, expect, it } from 'vitest';
import { matches } from '../src/components/Palette.tsx';
import { type Action, coerce, defaults, needsConfirm } from '../src/lib/actions.ts';
import { bytes, duration, label, stateTone, summary, value } from '../src/lib/format.ts';
import { layoutStatus } from '../src/lib/layout.ts';

describe('format', () => {
  it('formats values by key convention', () => {
    expect(value('uptime_s', 4320)).toBe('1h 12m');
    expect(value('memory_mb', 10547)).toBe('10 GB');
    expect(value('memory_mb', 512)).toBe('512 MB');
    expect(value('bytes', 1536)).toBe('1.5 KB');
    expect(value('keepalive', true)).toBe('On');
    expect(value('players_online', null)).toBe('—');
    expect(value('state', 'running')).toBe('Running');
    expect(duration(59)).toBe('59s');
    expect(duration(90061)).toBe('1d 1h');
    expect(bytes(0)).toBe('0 B');
    expect(label('memory_mb')).toBe('Memory');
    expect(label('last_backup')).toBe('Last backup');
  });

  it('maps states to tones', () => {
    expect(stateTone('running')).toBe('ok');
    expect(stateTone('starting')).toBe('warn');
    expect(stateTone('crashed')).toBe('bad');
    expect(stateTone('stopped')).toBe('mute');
  });

  it('summarizes results', () => {
    expect(summary({ file: 'world-20260920T030000Z.tar.gz', bytes: 2048, deleted: [] })).toBe(
      'world-20260920T030000Z.tar.gz · 2.0 KB',
    );
    expect(summary({ count: 29, mods: [] })).toBe('count 29');
    expect(value('file', 'world.tar.gz')).toBe('world.tar.gz');
    expect(summary(undefined)).toBe('done');
    expect(summary([1, 2])).toBe('2 items');
  });
});

describe('layoutStatus', () => {
  it('turns Minecraft status into tiles, sections and details', () => {
    const { tiles, sections, details } = layoutStatus({
      players_online: 1,
      state: 'running',
      unit_state: 'active',
      players_max: 20,
      players: ['Steve'],
      version: 'NeoForge 1.21.1',
      motd: 'A very long message of the day for this server',
      services: { playit: 'active' },
      backups: [{ file: 'w.tar.gz', bytes: 1, at: '2026-09-20T03:00:00Z' }],
      error: 'x',
    });
    expect(tiles.map((t) => t.key)).toEqual(['state', 'players_online']);
    expect(tiles[1]?.max).toBe(20);
    expect(sections.map((s) => [s.key, s.kind])).toEqual([
      ['players', 'list'],
      ['services', 'map'],
      ['backups', 'table'],
    ]);
    expect(details.map(([k]) => k)).toEqual(['version', 'motd']);
  });
});

describe('actions', () => {
  const stop: Action = {
    id: 'server.stop',
    label: 'Stop server',
    ai: 'confirm',
    params: { delay_min: { type: 'int', default: 0, min: 0, max: 30 } },
  };
  const say: Action = {
    id: 'server.say',
    label: 'Say',
    ai: 'safe',
    params: { message: { type: 'string' } },
  };

  it('fills defaults and types values', () => {
    expect(defaults(stop)).toEqual({ delay_min: '0' });
    expect(coerce(stop, { delay_min: '5' })).toEqual({ params: { delay_min: 5 } });
    expect(coerce(stop, { delay_min: '2.5' })).toEqual({
      error: 'delay_min must be a whole number',
    });
    expect(coerce(stop, { delay_min: '' })).toEqual({ params: {} });
    expect(coerce(say, { message: ' ' })).toEqual({ error: 'message is required' });
    expect(coerce(say, { message: ' hi ' })).toEqual({ params: { message: 'hi' } });
  });

  it('asks before anything the assistant would need approval for', () => {
    expect(needsConfirm(stop)).toBe(true);
    expect(needsConfirm(say)).toBe(false);
  });
});

describe('palette search', () => {
  it('matches every word, in any order', () => {
    expect(matches('Minecraft: Back up world', 'back mine')).toBe(true);
    expect(matches('Minecraft: Back up world', 'restore')).toBe(false);
    expect(matches('Anything', '')).toBe(true);
  });
});
