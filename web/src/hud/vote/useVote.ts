// Hook bridging the pure reducer to React: local 200 ms countdown ticks
// (re-synced whenever the server sends remainingMs) and message ingestion.
import { useCallback, useEffect, useMemo, useReducer, useRef } from 'react';
import { initialVoteState, voteReducer } from './model';
import type { VoteViewState } from './model';
import { parseVoteMessage } from './protocolAdapter';

export interface VoteController {
  state: VoteViewState;
  /** Marks the vote locally and reports the choice; no server echo required. */
  sendVote: (index: number) => void;
  /** Feed any server message; non-vote messages are ignored. */
  ingestMessage: (message: unknown) => void;
  reset: () => void;
}

export function useVote(onVote?: ((optionIndex: number) => void) | undefined): VoteController {
  const [state, dispatch] = useReducer(voteReducer, initialVoteState);
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

  const sendVote = useCallback(
    (index: number) => {
      const canCast =
        state.phase === 'open' &&
        state.remainingMs > 0 &&
        Number.isInteger(index) &&
        index >= 0 &&
        index < state.options.length;
      dispatch({ type: 'cast', index });
      if (canCast) onVote?.(index);
    },
    [state, onVote],
  );

  const ingestMessage = useCallback((message: unknown) => {
    const event = parseVoteMessage(message);
    if (event !== null) dispatch(event);
  }, []);

  const reset = useCallback(() => dispatch({ type: 'reset' }), []);

  return useMemo(
    () => ({ state, sendVote, ingestMessage, reset }),
    [state, sendVote, ingestMessage, reset],
  );
}
