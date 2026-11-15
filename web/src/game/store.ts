// HUD-facing client state. The hot path (positions, prediction) lives outside React/zustand.
import { create } from "zustand";
import type { DraftCard, RunInfo, RunSummary, ShopItem, TileState } from "@/protocol/generated";

export interface DraftOffer {
  enemyPicks: number;
  rewardPicks: number;
  cards: DraftCard[];
  /** True once this client has submitted its picks and waits for the other players. */
  submitted: boolean;
}

/** A short-lived message for the HUD (altar results, purchases, level events). */
export interface Notice {
  /** Increasing id, for React keys and expiry. */
  seq: number;
  /** NOTICE_ALTAR / NOTICE_BOUGHT / NOTICE_EVENT. */
  kind: number;
  /** Content ids (cards, shop items, event ids) the notice is about. */
  ids: string[];
  /** performance.now() when it arrived. */
  at: number;
}

export type ConnStatus = "connecting" | "connected" | "closed";

export interface GameStore {
  status: ConnStatus;
  error: string | null;
  playerId: number | null;
  serverTick: number;
  pingMs: number | null;
  playerCount: number;
  pointerLocked: boolean;
  /** Current level number (0 until the first level arrives). */
  level: number;
  giftsTotal: number;
  /** Gift ids collected on this level (team-wide). */
  giftsTaken: number[];
  /** Set when the local player died; cleared by the next Level message. */
  runOver: RunSummary | null;
  /** Rendered frames per second (measured in the canvas). */
  fps: number;
  /** Run-wide state from the server (phase, currency, lives, drafted cards, tuning). */
  runInfo: RunInfo | null;
  /** The current lobby draft offer, if any. */
  draft: DraftOffer | null;
  /** This player's mode from the latest snapshot (PLAYER_ACTIVE / FINISHED / DEAD / WAITING). */
  myMode: number;
  /** Ids of players still playing the level (for the spectator HUD and follow camera). */
  activeIds: number[];
  /** Lobby shop offer (null outside the lobby or before the server sends one). */
  shop: ShopItem[] | null;
  /** Recent notices, newest last (the HUD expires them). */
  notices: Notice[];
  /**
   * Vote traffic from the server, oldest first (T-149).
   *
   * The pedestal vote opens every level at the collapse window and the server
   * broadcasts VoteOpen / VoteState / VoteResult. Nothing in the client used to
   * read those messages, so the vote ran invisibly and the server resolved it
   * with a random pick. `RunVoteOverlay` consumes this log.
   */
  voteMessages: unknown[];
  /** This player's class (CLASS_*), from the latest snapshot. */
  myClass: number;
  /** Platforms not in their normal state (crumbling or gone), from the latest snapshot. */
  tiles: TileState[];
  /** Altar ids already used on this level. */
  altarsUsed: number[];
  /** LEVEL_COLLECT / LEVEL_COLLAPSE_PENDING / LEVEL_COLLAPSING (phase 2: golden gifts, tiles fall). */
  levelPhase: number;
  /** Golden gift ids collected in phase 2 (golden gifts sit at the positions of collected normal gifts). */
  goldenTaken: number[];
  /** Milliseconds until the collapse ends (0 outside phase 2); updated about twice a second. */
  collapseMs: number;
  /**
   * The last tester-panel answer (T-111). `command` and `args` echo what was sent so the
   * panel can log the pair; `seq` makes identical answers distinguishable.
   */
  debugResult: { seq: number; command: string; args: string; ok: boolean; message: string } | null;
  set(patch: Partial<Omit<GameStore, "set">>): void;
}

export const useGameStore = create<GameStore>((set) => ({
  status: "connecting",
  error: null,
  playerId: null,
  serverTick: 0,
  pingMs: null,
  playerCount: 0,
  pointerLocked: false,
  level: 0,
  giftsTotal: 0,
  giftsTaken: [],
  runOver: null,
  fps: 0,
  runInfo: null,
  draft: null,
  myMode: 0,
  activeIds: [],
  shop: null,
  notices: [],
  voteMessages: [],
  myClass: 0,
  tiles: [],
  altarsUsed: [],
  levelPhase: 0,
  goldenTaken: [],
  collapseMs: 0,
  debugResult: null,
  set: (patch) => set(patch),
}));
