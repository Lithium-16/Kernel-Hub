/** Where the node is. Phase 0 keeps this in local storage; pairing (phase 2) replaces the token. */

export interface NodeSettings {
  url: string;
  token: string;
}

const KEY = 'kernel.node';
export const DEFAULT_SETTINGS: NodeSettings = { url: 'ws://pluto:47800/ws', token: '' };

export function loadSettings(): NodeSettings {
  try {
    const raw = localStorage.getItem(KEY);
    if (!raw) return DEFAULT_SETTINGS;
    const parsed = JSON.parse(raw) as Partial<NodeSettings>;
    return {
      url: typeof parsed.url === 'string' ? parsed.url : DEFAULT_SETTINGS.url,
      token: typeof parsed.token === 'string' ? parsed.token : '',
    };
  } catch {
    return DEFAULT_SETTINGS;
  }
}

export function saveSettings(s: NodeSettings): void {
  try {
    localStorage.setItem(KEY, JSON.stringify(s));
  } catch {
    // Private mode or blocked storage: the settings still apply for this session.
  }
}

export function validUrl(url: string): boolean {
  try {
    const u = new URL(url);
    return u.protocol === 'ws:' || u.protocol === 'wss:';
  } catch {
    return false;
  }
}
