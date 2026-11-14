import { describe, expect, it } from 'vitest';
import { CURSES, UPGRADES } from '../content';
import { categorizeMods, classifyModId } from './modChips';

describe('classifyModId', () => {
  it('classifies a known upgrade id as upgrade', () => {
    const upgrade = UPGRADES[0];
    if (!upgrade) return; // manifest empty: nothing to assert
    expect(classifyModId(upgrade.id)).toBe('upgrade');
  });

  it('classifies a known curse id as curse', () => {
    const curse = CURSES[0];
    if (!curse) return;
    expect(classifyModId(curse.id)).toBe('curse');
  });

  it('returns null for an id the manifest does not know', () => {
    expect(classifyModId('zz-unknown-card-id-0000')).toBeNull();
  });
});

describe('categorizeMods', () => {
  it('re-files a swapped wire array by the manifest', () => {
    const upgrade = UPGRADES[0];
    const curse = CURSES[0];
    if (!upgrade || !curse) return; // manifest empty: nothing to assert
    expect(categorizeMods([curse.id], [upgrade.id])).toEqual([
      { id: curse.id, group: 'curse' },
      { id: upgrade.id, group: 'upgrade' },
    ]);
  });

  it('deduplicates ids present in both wire lists (first occurrence wins)', () => {
    const upgrade = UPGRADES[0];
    if (!upgrade) return;
    expect(categorizeMods([upgrade.id, upgrade.id], [upgrade.id])).toEqual([
      { id: upgrade.id, group: 'upgrade' },
    ]);
  });

  it('keeps unknown ids in the wire group they arrived in', () => {
    expect(categorizeMods(['zz-unknown-upgrade-0000'], ['zz-unknown-curse-0000'])).toEqual([
      { id: 'zz-unknown-upgrade-0000', group: 'upgrade' },
      { id: 'zz-unknown-curse-0000', group: 'curse' },
    ]);
  });
});
