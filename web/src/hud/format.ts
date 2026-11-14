export type DeathCause = "void" | "enemy" | "projectile" | "disconnect";

const CAUSE_TEXT: Record<DeathCause, string> = {
  void: "Fell into the void",
  enemy: "Caught by an enemy",
  projectile: "Hit by a bolt",
  disconnect: "Lost connection",
};

export function causeText(cause: DeathCause): string {
  return CAUSE_TEXT[cause];
}

export function giftsLabel(remaining: number, total: number): string {
  const safeTotal = Number.isFinite(total) && total > 0 ? Math.floor(total) : 0;
  const safeRemaining = Number.isFinite(remaining) ? Math.floor(remaining) : 0;
  const clamped = Math.min(Math.max(safeRemaining, 0), safeTotal);
  return `${clamped} / ${safeTotal}`;
}
