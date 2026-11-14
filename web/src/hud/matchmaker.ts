// Matchmaker (lobby browser) client: types, payload validation, cold-start
// timings and the version-filtered fetch. Pure and unit-tested; the React
// components live in MatchList.tsx. Nothing here touches the DOM.
import { parseJoinAddress } from './join';

/** Keep in sync with web/package.json "version"; the matchmaker filters entries exactly. */
export const CLIENT_VERSION = '0.4.0-alpha.1';

/** Slow first fetch: after this long without a response we show "Waking server…". */
export const MATCHMAKER_WAKE_AFTER_MS = 2_500;
/** No error is surfaced before this long; the matchmaker may be cold-starting. */
export const MATCHMAKER_ERROR_AFTER_MS = 90_000;

/** Shape of one entry in the frozen GET /matches?version=<str> contract. */
export interface MatchEntry {
  name: string;
  address: string;
  players: number;
  max_players: number;
  level: string;
  phase: string;
  joinable: boolean;
  version: string;
}

/**
 * Base URL of the matchmaker from NEXT_PUBLIC_MATCHMAKER_URL, or null when
 * it is unset/blank (the lobby browser then renders nothing and the manual
 * join form below it still works). Trailing slashes are stripped.
 */
export function matchmakerBaseUrl(): string | null {
  const raw = process.env.NEXT_PUBLIC_MATCHMAKER_URL;
  if (typeof raw !== 'string') {
    return null;
  }
  const trimmed = raw.trim().replace(/\/+$/, '');
  return trimmed.length === 0 ? null : trimmed;
}

function parseMatchEntry(value: unknown): MatchEntry | null {
  if (typeof value !== 'object' || value === null) {
    return null;
  }
  const record = value as Record<string, unknown>;
  const { name, address, players, max_players, level, phase, joinable, version } = record;
  if (typeof name !== 'string' || name.trim().length === 0) {
    return null;
  }
  if (typeof address !== 'string' || address.trim().length === 0) {
    return null;
  }
  if (typeof players !== 'number' || !Number.isFinite(players) || players < 0) {
    return null;
  }
  if (typeof max_players !== 'number' || !Number.isFinite(max_players) || max_players < 0) {
    return null;
  }
  if (typeof level !== 'string') {
    return null;
  }
  if (typeof phase !== 'string') {
    return null;
  }
  if (typeof joinable !== 'boolean') {
    return null;
  }
  if (typeof version !== 'string') {
    return null;
  }
  return { name, address, players, max_players, level, phase, joinable, version };
}

/**
 * Validate a /matches payload. Returns null when the body is not an array;
 * otherwise returns the well-formed entries and drops individual malformed
 * ones so one bad row cannot blank the whole list.
 */
export function parseMatchList(value: unknown): MatchEntry[] | null {
  if (!Array.isArray(value)) {
    return null;
  }
  const entries: MatchEntry[] = [];
  for (const item of value) {
    const entry = parseMatchEntry(item);
    if (entry !== null) {
      entries.push(entry);
    }
  }
  return entries;
}

/**
 * The join path for a listed address: the existing parseJoinAddress with
 * the /play/ suffix appended, exactly like the manual JOIN form navigates.
 * Returns null when the address is unusable.
 */
export function resolveJoinUrl(address: string): string | null {
  const parsed = parseJoinAddress(address);
  return parsed === null ? null : `${parsed}/play/`;
}

/**
 * GET <baseUrl>/matches?version=<version>. Rejects on HTTP errors and on
 * payloads that are not an array of entries. Pass an AbortSignal to cancel.
 */
export async function fetchMatches(
  baseUrl: string,
  version: string,
  signal?: AbortSignal,
  fetchImpl: typeof fetch = fetch,
): Promise<MatchEntry[]> {
  const url = `${baseUrl}/matches?version=${encodeURIComponent(version)}`;
  const response = await fetchImpl(url, { signal, headers: { Accept: 'application/json' } });
  if (!response.ok) {
    throw new Error(`matchmaker responded with HTTP ${response.status}`);
  }
  const body: unknown = await response.json();
  const entries = parseMatchList(body);
  if (entries === null) {
    throw new Error('matchmaker returned an unexpected payload');
  }
  return entries;
}
