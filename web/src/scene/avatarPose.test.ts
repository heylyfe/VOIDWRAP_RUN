import { describe, expect, it } from "vitest";
import { createPoseState, stepPose } from "./avatarPose";
import type { AvatarMotion } from "./avatarPose";

const DT = 1 / 60;

function motionOf(partial: Partial<AvatarMotion>): AvatarMotion {
  return { speed: 0, vy: 0, grounded: true, dashing: false, ...partial };
}

function stepN(state: ReturnType<typeof createPoseState>, m: AvatarMotion, n: number): void {
  for (let i = 0; i < n; i++) {
    stepPose(state, m, DT);
  }
}

describe("avatarPose", () => {
  it("advances run cadence phase faster at higher speed", () => {
    const slow = createPoseState();
    const fast = createPoseState();
    stepN(slow, motionOf({ speed: 4.5 }), 1);
    stepN(fast, motionOf({ speed: 9 }), 1);
    expect(fast.phase).toBeGreaterThan(slow.phase);
  });

  it("breathes while idle starting from a fresh pose state", () => {
    const state = createPoseState();
    let maxAbsBodyY = 0;
    for (let i = 0; i < 60; i++) {
      stepN(state, motionOf({ speed: 0 }), 1);
      maxAbsBodyY = Math.max(maxAbsBodyY, Math.abs(state.pose.bodyY));
    }
    expect(maxAbsBodyY).toBeGreaterThan(0.005);
  });

  it("keeps phase in [0, 1) after 10000 steps", () => {
    const state = createPoseState();
    stepN(state, motionOf({ speed: 9 }), 10000);
    expect(state.phase).toBeGreaterThanOrEqual(0);
    expect(state.phase).toBeLessThan(1);
    expect(Number.isFinite(state.phase)).toBe(true);
  });

  it("triggers land squash on landing and recovers after 0.2 s", () => {
    const state = createPoseState();
    stepN(state, motionOf({ speed: 0, vy: -10, grounded: false }), 1);
    stepN(state, motionOf({ speed: 0.6, vy: 0, grounded: true }), 1);
    expect(state.pose.scaleY).toBeLessThan(0.9);
    stepN(state, motionOf({ speed: 0.6, vy: 0, grounded: true }), 12);
    expect(state.pose.scaleY).toBeGreaterThan(0.95);
  });

  it("leans forward strongly while dashing", () => {
    const state = createPoseState();
    stepN(state, motionOf({ speed: 28, vy: 0, grounded: true, dashing: true }), 6);
    expect(state.pose.torsoPitch).toBeGreaterThan(0.4);
  });

  it("raises the arms overhead while jump rising", () => {
    const state = createPoseState();
    stepN(state, motionOf({ speed: 0, vy: 12, grounded: false }), 6);
    expect(state.pose.leftArm).toBeLessThan(-1);
    expect(state.pose.rightArm).toBeLessThan(-1);
  });

  it("produces finite output for dt 0, huge speeds and huge dt", () => {
    const state = createPoseState();
    stepPose(state, motionOf({ speed: 0 }), 0);
    stepN(state, motionOf({ speed: 0 }), 1);
    stepPose(state, motionOf({ speed: 1e12, vy: -1e9, grounded: false }), 0.05);
    stepPose(state, motionOf({ speed: 1e12, vy: 1e9, grounded: true, dashing: true }), 1e9);
    const p = state.pose;
    const values = [
      p.leftArm,
      p.rightArm,
      p.leftArmRoll,
      p.rightArmRoll,
      p.leftLeg,
      p.rightLeg,
      p.torsoPitch,
      p.bodyY,
      p.scaleY,
      p.scaleXZ,
    ];
    for (const v of values) {
      expect(Number.isFinite(v)).toBe(true);
    }
  });
});
