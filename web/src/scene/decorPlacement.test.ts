import { describe, expect, it } from "vitest";
import type { Platform } from "@/protocol/generated";
import {
  MAX_LANTERNS,
  MAX_VINES,
  computeDecor,
  isLanternEligible,
  lanternBudget,
  walkwayUnits,
} from "./decorPlacement";

function plat(
  id: number,
  min: [number, number, number],
  max: [number, number, number],
  kind = 0,
): Platform {
  return {
    id,
    kind,
    bounds: {
      min: { x: min[0], y: min[1], z: min[2] },
      max: { x: max[0], y: max[1], z: max[2] },
    },
  };
}

const LEVEL: Platform[] = [
  plat(1, [-8, -1, -7], [8, 0, 7]), // spawn hub 16x14
  plat(2, [0, 0, 12], [14, 1, 15]), // bridge 14x3
  plat(3, [20, 0, 20], [25, 1, 25], 2), // jump pad — no lanterns
  plat(4, [30, 0, 30], [36, 1, 36], 1), // crumble — no lanterns
  plat(5, [0, 0, 40], [1.5, 1, 41.5]), // too small for a post
  plat(6, [50, 0, 0], [60, 2, 6]),
];

describe("walkwayUnits / lanternBudget", () => {
  it("sums (w + d) and excludes jump pads", () => {
    // 30 + 17 + (jump pad excluded) + 12 + 3 + 16 = 78
    expect(walkwayUnits(LEVEL)).toBe(78);
  });

  it("grants one lantern per 40 u of walkway, capped", () => {
    expect(lanternBudget(LEVEL)).toBe(Math.floor(78 / 40));
    const huge = Array.from({ length: 80 }, (_, i) => plat(100 + i, [i * 20, 0, 0], [i * 20 + 10, 1, 5]));
    expect(lanternBudget(huge)).toBe(MAX_LANTERNS);
  });
});

describe("isLanternEligible", () => {
  it("accepts plain spacious platforms only", () => {
    expect(isLanternEligible(plat(1, [0, 0, 0], [3, 1, 3]))).toBe(true);
    expect(isLanternEligible(plat(1, [0, 0, 0], [3, 1, 3], 1))).toBe(false);
    expect(isLanternEligible(plat(1, [0, 0, 0], [3, 1, 3], 2))).toBe(false);
    expect(isLanternEligible(plat(1, [0, 0, 0], [1.5, 1, 3]))).toBe(false);
  });
});

describe("computeDecor", () => {
  it("is deterministic for the same seed", () => {
    const a = computeDecor(LEVEL, 12345);
    const b = computeDecor(LEVEL, 12345);
    expect(a).toEqual(b);
  });

  it("produces different output for different seeds", () => {
    const a = computeDecor(LEVEL, 1);
    const b = computeDecor(LEVEL, 999);
    expect(JSON.stringify(a)).not.toBe(JSON.stringify(b));
  });

  it("respects caps and placement rules", () => {
    const many = Array.from({ length: 60 }, (_, i) => plat(i + 1, [i * 30, 0, 0], [i * 30 + 12, 1, 5]));
    const decor = computeDecor(many, 777);
    expect(decor.vines.length).toBeLessThanOrEqual(MAX_VINES);
    expect(decor.lanterns.length).toBeLessThanOrEqual(lanternBudget(many));
    for (const l of decor.lanterns) {
      const src = many.find((p) => p.id === l.platformId);
      expect(src).toBeDefined();
      if (!src) continue;
      expect(src.kind).toBe(0);
      expect(l.x).toBeGreaterThanOrEqual(src.bounds.min.x + 0.2);
      expect(l.x).toBeLessThanOrEqual(src.bounds.max.x - 0.2);
      expect(l.z).toBeGreaterThanOrEqual(src.bounds.min.z + 0.2);
      expect(l.z).toBeLessThanOrEqual(src.bounds.max.z - 0.2);
      expect(l.y).toBe(src.bounds.max.y);
    }
  });

  it("hangs vines from underside edges, in range", () => {
    const decor = computeDecor(LEVEL, 42);
    for (const v of decor.vines) {
      const src = LEVEL.find((p) => p.id === v.platformId);
      expect(src).toBeDefined();
      if (!src) continue;
      expect(v.y).toBe(src.bounds.min.y);
      expect(v.length).toBeGreaterThanOrEqual(0.8);
      expect(v.length).toBeLessThanOrEqual(2.3);
      expect(v.x).toBeGreaterThanOrEqual(src.bounds.min.x - 0.001);
      expect(v.x).toBeLessThanOrEqual(src.bounds.max.x + 0.001);
      expect(v.z).toBeGreaterThanOrEqual(src.bounds.min.z - 0.001);
      expect(v.z).toBeLessThanOrEqual(src.bounds.max.z + 0.001);
    }
  });
});
