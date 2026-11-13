import { describe, expect, it } from "vitest";
import { CARD_CURSE, CARD_ENEMY, CARD_UPGRADE, CURSES, ENEMIES, UPGRADES, gradeForTheme, lookupCard } from "./content";
import { THEMES } from "./scene/theme";

describe("content", () => {
  it("resolves every card by kind and id, and rejects unknown ones", () => {
    for (const e of ENEMIES) expect(lookupCard(CARD_ENEMY, e.id)?.card).toBe(e);
    for (const u of UPGRADES) expect(lookupCard(CARD_UPGRADE, u.id)?.card).toBe(u);
    for (const c of CURSES) expect(lookupCard(CARD_CURSE, c.id)?.card).toBe(c);
    expect(lookupCard(CARD_ENEMY, "nope")).toBeNull();
    expect(lookupCard(9, ENEMIES[0]?.id ?? "")).toBeNull();
  });
  it("has a screen grade for every theme", () => {
    for (const t of THEMES) expect(gradeForTheme(t.id).tint).toMatch(/^#[0-9a-f]{6}$/i);
  });
});
