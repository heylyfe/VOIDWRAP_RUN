// The pre-match poll panel (theme, then difficulty — T-159/T-161). Controlled:
// state comes from pollReducer. Mirrors VoteWidget (variant="poll").
import type { JSX } from 'react';
import type { PollKind, PollViewState } from './model';
import { CountdownRing } from '../vote/CountdownRing';

const KIND_LABEL: Record<PollKind, string> = {
  theme: 'THEME',
  difficulty: 'DIFFICULTY',
};

const KIND_BADGE: Record<PollKind, string> = {
  theme: 'border-[#38e8ff]/60 text-[#8ef1ff]',
  difficulty: 'border-[#ffd23d]/60 text-[#ffe98d]',
};

const QUESTION_TITLE: Record<PollKind, string> = {
  theme: 'CHOOSE THE THEME',
  difficulty: 'CHOOSE THE DIFFICULTY',
};

const QUESTION_TAGLINE: Record<PollKind, string> = {
  theme: "Your pick biases the next level's tag weights.",
  difficulty: 'Your pick sets how hard the run plays.',
};

export interface PollWidgetProps {
  state: PollViewState;
  onVote: (index: number) => void;
  /** When set, a DISMISS button is shown on the results reveal. */
  onDismissReveal?: (() => void) | undefined;
}

export function PollWidget({
  state,
  onVote,
  onDismissReveal,
}: PollWidgetProps): JSX.Element {
  const resolvedTitle =
    state.question !== null ? QUESTION_TITLE[state.question] : 'PRE-MATCH POLL';
  const canVote = state.phase === 'open' && state.remainingMs > 0;
  const winner =
    state.winnerIndex !== null ? state.options[state.winnerIndex] ?? null : null;
  const ballots = state.voted.length;

  const optionButton = (optionIndex: number): JSX.Element | null => {
    const option = state.options[optionIndex];
    if (!option) return null;
    const selected = state.yourVote === optionIndex;
    const tone = selected
      ? 'border-[#38e8ff] bg-[#38e8ff]/10'
      : 'border-white/15 bg-black/50 hover:border-[#38e8ff]/60';
    return (
      <button
        key={option.id}
        type="button"
        disabled={!canVote}
        aria-pressed={selected}
        onClick={() => onVote(optionIndex)}
        className={`flex items-center justify-between gap-3 rounded-lg border px-3 py-2 text-left transition focus:outline-none focus-visible:ring-2 focus-visible:ring-[#38e8ff]/60 disabled:cursor-not-allowed disabled:opacity-80 ${tone}`}
      >
        <span
          className={`rounded-full border px-2 py-0.5 text-[10px] font-bold tracking-[0.25em] ${KIND_BADGE[option.kind]}`}
        >
          {KIND_LABEL[option.kind]}
        </span>
        <span className="flex-1 text-sm font-bold leading-snug">
          {option.name}
        </span>
        <span className="flex flex-col items-center leading-none">
          <span className="text-xl font-black tabular-nums">
            {state.counts[optionIndex] ?? 0}
          </span>
          <span className="mt-1 text-[9px] tracking-[0.3em] text-white/55">VOTES</span>
        </span>
        {selected && (
          <span className="text-[10px] font-bold tracking-[0.25em] text-[#38e8ff]">
            ✓ YOUR VOTE
          </span>
        )}
      </button>
    );
  };

  return (
    <div role="group" aria-label={resolvedTitle} className="pointer-events-auto rounded-xl border border-white/10 bg-[#120f1f]/95 p-3 font-mono text-white">
      {state.phase === 'open' && (
        <div className="mb-2 flex items-center justify-between gap-3 border-b border-white/10 pb-2">
          <div className="min-w-0">
            <h3 className="truncate text-xs font-bold tracking-[0.3em] text-[#38e8ff] md:text-sm">
              {resolvedTitle}
            </h3>
            <p className="text-[10px] text-white/55">
              {canVote
                ? state.question !== null
                  ? QUESTION_TAGLINE[state.question]
                  : 'Tap an option to answer.'
                : 'Window closed — waiting for the result…'}
            </p>
          </div>
          <CountdownRing remainingMs={state.remainingMs} durationMs={state.durationMs} />
        </div>
      )}

      {state.phase === 'idle' && (
        <div role="status" className="py-6 text-center">
          <p className="text-sm text-white/60">No poll is running.</p>
        </div>
      )}

      {state.phase === 'open' && (
        <div className="flex flex-col gap-2">{state.options.map((_, i) => optionButton(i))}</div>
      )}

      {state.phase === 'open' && (
        <p role="status" className="mt-2 text-center text-[10px] text-white/55">
          {canVote
            ? state.yourVote === null
              ? 'Not answered yet.'
              : 'You can change your answer until the window closes.'
            : 'Window closed — waiting for the result…'}
          {' · '}
          {ballots} {ballots === 1 ? 'ballot' : 'ballots'} cast
        </p>
      )}

      {state.phase === 'revealed' && (
        <div role="status" className="py-3 text-center">
          <p className="text-[11px] font-bold tracking-[0.3em] text-white/55">POLL CLOSED</p>
          <p className="mt-1 text-2xl font-black leading-tight text-white md:text-3xl">
            {winner ? winner.name : 'Unknown winner'}
          </p>
          {winner && (
            <span
              className={`mt-1 inline-block rounded-full border px-2 py-0.5 text-[10px] font-bold tracking-[0.25em] ${KIND_BADGE[winner.kind]}`}
            >
              {KIND_LABEL[winner.kind]}
            </span>
          )}
          <p className="mt-2 text-xs text-white/65">
            {state.options.map((o, i) => `${o.name} ${state.counts[i] ?? 0}`).join('  ·  ')}
          </p>
          <p className="mt-3 text-[11px] leading-snug text-[#8ef1ff]">
            The server applies the winning choice to the run.
          </p>
          {state.yourVote !== null && state.options[state.yourVote] && (
            <p className="mt-2 text-[10px] text-white/55">
              Your answer: {state.options[state.yourVote]?.name}
            </p>
          )}
          {onDismissReveal && (
            <div>
              <button
                type="button"
                onClick={onDismissReveal}
                className="mt-3 rounded border border-white/15 px-4 py-1 text-[11px] tracking-widest text-white/70 transition hover:text-white focus:outline-none focus-visible:ring-2 focus-visible:ring-white/40"
              >
                DISMISS
              </button>
            </div>
          )}
        </div>
      )}

      {state.error !== null && (
        <p role="alert" className="mt-2 text-center text-xs text-[#ff4d6d]">
          {state.error}
        </p>
      )}
    </div>
  );
}
