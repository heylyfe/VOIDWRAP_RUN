import { describe, expect, it } from "vitest";
import { ENEMY_LOOKS } from "./enemyLook";
import { deltaE } from "./enemyPalette";
import { BEAM_COLOR, BEAM_MIN_DELTA_E, GOLD_BODY, GOLD_EMISSIVE } from "./giftPalette";
import { COMMON } from "./theme";

describe("gift palette", () => {
  // The straggler beam marks a REAL gift in the last 20% of a level. LURE's shimmer
  // wears the normal gift glow (JUDGMENT CALL 2), so if the beam shares that colour the
  // marker stops meaning "this one is real". It used to be byte-identical.
  it("does not wear the normal gift glow, which is also LURE's disguise", () => {
    expect(BEAM_COLOR).not.toBe(COMMON.gift.glow);
    expect(deltaE(BEAM_COLOR, COMMON.gift.glow)).toBeGreaterThanOrEqual(BEAM_MIN_DELTA_E);
  });

  it("is distinct from the golden gift", () => {
    expect(deltaE(BEAM_COLOR, GOLD_BODY)).toBeGreaterThanOrEqual(BEAM_MIN_DELTA_E);
    expect(deltaE(BEAM_COLOR, GOLD_EMISSIVE)).toBeGreaterThanOrEqual(BEAM_MIN_DELTA_E);
  });

  it("is distinct from the beacon", () => {
    expect(deltaE(BEAM_COLOR, COMMON.beacon.beam)).toBeGreaterThanOrEqual(BEAM_MIN_DELTA_E);
    expect(deltaE(BEAM_COLOR, COMMON.beacon.core)).toBeGreaterThanOrEqual(BEAM_MIN_DELTA_E);
  });

  it("is distinct from every enemy accent", () => {
    const clashes = ENEMY_LOOKS.map((l) => ({ id: l.behavior, d: deltaE(BEAM_COLOR, l.accent) }))
      .filter((x) => x.d < BEAM_MIN_DELTA_E)
      .map((x) => `${x.id} ${x.d.toFixed(1)}`);
    expect(clashes).toEqual([]);
  });

  it("keeps the golden gift distinct from the normal gift", () => {
    // Gold body vs the pale normal glow: the two gift tiers must not read as one.
    expect(deltaE(GOLD_BODY, COMMON.gift.glow)).toBeGreaterThanOrEqual(BEAM_MIN_DELTA_E);
  });
});
