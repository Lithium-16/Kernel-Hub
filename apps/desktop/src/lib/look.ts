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

export interface Look {
  theme: Theme;
  accent: string;
}

const KEY = 'kernel.look';
export const DEFAULT_LOOK: Look = { theme: 'dim', accent: '#2563eb' };

export function loadLook(): Look {
  try {
    const raw = JSON.parse(localStorage.getItem(KEY) ?? '{}') as Partial<Look>;
    return {
      theme: THEMES.some((t) => t.id === raw.theme) ? (raw.theme as Theme) : DEFAULT_LOOK.theme,
      accent: ACCENTS.some((a) => a.color === raw.accent)
        ? (raw.accent as string)
        : DEFAULT_LOOK.accent,
    };
  } catch {
    return DEFAULT_LOOK;
  }
}

export function applyLook(look: Look): void {
  const root = document.documentElement;
  root.dataset.theme = look.theme;
  root.style.setProperty('--accent', look.accent);
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
