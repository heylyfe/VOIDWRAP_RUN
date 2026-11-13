// HUD-facing client state. The hot path (positions, prediction) lives outside React/zustand.
import { create } from "zustand";
import type { RunSummary } from "@/protocol/generated";

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
  set: (patch) => set(patch),
}));
