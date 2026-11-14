import { describe, it, expect } from "vitest";
import {
  ENEMY_ATTACK,
  ENEMY_DISGUISED,
  ENEMY_DORMANT,
  ENEMY_HUNTING,
  ENEMY_RECOVER,
  ENEMY_TELEGRAPH,
} from "../protocol/generated";
import { enemyBehaviorFor, modeForState } from "./enemyBehavior";

describe("enemyBehaviorFor", () => {
  it("routes every roster kind to its behaviour id", () => {
    const expected = [
      "pallbearer", "snapshrike", "moteling", "rift_warden", "gale_wight",
      "lantern_leech", "turret", "mimic", "corrupter",
    ] as const;
    expected.forEach((id, kind) => {
      expect(enemyBehaviorFor(kind)).toBe(id);
    });
  });

  it("falls back to pallbearer for unknown or missing kinds", () => {
    expect(enemyBehaviorFor(undefined)).toBe("pallbearer");
    expect(enemyBehaviorFor(99)).toBe("pallbearer");
    expect(enemyBehaviorFor(-3)).toBe("pallbearer");
  });
});

describe("modeForState", () => {
  it("maps dormant and disguised", () => {
    expect(modeForState(ENEMY_DORMANT)).toBe("dormant");
    expect(modeForState(ENEMY_DISGUISED)).toBe("disguised");
  });

  it("reads everything else (including missing info) as hunting", () => {
    expect(modeForState(ENEMY_HUNTING)).toBe("hunting");
    expect(modeForState(ENEMY_TELEGRAPH)).toBe("hunting");
    expect(modeForState(ENEMY_ATTACK)).toBe("hunting");
    expect(modeForState(ENEMY_RECOVER)).toBe("hunting");
    expect(modeForState(undefined)).toBe("hunting");
  });
});
