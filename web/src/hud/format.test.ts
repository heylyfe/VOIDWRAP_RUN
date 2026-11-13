import { describe, expect, it } from "vitest";

import { causeText, giftsLabel } from "./format";

describe("causeText", () => {
  it('maps "void" to its message', () => {
    expect(causeText("void")).toBe("Fell into the void");
  });

  it('maps "enemy" to its message', () => {
    expect(causeText("enemy")).toBe("Caught by an enemy");
  });

  it('maps "disconnect" to its message', () => {
    expect(causeText("disconnect")).toBe("Lost connection");
  });
});

describe("giftsLabel", () => {
  it("formats a value inside the range", () => {
    expect(giftsLabel(3, 5)).toBe("3 / 5");
  });

  it("clamps remaining above total down to total", () => {
    expect(giftsLabel(9, 5)).toBe("5 / 5");
  });

  it("clamps negative remaining up to 0", () => {
    expect(giftsLabel(-2, 5)).toBe("0 / 5");
  });

  it("treats a NaN total as 0", () => {
    expect(giftsLabel(1, NaN)).toBe("0 / 0");
  });

  it("treats a negative total as 0", () => {
    expect(giftsLabel(2, -3)).toBe("0 / 0");
  });

  it("treats a NaN remaining as 0", () => {
    expect(giftsLabel(NaN, 5)).toBe("0 / 5");
  });
});
