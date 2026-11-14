// The single voting widget, reused by the in-run pedestals (variant="pedestal")
// and the lobby poll (variant="poll"). Controlled: state comes from voteReducer.
import type { JSX } from 'react';
import type { VoteKind, VoteViewState } from './model';
import { CountdownRing } from './CountdownRing';

const KIND_LABEL: Record<VoteKind, string> = {
  enemy: 'ENEMY',
  curse: 'CURSE',
  theme: 'THEME',
  difficulty: 'DIFFICULTY',
};

const KIND_BADGE: Record<VoteKind, string> = {
  enemy: 'border-[#ff3db8]/60 text-[#ff8ed2]',
  curse: 'border-[#b06bff]/60 text-[#d3a6ff]',
  theme: 'border-[#38e8ff]/60 text-[#8ef1ff]',
  difficulty: 'border-[#ffd23d]/60 text-[#ffe98d]',
};

export interface VoteWidgetProps {
  state: VoteViewState;
  onVote: (index: number) => void;
  variant: 'pedestal' | 'poll';
  /** Accessible group label and header text. */
  title?: string | undefined;
  /** When set, a DISMISS button is shown on the results reveal. */
  onDismissReveal?: (() => void) | undefined;
}

export function VoteWidget({
  state,
  onVote,
  variant,
  title,
  onDismissReveal,
}: VoteWidgetProps): JSX.Element {
  const resolvedTitle = title ?? (variant === 'pedestal' ? 'CHOOSE THE NEW PERMANENT' : 'PRE-MATCH POLL');
  const canVote = state.phase === 'open' && state.remainingMs > 0;
  const winner =
    state.winnerIndex !== null ? state.options[state.winnerIndex] ?? null : null;

  const rootClass =
    variant === 'pedestal'
      ? 'pointer-events-auto rounded-xl border border-[#38e8ff]/30 bg-black/70 px-4 py-3 font-mono text-white shadow-[0_0_30px_rgba(56,232,255,0.15)] backdrop-blur-md'
      : 'pointer-events-auto rounded-xl border border-white/10 bg-[#120f1f]/95 p-3 font-mono text-white';
  const gridClass = variant === 'pedestal' ? 'grid grid-cols-3 gap-2 sm:gap-3' : 'flex flex-col gap-2';

  const optionButton = (optionIndex: number): JSX.Element | null => {
    const option = state.options[optionIndex];
    if (!option) return null;
    const selected = state.yourVote === optionIndex;
    const base =
      variant === 'pedestal'
        ? 'flex min-h-40 flex-col items-center justify-between gap-1 rounded-lg border-2 px-2 py-3 text-center transition focus:outline-none focus-visible:ring-2 focus-visible:ring-[#38e8ff]/60 disabled:cursor-not-allowed disabled:opacity-80'
        : 'flex items-center justify-between gap-3 rounded-lg border px-3 py-2 text-left transition focus:outline-none focus-visible:ring-2 focus-visible:ring-[#38e8ff]/60 disabled:cursor-not-allowed disabled:opacity-80';
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
        className={`${base} ${tone}`}
      >
        <span
          className={`rounded-full border px-2 py-0.5 text-[10px] font-bold tracking-[0.25em] ${KIND_BADGE[option.kind]}`}
        >
          {KIND_LABEL[option.kind]}
        </span>
        <span
          className={
            variant === 'pedestal'
              ? 'text-base font-bold leading-tight md:text-2xl'
              : 'text-sm font-bold leading-snug'
          }
        >
          {option.name}
        </span>
        <span className="flex flex-col items-center leading-none">
          <span className={variant === 'pedestal' ? 'text-3xl font-black tabular-nums md:text-4xl' : 'text-xl font-black tabular-nums'}>
            {state.counts[optionIndex] ?? 0}
          </span>
          <span className="mt-1 text-[9px] tracking-[0.3em] text-white/55">VOTES</span>
        </span>
        {selected && (
          <span className="text-[10px] font-bold tracking-[0.25em] text-[#38e8ff]">
            ✓ YOUR VOTE
          </span>
        )}
        {variant === 'pedestal' && (
          <span
            aria-hidden="true"
            className="mt-1 block h-1.5 w-4/5 rounded-sm bg-gradient-to-r from-transparent via-[#38e8ff]/70 to-transparent"
          />
        )}
      </button>
    );
  };

  return (
    <div role="group" aria-label={resolvedTitle} className={rootClass}>
      {state.phase === 'open' && (
        <div className="mb-2 flex items-center justify-between gap-3 border-b border-white/10 pb-2">
          <div className="min-w-0">
            <h3 className="truncate text-xs font-bold tracking-[0.3em] text-[#38e8ff] md:text-sm">
              {resolvedTitle}
            </h3>
            <p className="text-[10px] text-white/55">
              {canVote ? 'Tap a pedestal to vote.' : 'Window closed — waiting for the result…'}
            </p>
          </div>
          <CountdownRing remainingMs={state.remainingMs} durationMs={state.durationMs} />
        </div>
      )}

      {state.phase === 'idle' && (
        <div role="status" className="py-6 text-center">
          <p className="text-sm text-white/60">
            {variant === 'poll' ? 'No poll is running.' : 'No vote is running.'}
          </p>
          {variant === 'pedestal' && (
            <p className="mt-1 text-[10px] text-white/40">
              Pedestals appear near the exit during the collapse window.
            </p>
          )}
        </div>
      )}

      {state.phase === 'open' && <div className={gridClass}>{state.options.map((_, i) => optionButton(i))}</div>}

      {state.phase === 'open' && (
        <p role="status" className="mt-2 text-center text-[10px] text-white/55">
          {canVote
            ? state.yourVote === null
              ? 'Not voted yet.'
              : 'You can change your vote until the window closes.'
            : 'Window closed — waiting for the result…'}
        </p>
      )}

      {state.phase === 'revealed' && (
        <div role="status" className="py-3 text-center">
          <p className="text-[11px] font-bold tracking-[0.3em] text-white/55">VOTE CLOSED</p>
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
          {variant === 'pedestal' ? (
            <div
              data-testid="new-permanent-banner"
              className="mt-3 inline-block rounded-md border-2 border-[#ff3db8] bg-[#ff3db8]/15 px-5 py-2 text-xs font-bold tracking-[0.25em] text-[#ffb3dd] md:text-sm"
            >
              NEW PERMANENT: {winner ? winner.name.toUpperCase() : 'UNKNOWN'}
            </div>
          ) : (
            <p className="mt-3 text-[11px] leading-snug text-[#8ef1ff]">
              Your choice was reported. The pattern engine applies it to the next level&apos;s tag
              weights.
            </p>
          )}
          {state.yourVote !== null && state.options[state.yourVote] && (
            <p className="mt-2 text-[10px] text-white/55">
              Your vote: {state.options[state.yourVote]?.name}
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
