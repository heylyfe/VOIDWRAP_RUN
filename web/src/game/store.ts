// HUD-facing client state. The hot path (positions, prediction) lives outside React/zustand.
import { create } from "zustand";
import type { DraftCard, RunInfo, RunSummary } from "@/protocol/generated";

export interface DraftOffer {
  enemyPicks: number;
  rewardPicks: number;
  cards: DraftCard[];
  /** True once this client has submitted its picks and waits for the other players. */
  submitted: boolean;
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
  set: (patch) => set(patch),
}));
