// Pure decoration placement — no three.js imports (unit-testable).
// Deterministic from (platforms, seed). Density capped:
//   * at most MAX_VINES vines, hung from underside edges of walkable platforms,
//   * at most one lantern per 40 u of walkway (floor(walkwayUnits / 40)), capped at MAX_LANTERNS,
//   * lanterns only on NORMAL platforms and ramps (not CRUMBLE, not JUMP_PAD) with min horizontal
//     span >= 2.2 u (posts need stable footing); for ramps the post stands on the ramp surface,
//     so its y is the surface height at (x, z), not bounds.max.y.
// walkwayUnits estimates path length as the sum of (width + depth) of non-JUMP_PAD platforms.
import type { Platform } from "@/protocol/generated";
import { rampInfo, rampSurfaceHeight } from "./platformUtils";

// Mirrors web/src/protocol/generated.ts (redeclared so this module stays runtime-dependency-free for tests).
const PLATFORM_NORMAL = 0;
const PLATFORM_JUMP_PAD = 2;

export const MAX_VINES = 180;
export const MAX_LANTERNS = 24;
export const LANTERN_SPACING_UNITS = 40;
const VINE_KEEP_PROB = 0.55;
const VINE_MIN_LEN = 0.8;
const VINE_MAX_LEN = 2.3;
const VINE_INSET = 0.25;
const VINE_ANCHOR_SPACING = 2.4;
const LANTERN_INSET = 0.32;
const LANTERN_MIN_SPAN = 2.2;
const LANTERN_CANDIDATE_PROB = 0.6;

export interface VinePlacement {
  platformId: number;
  /** Anchor point on the platform's underside edge. */
  x: number;
  y: number;
  z: number;
  length: number;
  /** 0..2π, used to vary strand thickness and to allow future sway. */
  phase: number;
}

export interface LanternPlacement {
  platformId: number;
  x: number;
  /** Surface y at (x, z) — the post stands on the deck; for ramps this is the ramp surface height. */
  y: number;
  z: number;
}

export interface DecorPlacement {
  vines: VinePlacement[];
  lanterns: LanternPlacement[];
}

/** 32-bit integer hash of (a, b). Stable across runs and platforms. */
export function hash32(a: number, b: number): number {
  let h = (a >>> 0) ^ Math.imul((b >>> 0) + 0x9e3779b9, 0x85ebca6b);
  h = Math.imul(h ^ (h >>> 13), 0xc2b2ae35);
  h ^= h >>> 16;
  return h >>> 0;
}

