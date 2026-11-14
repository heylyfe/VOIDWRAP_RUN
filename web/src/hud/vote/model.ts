// Pure voting state logic for the collapse-window pedestal vote and the lobby
// poll. Deterministic, fully typed, no protocol imports: the wire mapping
// lives in protocolAdapter.ts and components render from VoteViewState.
export type VoteKind = 'enemy' | 'curse' | 'theme' | 'difficulty';

export interface VoteOption {
  id: string;
  name: string;
  kind: VoteKind;
}

export type VotePhase = 'idle' | 'open' | 'revealed';

export interface VoteViewState {
  phase: VotePhase;
  options: VoteOption[];
  counts: number[];
  /** Index into options, or null when this player has not voted. */
  yourVote: number | null;
  /** Clamped to [0, durationMs]. The authoritative countdown comes from the server; local ticks keep it smooth. */
  remainingMs: number;
  durationMs: number;
  winnerIndex: number | null;
  error: string | null;
}

export type VoteEvent =
  | { type: 'reset' }
  // remainingMs is optional; when absent the full duration is used.
  | { type: 'opened'; options: VoteOption[]; durationMs: number; remainingMs?: number | undefined }
  | {
      type: 'progress';
      counts?: number[] | undefined;
      remainingMs?: number | undefined;
      /** -1 maps to null ("this player has no vote"). undefined = field not sent. */
      yourVote?: number | null | undefined;
    }
  | { type: 'cast'; index: number }
  | { type: 'result'; winnerIndex: number | null; counts?: number[] | undefined }
  | { type: 'tick'; deltaMs: number }
  | { type: 'failed'; message: string };

/** The collapse-window vote always lasts 20 s (T-078 server-side). */
export const VOTE_DURATION_MS = 20_000;

export function createInitialVoteState(): VoteViewState {
  return {
    phase: 'idle',
    options: [],
    counts: [],
    yourVote: null,
    remainingMs: 0,
    durationMs: VOTE_DURATION_MS,
    winnerIndex: null,
    error: null,
  };
}

export const initialVoteState: VoteViewState = createInitialVoteState();

export function secondsRemaining(remainingMs: number): number {
  return Math.max(0, Math.ceil(remainingMs / 1000));
}

export function isVotingOpen(state: VoteViewState): boolean {
  return state.phase === 'open' && state.remainingMs > 0;
}

function isFiniteNumber(value: unknown): value is number {
  return typeof value === 'number' && Number.isFinite(value);
}

function isValidIndex(index: unknown, length: number): index is number {
  return typeof index === 'number' && Number.isInteger(index) && index >= 0 && index < length;
}

/** Returns fallback unchanged when counts is undefined; null on length mismatch. */
function normalizeCounts(counts: readonly number[], length: number): number[] | null {
  if (counts.length !== length) return null;
  return counts.map((c) => (isFiniteNumber(c) && c > 0 ? Math.floor(c) : 0));
}

export function voteReducer(state: VoteViewState, event: VoteEvent): VoteViewState {
  switch (event.type) {
    case 'reset':
      return createInitialVoteState();

    case 'opened': {
      if (!Array.isArray(event.options) || event.options.length === 0) {
        return { ...createInitialVoteState(), error: 'Vote opened with no options.' };
      }
      const durationMs =
        isFiniteNumber(event.durationMs) && event.durationMs > 0
          ? Math.floor(event.durationMs)
          : VOTE_DURATION_MS;
      const remainingMs =
        event.remainingMs !== undefined && isFiniteNumber(event.remainingMs)
          ? Math.min(Math.max(0, Math.floor(event.remainingMs)), durationMs)
          : durationMs;
      const options = event.options.map((o) => ({ ...o }));
      return {
        phase: 'open',
        options,
        counts: options.map(() => 0),
        yourVote: null,
        remainingMs,
        durationMs,
        winnerIndex: null,
        error: null,
      };
    }

    case 'progress': {
      if (state.phase !== 'open') return state;
      let counts = state.counts;
      let error: string | null = state.error;
      if (event.counts !== undefined) {
        const normalized = normalizeCounts(event.counts, state.options.length);
        if (normalized === null) {
          error = 'Vote update had the wrong number of counts.';
        } else {
          counts = normalized;
        }
      }
      let remainingMs = state.remainingMs;
      if (event.remainingMs !== undefined && isFiniteNumber(event.remainingMs)) {
        remainingMs = Math.min(Math.max(0, Math.floor(event.remainingMs)), state.durationMs);
      }
      let yourVote = state.yourVote;
      if (event.yourVote !== undefined) {
        if (event.yourVote === null) {
          yourVote = null;
        } else if (isValidIndex(event.yourVote, state.options.length)) {
          yourVote = event.yourVote;
        }
      }
      return { ...state, counts, remainingMs, yourVote, error };
    }

    case 'cast': {
      if (state.phase !== 'open') return { ...state, error: 'No vote is open.' };
      if (state.remainingMs <= 0) {
        return { ...state, error: 'The vote window has closed.' };
      }
      if (!isValidIndex(event.index, state.options.length)) {
        return { ...state, error: 'That option does not exist.' };
      }
      return { ...state, yourVote: event.index, error: null };
    }

    case 'tick': {
      if (state.phase !== 'open') return state;
      if (!isFiniteNumber(event.deltaMs) || event.deltaMs <= 0) return state;
      const remainingMs = Math.max(0, state.remainingMs - Math.floor(event.deltaMs));
      if (remainingMs === state.remainingMs) return state;
      return { ...state, remainingMs };
    }

    case 'result': {
      if (state.options.length === 0) {
        return { ...state, error: 'Vote result received with no open vote.' };
      }
      let counts = state.counts;
      let error: string | null = null;
      if (event.counts !== undefined) {
        const normalized = normalizeCounts(event.counts, state.options.length);
        if (normalized === null) {
          error = 'Vote result had the wrong number of counts.';
        } else {
          counts = normalized;
        }
      }
      let winnerIndex: number | null = null;
      if (event.winnerIndex !== null && isValidIndex(event.winnerIndex, state.options.length)) {
        winnerIndex = event.winnerIndex;
      } else {
        error = error ?? 'Vote result did not name a valid winner.';
      }
      return { ...state, phase: 'revealed', counts, remainingMs: 0, winnerIndex, error };
    }

    case 'failed':
      return { ...state, error: event.message };

    default:
      return state;
  }
}
