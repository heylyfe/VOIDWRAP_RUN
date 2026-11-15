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
 * Slope of a ramp's top face, for the checker shader.
 *
 * The checker is drawn from world XZ, which is a projection of the inclined top
 * face: along the slope axis the pattern is compressed by cos(theta), so the
 * cells read as stretched by 1/cos(theta) on the incline. Dividing that axis by
 * cos(theta) turns the projection back into arc length and the cells stay square.
 *
 * `axis` is 0 for a ramp that rises along X and 1 for one that rises along Z;
 * `cos` is 0 for a flat platform (no correction).
 */
export interface RampSlope {
  axis: 0 | 1;
  cos: number;
}

export function rampSlope(kind: number, size: Vec3Tuple): RampSlope {
  const info = rampInfo(kind);
  if (!info.isRamp || info.axis === null) return { axis: 0, cos: 0 };
  const run = info.axis === "x" ? size[0] : size[2];
  const rise = size[1];
  const len = Math.hypot(run, rise);
  if (!(len > 0)) return { axis: 0, cos: 0 };
  return { axis: info.axis === "x" ? 0 : 1, cos: run / len };
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
