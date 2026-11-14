// In-run collapse-window vote: three pedestals as a HUD overlay anchored to the
// bottom edge so the beacon (world centre / upper screen) is never occluded and
// pointer events pass through except on the vote buttons.
import { useEffect, useRef } from 'react';
import type { JSX } from 'react';
import { VoteWidget } from './vote/VoteWidget';
import { useVote } from './vote/useVote';

export interface RunVoteOverlayProps {
  /** Host encodes and sends the client Vote message (report only). */
  onVote?: ((optionIndex: number) => void) | undefined;
  /** Server messages, e.g. a growing readonly array the HUD appends to. */
  messages?: readonly unknown[] | undefined;
  /** Called after the local reveal is dismissed. */
  onRevealDismiss?: (() => void) | undefined;
}

export function RunVoteOverlay({
  onVote,
  messages,
  onRevealDismiss,
}: RunVoteOverlayProps): JSX.Element | null {
  const { state, sendVote, ingestMessage, reset } = useVote(onVote);
  const appliedCount = useRef(0);

  useEffect(() => {
    if (messages === undefined) return;
    if (messages.length < appliedCount.current) appliedCount.current = 0;
    for (let i = appliedCount.current; i < messages.length; i += 1) {
      ingestMessage(messages[i]);
    }
    appliedCount.current = messages.length;
  }, [messages, ingestMessage]);

  if (state.phase === 'idle') return null;

  return (
    <div className="pointer-events-none absolute inset-x-0 bottom-4 z-30 flex justify-center">
      <div className="w-full max-w-3xl px-4">
        <VoteWidget
          state={state}
          onVote={sendVote}
          variant="pedestal"
          onDismissReveal={() => {
            reset();
            onRevealDismiss?.();
          }}
        />
      </div>
    </div>
  );
}
