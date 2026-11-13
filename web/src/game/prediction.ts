// Client-side prediction with server reconciliation for the local player.
//
// Every physics step the client records an InputFrame, applies it locally, and sends it.
// Each server snapshot says which seq it last applied and the authoritative state after it;
// we rewind to that state and replay every still-unacknowledged frame. Any resulting jump in
// position is hidden by a visual offset that decays over a few frames (or snaps if large).
import type { ControllerState, InputFrame, Vec3 } from "@/protocol/generated";
import type { Stepper } from "./sim";

/** Never keep more than ~2 s of unacknowledged input; beyond that the link is effectively dead. */
export const MAX_PENDING = 120;
/** Corrections larger than this (units) snap instead of smoothing (respawns, teleports). */
export const SNAP_DISTANCE = 4;
/** Visual error decay rate, 1/s. */
const SMOOTHING_RATE = 15;

const sub = (a: Vec3, b: Vec3): Vec3 => ({ x: a.x - b.x, y: a.y - b.y, z: a.z - b.z });
const add = (a: Vec3, b: Vec3): Vec3 => ({ x: a.x + b.x, y: a.y + b.y, z: a.z + b.z });
const len = (v: Vec3) => Math.hypot(v.x, v.y, v.z);
const ZERO: Vec3 = { x: 0, y: 0, z: 0 };

export class Predictor {
  private pending: InputFrame[] = [];
  private nextSeq = 1;
  private previous: ControllerState | null = null;
  private current: ControllerState | null = null;
  /** Visual-only offset added to the rendered position after a correction. */
  private offset: Vec3 = ZERO;

  constructor(private readonly sim: Stepper) {}

  get state(): ControllerState | null {
    return this.current;
  }

  get pendingCount(): number {
    return this.pending.length;
  }

  /** Hard reset to an authoritative state (join, new level). */
  reset(state: ControllerState): void {
    this.sim.setState(state);
    this.previous = state;
    this.current = state;
    this.pending = [];
    this.offset = ZERO;
  }

  /** Apply one physics step of local input. Returns the frame to send to the server. */
  advance(moveX: number, moveZ: number, buttons: number): InputFrame | null {
    if (!this.current) return null;
    const frame: InputFrame = { seq: this.nextSeq++, moveX, moveZ, buttons };
    this.pending.push(frame);
    if (this.pending.length > MAX_PENDING) this.pending.shift();
    this.sim.step(moveX, moveZ, buttons);
    this.previous = this.current;
    this.current = this.sim.getState();
    return frame;
  }

  /** Rewind to the server's state after `ack` and replay the unacknowledged frames. */
  reconcile(ack: number, authoritative: ControllerState): void {
    if (!this.current || !this.previous) {
      this.reset(authoritative);
      return;
    }
    this.pending = this.pending.filter((f) => f.seq > ack);
    this.sim.setState(authoritative);
    for (const f of this.pending) this.sim.step(f.moveX, f.moveZ, f.buttons);
    const corrected = this.sim.getState();
    const error = sub(this.current.pos, corrected.pos);
    if (len(error) > SNAP_DISTANCE) {
      this.offset = ZERO;
      this.previous = corrected;
    } else {
      this.offset = add(this.offset, error);
      // Shift the interpolation origin too, so the rendered path stays continuous.
      this.previous = { ...this.previous, pos: sub(this.previous.pos, error) };
    }
    this.current = corrected;
  }

  /** Position to draw: interpolated between the last two steps plus the decaying offset. */
  renderPosition(alpha: number, dtSeconds: number): Vec3 | null {
    if (!this.current || !this.previous) return null;
    const k = Math.exp(-SMOOTHING_RATE * dtSeconds);
    this.offset = len(this.offset) < 1e-4 ? ZERO : { x: this.offset.x * k, y: this.offset.y * k, z: this.offset.z * k };
    const a = Math.min(Math.max(alpha, 0), 1);
    const p = this.previous.pos;
    const c = this.current.pos;
    return {
      x: p.x + (c.x - p.x) * a + this.offset.x,
      y: p.y + (c.y - p.y) * a + this.offset.y,
      z: p.z + (c.z - p.z) * a + this.offset.z,
    };
  }
}
