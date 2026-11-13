import { describe, expect, it } from "vitest";
import { BTN_ABILITY, BTN_JUMP } from "@/protocol/generated";
import { buttonsFromKeys, emptyKeys, wishFromKeys } from "./input";

describe("wishFromKeys", () => {
  it("forward at yaw 0 is -Z", () => {
    expect(wishFromKeys({ ...emptyKeys(), forward: true }, 0)).toEqual({ moveX: 0, moveZ: -127 });
  });

  it("right at yaw 0 is +X; forward after turning 90° left is -X", () => {
    expect(wishFromKeys({ ...emptyKeys(), right: true }, 0)).toEqual({ moveX: 127, moveZ: 0 });
    expect(wishFromKeys({ ...emptyKeys(), forward: true }, Math.PI / 2)).toEqual({ moveX: -127, moveZ: 0 });
  });

  it("normalises diagonals and cancels opposing keys", () => {
    const d = wishFromKeys({ ...emptyKeys(), forward: true, right: true }, 0);
    expect(Math.hypot(d.moveX, d.moveZ)).toBeLessThanOrEqual(127.5);
    expect(wishFromKeys({ ...emptyKeys(), left: true, right: true }, 1)).toEqual({ moveX: 0, moveZ: 0 });
  });

  it("maps buttons", () => {
    expect(buttonsFromKeys({ ...emptyKeys(), jump: true }, true)).toBe(BTN_JUMP | BTN_ABILITY);
  });
});
