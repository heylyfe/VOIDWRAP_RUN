import { describe, it, expect } from "vitest";
import {
  ENEMY_ATTACK,
  ENEMY_DORMANT,
  ENEMY_TELEGRAPH,
} from "../protocol/generated";
import {
  ENEMY_LOOKS,
  ENEMY_KIND_COUNT,
  enemyLookFor,
  isEnemyKind,
  isTelegraphState,
  clamp01,
  prog01,
  lerp,
  telegraphAlpha,
  motelingRingRadius,
  expandingRingRadius,
  snapshrikeAlpha,
  mimicShimmerVisible,
  mimicShimmerOpacity,
  yawToward,
  MIMIC_GIFT_SCALE,
  MIMIC_SHIMMER_PERIOD,
} from "./enemyLook";
import { MODEL_COMPONENTS } from "./EnemyField";

const EXPECTED_BEHAVIORS = [
  "pallbearer", "snapshrike", "moteling", "rift_warden", "gale_wight",
  "lantern_leech", "turret", "mimic", "corrupter",
  "rewind", "warden", "peal", "kite", "static",
  "blight", "metronome", "overlook", "chorus", "coil", "lure",
] as const;

describe("ENEMY_LOOKS", () => {
  it("has exactly twenty looks in wire kind order", () => {
    expect(ENEMY_LOOKS.map((l) => l.behavior)).toEqual([...EXPECTED_BEHAVIORS]);
    expect(ENEMY_KIND_COUNT).toBe(20);
  });

  it("gives every enemy a unique behaviour id", () => {
    const behaviours = new Set(ENEMY_LOOKS.map((l) => l.behavior));
    expect(behaviours.size).toBe(ENEMY_LOOKS.length);
  });

  it("gives every enemy a unique accent colour", () => {
    const accents = new Set(ENEMY_LOOKS.map((l) => l.accent));
    expect(accents.size).toBe(ENEMY_LOOKS.length);
  });

  it("gives every enemy a unique glow colour", () => {
    const glows = new Set(ENEMY_LOOKS.map((l) => l.glow));
    expect(glows.size).toBe(ENEMY_LOOKS.length);
  });

  it("registers exactly one model component per kind", () => {
    expect(MODEL_COMPONENTS.length).toBe(ENEMY_KIND_COUNT);
  });
});

describe("enemyLookFor / isEnemyKind", () => {
  it("maps each kind to its look", () => {
    expect(enemyLookFor(0).behavior).toBe("pallbearer");
    expect(enemyLookFor(4).behavior).toBe("gale_wight");
    expect(enemyLookFor(8).behavior).toBe("corrupter");
    expect(enemyLookFor(9).behavior).toBe("rewind");
    expect(enemyLookFor(10).behavior).toBe("warden");
    expect(enemyLookFor(18).behavior).toBe("coil");
    expect(enemyLookFor(19).behavior).toBe("lure");
  });

  it("enemyLookFor(19) is the lure and not the pallbearer", () => {
    const look = enemyLookFor(19);
    expect(look.behavior).toBe("lure");
    expect(look.behavior).not.toBe("pallbearer");
  });

  it("falls back to pallbearer for unknown or missing kinds", () => {
    expect(enemyLookFor(undefined).behavior).toBe("pallbearer");
    expect(enemyLookFor(20).behavior).toBe("pallbearer");
    expect(enemyLookFor(99).behavior).toBe("pallbearer");
    expect(enemyLookFor(-1).behavior).toBe("pallbearer");
    expect(enemyLookFor(2.5).behavior).toBe("pallbearer");
  });

  it("isEnemyKind validates the index range", () => {
    expect(isEnemyKind(0)).toBe(true);
    expect(isEnemyKind(8)).toBe(true);
    expect(isEnemyKind(19)).toBe(true);
    expect(isEnemyKind(20)).toBe(false);
    expect(isEnemyKind(-1)).toBe(false);
  });
});

