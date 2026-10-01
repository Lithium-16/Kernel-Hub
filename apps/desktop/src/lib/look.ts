/** How the app looks: theme and accent color, kept per PC in local storage. */

export type Theme = 'dim' | 'dark' | 'warm';
export const THEMES: { id: Theme; name: string; hint: string }[] = [
  { id: 'dim', name: 'Dim', hint: 'soft gray text on charcoal' },
  { id: 'dark', name: 'Dark', hint: 'black with bright text' },
  { id: 'warm', name: 'Warm', hint: 'amber grays for late at night' },
];

export const ACCENTS: { name: string; color: string }[] = [
  { name: 'Blue', color: '#2563eb' },
  { name: 'Purple', color: '#7c3aed' },
  { name: 'Pink', color: '#db2777' },
  { name: 'Green', color: '#16a34a' },
  { name: 'Orange', color: '#ea580c' },
  { name: 'Teal', color: '#0d9488' },
];

export type TextSize = 'small' | 'normal' | 'large';
export const TEXT_SIZES: { id: TextSize; name: string; zoom: number }[] = [
  { id: 'small', name: 'Small', zoom: 0.92 },
  { id: 'normal', name: 'Normal', zoom: 1 },
  { id: 'large', name: 'Large', zoom: 1.12 },
];

export interface Look {
  theme: Theme;
  accent: string;
  textSize: TextSize;
  /** Each module's icon and page in its own color. */
  moduleColors: boolean;
}

/** Module colors: the known ones by what they are, anything else from a fixed set by name. */
const TINTS: Record<string, string> = {
  minecraft: '#5fbf6a',
  roblox: '#e0665c',
  comfyui: '#a77bf0',
  vram: '#e8954a',
  'pc-monitor': '#4fb8c9',
  node: '#8b9bb4',
};
const SPARE = ['#d9779f', '#c9b458', '#6f9be0', '#5fc4a0'];

export function moduleTint(id: string): string {
  const known = TINTS[id];
  if (known) return known;
  let h = 0;
  for (const c of id) h = (h * 31 + c.charCodeAt(0)) >>> 0;
  return SPARE[h % SPARE.length] as string;
}

const KEY = 'kernel.look';
export const DEFAULT_LOOK: Look = {
  theme: 'dim',
  accent: '#2563eb',
  textSize: 'normal',
  moduleColors: true,
};

export function loadLook(): Look {
  try {
    const raw = JSON.parse(localStorage.getItem(KEY) ?? '{}') as Partial<Look>;
    return {
      theme: THEMES.some((t) => t.id === raw.theme) ? (raw.theme as Theme) : DEFAULT_LOOK.theme,
      accent: ACCENTS.some((a) => a.color === raw.accent)
        ? (raw.accent as string)
        : DEFAULT_LOOK.accent,
      textSize: TEXT_SIZES.some((t) => t.id === raw.textSize)
        ? (raw.textSize as TextSize)
        : DEFAULT_LOOK.textSize,
      moduleColors:
        typeof raw.moduleColors === 'boolean' ? raw.moduleColors : DEFAULT_LOOK.moduleColors,
    };
  } catch {
    return DEFAULT_LOOK;
  }
}

export function applyLook(look: Look): void {
  const root = document.documentElement;
  root.dataset.theme = look.theme;
  root.dataset.tint = look.moduleColors ? 'on' : 'off';
  root.style.setProperty('--accent', look.accent);
  // Chromium's zoom scales every size at once, so nothing gets out of proportion.
  const zoom = TEXT_SIZES.find((t) => t.id === look.textSize)?.zoom ?? 1;
  (root.style as CSSStyleDeclaration & { zoom: string }).zoom = String(zoom);
}

export function saveLook(look: Look): void {
  try {
    localStorage.setItem(KEY, JSON.stringify(look));
  } catch {
    // Private mode: it still applies until the app closes.
  }
  applyLook(look);
}

/** Other windows (the palette) follow changes made in Settings. */
export function followLook(): void {
  window.addEventListener('storage', (e) => {
    if (e.key === KEY) applyLook(loadLook());
  });
}
