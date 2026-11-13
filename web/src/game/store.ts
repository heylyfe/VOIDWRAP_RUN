// HUD-facing client state. The hot path (positions, prediction) lives outside React/zustand.
import { create } from "zustand";

export type ConnStatus = "connecting" | "connected" | "closed";

export interface GameStore {
  status: ConnStatus;
  error: string | null;
  playerId: number | null;
  serverTick: number;
  pingMs: number | null;
  playerCount: number;
  pointerLocked: boolean;
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
  set: (patch) => set(patch),
}));
