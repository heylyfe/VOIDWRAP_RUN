// One play session: owns the connection, the predictor and the input state, and runs the
// fixed-step client loop. Rendering reads from it every frame; React only sees HUD state.
import {
  MAX_INPUTS_PER_MSG,
  PHYSICS_HZ,
  PROTOCOL_VERSION,
  type InputFrame,
  type LevelData,
  type ServerMsg,
  type Vec3,
} from "@/protocol/generated";
import { buttonsFromKeys, emptyKeys, wishFromKeys, type KeyState } from "./input";
import { SnapshotBuffer } from "./interpolation";
import { Connection } from "./net";
import { Predictor } from "./prediction";
import type { Stepper } from "./sim";
import { useGameStore } from "./store";

export const DT = 1 / PHYSICS_HZ;
/** Longest frame we simulate in one go (tab was in background, debugger pause, ...). */
const MAX_FRAME_DT = 0.25;

export class GameSession {
  readonly keys: KeyState = emptyKeys();
  rightMouse = false;
  /** Camera yaw/pitch in radians; see input.ts for the convention. */
  yaw = 0;
  pitch = 0.35;
  level: LevelData | null = null;
  playerId: number | null = null;
  onLevel: (level: LevelData) => void = () => {};

  private readonly predictor: Predictor;
  private readonly remote = new SnapshotBuffer();
  private readonly conn: Connection;
  private accumulator = 0;
  private outbox: InputFrame[] = [];
  private pingTimer: ReturnType<typeof setInterval> | null = null;

  constructor(
    private readonly sim: Stepper,
    url: string,
    private readonly name: string,
  ) {
    this.predictor = new Predictor(sim);
    const store = useGameStore.getState();
    store.set({ status: "connecting", error: null });
    this.conn = new Connection(url, {
      onOpen: () => {
        this.conn.send({ type: "Hello", protocolVersion: PROTOCOL_VERSION, name: this.name });
        this.pingTimer = setInterval(() => this.conn.send({ type: "Ping", t: Math.floor(performance.now()) >>> 0 }), 1000);
        useGameStore.getState().set({ status: "connected" });
      },
      onMessage: (m) => this.onMessage(m),
      onClose: (reason) => {
        this.stopPing();
        useGameStore.getState().set({ status: "closed", error: useGameStore.getState().error ?? reason });
      },
    });
  }

  private stopPing() {
    if (this.pingTimer) clearInterval(this.pingTimer);
    this.pingTimer = null;
  }

  private onMessage(msg: ServerMsg) {
    const store = useGameStore.getState();
    switch (msg.type) {
      case "Welcome":
        this.playerId = msg.playerId;
        store.set({ playerId: msg.playerId });
        return;
      case "Level":
        this.sim.loadLevel(msg.data);
        this.level = msg.data;
        this.onLevel(msg.data);
        return;
      case "Snapshot": {
        if (!this.level) return;
        if (this.predictor.state === null) this.predictor.reset(msg.you);
        else this.predictor.reconcile(msg.lastInputSeq, msg.you);
        this.remote.push(
          performance.now(),
          msg.players.filter((p) => p.id !== this.playerId),
        );
        store.set({ serverTick: msg.tick, playerCount: msg.players.length });
        return;
      }
      case "Pong": {
        const rtt = ((Math.floor(performance.now()) >>> 0) - msg.t) >>> 0;
        if (rtt < 60_000) store.set({ pingMs: rtt });
        return;
      }
      case "Error":
        store.set({ error: msg.message });
        return;
    }
  }

  /** Advance the fixed-step loop by one rendered frame and flush inputs to the server. */
  update(dtSeconds: number): void {
    if (!this.level || this.predictor.state === null) return;
    this.accumulator += Math.min(dtSeconds, MAX_FRAME_DT);
    while (this.accumulator >= DT) {
      const { moveX, moveZ } = wishFromKeys(this.keys, this.yaw);
      const frame = this.predictor.advance(moveX, moveZ, buttonsFromKeys(this.keys, this.rightMouse));
      if (frame) this.outbox.push(frame);
      this.accumulator -= DT;
    }
    while (this.outbox.length > 0) {
      this.conn.send({ type: "Inputs", frames: this.outbox.splice(0, MAX_INPUTS_PER_MSG) });
    }
  }

  playerPosition(dtSeconds: number): Vec3 | null {
    return this.predictor.renderPosition(this.accumulator / DT, dtSeconds);
  }

  remotePlayers(now: number): Map<number, Vec3> {
    return this.remote.sample(now);
  }

  dispose(): void {
    this.stopPing();
    this.conn.close();
  }
}
