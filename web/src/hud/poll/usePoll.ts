// Hook bridging the pure reducer to React: local 200 ms countdown ticks
// (re-synced whenever the server sends secondsLeft) and message ingestion.
// Mirrors hud/vote/useVote.ts.
import { useCallback, useEffect, useMemo, useReducer, useRef } from 'react';
import { initialPollState, pollReducer } from './model';
import type { PollViewState } from './model';
import { parsePollMessage } from './pollAdapter';

export interface PollController {
  state: PollViewState;
  /** Marks the poll locally and reports the choice; no server echo required. */
  sendPoll: (index: number) => void;
  /** Feed any server message; non-poll messages are ignored. */
  ingestMessage: (message: unknown) => void;
  reset: () => void;
}

export function usePoll(onPoll?: ((optionIndex: number) => void) | undefined): PollController {
  const [state, dispatch] = useReducer(pollReducer, initialPollState);
  const lastTick = useRef<number | null>(null);

  useEffect(() => {
    const id = window.setInterval(() => {
      const now = Date.now();
      const previous = lastTick.current ?? now;
      lastTick.current = now;
      const delta = now - previous;
      if (delta > 0) dispatch({ type: 'tick', deltaMs: delta });
    }, 200);
    return () => window.clearInterval(id);
  }, []);

  const sendPoll = useCallback(
    (index: number) => {
      const canCast =
        state.phase === 'open' &&
        state.remainingMs > 0 &&
        Number.isInteger(index) &&
        index >= 0 &&
        index < state.options.length;
      dispatch({ type: 'cast', index });
      if (canCast) onPoll?.(index);
    },
    [state, onPoll],
  );

  const ingestMessage = useCallback((message: unknown) => {
    const event = parsePollMessage(message);
    if (event !== null) dispatch(event);
  }, []);

  const reset = useCallback(() => dispatch({ type: 'reset' }), []);

  return useMemo(
    () => ({ state, sendPoll, ingestMessage, reset }),
    [state, sendPoll, ingestMessage, reset],
  );
}
