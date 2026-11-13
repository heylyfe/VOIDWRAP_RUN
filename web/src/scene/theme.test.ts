import { describe, expect, it } from "vitest";
import { THEMES, themeForLevel } from "./theme";

const HEX = /^#[0-9a-f]{6}$/;

describe("themes", () => {
  it("cycles one theme per level", () => {
    expect(themeForLevel(1)).toBe(THEMES[0]);
    expect(themeForLevel(THEMES.length + 1)).toBe(THEMES[0]);
    expect(themeForLevel(2)).toBe(THEMES[1]);
    expect(themeForLevel(0)).toBe(THEMES[0]);
    expect(themeForLevel(Number.NaN)).toBe(THEMES[0]);
  });
  it("every colour in the palette is a 6-digit hex", () => {
    for (const t of THEMES) {
      const colours = [...Object.values(t.sky), t.fog.color, ...Object.values(t.tile), t.accent, t.debris];
      for (const c of colours) expect(c).toMatch(HEX);
      expect(t.fog.near).toBeLessThan(t.fog.far);
    }
  });
});