/** mulberry32 PRNG — small, fast, deterministic. */
export function mulberry32(seed: number): () => number {
  let s = seed >>> 0;
  return () => {
    s = (s + 0x6d2b79f5) | 0;
    let t = Math.imul(s ^ (s >>> 15), 1 | s);
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

export function platformFootprint(p: Platform): {
  w: number;
  h: number;
  d: number;
  cx: number;
  cz: number;
  top: number;
  bottom: number;
} {
  const w = p.bounds.max.x - p.bounds.min.x;
  const h = p.bounds.max.y - p.bounds.min.y;
  const d = p.bounds.max.z - p.bounds.min.z;
  return {
    w,
    h,
    d,
    cx: (p.bounds.min.x + p.bounds.max.x) / 2,
    cz: (p.bounds.min.z + p.bounds.max.z) / 2,
    top: p.bounds.max.y,
    bottom: p.bounds.min.y,
  };
}

export function isLanternEligible(p: Platform): boolean {
  // Ramps are stable footing like NORMAL decks; CRUMBLE collapses and JUMP_PAD tops launch.
  if (p.kind !== PLATFORM_NORMAL && !rampInfo(p.kind).isRamp) return false;
  const { w, d } = platformFootprint(p);
  return Math.min(w, d) >= LANTERN_MIN_SPAN;
}

/** Sum of (w + d) over non-JUMP_PAD platforms; approximates walkway length in units. */
export function walkwayUnits(platforms: readonly Platform[]): number {
  let u = 0;
  for (const p of platforms) {
    if (p.kind === PLATFORM_JUMP_PAD) continue;
    const { w, d } = platformFootprint(p);
    u += w + d;
  }
  return u;
}

/** Max number of lantern lights for a level: one per 40 u of walkway, capped. */
export function lanternBudget(platforms: readonly Platform[]): number {
  return Math.min(MAX_LANTERNS, Math.floor(walkwayUnits(platforms) / LANTERN_SPACING_UNITS));
}

export function computeDecor(platforms: readonly Platform[], seed: number): DecorPlacement {
  const vines: VinePlacement[] = [];
  const lanternCandidates: Array<{ p: Platform; sort: number }> = [];

  for (const p of platforms) {
    const { bottom } = platformFootprint(p);
    const rng = mulberry32(hash32(seed, p.id + 1));

    // Hanging vines under the four underside edges (JUMP_PAD bottoms are allowed —
    // pads survive collapse, their undersides are still static geometry).
    // For ramps the underside is the flat plane at bounds.min.y (the wedge bottom),
    // which is exactly what `bottom` reports.
    const edges: Array<{ axis: "x" | "z"; fixed: number; from: number; to: number }> = [
      { axis: "x", fixed: p.bounds.min.z, from: p.bounds.min.x, to: p.bounds.max.x },
      { axis: "x", fixed: p.bounds.max.z, from: p.bounds.min.x, to: p.bounds.max.x },
      { axis: "z", fixed: p.bounds.min.x, from: p.bounds.min.z, to: p.bounds.max.z },
      { axis: "z", fixed: p.bounds.max.x, from: p.bounds.min.z, to: p.bounds.max.z },
    ];
    for (const e of edges) {
      const len = e.to - e.from;
      if (len < 1.6) continue;
      const anchors = Math.floor(len / VINE_ANCHOR_SPACING);
      for (let i = 1; i <= anchors; i += 1) {
        if (vines.length >= MAX_VINES) break;
        if (rng() > VINE_KEEP_PROB) continue;
        const along = (len * i) / (anchors + 1);
        const jitter = (rng() - 0.5) * 1.1;
        const at = Math.min(Math.max(e.from + along + jitter, e.from + VINE_INSET), e.to - VINE_INSET);
        const length = VINE_MIN_LEN + rng() * (VINE_MAX_LEN - VINE_MIN_LEN);
        const phase = rng() * Math.PI * 2;
        if (e.axis === "x") {
          vines.push({ platformId: p.id, x: at, y: bottom, z: e.fixed, length, phase });
        } else {
          vines.push({ platformId: p.id, x: e.fixed, y: bottom, z: at, length, phase });
        }
      }
      if (vines.length >= MAX_VINES) break;
    }

    // Lantern candidates: stable, spacious, plain platforms and ramps.
    if (isLanternEligible(p) && rng() < LANTERN_CANDIDATE_PROB) {
      lanternCandidates.push({ p, sort: hash32(seed ^ 0x51ab, p.id + 7) });
    }
  }

  // Even spread: pick the lowest-hash candidates until the walkway budget is spent.
  lanternCandidates.sort((a, b) => a.sort - b.sort);
  const budget = lanternBudget(platforms);
  const lanterns: LanternPlacement[] = [];
  for (const c of lanternCandidates) {
    if (lanterns.length >= budget) break;
    const p = c.p;
    const fp = platformFootprint(p);
    const cornerRng = mulberry32(hash32(seed ^ 0x2b1, p.id + 13));
    const corner = Math.floor(cornerRng() * 4);
    const sx = corner % 2 === 0 ? -1 : 1;
    const sz = corner < 2 ? -1 : 1;
    const x = fp.cx + sx * (fp.w / 2 - LANTERN_INSET);
    const z = fp.cz + sz * (fp.d / 2 - LANTERN_INSET);
    lanterns.push({
      platformId: p.id,
      x,
      y: rampSurfaceHeight(p, x, z),
      z,
    });
  }

  return { vines, lanterns };
}
