import { describe, expect, it } from "vitest";
import type { ControllerState, LevelData } from "@/protocol/generated";
import { MAX_PENDING, Predictor, SNAP_DISTANCE } from "./prediction";
import type { Stepper } from "./sim";

function state(x: number): ControllerState {
  const zero = { x: 0, y: 0, z: 0 };
  return {
    pos: { x, y: 0, z: 0 },
    vel: zero,
    grounded: true,
    coyoteFrames: 0,
    jumpBufferFrames: 0,
    prevButtons: 0,
    airJumpsUsed: 0,
    abilityCooldownFrames: 0,
    abilityActiveFrames: 0,
    abilityDir: zero,
    class: 0,
  };
}

/** Toy deterministic stepper: x += moveX / 127 per step. */
class FakeStepper implements Stepper {
  s: ControllerState = state(0);
  loadLevel(_level: LevelData) {}
  setTuning() {}
  setTiles() {}
  setState(s: ControllerState) {
    this.s = structuredClone(s);
  }
  step(moveX: number) {
    this.s = { ...this.s, pos: { ...this.s.pos, x: this.s.pos.x + moveX / 127 } };
  }
  getState() {
    return structuredClone(this.s);
  }
}

describe("Predictor", () => {
  it("replays unacknowledged inputs on top of the server state", () => {
    const p = new Predictor(new FakeStepper());
    p.reset(state(0));
    for (let i = 0; i < 5; i++) p.advance(127, 0, 0); // x = 5, seqs 1..5
    p.reconcile(3, state(3)); // server applied 1..3 and agrees
    expect(p.state!.pos.x).toBe(5);
    expect(p.pendingCount).toBe(2);
  });

  it("corrects mispredictions and hides small errors with a decaying offset", () => {
    const p = new Predictor(new FakeStepper());
    p.reset(state(0));
    for (let i = 0; i < 4; i++) p.advance(127, 0, 0); // x = 4
    p.reconcile(2, state(1)); // server: x=1 after 2 steps → corrected x = 3
    expect(p.state!.pos.x).toBe(3);
    const first = p.renderPosition(1, 0)!.x;
    expect(first).toBeCloseTo(4); // visually still where we were
    let x = first;
    for (let i = 0; i < 60; i++) x = p.renderPosition(1, 1 / 60)!.x;
    expect(x).toBeCloseTo(3, 3);
  });

  it("snaps on large corrections", () => {
    const p = new Predictor(new FakeStepper());
    p.reset(state(0));
    p.advance(127, 0, 0);
    p.reconcile(1, state(SNAP_DISTANCE + 10));
    expect(p.renderPosition(1, 0)!.x).toBe(SNAP_DISTANCE + 10);
  });

  it("bounds the pending queue", () => {
    const p = new Predictor(new FakeStepper());
    p.reset(state(0));
    for (let i = 0; i < MAX_PENDING + 50; i++) p.advance(0, 0, 0);
    expect(p.pendingCount).toBe(MAX_PENDING);
  });
});
