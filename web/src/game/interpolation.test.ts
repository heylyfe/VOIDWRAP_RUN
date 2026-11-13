import { describe, expect, it } from "vitest";
import { INTERP_DELAY_MS, SnapshotBuffer } from "./interpolation";

describe("SnapshotBuffer", () => {
  it("interpolates between bracketing snapshots, delayed", () => {
    const b = new SnapshotBuffer();
    b.push(1000, [{ id: 1, pos: { x: 0, y: 0, z: 0 } }]);
    b.push(1050, [{ id: 1, pos: { x: 10, y: 0, z: 0 } }]);
    expect(b.sample(1025 + INTERP_DELAY_MS).get(1)!.x).toBeCloseTo(5);
    expect(b.sample(5000).get(1)!.x).toBe(10); // clamps to newest
    expect(b.sample(0).get(1)!.x).toBe(0); // clamps to oldest
  });

  it("drops entities that left", () => {
    const b = new SnapshotBuffer();
    b.push(0, [{ id: 1, pos: { x: 0, y: 0, z: 0 } }]);
    b.push(50, []);
    expect(b.sample(10_000).size).toBe(0);
  });
});
