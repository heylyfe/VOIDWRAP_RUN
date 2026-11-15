// Pure pre-match poll state logic for the two questions the server asks before
// level 1 (theme, then difficulty — T-159). Deterministic, fully typed, no protocol
// imports: the wire mapping lives in pollAdapter.ts and components render from
// PollViewState. Mirrors hud/vote/model.ts (T-078/T-149).
export type PollKind = 'theme' | 'difficulty';

export interface PollOption {
  id: string;
  name: string;
  kind: PollKind;
}

export type PollPhase = 'idle' | 'open' | 'revealed';

export interface PollViewState {
  phase: PollPhase;
  /** Which question is on screen, derived from the options (or the result kind). null when idle. */
  question: PollKind | null;
  options: PollOption[];
  counts: number[];
  /** Player ids that have answered (PollState.voted). */
  voted: number[];
  /** Index into options, or null when this player has not voted. */
  yourVote: number | null;
  /** Clamped to [0, durationMs]. The authoritative countdown comes from the server; local ticks keep it smooth. */
  remainingMs: number;
  durationMs: number;
  winnerIndex: number | null;
  error: string | null;
}

export type PollEvent =
  | { type: 'reset' }
  // remainingMs is optional; when absent the full duration is used.
  | { type: 'opened'; options: PollOption[]; durationMs: number; remainingMs?: number | undefined }
  | {
      type: 'progress';
      counts?: number[] | undefined;
      remainingMs?: number | undefined;
      /** -1 maps to null ("this player has no vote"). undefined = field not sent. */
      yourVote?: number | null | undefined;
      voted?: number[] | undefined;
    }
  | { type: 'cast'; index: number }
  | {
      type: 'result';
      winnerIndex: number | null;
      counts?: number[] | undefined;
      /** Which question resolved (PollResult.kind). undefined = not sent or unknown. */
      question?: PollKind | undefined;
    }
  | { type: 'tick'; deltaMs: number }
  | { type: 'failed'; message: string };

/** The pre-match poll always lasts 20 s (T-159 server-side: POLL_COUNTDOWN_TICKS = 20 * TICK_HZ). */
export const POLL_DURATION_MS = 20_000;

export function createInitialPollState(): PollViewState {
  return {
    phase: 'idle',
    question: null,
    options: [],
    counts: [],
    voted: [],
    yourVote: null,
    remainingMs: 0,
    durationMs: POLL_DURATION_MS,
    winnerIndex: null,
    error: null,
  };
}

export const initialPollState: PollViewState = createInitialPollState();

export function secondsRemaining(remainingMs: number): number {
  return Math.max(0, Math.ceil(remainingMs / 1000));
}

export function isPollOpen(state: PollViewState): boolean {
  return state.phase === 'open' && state.remainingMs > 0;
}

function isFiniteNumber(value: unknown): value is number {
  return typeof value === 'number' && Number.isFinite(value);
}

function isValidIndex(index: unknown, length: number): index is number {
  return typeof index === 'number' && Number.isInteger(index) && index >= 0 && index < length;
}

function normalizeCount(value: number): number {
  return isFiniteNumber(value) && value > 0 ? Math.floor(value) : 0;
}

/** Returns fallback unchanged when counts is undefined; null on length mismatch. */
function normalizeCounts(counts: readonly number[], length: number): number[] | null {
  if (counts.length !== length) return null;
  return counts.map(normalizeCount);
}

function normalizeVoted(voted: readonly number[]): number[] {
  return voted.filter((id) => isFiniteNumber(id) && id >= 0).map((id) => Math.floor(id));
}

export function pollReducer(state: PollViewState, event: PollEvent): PollViewState {
  switch (event.type) {
    case 'reset':
      return createInitialPollState();

    case 'opened': {
      if (!Array.isArray(event.options) || event.options.length === 0) {
        return { ...createInitialPollState(), error: 'Poll opened with no options.' };
      }
      const durationMs =
        isFiniteNumber(event.durationMs) && event.durationMs > 0
          ? Math.floor(event.durationMs)
          : POLL_DURATION_MS;
      const remainingMs =
        event.remainingMs !== undefined && isFiniteNumber(event.remainingMs)
          ? Math.min(Math.max(0, Math.floor(event.remainingMs)), durationMs)
          : durationMs;
      const options = event.options.map((o) => ({ ...o }));
      return {
        phase: 'open',
        // All options in one poll carry the question kind (POLL_THEME / POLL_DIFFICULTY).
        question: options[0]?.kind ?? null,
        options,
        counts: options.map(() => 0),
        voted: [],
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
          error = 'Poll update had the wrong number of counts.';
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
      const voted = event.voted !== undefined ? normalizeVoted(event.voted) : state.voted;
      return { ...state, counts, remainingMs, yourVote, voted, error };
    }

    case 'cast': {
      if (state.phase !== 'open') return { ...state, error: 'No poll is open.' };
      if (state.remainingMs <= 0) {
        return { ...state, error: 'The poll window has closed.' };
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
        return { ...state, error: 'Poll result received with no open poll.' };
      }
      let counts = state.counts;
      let error: string | null = null;
      if (event.counts !== undefined) {
        const normalized = normalizeCounts(event.counts, state.options.length);
        if (normalized === null) {
          error = 'Poll result had the wrong number of counts.';
        } else {
          counts = normalized;
        }
      }
      let winnerIndex: number | null = null;
      if (event.winnerIndex !== null && isValidIndex(event.winnerIndex, state.options.length)) {
        winnerIndex = event.winnerIndex;
      } else {
        error = error ?? 'Poll result did not name a valid winner.';
      }
      const question = event.question ?? state.question;
      return { ...state, phase: 'revealed', counts, remainingMs: 0, winnerIndex, question, error };
    }

    case 'failed':
      return { ...state, error: event.message };

    default:
      return state;
  }
}
