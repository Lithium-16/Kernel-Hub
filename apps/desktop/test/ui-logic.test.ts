import { describe, expect, it } from 'vitest';
import { matches } from '../src/components/Palette.tsx';
import { type Action, coerce, defaults, needsConfirm } from '../src/lib/actions.ts';
import { bytes, duration, label, stateTone, summary, value } from '../src/lib/format.ts';
import { layoutStatus } from '../src/lib/layout.ts';
import { edits, fromText, HIDDEN, type SettingField, toText } from '../src/lib/module-settings.ts';
import { shouldNotify } from '../src/lib/notify.ts';

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
    expect(label('cpu_pct')).toBe('CPU');
    expect(label('vram_mb')).toBe('VRAM');
    expect(label('gpu_temp_c')).toBe('GPU temp');
    expect(value('gpu_temp_c', 64.4)).toBe('64 °C');
    expect(value('net_down_bps', 1536)).toBe('1.5 KB/s');
    expect(value('used_pct', 71.2)).toBe('71%');
  });

  it('maps states to tones', () => {
    expect(stateTone('running')).toBe('ok');
    expect(stateTone('starting')).toBe('warn');
    expect(stateTone('crashed')).toBe('bad');
    expect(stateTone('stopped')).toBe('mute');
    expect(stateTone('in_game')).toBe('ok');
    expect(stateTone('disconnected')).toBe('bad');
    expect(value('state', 'in_game')).toBe('In game');
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

describe('PC monitor status', () => {
  it('pairs used with total, and keeps disks as a table', () => {
    const { tiles, sections, details } = layoutStatus({
      cpu_pct: 12.5,
      cores: 8,
      memory_used_mb: 10240,
      memory_total_mb: 65536,
      gpu: 'NVIDIA GeForce GTX 1080 Ti',
      gpu_pct: 37,
      gpu_temp_c: 64,
      gpu_power_w: 231,
      gpu_power_limit_w: 250,
      vram_used_mb: 3072,
      vram_total_mb: 11264,
      net_down_bps: null,
      uptime_s: 3600,
      disks: [{ disk: 'C:\\', used_pct: 71.2, free_bytes: 1, total_bytes: 2 }],
    });
    const byKey = Object.fromEntries(tiles.map((t) => [t.key, t]));
    expect(Object.keys(byKey)).toEqual([
      'cpu_pct',
      'cores',
      'memory_used_mb',
      'gpu_pct',
      'gpu_temp_c',
      'gpu_power_w',
      'vram_used_mb',
      'net_down_bps',
      'uptime_s',
    ]);
    expect(label(byKey.memory_used_mb?.name ?? '')).toBe('Memory');
    expect(byKey.memory_used_mb?.max).toBe(65536);
    expect(label(byKey.vram_used_mb?.name ?? '')).toBe('VRAM');
    expect(label(byKey.gpu_power_w?.name ?? '')).toBe('GPU power');
    expect(byKey.gpu_power_w?.max).toBe(250);
    expect(value('gpu_power_w', 231.4)).toBe('231 W');
    expect(sections.map((s) => [s.key, s.kind])).toEqual([['disks', 'table']]);
    expect(details.map(([k]) => k)).toEqual(['gpu']);
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

describe('module settings form', () => {
  const field = (over: Partial<SettingField>): SettingField => ({
    key: 'poll_s',
    type: 'float',
    value: 2,
    default: 2,
    changed: false,
    secret: false,
    locked: false,
    ...over,
  });

  it('sends only what was edited, typed', () => {
    const fields = [
      field({}),
      field({ key: 'auto', type: 'bool', value: true, default: true }),
      field({ key: 'apps', type: 'list', value: ['a'], default: ['a'] }),
      field({ key: 'start_command', type: 'list', value: [], default: [], locked: true }),
      field({ key: 'hf_token', type: 'string', value: HIDDEN, default: '', secret: true }),
    ];
    expect(edits(fields, { poll_s: '2', auto: true })).toEqual({ values: {} });
    expect(
      edits(fields, {
        poll_s: '3.5',
        auto: false,
        apps: '["a", "b"]',
        start_command: '["calc.exe"]',
      }),
    ).toEqual({ values: { poll_s: 3.5, auto: false, apps: ['a', 'b'] } });
    expect(edits(fields, { hf_token: HIDDEN })).toEqual({ values: {} });
    expect(edits(fields, { hf_token: 'hf_new' })).toEqual({ values: { hf_token: 'hf_new' } });
  });

  it('explains bad input', () => {
    expect(fromText(field({ type: 'int', key: 'fps_cap' }), '1.5')).toEqual({
      error: 'fps_cap must be a whole number',
    });
    expect(fromText(field({}), 'fast')).toEqual({ error: 'poll_s must be a number' });
    expect(fromText(field({ type: 'list', key: 'apps' }), '{"a": 1}')).toHaveProperty('error');
    expect(toText(field({ type: 'list', value: ['x'] }))).toBe('["x"]');
  });
});

describe('notifications', () => {
  it('filters by level', () => {
    expect(shouldNotify('warn', { level: 'error' })).toBe(true);
    expect(shouldNotify('warn', { level: 'warn' })).toBe(true);
    expect(shouldNotify('warn', { level: 'info' })).toBe(false);
    expect(shouldNotify('info', { level: 'info' })).toBe(true);
    expect(shouldNotify('off', { level: 'error' })).toBe(false);
  });
});
