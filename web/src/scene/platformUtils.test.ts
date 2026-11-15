import { describe, expect, it } from "vitest";
import type { Platform } from "@/protocol/generated";
import {
  PLATFORM_JUMP_PAD,
  PLATFORM_NORMAL,
  PLATFORM_RAMP_NX,
  PLATFORM_RAMP_NZ,
  PLATFORM_RAMP_PX,
  PLATFORM_RAMP_PZ,
} from "@/protocol/generated";
import { isPillar, platformCenter, platformSize, rampSlope } from "./platformUtils";

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

describe("rampSlope", () => {
  it("reports the slope axis and cos(theta) for each ramp kind", () => {
    // 8 u run, 4 u rise: cos = 8 / sqrt(80) = 0.8944.
    const cos = 8 / Math.hypot(8, 4);
    expect(rampSlope(PLATFORM_RAMP_PX, [8, 4, 4])).toEqual({ axis: 0, cos });
    expect(rampSlope(PLATFORM_RAMP_NX, [8, 4, 4])).toEqual({ axis: 0, cos });
    expect(rampSlope(PLATFORM_RAMP_PZ, [4, 4, 8])).toEqual({ axis: 1, cos });
    expect(rampSlope(PLATFORM_RAMP_NZ, [4, 4, 8])).toEqual({ axis: 1, cos });
  });

  it("uses the run along the slope axis, not the other horizontal extent", () => {
    // A ramp rising along X: the Z extent must not enter the angle.
    const a = rampSlope(PLATFORM_RAMP_PX, [8, 4, 40]);
    const b = rampSlope(PLATFORM_RAMP_PX, [8, 4, 4]);
    expect(a.cos).toBeCloseTo(b.cos, 10);
  });

  it("reports no correction for flat platforms", () => {
    expect(rampSlope(PLATFORM_NORMAL, [8, 4, 4])).toEqual({ axis: 0, cos: 0 });
    expect(rampSlope(PLATFORM_JUMP_PAD, [8, 4, 4])).toEqual({ axis: 0, cos: 0 });
  });

  it("never divides by zero on a degenerate ramp", () => {
    expect(rampSlope(PLATFORM_RAMP_PX, [0, 0, 0])).toEqual({ axis: 0, cos: 0 });
    expect(rampSlope(PLATFORM_RAMP_PZ, [0, 0, 0])).toEqual({ axis: 0, cos: 0 });
  });

  it("a 45-degree ramp reports cos = 1/sqrt(2)", () => {
    expect(rampSlope(PLATFORM_RAMP_PZ, [4, 4, 4]).cos).toBeCloseTo(Math.SQRT1_2, 10);
  });
});

describe("platformSize / platformCenter", () => {
  it("computes size and center from bounds", () => {
    const p = plat(7, [-2, 1, -4], [6, 3, 8]);
    expect(platformSize(p)).toEqual([8, 2, 12]);
    expect(platformCenter(p)).toEqual([2, 2, 2]);
  });
});
