import { afterEach, describe, expect, it } from 'vitest';
import {
  CLIENT_VERSION,
  MATCHMAKER_ERROR_AFTER_MS,
  MATCHMAKER_WAKE_AFTER_MS,
  fetchMatches,
  matchmakerBaseUrl,
  parseMatchList,
  resolveJoinUrl,
} from './matchmaker';

const ORIGINAL_ENV = process.env.NEXT_PUBLIC_MATCHMAKER_URL;

const validMatch = {
  name: 'Alpha Keep',
  address: '203.0.113.10:7879',
  players: 3,
  max_players: 8,
  level: 'Neon Ruins',
  phase: 'Lobby',
  joinable: true,
  version: CLIENT_VERSION,
};

afterEach(() => {
  if (ORIGINAL_ENV === undefined) {
    delete process.env.NEXT_PUBLIC_MATCHMAKER_URL;
  } else {
    process.env.NEXT_PUBLIC_MATCHMAKER_URL = ORIGINAL_ENV;
  }
});

describe('cold-start timings', () => {
  it('shows Waking server before any error and errors only after ~90 s', () => {
    expect(MATCHMAKER_WAKE_AFTER_MS).toBe(2_500);
    expect(MATCHMAKER_ERROR_AFTER_MS).toBe(90_000);
    expect(MATCHMAKER_WAKE_AFTER_MS).toBeLessThan(MATCHMAKER_ERROR_AFTER_MS);
  });
});

describe('matchmakerBaseUrl', () => {
  it('returns null when NEXT_PUBLIC_MATCHMAKER_URL is unset or blank', () => {
    delete process.env.NEXT_PUBLIC_MATCHMAKER_URL;
    expect(matchmakerBaseUrl()).toBeNull();
    process.env.NEXT_PUBLIC_MATCHMAKER_URL = '   ';
    expect(matchmakerBaseUrl()).toBeNull();
  });

  it('trims whitespace and trailing slashes from the configured URL', () => {
    process.env.NEXT_PUBLIC_MATCHMAKER_URL = '  https://matchmaker.example.com/  ';
    expect(matchmakerBaseUrl()).toBe('https://matchmaker.example.com');
  });
});

describe('parseMatchList', () => {
  it('accepts a well-formed list', () => {
    expect(parseMatchList([validMatch])).toEqual([validMatch]);
  });

  it('returns null when the payload is not an array', () => {
    expect(parseMatchList(null)).toBeNull();
    expect(parseMatchList({})).toBeNull();
    expect(parseMatchList('nope')).toBeNull();
  });

  it('drops malformed entries and keeps the valid ones, including joinable:false', () => {
    const listedButDown = { ...validMatch, joinable: false };
    const parsed = parseMatchList([
      listedButDown,
      { name: 42 },
      null,
      { ...validMatch, joinable: 'yes' },
      { ...validMatch, address: '   ' },
      { ...validMatch, max_players: 'eight' },
    ]);
    expect(parsed).toEqual([listedButDown]);
  });
});

describe('resolveJoinUrl (address handoff to the existing join path)', () => {
  it('feeds listed addresses through parseJoinAddress with the /play/ suffix', () => {
    expect(resolveJoinUrl('192.168.1.23:7879')).toBe('http://192.168.1.23:7879/play/');
    expect(resolveJoinUrl('https://games.example.com:8443')).toBe('https://games.example.com:8443/play/');
  });

  it('rejects unusable addresses', () => {
    expect(resolveJoinUrl('not a url')).toBeNull();
    expect(resolveJoinUrl('ftp://192.168.1.23:7879')).toBeNull();
  });
});

describe('fetchMatches', () => {
  it('requests /matches filtered by the client version', async () => {
    const calls: string[] = [];
    const fetchImpl: typeof fetch = async (input) => {
      calls.push(String(input));
      return new Response(JSON.stringify([validMatch]), { status: 200 });
    };
    const entries = await fetchMatches('https://matchmaker.example.com', CLIENT_VERSION, undefined, fetchImpl);
    expect(entries).toEqual([validMatch]);
    expect(calls).toEqual([
      `https://matchmaker.example.com/matches?version=${encodeURIComponent(CLIENT_VERSION)}`,
    ]);
  });

  it('rejects on HTTP failure', async () => {
    const fetchImpl: typeof fetch = async () => new Response('boom', { status: 503 });
    await expect(
      fetchMatches('https://matchmaker.example.com', CLIENT_VERSION, undefined, fetchImpl),
    ).rejects.toThrow();
  });

  it('rejects when the payload is not a match array', async () => {
    const fetchImpl: typeof fetch = async () =>
      new Response(JSON.stringify({ not: 'an array' }), { status: 200 });
    await expect(
      fetchMatches('https://matchmaker.example.com', CLIENT_VERSION, undefined, fetchImpl),
    ).rejects.toThrow(/unexpected payload/);
  });
});

describe('client version', () => {
  it('matches web/package.json so the exact-match lobby filter cannot silently hide every host', async () => {
    const pkg = (await import('../../package.json')) as { version: string };
    expect(CLIENT_VERSION).toBe(pkg.version);
  });
});
