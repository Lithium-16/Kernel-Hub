/** Display rules for module status values, driven by key naming conventions. */

export function label(key: string): string {
  const words = key
    .replace(/_(s|mb|pct|bytes|bps|c)$/, '')
    .replace(/_/g, ' ')
    .replace(/\b(cpu|gpu|vram|ram|motd|mac|id)\b/g, (w) => w.toUpperCase());
  return words.charAt(0).toUpperCase() + words.slice(1);
}

export function duration(seconds: number): string {
  const s = Math.max(0, Math.round(seconds));
  const d = Math.floor(s / 86400);
  const h = Math.floor((s % 86400) / 3600);
  const m = Math.floor((s % 3600) / 60);
  if (d) return `${d}d ${h}h`;
  if (h) return `${h}h ${m}m`;
  if (m) return `${m}m`;
  return `${s}s`;
}

export function bytes(n: number): string {
  const units = ['B', 'KB', 'MB', 'GB', 'TB'];
  let v = n;
  let i = 0;
  while (v >= 1024 && i < units.length - 1) {
    v /= 1024;
    i++;
  }
  return `${i === 0 ? v : v.toFixed(v < 10 ? 1 : 0)} ${units[i]}`;
}

const ISO = /^\d{4}-\d\d-\d\dT\d\d:\d\d(:\d\d(\.\d+)?)?(Z|[+-]\d\d:\d\d)$/;

export function when(iso: string, now: Date = new Date()): string {
  const d = new Date(iso);
  const sameDay = d.toDateString() === now.toDateString();
  const time = d.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' });
  if (sameDay) return time;
  return `${d.toLocaleDateString([], { weekday: 'short', month: 'short', day: 'numeric' })} ${time}`;
}

/** Format one status value using its key: `_s` durations, `_mb` sizes, `bytes`, ISO times. */
export function value(key: string, v: unknown): string {
  if (v === null || v === undefined || v === '') return '—';
  if (typeof v === 'boolean') return v ? 'On' : 'Off';
  if (typeof v === 'number') {
    if (key.endsWith('_s')) return duration(v);
    if (key.endsWith('_mb')) return bytes(v * 1024 * 1024);
    if (key === 'bytes' || key.endsWith('_bytes')) return bytes(v);
    if (key.endsWith('_pct')) return `${Math.round(v)}%`;
    if (key.endsWith('_c')) return `${Math.round(v)} °C`;
    if (key.endsWith('_w')) return `${Math.round(v)} W`;
    if (key.endsWith('_bps')) return `${bytes(v)}/s`;
    return Number.isInteger(v) ? String(v) : v.toFixed(1);
  }
  if (typeof v === 'string') {
    if (ISO.test(v)) return when(v);
    // Status words read better capitalized ("Running", "In game"); names and files stay as they are.
    if (!/^[a-z][a-z_-]*$/.test(v)) return v;
    const words = v.replace(/_/g, ' ');
    return words.charAt(0).toUpperCase() + words.slice(1);
  }
  return JSON.stringify(v);
}

export type Tone = 'ok' | 'warn' | 'bad' | 'mute';

export function stateTone(state: unknown): Tone {
  switch (state) {
    case 'running':
    case 'active':
    case 'online':
    case 'in_game':
    case 'current':
      return 'ok';
    case 'starting':
    case 'stopping':
    case 'activating':
    case 'deactivating':
    case 'connecting':
    case 'joining':
    case 'available':
    case 'checking':
    case 'downloading':
    case 'installing':
      return 'warn';
    case 'crashed':
    case 'failed':
    case 'unauthorized':
    case 'disconnected':
      return 'bad';
    default:
      return 'mute';
  }
}

/** Keys whose value speaks for itself in a summary ("world.tar.gz · 2.0 KB"). */
const SELF_EVIDENT = new Set(['file', 'name', 'message', 'command', 'bytes']);

function selfEvident(key: string): boolean {
  return SELF_EVIDENT.has(key) || /_(s|mb|bytes|bps|c|pct)$/.test(key);
}

/** Short summary of an action result for the status bar. */
export function summary(result: unknown): string {
  if (result === null || result === undefined) return 'done';
  if (typeof result !== 'object') return String(result);
  if (Array.isArray(result)) return `${result.length} items`;
  const parts: string[] = [];
  for (const [k, v] of Object.entries(result as Record<string, unknown>)) {
    if (v === null || typeof v === 'object') continue;
    parts.push(selfEvident(k) ? value(k, v) : `${label(k).toLowerCase()} ${value(k, v)}`);
    if (parts.length === 3) break;
  }
  return parts.join(' · ') || 'done';
}
