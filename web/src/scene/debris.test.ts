import { describe, expect, it } from "vitest";
import { debrisLayout } from "./Debris";
import { mulberry32 } from "./rng";

const CENTER = { x: 10, y: 4, z: -6 };
const RADIUS = 40;
const EPSILON = 1e-6;

function distance(a: [number, number, number], b: [number, number, number]): number {
  return Math.hypot(a[0] - b[0], a[1] - b[1], a[2] - b[2]);
}

describe("mulberry32", () => {
  it("is deterministic for the same seed", () => {
    const first = mulberry32(42);
    const second = mulberry32(42);
    const firstSequence = Array.from({ length: 16 }, () => first());
    const secondSequence = Array.from({ length: 16 }, () => second());
    expect(firstSequence).toEqual(secondSequence);
  });

  it("produces different sequences for different seeds", () => {
    const first = mulberry32(1);
    const second = mulberry32(2);
    const firstSequence = Array.from({ length: 16 }, () => first());
    const secondSequence = Array.from({ length: 16 }, () => second());
    expect(firstSequence).not.toEqual(secondSequence);
  });

  it("returns floats in [0, 1)", () => {
    const rand = mulberry32(7);
    for (let i = 0; i < 1000; i += 1) {
      const value = rand();
      expect(value).toBeGreaterThanOrEqual(0);
      expect(value).toBeLessThan(1);
    }
  });
});

describe("debrisLayout", () => {
  it("is deterministic for the same seed", () => {
    const first = debrisLayout(1234, 60, CENTER, RADIUS);
    const second = debrisLayout(1234, 60, CENTER, RADIUS);
    expect(first).toEqual(second);
  });

  it("differs for a different seed", () => {
    const first = debrisLayout(1234, 60, CENTER, RADIUS);
    const second = debrisLayout(4321, 60, CENTER, RADIUS);
    expect(first).not.toEqual(second);
  });

  it("returns exactly the requested number of items", () => {
    expect(debrisLayout(9, 140, CENTER, RADIUS)).toHaveLength(140);
    expect(debrisLayout(9, 1, CENTER, RADIUS)).toHaveLength(1);
    expect(debrisLayout(9, 0, CENTER, RADIUS)).toHaveLength(0);
  });

  it("keeps every item inside the horizontal shell and the y band", () => {
    const items = debrisLayout(77, 200, CENTER, RADIUS);
    for (const item of items) {
      const dx = item.position[0] - CENTER.x;
      const dz = item.position[2] - CENTER.z;
      const horizontal = Math.hypot(dx, dz);
      expect(horizontal).toBeGreaterThanOrEqual(RADIUS * 0.55 - EPSILON);
      expect(horizontal).toBeLessThanOrEqual(RADIUS * 1.15 + EPSILON);
      expect(item.position[1]).toBeGreaterThanOrEqual(CENTER.y - 12 - EPSILON);
      expect(item.position[1]).toBeLessThanOrEqual(CENTER.y + 18 + EPSILON);
    }
  });

  it("keeps scale, spin, orbit speed and bob phase in range", () => {
    const items = debrisLayout(5, 200, CENTER, RADIUS);
    for (const item of items) {
      expect(item.scale).toBeGreaterThanOrEqual(0.15);
      expect(item.scale).toBeLessThanOrEqual(0.9);
      for (const axis of item.spin) {
        expect(axis).toBeGreaterThanOrEqual(-1.2);
        expect(axis).toBeLessThanOrEqual(1.2);
      }
      expect(item.orbitSpeed).toBeGreaterThanOrEqual(-0.03);
      expect(item.orbitSpeed).toBeLessThanOrEqual(0.03);
      expect(item.bobPhase).toBeGreaterThanOrEqual(0);
      expect(item.bobPhase).toBeLessThan(Math.PI * 2);
    }
  });

  it("groups roughly 30% of the items into clusters", () => {
    const items = debrisLayout(2024, 100, CENTER, RADIUS);
    let clustered = 0;
    for (const item of items) {
      const hasNeighbour = items.some(
        (other) => other !== item && distance(item.position, other.position) < 6,
      );
      if (hasNeighbour) clustered += 1;
    }
    expect(clustered).toBeGreaterThanOrEqual(20);
  });
});
