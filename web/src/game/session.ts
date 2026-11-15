// One play session: owns the connection, the predictor and the input state, and runs the
// fixed-step client loop. Rendering reads from it every frame; React only sees HUD state.
import {
  MAX_INPUTS_PER_MSG,
  PHASE_ENEMY_DRAFT,
  PHASE_PLAYING,
  PLAYER_ACTIVE,
  PHYSICS_HZ,
  PROTOCOL_VERSION,
  type ControllerState,
  type InputFrame,
  type LevelData,
  type ServerMsg,
  type TileState,
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
  /** Spectator follow target (index into the players still playing), or null for free flight. */
  followIndex: number | null = null;
  level: LevelData | null = null;
  playerId: number | null = null;
  onLevel: (level: LevelData) => void = () => {};

  private readonly predictor: Predictor;
  private lastTick: number | null = null;
  private minTick: number | null = null;
  private readonly remote = new SnapshotBuffer();
  private readonly enemies = new SnapshotBuffer();
  /** Body radius per enemy id, from the latest snapshot (draw size = server body size). */
  readonly enemyRadius = new Map<number, number>();
  /** Enemy state (ENEMY_DORMANT / HUNTING / DISGUISED) and content kind per enemy id, latest snapshot. */
  readonly enemyInfo = new Map<
    number,
    { kind: number; state: number; target: Vec3; prog: number; flags: number }
  >();
  private readonly projectiles = new SnapshotBuffer();
  private noticeSeq = 0;
  private needsReset = true;
  private readonly conn: Connection;
  private accumulator = 0;
  private outbox: InputFrame[] = [];
  private pingTimer: ReturnType<typeof setInterval> | null = null;
  /** The command in flight, so the answer can be logged against what was sent. */
  private pendingDebug: { command: string; args: string } | null = null;

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
        // New run or next level: everyone is teleported to the spawn, so the next
        // snapshot hard-resets prediction instead of reconciling across the jump.
        this.sim.loadLevel(msg.data);
        this.level = msg.data;
        this.needsReset = true;
        this.accumulator = 0;
        // Server ticks only grow: any snapshot not newer than the last one seen before this
        // Level is a leftover of the previous run (same level number on a restart) - drop it.
        this.minTick = this.lastTick;
        this.onLevel(msg.data);
        this.sim.setTiles([]);
        store.set({
          level: msg.data.level,
          giftsTotal: msg.data.gifts.length,
          giftsTaken: [],
          runOver: null,
          tiles: [],
          altarsUsed: [],
          levelPhase: 0,
          goldenTaken: [],
          collapseMs: 0,
        });
        return;
      case "Snapshot": {
        if (!this.level || msg.level !== this.level.level) return; // stale: from before a level change
        if (this.lastTick !== null && ((msg.tick - this.lastTick) >>> 0) > 0x7fffffff) return; // out of order
        if (this.minTick !== null) {
          if (((msg.tick - this.minTick) >>> 0) === 0 || ((msg.tick - this.minTick) >>> 0) > 0x7fffffff) return; // previous run
          this.minTick = null;
        }
        this.lastTick = msg.tick;
        if (this.needsReset || this.predictor.state === null) {
          this.predictor.reset(msg.you);
          this.needsReset = false;
        } else {
          this.predictor.reconcile(msg.lastInputSeq, msg.you);
        }
        const now = performance.now();
        this.remote.push(
          now,
          msg.players.filter((p) => p.id !== this.playerId && p.mode === PLAYER_ACTIVE),
        );
        this.enemies.push(now, msg.enemies);
        this.enemyRadius.clear();
        this.enemyInfo.clear();
        for (const en of msg.enemies) {
          this.enemyRadius.set(en.id, en.radius);
          this.enemyInfo.set(en.id, { kind: en.kind, state: en.state, target: en.target, prog: en.prog, flags: en.flags });
        }
        this.projectiles.push(now, msg.projectiles);
        if (!sameTiles(msg.tiles, store.tiles)) {
          // Crumbled platforms must stop being solid in the predictor too, or prediction diverges.
          this.sim.setTiles(msg.tiles);
          store.set({ tiles: msg.tiles });
        }
        if (msg.altarsUsed.length !== store.altarsUsed.length) store.set({ altarsUsed: msg.altarsUsed });
        if (msg.levelPhase !== store.levelPhase) store.set({ levelPhase: msg.levelPhase });
        if (msg.goldenTaken.length !== store.goldenTaken.length) store.set({ goldenTaken: msg.goldenTaken });
        // Coarse (0.5 s) so the HUD re-renders twice a second, not 20 times.
        const collapseHalfSecs = Math.ceil(msg.collapseMs / 500);
        if (collapseHalfSecs !== Math.ceil(store.collapseMs / 500)) store.set({ collapseMs: msg.collapseMs });
        if (msg.you.class !== store.myClass) store.set({ myClass: msg.you.class });
        const me = msg.players.find((p) => p.id === this.playerId);
        const activeIds = msg.players.filter((p) => p.mode === PLAYER_ACTIVE).map((p) => p.id);
        if (me && me.mode !== store.myMode) store.set({ myMode: me.mode });
        if (activeIds.join(",") !== store.activeIds.join(",")) store.set({ activeIds });
        if (msg.giftsTaken.length !== store.giftsTaken.length) store.set({ giftsTaken: msg.giftsTaken });
        // Debug-panel numbers: only written when they change visibly (1 Hz), not every snapshot.
        if (msg.tick % 20 === 0 || msg.players.length !== store.playerCount) {
          store.set({ serverTick: msg.tick, playerCount: msg.players.length });
        }
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
      case "RunOver":
        store.set({ runOver: msg.summary });
        return;
      case "RunState":
        this.sim.setTuning(msg.info.tuning);
        store.set({ runInfo: msg.info });
        if (msg.info.phase !== PHASE_ENEMY_DRAFT) store.set({ draft: null, shop: null });
        return;
      case "Shop":
        store.set({ shop: msg.items });
        return;
      case "Notice": {
        const notice = { seq: ++this.noticeSeq, kind: msg.kind, ids: msg.ids, at: performance.now() };
        store.set({ notices: [...store.notices.slice(-4), notice] });
        return;
      }
      case "DebugResult": {
        const previous = store.debugResult;
        store.set({
          debugResult: {
            seq: (previous?.seq ?? 0) + 1,
            command: this.pendingDebug?.command ?? "",
            args: this.pendingDebug?.args ?? "",
            ok: msg.ok,
            message: msg.message,
          },
        });
        this.pendingDebug = null;
        return;
      }
      case "Draft":
        store.set({
          draft: { enemyPicks: msg.enemyPicks, rewardPicks: msg.rewardPicks, cards: msg.cards, submitted: false },
          runOver: null,
        });
        return;
      // T-149: the pedestal vote. The server opens a session every level at the collapse
      // window and broadcasts these three; nothing in the client used to read them, so the
      // vote ran invisibly and `VoteSession::resolve` fell through to its random pick.
      // `RunVoteOverlay` replays this log through `useVote`.
      case "VoteOpen":
      case "VoteState":
      case "VoteResult":
        store.set({ voteMessages: [...store.voteMessages.slice(-63), msg] });
        return;
    }
  }

  /** Submit this player's draft picks (indices into the offered cards). */
  submitDraft(picks: number[]): void {
    const store = useGameStore.getState();
    if (!store.draft || store.draft.submitted) return;
    this.conn.send({ type: "DraftPick", picks });
    store.set({ draft: { ...store.draft, submitted: true } });
  }

  /**
   * Send one tester-panel command (T-111). The code goes to the server, which is the only
   * thing that decides whether the route is open; nothing is validated here.
   */
  debugCommand(code: string, command: string, args: string): void {
    this.pendingDebug = { command, args };
    this.conn.send({ type: "Debug", code, command, args });
  }

  /** Buy one shop item (lobby only; the server checks money and stock). */
  buy(itemId: string): void {
    this.conn.send({ type: "Buy", item: itemId });
  }

  /** Pick this player's class (CLASS_*; the server accepts it in the lobby before level 1). */
  chooseClass(cls: number): void {
    this.conn.send({ type: "ChooseClass", class: cls });
  }

  /**
   * Cast this player's pedestal ballot (T-149).
   *
   * The server opens a vote session every level at the collapse window and rejects a
   * ballot when no session is open, so the client does not need to track that state --
   * it just forwards the index the overlay offered.
   */
  vote(option: number): void {
    this.conn.send({ type: "Vote", option });
  }

  /** Projectile positions drawn at the newest server position (like enemies). */
  projectilePositions(now: number): Map<number, Vec3> {
    return this.projectiles.sample(now, 0, 1.5);
  }

  /** Ask the server for a fresh run (honoured once the whole team is down). */
  restart(): void {
    this.conn.send({ type: "Restart" });
  }

  /** Advance the fixed-step loop by one rendered frame and flush inputs to the server. */
  update(dtSeconds: number): void {
    if (!this.level || this.predictor.state === null) return;
    const st = useGameStore.getState();
    if (st.runOver) return; // run over: no input, frozen in place
    // Spectating (beacon reached / out of lives / joined mid-level): the camera is free, the body is not ours.
    if (st.myMode !== PLAYER_ACTIVE && st.runInfo?.phase === PHASE_PLAYING) return;
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

  /** Enemies are drawn at the newest server position (lightly extrapolated), not delayed like
   *  other players: the server tests hits against where they are now. */
  enemyPositions(now: number): Map<number, Vec3> {
    return this.enemies.sample(now, 0, 1.5);
  }

  /** Full predicted controller state of the local player (animation reads grounded/dash). */
  playerController(): ControllerState | null {
    return this.predictor.state;
  }

  /** Predicted velocity of the local player (for facing and walk animation). */
  playerVelocity(): Vec3 | null {
    return this.predictor.state?.vel ?? null;
  }

  dispose(): void {
    this.stopPing();
    this.conn.close();
  }
}

function sameTiles(a: readonly TileState[], b: readonly TileState[]): boolean {
  return a.length === b.length && a.every((t, i) => t.id === b[i]?.id && t.state === b[i]?.state);
}
