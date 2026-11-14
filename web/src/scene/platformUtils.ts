// Pure platform math helpers — no three.js imports (unit-testable).
import type { Platform } from "@/protocol/generated";

export type Vec3Tuple = [number, number, number];

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
