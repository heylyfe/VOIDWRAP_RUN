// The ONLY module that knows the wire shape. Mirrors hud/vote/protocolAdapter.ts
// (T-149). The generated TS uses camelCase (`countdownS`, `secondsLeft`) while the
// Rust fields are snake_case (`countdown_s`, `seconds_left`), so both spellings are
// accepted — along with the legacy millisecond fields the vote adapter also accepts.
//
// REAL WIRE SHAPES (protocol/generated.ts):
//   PollOpen   { type: 'PollOpen'; options: PollOption[]; countdownS: number }
//   PollState  { type: 'PollState'; counts: number[]; voted: number[]; secondsLeft: number }
//   PollResult { type: 'PollResult'; winner: number; id: string; name: string; kind: number }
//   PollOption { id: string; name: string; kind: number }  // kind: POLL_THEME (0) or POLL_DIFFICULTY (1)
import { POLL_DIFFICULTY, POLL_THEME } from '../../protocol/generated';
import { POLL_DURATION_MS } from './model';
import type { PollEvent, PollKind, PollOption } from './model';

function asRecord(value: unknown): Record<string, unknown> | null {
  return typeof value === 'object' && value !== null ? (value as Record<string, unknown>) : null;
}

function isNonNegativeFinite(value: unknown): value is number {
  return typeof value === 'number' && Number.isFinite(value) && value >= 0;
}

function toCount(value: unknown): number {
  return typeof value === 'number' && Number.isFinite(value) && value > 0 ? Math.floor(value) : 0;
}

function toCounts(value: unknown): number[] | undefined {
  return Array.isArray(value) ? value.map(toCount) : undefined;
}

function toVoted(value: unknown): number[] | undefined {
  if (!Array.isArray(value)) return undefined;
  return value
    .filter(isNonNegativeFinite)
    .map((id) => Math.floor(id));
}

function kindFromWire(value: unknown): PollKind | null {
  if (typeof value === 'string') {
    const v = value.toLowerCase();
    if (v === 'theme' || v === 'difficulty') return v;
    return null;
  }
  if (value === POLL_THEME) return 'theme';
  if (value === POLL_DIFFICULTY) return 'difficulty';
  return null;
}

/** Whole seconds from the server, either spelling; undefined when absent/invalid. */
function secondsFromWire(camel: unknown, snake: unknown): number | undefined {
  const value = isNonNegativeFinite(camel) ? camel : isNonNegativeFinite(snake) ? snake : undefined;
  return value === undefined ? undefined : Math.floor(value * 1000);
}

function optionFromWire(value: unknown): PollOption | null {
  const rec = asRecord(value);
  if (rec === null) return null;
  const { id, name } = rec;
  if (typeof id !== 'string' || id.length === 0) return null;
  if (typeof name !== 'string' || name.trim().length === 0) return null;
  const kind = kindFromWire(rec.kind);
  if (kind === null) return null;
  return { id, name: name.trim(), kind };
}

/**
 * Maps any incoming message to a PollEvent, or null when the message is not a
 * poll message. Never throws: malformed poll messages become 'failed' events
 * so the UI can show an error instead of crashing the HUD.
 */
export function parsePollMessage(message: unknown): PollEvent | null {
  const rec = asRecord(message);
  if (rec === null || typeof rec.type !== 'string') return null;

  switch (rec.type) {
    case 'PollOpen': {
      if (!Array.isArray(rec.options)) {
        return { type: 'failed', message: 'PollOpen was missing its options.' };
      }
      const options: PollOption[] = [];
      for (const item of rec.options) {
        const option = optionFromWire(item);
        if (option === null) {
          return { type: 'failed', message: 'PollOpen contained a malformed option.' };
        }
        options.push(option);
      }
      if (options.length === 0) {
        return { type: 'failed', message: 'Poll opened with no options.' };
      }
      // The real protocol sends `countdownS` (whole seconds); the Rust field is `countdown_s`.
      // Prefer both spellings, fall back to the legacy millisecond field, then to the local default.
      const durationMs =
        secondsFromWire(rec.countdownS, rec.countdown_s) ??
        (isNonNegativeFinite(rec.durationMs) && rec.durationMs > 0 ? Math.floor(rec.durationMs) : POLL_DURATION_MS);
      const remainingMs =
        secondsFromWire(rec.secondsLeft, rec.seconds_left) ??
        (isNonNegativeFinite(rec.remainingMs) ? Math.floor(rec.remainingMs) : undefined);
      if (remainingMs !== undefined) {
        return { type: 'opened', options, durationMs, remainingMs };
      }
      return { type: 'opened', options, durationMs };
    }

    case 'PollState': {
      // The real protocol sends `secondsLeft` (whole seconds); the Rust field is `seconds_left`.
      const remainingMs =
        secondsFromWire(rec.secondsLeft, rec.seconds_left) ??
        (isNonNegativeFinite(rec.remainingMs) ? Math.floor(rec.remainingMs) : undefined);
      let yourVote: number | null | undefined;
      const youRaw = rec.yourVote !== undefined ? rec.yourVote : rec.you;
      if (typeof youRaw === 'number' && Number.isFinite(youRaw)) {
        yourVote = youRaw < 0 ? null : Number.isInteger(youRaw) ? Math.floor(youRaw) : undefined;
      }
      return {
        type: 'progress',
        counts: toCounts(rec.counts),
        remainingMs,
        yourVote,
        voted: toVoted(rec.voted),
      };
    }

    case 'PollResult': {
      const winnerRaw =
        typeof rec.winner === 'number' ? rec.winner
        : typeof rec.winnerIndex === 'number' ? rec.winnerIndex
        : undefined;
      if (winnerRaw === undefined || !Number.isFinite(winnerRaw)) {
        return { type: 'failed', message: 'PollResult did not name a winner.' };
      }
      // PollResult.kind says which question resolved (POLL_THEME / POLL_DIFFICULTY).
      const question = kindFromWire(rec.kind) ?? undefined;
      return {
        type: 'result',
        winnerIndex: Math.floor(winnerRaw),
        counts: toCounts(rec.counts),
        question,
      };
    }

    default:
      return null;
  }
}
