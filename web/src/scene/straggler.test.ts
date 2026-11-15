import { describe, expect, it } from "vitest";
import { isStragglerWindow, STRAGGLER_FRACTION } from "./straggler";

describe("isStragglerWindow", () => {
  it("opens at exactly 20% remaining", () => {
    expect(isStragglerWindow(100, 20)).toBe(true);
    expect(isStragglerWindow(100, 21)).toBe(false);
  });

  it("stays open as the count falls", () => {
    for (const remaining of [1, 2, 5, 10, 19, 20]) {
      expect(isStragglerWindow(100, remaining)).toBe(true);
    }
  });

  it("is closed while most gifts are still out", () => {
    expect(isStragglerWindow(100, 100)).toBe(false);
    expect(isStragglerWindow(100, 50)).toBe(false);
    expect(isStragglerWindow(183, 37)).toBe(false);
  });

  it("is closed when nothing is left to highlight", () => {
    expect(isStragglerWindow(100, 0)).toBe(false);
    expect(isStragglerWindow(0, 0)).toBe(false);
  });

  it("is closed for a level with no gifts", () => {
    expect(isStragglerWindow(0, 1)).toBe(false);
  });

  it("rejects nonsense counts rather than highlighting", () => {
    expect(isStragglerWindow(Number.NaN, 1)).toBe(false);
    expect(isStragglerWindow(10, Number.NaN)).toBe(false);
    expect(isStragglerWindow(10, 11)).toBe(false);
    expect(isStragglerWindow(-5, 1)).toBe(false);
  });

  it("uses a 20% fraction", () => {
    expect(STRAGGLER_FRACTION).toBe(0.2);
  });
});
