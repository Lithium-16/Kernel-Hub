/** Split a module's status object into tiles, lists, tables and detail rows, by value shape. */

export interface Tile {
  key: string;
  value: unknown;
  max?: unknown;
}

export interface Section {
  key: string;
  kind: 'list' | 'table' | 'map';
  value: unknown;
}

const HIDDEN = new Set(['error', 'unit_state']);

function shortScalar(v: unknown): boolean {
  if (v === null || typeof v === 'number' || typeof v === 'boolean') return true;
  // Longer text (a version string, a MOTD) would be cut off in a tile; it goes to Details.
  return typeof v === 'string' && v.length <= 10;
}

export function layoutStatus(status: Record<string, unknown> | null | undefined): {
  tiles: Tile[];
  details: [string, unknown][];
  sections: Section[];
} {
  const tiles: Tile[] = [];
  const details: [string, unknown][] = [];
  const sections: Section[] = [];
  if (!status) return { tiles, details, sections };
  const keys = Object.keys(status);
  for (const key of keys) {
    const v = status[key];
    if (HIDDEN.has(key)) continue;
    // `players_max` is shown as the "/ 20" of `players_online`.
    if (key.endsWith('_max') && keys.includes(key.replace(/_max$/, '_online'))) continue;
    if (Array.isArray(v)) {
      const objects =
        v.length > 0 && v.every((x) => x !== null && typeof x === 'object' && !Array.isArray(x));
      sections.push({ key, kind: objects ? 'table' : 'list', value: v });
    } else if (v !== null && typeof v === 'object') {
      sections.push({ key, kind: 'map', value: v });
    } else if (key === 'state' || shortScalar(v)) {
      const max = key.endsWith('_online') ? status[key.replace(/_online$/, '_max')] : undefined;
      tiles.push(max === undefined ? { key, value: v } : { key, value: v, max });
    } else {
      details.push([key, v]);
    }
  }
  // Status first, the way the design leads with it.
  tiles.sort((a, b) => Number(b.key === 'state') - Number(a.key === 'state'));
  return { tiles, details, sections };
}
