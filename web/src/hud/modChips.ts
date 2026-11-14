// Classify drafted upgrade/curse ids against the content manifest so a swapped
// or mislabelled wire array can never show a curse as an upgrade (or vice
// versa). Pure and unit-tested; no DOM, no React.
import { CURSES, UPGRADES } from '../content';

export type ModGroup = 'upgrade' | 'curse';

export interface CategorizedMod {
  id: string;
  group: ModGroup;
}

const UPGRADE_IDS = new Set(UPGRADES.map((card) => card.id));
const CURSE_IDS = new Set(CURSES.map((card) => card.id));

/**
 * Manifest-only classification. Returns null when the id is unknown, or present
 * in both manifests (ambiguous) — callers then fall back to the wire group.
 */
export function classifyModId(id: string): ModGroup | null {
  const inUpgrades = UPGRADE_IDS.has(id);
  const inCurses = CURSE_IDS.has(id);
  if (inUpgrades && !inCurses) {
    return 'upgrade';
  }
  if (inCurses && !inUpgrades) {
    return 'curse';
  }
  return null;
}

/**
 * Merge the wire `upgrades` and `curses` id lists into one manifest-classified
 * list. Duplicates collapse (first wire occurrence wins). Ids the manifest
 * cannot classify keep the group the wire reported, so nothing is dropped.
 */
export function categorizeMods(
  upgrades: readonly string[],
  curses: readonly string[],
): CategorizedMod[] {
  const seen = new Set<string>();
  const out: CategorizedMod[] = [];
  const wire: readonly (readonly [readonly string[], ModGroup])[] = [
    [upgrades, 'upgrade'],
    [curses, 'curse'],
  ];
  for (const [list, fallback] of wire) {
    for (const id of list) {
      if (seen.has(id)) continue;
      seen.add(id);
      out.push({ id, group: classifyModId(id) ?? fallback });
    }
  }
  return out;
}
