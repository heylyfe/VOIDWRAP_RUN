import { describe, expect, it } from "vitest";
import { PALETTE, platformColor } from "./palette";

describe("platformColor", () => {
  it("is deterministic for the same id", () => {
    for (let id = -120; id < 120; id++) {
      expect(platformColor(id)).toBe(platformColor(id));
    }
  });

  it("always returns one of the four platform top colours", () => {
    for (let id = -400; id < 400; id += 7) {
      expect(PALETTE.platformTopColors).toContain(platformColor(id));
    }
  });

  it("cycles with period 4", () => {
    expect(platformColor(0)).toBe(platformColor(4));
    expect(platformColor(1)).toBe(platformColor(5));
    expect(platformColor(2)).toBe(platformColor(6));
    expect(platformColor(3)).toBe(platformColor(7));
  });

  it("handles negative ids like their positive equivalents", () => {
    expect(platformColor(-1)).toBe(platformColor(3));
    expect(platformColor(-2)).toBe(platformColor(2));
    expect(platformColor(-3)).toBe(platformColor(1));
    expect(platformColor(-4)).toBe(platformColor(0));
  });

  it("handles large ids", () => {
    expect(platformColor(1_000_000_001)).toBe(platformColor(1));
    expect(platformColor(2 ** 40)).toBe(platformColor(0));
    expect(platformColor(Number.MAX_SAFE_INTEGER)).toBe(
      platformColor(Number.MAX_SAFE_INTEGER - 4),
    );
  });
});
