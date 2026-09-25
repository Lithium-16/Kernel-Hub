/** Split a module's status object into tiles, lists, tables and detail rows, by value shape. */

export interface Tile {
  key: string;
  /** What the tile is called: `players_online` and `memory_used_mb` read as Players, Memory. */
  name: string;
  value: unknown;
  max?: unknown;
}

/** Pairs shown as one tile, "value / max": players_online + players_max, memory_used_mb + memory_total_mb. */
const PAIRS: {
  re: RegExp;
  max: (m: RegExpMatchArray) => string;
  name: (m: RegExpMatchArray) => string;
}[] = [
  { re: /^(.+)_online$/, max: (m) => `${m[1]}_max`, name: (m) => m[1] ?? '' },
  {
    re: /^(.+)_used(_[a-z]+)?$/,
    max: (m) => `${m[1]}_total${m[2] ?? ''}`,
    name: (m) => `${m[1]}${m[2] ?? ''}`,
  },
];

function partnerOf(key: string): { max: string; name: string } | null {
  for (const p of PAIRS) {
    const m = key.match(p.re);
    if (m) return { max: p.max(m), name: p.name(m) };
  }
  return null;
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
    // The "/ max" half of a pair is shown inside its partner's tile.
    if (keys.some((k) => k !== key && partnerOf(k)?.max === key)) continue;
    if (Array.isArray(v)) {
      const objects =
        v.length > 0 && v.every((x) => x !== null && typeof x === 'object' && !Array.isArray(x));
      sections.push({ key, kind: objects ? 'table' : 'list', value: v });
    } else if (v !== null && typeof v === 'object') {
      sections.push({ key, kind: 'map', value: v });
    } else if (key === 'state' || shortScalar(v)) {
      const pair = partnerOf(key);
      const max = pair && pair.max in status ? status[pair.max] : undefined;
      tiles.push(
        max === undefined
          ? { key, name: key, value: v }
          : { key, name: pair?.name ?? key, value: v, max },
      );
    } else {
      details.push([key, v]);
    }
  }
  // Status first, the way the design leads with it.
  tiles.sort((a, b) => Number(b.key === 'state') - Number(a.key === 'state'));
  return { tiles, details, sections };
}
