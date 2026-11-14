// Pure helpers for the gift-pickup burst effect. No DOM, no React, no three.js.
import { hashString, mulberry32 } from "./rng";

/** Any gift-like record; only the id and world position are needed. */
export interface GiftLike {
  id: number;
  pos: { x: number; y: number; z: number };
}

/** Ids present in `next` but not in `prev`, in `next` order, deduplicated. */
export function newlyTaken(prev: readonly number[], next: readonly number[]): number[] {
  const before = new Set(prev);
  const added = new Set<number>();
  const out: number[] = [];
  for (const id of next) {
    if (before.has(id) || added.has(id)) continue;
    added.add(id);
    out.push(id);
  }
  return out;
}

/** World position of the gift with `id`; null when the level has no such gift. */
export function findGiftPosition(
  gifts: readonly GiftLike[],
  id: number,
): { x: number; y: number; z: number } | null {
  for (const gift of gifts) {
    if (gift.id === id) {
      return { x: gift.pos.x, y: gift.pos.y, z: gift.pos.z };
    }
  }
  return null;
}

const TAU = Math.PI * 2;

export interface BurstParticle {
  /** Outward direction (y-biased so the burst arcs upward). Unit length in XZ+Y. */
  dirX: number;
  dirY: number;
  dirZ: number;
  /** Initial speed in units per second. */
  speed: number;
  /** Relative size multiplier. */
  size: number;
}

/**
 * Deterministic spherical burst with an upward bias. Same seed always yields
 * the same particles, so replays (and vitest) agree.
 */
export function burstParticles(seed: string, count: number): BurstParticle[] {
  const total = Math.max(0, Math.floor(count));
  const rand = mulberry32(hashString(`gift-pickup:${seed}`));
  const particles: BurstParticle[] = [];
  for (let i = 0; i < total; i += 1) {
    const theta = rand() * TAU;
    const lift = 0.25 + rand() * 0.75;
    const ring = Math.sqrt(Math.max(0, 1 - lift * lift));
    particles.push({
      dirX: Math.cos(theta) * ring,
      dirY: lift,
      dirZ: Math.sin(theta) * ring,
      speed: 1.6 + rand() * 1.4,
      size: 0.6 + rand() * 0.8,
    });
  }
  return particles;
}
