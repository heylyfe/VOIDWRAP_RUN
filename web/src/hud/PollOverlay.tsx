// Pre-match poll (theme, then difficulty — T-159/T-161): a HUD overlay anchored to the
// bottom edge so it does not fight the beacon (world centre / upper screen), and sized to
// sit above the lobby draft screen. pointer events pass through except on the poll buttons.
// Renders null whenever no poll is open — no empty box, no placeholder.
// z-50 because the lobby phase is PHASE_ENEMY_DRAFT, so DraftScreen (z-40) is on screen
// while the poll blocks the level; the vote overlay's z-30 would hide the poll behind it.
import { useEffect, useRef } from 'react';
import type { JSX } from 'react';
import { PollWidget } from './poll/PollWidget';
import { usePoll } from './poll/usePoll';

export interface PollOverlayProps {
  /** Host encodes and sends the client Poll message (report only). */
  onPoll?: ((optionIndex: number) => void) | undefined;
  /** Server messages, e.g. a growing readonly array the HUD appends to. */
  messages?: readonly unknown[] | undefined;
  /** Called after the local reveal is dismissed. */
  onRevealDismiss?: (() => void) | undefined;
}

export function PollOverlay({
  onPoll,
  messages,
  onRevealDismiss,
}: PollOverlayProps): JSX.Element | null {
  const { state, sendPoll, ingestMessage, reset } = usePoll(onPoll);
  const appliedCount = useRef(0);

  useEffect(() => {
    if (messages === undefined) return;
    if (messages.length < appliedCount.current) appliedCount.current = 0;
    for (let i = appliedCount.current; i < messages.length; i += 1) {
      ingestMessage(messages[i]);
    }
    appliedCount.current = messages.length;
  }, [messages, ingestMessage]);

  // T-161: render NOTHING when no poll is open. Not an empty box, not a placeholder.
  if (state.phase === 'idle') return null;

  return (
    <div className="pointer-events-none absolute inset-x-0 bottom-4 z-50 flex justify-center">
      <div className="w-full max-w-3xl px-4">
        <PollWidget
          state={state}
          onVote={sendPoll}
          onDismissReveal={() => {
            reset();
            onRevealDismiss?.();
          }}
        />
      </div>
    </div>
  );
}
