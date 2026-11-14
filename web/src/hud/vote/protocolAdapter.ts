// The ONLY module that knows the wire shape. T-078 is not merged yet and its
// exact field names are not in this ticket, so the assumed shapes are listed
// here and validated at runtime; if the regenerated protocol differs, this
// file (and nothing else) absorbs the change.
//
// ASSUMED WIRE SHAPES (T-078):
//   VoteOpen   (server): { type: 'VoteOpen';
//                          options: [{ id: string; name: string;
//                                      kind: number | string }];
//                          durationMs?: number; remainingMs?: number }
//                kind: CARD_ENEMY(0) or CARD_CURSE(2), or the string
//                'enemy' | 'curse' | 'theme' | 'difficulty'.
//   VoteState  (server): { type: 'VoteState'; counts: number[];
//                          remainingMs?: number;
//                          yourVote?: number; you?: number }
//                yourVote/you: option index, or any negative number for "no vote".
//   VoteResult (server): { type: 'VoteResult'; winner?: number; winnerIndex?: number;
//                          counts?: number[] }
//   Vote       (client): { type: 'Vote'; option: number } — not encoded here;
//                the host encodes it via the send callback threaded into the HUD.
import { CARD_CURSE, CARD_ENEMY } from '../../protocol/generated';
import { VOTE_DURATION_MS } from './model';
import type { VoteEvent, VoteKind, VoteOption } from './model';

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

function kindFromWire(value: unknown): VoteKind | null {
  if (typeof value === 'string') {
    const v = value.toLowerCase();
    if (v === 'enemy' || v === 'curse' || v === 'theme' || v === 'difficulty') return v;
    return null;
  }
  if (value === CARD_ENEMY) return 'enemy';
  if (value === CARD_CURSE) return 'curse';
  return null;
}

function optionFromWire(value: unknown): VoteOption | null {
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
 * Maps any incoming message to a VoteEvent, or null when the message is not a
 * vote message. Never throws: malformed vote messages become 'failed' events
 * so the UI can show an error instead of crashing the HUD.
 */
export function parseVoteMessage(message: unknown): VoteEvent | null {
  const rec = asRecord(message);
  if (rec === null || typeof rec.type !== 'string') return null;

  switch (rec.type) {
    case 'VoteOpen': {
      if (!Array.isArray(rec.options)) {
        return { type: 'failed', message: 'VoteOpen was missing its options.' };
      }
      const options: VoteOption[] = [];
      for (const item of rec.options) {
        const option = optionFromWire(item);
        if (option === null) {
          return { type: 'failed', message: 'VoteOpen contained a malformed option.' };
        }
        options.push(option);
      }
      if (options.length === 0) {
        return { type: 'failed', message: 'Vote opened with no options.' };
      }
      const durationMs = isNonNegativeFinite(rec.durationMs) && rec.durationMs > 0
        ? Math.floor(rec.durationMs)
        : VOTE_DURATION_MS;
      if (isNonNegativeFinite(rec.remainingMs)) {
        return { type: 'opened', options, durationMs, remainingMs: Math.floor(rec.remainingMs) };
      }
      return { type: 'opened', options, durationMs };
    }

    case 'VoteState': {
      const remainingMs = isNonNegativeFinite(rec.remainingMs)
        ? Math.floor(rec.remainingMs)
        : undefined;
      let yourVote: number | null | undefined;
      const youRaw = rec.yourVote !== undefined ? rec.yourVote : rec.you;
      if (typeof youRaw === 'number' && Number.isFinite(youRaw)) {
        yourVote = youRaw < 0 ? null : Number.isInteger(youRaw) ? Math.floor(youRaw) : undefined;
      }
      return { type: 'progress', counts: toCounts(rec.counts), remainingMs, yourVote };
    }

    case 'VoteResult': {
      const winnerRaw =
        typeof rec.winner === 'number' ? rec.winner
        : typeof rec.winnerIndex === 'number' ? rec.winnerIndex
        : undefined;
      if (winnerRaw === undefined || !Number.isFinite(winnerRaw)) {
        return { type: 'failed', message: 'VoteResult did not name a winner.' };
      }
      return { type: 'result', winnerIndex: Math.floor(winnerRaw), counts: toCounts(rec.counts) };
    }

    default:
      return null;
  }
}
