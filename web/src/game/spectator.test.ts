"web/src/game/spectator.test.ts";
import { describe, expect, it } from "vitest";
import {
  FAST_MULTIPLIER,
  FLY_SPEED,
  followCamera,
  flyStep,
  nextFollow,
  type Vec,
} from "./spectator";

const NO_KEYS = {
  forward: false,
  back: false,
  left: false,
  right: false,
  up: false,
  down: false,
  fast: false,
};

function keys(overrides: Partial<typeof NO_KEYS>): typeof NO_KEYS {
  return { ...NO_KEYS, ...overrides };
}

function vec(x: number, y: number, z: number): Vec {
  return { x, y, z };
}

describe("flyStep", () => {
  it("moves -Z at FLY_SPEED for forward at yaw 0 pitch 0", () => {
    const pos = vec(10, 5, 20);
    flyStep(pos, keys({ forward: true }), 0, 0, 0.1);
    expect(pos.x).toBeCloseTo(10, 10);
    expect(pos.y).toBeCloseTo(5, 10);
    expect(pos.z).toBeCloseTo(20 - FLY_SPEED * 0.1, 10);
  });

  it("supports back as the opposite of forward", () => {
    const pos = vec(0, 0, 0);
    flyStep(pos, keys({ back: true }), 0, 0, 0.1);
    expect(pos.z).toBeCloseTo(FLY_SPEED * 0.1, 10);
  });

  it("lowers y when looking down while moving forward", () => {
    const pitch = Math.PI / 6; // positive pitch = looking down
    const dt = 0.1;
    const pos = vec(0, 0, 0);
    flyStep(pos, keys({ forward: true }), 0, pitch, dt);
    expect(pos.y).toBeLessThan(0);
    expect(pos.z).toBeLessThan(0);
    // Direction is the look direction: (-sin*0*cosP, -sinP, -cosP)
    expect(pos.y).toBeCloseTo(-Math.sin(pitch) * FLY_SPEED * dt, 10);
    expect(pos.z).toBeCloseTo(-Math.cos(pitch) * FLY_SPEED * dt, 10);
  });

  it("moves only along Y for up/down", () => {
    const pos = vec(3, 4, 5);
    flyStep(pos, keys({ up: true }), 1.234, 0.5, 0.1);
    expect(pos.x).toBeCloseTo(3, 10);
    expect(pos.y).toBeCloseTo(4 + FLY_SPEED * 0.1, 10);
    expect(pos.z).toBeCloseTo(5, 10);

    const posDown = vec(3, 4, 5);
    flyStep(posDown, keys({ down: true }), -2.5, -0.9, 0.1);
    expect(posDown.y).toBeCloseTo(4 - FLY_SPEED * 0.1, 10);
    expect(posDown.x).toBeCloseTo(3, 10);
    expect(posDown.z).toBeCloseTo(5, 10);
  });

  it("strafes left/right on the horizontal plane", () => {
    const pos = vec(0, 0, 0);
    flyStep(pos, keys({ right: true }), 0, 0, 0.1);
    // right = (cos 0, 0, -sin 0) = (1, 0, 0), forward is -Z so right is +X
    expect(pos.x).toBeCloseTo(FLY_SPEED * 0.1, 10);
    expect(pos.z).toBeCloseTo(0, 10);
    expect(pos.y).toBeCloseTo(0, 10);

    const posLeft = vec(0, 0, 0);
    flyStep(posLeft, keys({ left: true }), 0, 0, 0.1);
    expect(posLeft.x).toBeCloseTo(-FLY_SPEED * 0.1, 10);
  });

  it("multiplies speed when fast is held", () => {
    const pos = vec(0, 0, 0);
    flyStep(pos, keys({ forward: true, fast: true }), 0, 0, 0.1);
    expect(pos.z).toBeCloseTo(-FLY_SPEED * FAST_MULTIPLIER * 0.1, 10);
  });

  it("normalises diagonals so combined speed equals straight speed", () => {
    const pos = vec(0, 0, 0);
    flyStep(pos, keys({ forward: true, right: true, up: true }), 0, 0, 0.1);
    const travelled = Math.hypot(pos.x, pos.y, pos.z);
    expect(travelled).toBeCloseTo(FLY_SPEED * 0.1, 6);
  });

  it("clamps dt to [0, 0.1]", () => {
    // Oversized dt behaves exactly like dt = 0.1.
    const clamped = vec(0, 0, 0);
    flyStep(clamped, keys({ forward: true }), 0, 0, 5);
    expect(clamped.z).toBeCloseTo(-FLY_SPEED * 0.1, 10);

    // Negative dt is treated as 0, so nothing moves.
    const negative = vec(0, 0, 0);
    flyStep(negative, keys({ forward: true }), 0, 0, -1);
    expect(negative.x).toBe(0);
    expect(negative.y).toBe(0);
    expect(negative.z).toBe(0);
  });

  it("does not move with no keys held", () => {
    const pos = vec(1, 2, 3);
    flyStep(pos, NO_KEYS, 0.7, -0.3, 0.1);
    expect(pos).toEqual(vec(1, 2, 3));
  });
});

describe("followCamera", () => {
  it("places the camera at target - lookDir * distance + (0, 1.4, 0)", () => {
    const target = vec(10, 2, -4);
    const cam = followCamera(target, 0, 0, 8);
    // look = (0, 0, -1); camera is behind along +Z, lifted by 1.4
    expect(cam.x).toBeCloseTo(10, 10);
    expect(cam.y).toBeCloseTo(2 + 1.4, 10);
    expect(cam.z).toBeCloseTo(-4 + 8, 10);
  });

  it("honours yaw and pitch in the orbit", () => {
    const target = vec(0, 0, 0);
    const yaw = Math.PI / 2; // look = (-1, 0, 0)
    const pitch = Math.PI / 6; // positive pitch = looking down
    const distance = 5;
    const cam = followCamera(target, yaw, pitch, distance);
    const lookX = -Math.sin(yaw) * Math.cos(pitch);
    const lookZ = -Math.cos(yaw) * Math.cos(pitch);
    // cam = target - look * distance + (0, 1.4, 0); looking down lifts the
    // camera above the target: y = -(-sin(pitch)) * distance + 1.4.
    expect(cam.x).toBeCloseTo(-lookX * distance, 10);
    expect(cam.y).toBeCloseTo(Math.sin(pitch) * distance + 1.4, 10);
    expect(cam.z).toBeCloseTo(-lookZ * distance, 10);
  });
});

describe("nextFollow", () => {
  it("returns 0 for null", () => {
    expect(nextFollow(null, 3)).toBe(0);
  });

  it("advances within range", () => {
    expect(nextFollow(0, 3)).toBe(1);
    expect(nextFollow(1, 3)).toBe(2);
  });

  it("wraps around", () => {
    expect(nextFollow(2, 3)).toBe(0);
    expect(nextFollow(9, 4)).toBe(2);
  });

  it("returns null when there is nothing to follow", () => {
    expect(nextFollow(null, 0)).toBeNull();
    expect(nextFollow(2, 0)).toBeNull();
  });
});
