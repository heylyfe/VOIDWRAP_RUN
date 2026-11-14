// Pure platform math helpers — no three.js imports (unit-testable).
import type { Platform } from "@/protocol/generated";

export type Vec3Tuple = [number, number, number];

// Ramp kinds — mirror web/src/protocol/generated.ts (redeclared so this module stays runtime-dependency-free for tests).
const PLATFORM_RAMP_PX = 3;
const PLATFORM_RAMP_NX = 4;
const PLATFORM_RAMP_PZ = 5;
const PLATFORM_RAMP_NZ = 6;

export function platformSize(platform: Platform): Vec3Tuple {
  const { min, max } = platform.bounds;
  return [max.x - min.x, max.y - min.y, max.z - min.z];
}

export function platformCenter(platform: Platform): Vec3Tuple {
  const { min, max } = platform.bounds;
  return [(min.x + max.x) / 2, (min.y + max.y) / 2, (min.z + max.z) / 2];
}

/**
 * Ticket T-057b1: a platform is a "pillar" when its height exceeds 3x its
 * larger horizontal extent. Pillars render as dark stone, without the
 * checkerboard top.
 */
export function isPillar(size: Vec3Tuple): boolean {
  const [w, h, d] = size;
  return h > 3 * Math.max(w, d);
}

export type RampAxis = "x" | "z";

export interface RampInfo {
  /** True for the four ramp kinds. */
  isRamp: boolean;
  /** Slope axis for ramps, else null. */
  axis: RampAxis | null;
  /** Sign along the axis where the surface rises: +1 rises toward +axis, -1 rises toward -axis, 0 for non-ramps. */
  sign: 1 | -1 | 0;
}

/**
 * Reports whether a platform kind is a ramp and, for ramps, the slope axis and
 * the direction the surface rises in. Renderer and decor placement share this
 * so they agree on what a ramp is and which way it slopes.
 */
export function rampInfo(kind: number): RampInfo {
  switch (kind) {
    case PLATFORM_RAMP_PX:
      return { isRamp: true, axis: "x", sign: 1 };
    case PLATFORM_RAMP_NX:
      return { isRamp: true, axis: "x", sign: -1 };
    case PLATFORM_RAMP_PZ:
      return { isRamp: true, axis: "z", sign: 1 };
    case PLATFORM_RAMP_NZ:
      return { isRamp: true, axis: "z", sign: -1 };
    default:
      return { isRamp: false, axis: null, sign: 0 };
  }
}

/**
 * Walk-surface height of a platform at world (x, z). For ramps this is the
 * sloped line between the low edge (bounds.min.y) and the high edge
 * (bounds.max.y); for other kinds it is bounds.max.y. (x, z) are clamped to
 * the platform's footprint.
 */
export function rampSurfaceHeight(platform: Platform, x: number, z: number): number {
  const info = rampInfo(platform.kind);
  if (!info.isRamp || info.axis === null || info.sign === 0) return platform.bounds.max.y;
  const { min, max } = platform.bounds;
  const span = info.axis === "x" ? max.x - min.x : max.z - min.z;
  const coord = info.axis === "x" ? x - min.x : z - min.z;
  const t = span > 0 ? Math.min(1, Math.max(0, coord / span)) : 0;
  const k = info.sign === 1 ? t : 1 - t;
  return min.y + (max.y - min.y) * k;
}
