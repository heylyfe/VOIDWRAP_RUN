import { describe, expect, it } from "vitest";
import type { Platform } from "@/protocol/generated";
import { isPillar, platformCenter, platformSize } from "./platformUtils";

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

describe("isPillar", () => {
  it("flags tall thin boxes (height > 3x larger horizontal size)", () => {
    expect(isPillar([1, 3.5, 1])).toBe(true);
    expect(isPillar([2, 6.1, 2])).toBe(true);
    expect(isPillar([1, 3.1, 1.02])).toBe(true);
  });

  it("rejects wide or square boxes", () => {
    expect(isPillar([4, 5, 4])).toBe(false);
    expect(isPillar([6, 5, 2])).toBe(false);
    expect(isPillar([10, 2, 10])).toBe(false);
  });

  it("is strict at the boundary (equal is not a pillar)", () => {
    expect(isPillar([1, 3, 1])).toBe(false);
    expect(isPillar([2, 6, 2])).toBe(false);
  });
});

describe("platformSize / platformCenter", () => {
  it("computes size and center from bounds", () => {
    const p = plat(7, [-2, 1, -4], [6, 3, 8]);
    expect(platformSize(p)).toEqual([8, 2, 12]);
    expect(platformCenter(p)).toEqual([2, 2, 2]);
  });
});