describe("isTelegraphState", () => {
  it("is true only for TELEGRAPH and ATTACK", () => {
    expect(isTelegraphState(ENEMY_TELEGRAPH)).toBe(true);
    expect(isTelegraphState(ENEMY_ATTACK)).toBe(true);
    expect(isTelegraphState(ENEMY_DORMANT)).toBe(false);
    expect(isTelegraphState(undefined)).toBe(false);
    expect(isTelegraphState(999)).toBe(false);
  });
});

describe("curves", () => {
  it("clamp01 clamps and tolerates NaN/Infinity", () => {
    expect(clamp01(-1)).toBe(0);
    expect(clamp01(2)).toBe(1);
    expect(clamp01(0.5)).toBe(0.5);
    expect(clamp01(Number.NaN)).toBe(0);
    expect(clamp01(Number.POSITIVE_INFINITY)).toBe(0);
  });

  it("prog01 maps 0..255 onto 0..1", () => {
    expect(prog01(0)).toBe(0);
    expect(prog01(255)).toBe(1);
    expect(prog01(128)).toBeCloseTo(128 / 255, 5);
    expect(prog01(undefined)).toBe(0);
    expect(prog01(9999)).toBe(1);
  });

  it("lerp clamps its factor", () => {
    expect(lerp(0, 10, 0.5)).toBe(5);
    expect(lerp(0, 10, -1)).toBe(0);
    expect(lerp(0, 10, 2)).toBe(10);
  });

  it("telegraphAlpha ramps 0.3 → 1", () => {
    expect(telegraphAlpha(0)).toBeCloseTo(0.3, 5);
    expect(telegraphAlpha(1)).toBeCloseTo(1, 5);
    expect(telegraphAlpha(0.5)).toBeGreaterThan(telegraphAlpha(0.1));
  });

  it("motelingRingRadius tightens monotonically and stays positive", () => {
    const a = motelingRingRadius(0);
    const b = motelingRingRadius(0.5);
    const c = motelingRingRadius(1);
    expect(a).toBeGreaterThan(b);
    expect(b).toBeGreaterThan(c);
    expect(c).toBeGreaterThan(0);
  });

  it("expandingRingRadius grows from a positive floor to maxRadius", () => {
    expect(expandingRingRadius(0, 4)).toBeCloseTo(0.25, 5);
    expect(expandingRingRadius(0.5, 4)).toBeCloseTo(2, 5);
    expect(expandingRingRadius(1, 4)).toBeCloseTo(4, 5);
  });

  it("snapshrikeAlpha snaps to 1 once the lock freezes", () => {
    expect(snapshrikeAlpha(0.1)).toBeLessThan(0.4);
    expect(snapshrikeAlpha(0.6)).toBe(1);
    expect(snapshrikeAlpha(1)).toBe(1);
  });

  it("mimic shimmer fires for 0.5 s every 3.0 s", () => {
    expect(mimicShimmerVisible(0.1)).toBe(true);
    expect(mimicShimmerVisible(0.49)).toBe(true);
    expect(mimicShimmerVisible(0.5)).toBe(false);
    expect(mimicShimmerVisible(1.5)).toBe(false);
    expect(mimicShimmerVisible(MIMIC_SHIMMER_PERIOD)).toBe(true);
    expect(mimicShimmerOpacity(0.25)).toBeGreaterThan(0.8);
    expect(mimicShimmerOpacity(2)).toBe(0);
  });

  it("MIMIC_GIFT_SCALE keeps the disguise at real gift size", () => {
    expect(MIMIC_GIFT_SCALE).toBe(1);
  });

  it("yawToward faces +Z toward the target", () => {
    expect(yawToward(0, 0, 0, 5)).toBeCloseTo(0, 5);
    expect(yawToward(0, 0, 5, 0)).toBeCloseTo(Math.PI / 2, 5);
    expect(yawToward(0, 0, 0, -5)).toBeCloseTo(Math.PI, 5);
    expect(yawToward(2, 2, 2, 9)).toBeCloseTo(0, 5);
  });
});
