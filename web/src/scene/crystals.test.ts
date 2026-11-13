import { describe, expect, it } from "vitest";
import type { Platform } from "@/protocol/generated";
import { crystalLayout } from "./Crystals";
import type { CrystalSpot } from "./Crystals";

const PLATFORMS: readonly Platform[] = [
  { id: 1, kind: 0, bounds: { min: { x: -4, y: 0, z: -4 }, max: { x: 4, y: 1, z: 4 } } },
  { id: 2, kind: 0, bounds: { min: { x: 6, y: 2, z: -3 }, max: { x: 12, y: 3.5, z: 3 } } },
  { id: 3, kind: 1, bounds: { min: { x: -2, y: 4, z: 6 }, max: { x: 2, y: 5, z: 10 } } },
];

const SEEDS: readonly number[] = [1, 2, 3, 5, 8, 13, 21, 34];

function insideFootprint(spot: CrystalSpot, platform: Platform): boolean {
  const { min, max } = platform.bounds;
  return (
    spot.position[0] >= min.x &&
    spot.position[0] <= max.x &&
    spot.position[2] >= min.z &&
    spot.position[2] <= max.z
  );
}

describe("crystalLayout", () => {
  it("is deterministic for a given seed", () => {
    const first = crystalLayout(7, PLATFORMS, 2);
    const second = crystalLayout(7, PLATFORMS, 2);
    expect(first).toEqual(second);
  });

  it("changes when the seed changes", () => {
    const signatures = SEEDS.map((seed) => JSON.stringify(crystalLayout(seed, PLATFORMS, 2)));
    expect(new Set(signatures).size).toBeGreaterThan(1);
  });

  it("produces clusters for at least one seed", () => {
    const total = SEEDS.reduce((sum, seed) => sum + crystalLayout(seed, PLATFORMS, 2).length, 0);
    expect(total).toBeGreaterThan(0);
  });

  it("never places a crystal on or above a walkable top", () => {
    for (const seed of SEEDS) {
      const spots = crystalLayout(seed, PLATFORMS, 2);
      for (const spot of spots) {
        for (const platform of PLATFORMS) {
          if (!insideFootprint(spot, platform)) continue;
          // Inside a footprint the only legal place is strictly underneath it.
          expect(spot.position[1]).toBeLessThan(platform.bounds.min.y);
        }
      }
    }
  });

  it("hangs some clusters below a platform underside", () => {
    let hanging = 0;
    for (const seed of SEEDS) {
      const spots = crystalLayout(seed, PLATFORMS, 2);
      hanging += spots.filter((spot) =>
        PLATFORMS.some(
          (platform) => insideFootprint(spot, platform) && spot.position[1] < platform.bounds.min.y,
        ),
      ).length;
    }
    expect(hanging).toBeGreaterThan(0);
  });

  it("keeps scale and tilt inside their ranges", () => {
    for (const seed of SEEDS) {
      for (const spot of crystalLayout(seed, PLATFORMS, 2)) {
        expect(spot.scale).toBeGreaterThanOrEqual(0.25);
        expect(spot.scale).toBeLessThanOrEqual(1.1);
        expect(Math.abs(spot.tilt[0])).toBeLessThanOrEqual(0.5);
        expect(Math.abs(spot.tilt[1])).toBeLessThanOrEqual(0.5);
        expect(Number.isFinite(spot.yaw)).toBe(true);
      }
    }
  });

  it("respects perPlatform and empty inputs", () => {
    expect(crystalLayout(3, PLATFORMS, 0)).toEqual([]);
    expect(crystalLayout(3, [], 2)).toEqual([]);

    for (const seed of SEEDS) {
      const spots = crystalLayout(seed, PLATFORMS, 1);
      expect(spots.length).toBeLessThanOrEqual(PLATFORMS.length * 1 * 7);
    }
  });
});
