'use client';

// Lobby browser backed by the matchmaker. Fetches once when the panel opens
// (the title page unmounts JoinPanel on close, so reopen = refresh); there
// is deliberately no polling. A slow first fetch shows "Waking server…" and
// no error is surfaced before MATCHMAKER_ERROR_AFTER_MS has elapsed.
import { useEffect, useState } from 'react';
import type { JSX } from 'react';
import {
  CLIENT_VERSION,
  MATCHMAKER_ERROR_AFTER_MS,
  MATCHMAKER_WAKE_AFTER_MS,
  fetchMatches,
  matchmakerBaseUrl,
  type MatchEntry,
} from './matchmaker';

const MAGENTA = '#ff3db8';
const CYAN = '#38e8ff';

export type MatchListState =
  | { status: 'loading'; waking: boolean }
  | { status: 'error' }
  | { status: 'ready'; entries: MatchEntry[] };

export interface MatchListViewProps {
  state: MatchListState;
  onJoin(address: string): void;
}

/** Presentational list; state is owned by MatchList. Exported for tests. */
export function MatchListView({ state, onJoin }: MatchListViewProps): JSX.Element {
  return (
    <section aria-label="Open lobbies" className="flex flex-col gap-2 border-t border-white/10 pt-3">
      <h3 className="text-[11px] font-bold tracking-[0.25em] text-white/70">OPEN LOBBIES</h3>

      {state.status === 'loading' ? (
        <p
          role="status"
          className="flex items-center gap-2 rounded border border-white/10 bg-black/30 px-3 py-2 text-xs text-white/75"
        >
          <span
            aria-hidden="true"
            className="inline-block h-2 w-2 animate-pulse rounded-full"
            style={{ backgroundColor: CYAN }}
          />
          {state.waking ? 'Waking server…' : 'Contacting matchmaker…'}
        </p>
      ) : state.status === 'error' ? (
        <p
          role="alert"
          className="rounded border border-[#ff4d6d]/40 bg-[#ff4d6d]/10 px-3 py-2 text-xs leading-snug text-[#ffb3c2]"
        >
          Could not reach the matchmaker. It may still be waking from sleep — close and reopen this panel to try again.
        </p>
      ) : state.entries.length === 0 ? (
        <p className="rounded border border-white/10 bg-black/30 px-3 py-2 text-xs leading-snug text-white/60">
          No open lobbies right now. Host one and your friends can join you from here.
        </p>
      ) : (
        <ul className="flex flex-col gap-2">
          {state.entries.map((entry, index) => {
            const greyed = !entry.joinable;
            return (
              <li
                key={`${entry.address}-${index}`}
                className={`rounded-lg border p-3 ${
                  greyed ? 'border-white/5 bg-black/20 opacity-50' : 'border-white/10 bg-black/30'
                }`}
              >
                <div className="flex items-center justify-between gap-3">
                  <div className="min-w-0">
                    <p className="truncate text-sm font-bold text-white">{entry.name}</p>
                    <p
                      className="truncate text-[11px]"
                      style={{ color: greyed ? 'rgba(255,255,255,0.5)' : CYAN }}
                    >
                      {entry.address}
                    </p>
                    <p className="truncate text-[11px] text-white/60">
                      {entry.players}/{entry.max_players} players · {entry.level} · {entry.phase}
                    </p>
                  </div>
                  <button
                    type="button"
                    disabled={greyed}
                    aria-disabled={greyed}
                    onClick={() => onJoin(entry.address)}
                    className="shrink-0 rounded border px-3 py-1 text-[11px] font-bold tracking-[0.25em] transition focus:outline-none focus-visible:ring-2 focus-visible:ring-[#38e8ff]/60 disabled:cursor-not-allowed"
                    style={
                      greyed
                        ? { borderColor: 'rgba(255,255,255,0.15)', color: 'rgba(255,255,255,0.4)' }
                        : { borderColor: MAGENTA, color: MAGENTA }
                    }
                  >
                    JOIN
                  </button>
                </div>
                {greyed ? (
                  <p className="mt-2 text-[11px] text-white/50">host is unreachable from the internet</p>
                ) : null}
              </li>
            );
          })}
        </ul>
      )}
    </section>
  );
}

/**
 * Fetches /matches on mount and owns the timers. Refresh on reopen comes
 * from unmount/remount of the panel; no polling is ever scheduled.
 */
export function MatchList({ onJoin }: { onJoin: (address: string) => void }): JSX.Element | null {
  const baseUrl = matchmakerBaseUrl();
  const [state, setState] = useState<MatchListState>({ status: 'loading', waking: false });

  useEffect(() => {
    if (baseUrl === null) {
      return undefined;
    }
    const startedAt = Date.now();
    const controller = new AbortController();
    let failTimer: ReturnType<typeof setTimeout> | undefined;

    const wakeTimer = setTimeout(() => {
      setState((current) => (current.status === 'loading' ? { ...current, waking: true } : current));
    }, MATCHMAKER_WAKE_AFTER_MS);

    // If the fetch hangs, abort at the error threshold; the rejection funnels
    // into the same failure path and surfaces the error after ~90 s.
    const deadlineTimer = setTimeout(() => {
      controller.abort();
    }, MATCHMAKER_ERROR_AFTER_MS);

    const clearAll = (): void => {
      clearTimeout(wakeTimer);
      clearTimeout(deadlineTimer);
      if (failTimer !== undefined) {
        clearTimeout(failTimer);
      }
    };

    fetchMatches(baseUrl, CLIENT_VERSION, controller.signal).then(
      (entries) => {
        clearAll();
        setState({ status: 'ready', entries });
      },
      () => {
        clearAll();
        // Hold a hard floor of ~90 s from open before any error is shown;
        // until then show the cold-start state.
        setState((current) =>
          current.status === 'loading' ? { ...current, waking: true } : current,
        );
        const remaining = MATCHMAKER_ERROR_AFTER_MS - (Date.now() - startedAt);
        if (remaining <= 0) {
          setState({ status: 'error' });
          return;
        }
        failTimer = setTimeout(() => {
          setState({ status: 'error' });
        }, remaining);
      },
    );

    return () => {
      controller.abort();
      clearAll();
    };
  }, [baseUrl]);

  if (baseUrl === null) {
    return null;
  }
  return <MatchListView state={state} onJoin={onJoin} />;
}
