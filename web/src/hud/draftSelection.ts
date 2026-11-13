// Pure draft-selection logic: enemy cards (kind 0) and reward cards (kind 1/2)
// have separate pick limits; selecting beyond a limit replaces the oldest
// selection in that same group. No DOM, no React, fully deterministic.

/** Protocol DraftCard.kind for enemy cards (see content.ts CARD_ENEMY). */
const ENEMY_KIND = 0;

/** Safe kind lookup: undefined for out-of-range indices (avoids unchecked indexing). */
function kindAt(cards: readonly { kind: number }[], index: number): number | undefined {
  if (index < 0 || index >= cards.length) {
    return undefined;
  }
  return cards[index]?.kind;
}

function isEnemyKind(cards: readonly { kind: number }[], index: number): boolean {
  return kindAt(cards, index) === ENEMY_KIND;
}

export function toggleSelection(
  selected: readonly number[],
  index: number,
  cards: readonly { kind: number }[],
  enemyPicks: number,
  rewardPicks: number,
): number[] {
  // Out-of-range index -> unchanged.
  if (index < 0 || index >= cards.length) {
    return [...selected];
  }
  // Clicking a selected card deselects it.
  if (selected.includes(index)) {
    return selected.filter((i) => i !== index);
  }

  const enemy = isEnemyKind(cards, index);
  const limit = enemy ? enemyPicks : rewardPicks;
  // A group with no allowance can never be selected into.
  if (limit <= 0) {
    return [...selected];
  }

  const next = [...selected];
  const sameGroup = next.filter((i) => isEnemyKind(cards, i) === enemy);
  // Selecting beyond the limit for this group replaces the OLDEST selection
  // in the same group (oldest = first one still in the ordered selection).
  if (sameGroup.length >= limit) {
    for (const i of next) {
      if (isEnemyKind(cards, i) === enemy) {
        next.splice(next.indexOf(i), 1);
        break;
      }
    }
  }
  next.push(index);
  return next;
}

export function selectionComplete(
  selected: readonly number[],
  cards: readonly { kind: number }[],
  enemyPicks: number,
  rewardPicks: number,
): boolean {
  const inRange = selected.filter((i) => i >= 0 && i < cards.length);
  const enemies = inRange.filter((i) => isEnemyKind(cards, i));
  const rewards = inRange.filter((i) => !isEnemyKind(cards, i));
  return enemies.length === enemyPicks && rewards.length === rewardPicks;
}
