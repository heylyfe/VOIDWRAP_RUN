/**
 * T-112 straggler window: when at most this fraction of a level's normal gifts is
 * left, every remaining gift gets a highlight beam.
 *
 * A beam rather than a colour change: colour already carries normal-vs-golden, and
 * LURE's shimmer wears the normal gift glow (JUDGMENT CALL 2), so a third colour
 * would collide with both.
 */
export const STRAGGLER_FRACTION = 0.2;

/**
 * True when the level is in its straggler window: some gifts remain, and they are
 * at most `STRAGGLER_FRACTION` of the level's total.
 *
 * A level with no gifts is never in the window (nothing to highlight), and a level
 * with every gift still out is never in it either (the player has not started).
 */
export function isStragglerWindow(total: number, remaining: number): boolean {
  if (!Number.isFinite(total) || !Number.isFinite(remaining)) return false;
  if (total <= 0 || remaining <= 0) return false;
  if (remaining > total) return false;
  return remaining <= total * STRAGGLER_FRACTION;
}
