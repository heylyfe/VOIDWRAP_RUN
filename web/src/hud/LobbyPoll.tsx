// Pre-match poll, reusing the voting widget. Optional by design: one button to
// start, a SKIP button to dismiss, and nothing gates the run start. The server
// owns the effect (tag weights for the next generated level); this UI only
// reports the choice via onVote / tracks the server's VoteOpen..VoteResult flow.
import { useEffect, useRef, useState } from 'react';
import type { JSX } from 'react';
import { VoteWidget } from './vote/VoteWidget';
import { useVote } from './vote/useVote';

export interface LobbyPollProps {
  /** Host starts the poll server-side (protocol owned by T-078/host). */
  onStartPoll?: (() => void) | undefined;
  /** Host reports the chosen option index to the server. */
  onVote?: ((optionIndex: number) => void) | undefined;
  /** Called when the player dismisses the poll without running it. */
  onDismiss?: (() => void) | undefined;
  /** Server messages, e.g. a growing readonly array the HUD appends to. */
  messages?: readonly unknown[] | undefined;
  title?: string | undefined;
}

export function LobbyPoll({
  onStartPoll,
  onVote,
  onDismiss,
  messages,
  title = 'PRE-MATCH POLL',
}: LobbyPollProps): JSX.Element | null {
  const { state, sendVote, ingestMessage, reset } = useVote(onVote);
  const [dismissed, setDismissed] = useState(false);
  const appliedCount = useRef(0);

  useEffect(() => {
    if (messages === undefined) return;
    if (messages.length < appliedCount.current) appliedCount.current = 0;
    for (let i = appliedCount.current; i < messages.length; i += 1) {
      ingestMessage(messages[i]);
    }
    appliedCount.current = messages.length;
  }, [messages, ingestMessage]);

  if (dismissed) return null;

  return (
    <section
      aria-label={title}
      className="w-full max-w-md rounded-xl border border-white/10 bg-[#120f1f] p-4 font-mono text-white"
    >
      <div className="flex items-center justify-between gap-3 border-b border-white/10 pb-2">
        <h2 className="text-sm font-bold tracking-[0.25em] text-[#38e8ff]">{title}</h2>
        <button
          type="button"
          aria-label="Skip the poll"
          onClick={() => {
            setDismissed(true);
            onDismiss?.();
          }}
          className="rounded border border-white/15 px-3 py-1 text-[11px] tracking-widest text-white/70 transition hover:text-white focus:outline-none focus-visible:ring-2 focus-visible:ring-white/40"
        >
          SKIP
        </button>
      </div>

      <p className="mt-2 text-[11px] leading-snug text-white/60">
        Optional — the run can start without it. Theme and difficulty picks are reported and feed
        the pattern engine&apos;s tag weights for the next generated level.
      </p>

      {state.phase === 'idle' ? (
        <button
          type="button"
          onClick={() => onStartPoll?.()}
          className="mt-3 w-full rounded-lg border-2 border-[#ff3db8] px-4 py-3 text-sm font-bold tracking-[0.3em] text-[#ff3db8] transition hover:bg-[#ff3db8] hover:text-black focus:outline-none focus-visible:ring-2 focus-visible:ring-[#ff3db8]/60"
        >
          START POLL
        </button>
      ) : (
        <div className="mt-3">
          <VoteWidget state={state} onVote={sendVote} variant="poll" onDismissReveal={reset} />
        </div>
      )}
    </section>
  );
}
