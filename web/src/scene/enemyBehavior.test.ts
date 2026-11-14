import { describe, expect, it } from "vitest";
import { ENEMY_DISGUISED, ENEMY_DORMANT, ENEMY_HUNTING } from "../protocol/generated";
import { enemyBehaviorFor, modeForState } from "./enemyBehavior";

describe("enemyBehaviorFor", () => {
  it("falls back to chaser for undefined kind", () => {
    expect(enemyBehaviorFor(undefined)).toBe("chaser");
  });

  it("falls back to chaser for out-of-range kind", () => {
    expect(enemyBehaviorFor(9999)).toBe("chaser");
  });

  it("returns a known archetype for every indexed kind", () => {
    for (const kind of [0, 1, 2, 3, 4, 5, 6, 7]) {
      expect(["chaser", "turret", "mimic", "corrupter"]).toContain(enemyBehaviorFor(kind));
    }
  });
});

describe("modeForState", () => {
  it("maps dormant", () => {
    expect(modeForState(ENEMY_DORMANT)).toBe("dormant");
  });

  it("maps disguised", () => {
    expect(modeForState(ENEMY_DISGUISED)).toBe("disguised");
  });

  it("maps hunting", () => {
    expect(modeForState(ENEMY_HUNTING)).toBe("hunting");
  });

  it("treats missing state as hunting", () => {
    expect(modeForState(undefined)).toBe("hunting");
  });
});
