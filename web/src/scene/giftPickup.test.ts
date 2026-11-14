import { describe, expect, it } from "vitest";
import { burstParticles, findGiftPosition, newlyTaken } from "./giftPickup";

describe("newlyTaken", () => {
  it("returns ids present in next but not in prev, in next order", () => {
    expect(newlyTaken([1, 2], [1, 2, 3, 4])).toEqual([3, 4]);
  });

  it("deduplicates within next and never reports removed ids", () => {
    expect(newlyTaken([5], [5, 5, 6])).toEqual([6]);
    expect(newlyTaken([1, 2], [2])).toEqual([]);
  });

  it("treats an empty prev as everything being new", () => {
    expect(newlyTaken([], [7, 8])).toEqual([7, 8]);
  });
});

describe("findGiftPosition", () => {
  const gifts = [
    { id: 3, pos: { x: 1, y: 2, z: 3 } },
    { id: 9, pos: { x: 4, y: 5, z: 6 } },
  ];

  it("finds a gift by id", () => {
    expect(findGiftPosition(gifts, 9)).toEqual({ x: 4, y: 5, z: 6 });
  });

  it("returns null for an unknown id", () => {
    expect(findGiftPosition(gifts, 42)).toBeNull();
  });
});

describe("burstParticles", () => {
  it("is deterministic for the same seed", () => {
    expect(burstParticles("same-seed", 12)).toEqual(burstParticles("same-seed", 12));
  });

  it("produces the requested count with finite, upward-biased directions and positive speed and size", () => {
    const particles = burstParticles("seed", 12);
    expect(particles).toHaveLength(12);
    for (const p of particles) {
      expect(Number.isFinite(p.dirX)).toBe(true);
      expect(Number.isFinite(p.dirY)).toBe(true);
      expect(Number.isFinite(p.dirZ)).toBe(true);
      expect(Math.hypot(p.dirX, p.dirY, p.dirZ)).toBeGreaterThan(0);
      expect(p.dirY).toBeGreaterThan(0);
      expect(p.speed).toBeGreaterThan(0);
      expect(p.size).toBeGreaterThan(0);
    }
  });

  it("clamps a negative count to zero", () => {
    expect(burstParticles("seed", -3)).toEqual([]);
  });
});
