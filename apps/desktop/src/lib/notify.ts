/** Windows notifications for node events, at the level the person picked in Settings. */
import type { KernelEvent } from './node.ts';
import { inTauri } from './window.ts';

export type NotifyLevel = 'off' | 'error' | 'warn' | 'info';
export const NOTIFY_LEVELS: NotifyLevel[] = ['off', 'error', 'warn', 'info'];
const KEY = 'kernel.notify';
const RANK = { info: 0, warn: 1, error: 2 } as const;

export function loadNotifyLevel(): NotifyLevel {
  try {
    const v = localStorage.getItem(KEY);
    return NOTIFY_LEVELS.includes(v as NotifyLevel) ? (v as NotifyLevel) : 'warn';
  } catch {
    return 'warn';
  }
}

export function saveNotifyLevel(level: NotifyLevel): void {
  try {
    localStorage.setItem(KEY, level);
  } catch {
    // Private mode: the default applies.
  }
}

export function shouldNotify(level: NotifyLevel, e: Pick<KernelEvent, 'level'>): boolean {
  return level !== 'off' && RANK[e.level] >= RANK[level];
}

export function title(e: KernelEvent, moduleName: string): string {
  const mark = e.level === 'error' ? '🔴' : e.level === 'warn' ? '🟡' : '🟢';
  return `${mark} ${moduleName}`;
}

let allowed: boolean | null = null;

/** Show one notification. Asks for permission the first time; quietly does nothing if refused. */
export async function show(heading: string, body: string): Promise<void> {
  if (inTauri) {
    const n = await import('@tauri-apps/plugin-notification');
    if (allowed === null) {
      allowed = (await n.isPermissionGranted()) || (await n.requestPermission()) === 'granted';
    }
    if (allowed) n.sendNotification({ title: heading, body });
    return;
  }
  if (typeof Notification === 'undefined') return;
  if (Notification.permission === 'default') await Notification.requestPermission();
  if (Notification.permission === 'granted') new Notification(heading, { body });
}
