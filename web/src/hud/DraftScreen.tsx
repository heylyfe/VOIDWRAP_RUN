'use client';

// Full-screen draft overlay: enemy picks + reward picks, hotkeys 1..n, Enter
// to submit. The selection state lives in an inner component that is keyed by
// the card manifest, so a new draft restarts the choice cleanly (no setState
// in effects).
import { useCallback, useEffect, useState } from 'react';
import type { JSX } from 'react';
import '../content';
import { CARD_ENEMY } from '../content';
import { DraftCard } from './DraftCard';
import { selectionComplete, toggleSelection } from './draftSelection';

const DANGER = '#ff4d4d';
const MAGENTA = '#ff3db8';
const GOLD = '#ffd23d';
const MUTED = 'rgba(255,255,255,0.55)';

export interface DraftScreenProps {
  cards: readonly { kind: number; id: string }[];
  enemyPicks: number;
  rewardPicks: number;
  nextLevel: number;
  currency: number;
  lives: number;
  waiting: boolean;
  onSubmit: (picks: number[]) => void;
}

function DraftScreenInner(props: DraftScreenProps): JSX.Element {
  const { cards, enemyPicks, rewardPicks, nextLevel, currency, lives, waiting, onSubmit } = props;

  const enemyCount = cards.filter((c) => c.kind === CARD_ENEMY).length;
  const rewardCount = cards.length - enemyCount;
  // Never allow more picks than there are cards in a group.
  const enemyLimit = Math.min(enemyPicks, enemyCount);
  const rewardLimit = Math.min(rewardPicks, rewardCount);

  const [selected, setSelected] = useState<readonly number[]>([]);

  const toggle = useCallback(
    (index: number) => {
      if (waiting) {
        return;
      }
      setSelected((prev) => toggleSelection(prev, index, cards, enemyLimit, rewardLimit));
    },
    [cards, enemyLimit, rewardLimit, waiting],
  );

  const complete = selectionComplete(selected, cards, enemyLimit, rewardLimit);

  const submit = useCallback(() => {
    if (waiting || !complete) {
      return;
    }
    onSubmit([...selected].sort((a, b) => a - b));
  }, [complete, onSubmit, selected, waiting]);

  useEffect(() => {
    if (waiting) {
      return;
    }
    const onKeyDown = (event: KeyboardEvent): void => {
      if (event.repeat) {
        return;
      }
      if (event.ctrlKey || event.metaKey || event.altKey) {
        return;
      }
      if (event.key === 'Enter') {
        // A focused Continue button handles its own click via Enter.
        if (event.target instanceof HTMLElement && event.target.tagName === 'BUTTON') {
          return;
        }
        submit();
        return;
      }
      const digit = Number.parseInt(event.key, 10);
      if (!Number.isNaN(digit) && digit >= 1 && digit <= 9) {
        toggle(digit - 1);
      }
    };
    window.addEventListener('keydown', onKeyDown);
    return () => {
      window.removeEventListener('keydown', onKeyDown);
    };
  }, [submit, toggle, waiting]);

  const enemySelected = selected.filter((i) => i >= 0 && i < cards.length && cards[i]?.kind === CARD_ENEMY).length;
  const rewardSelected = selected.length - enemySelected;

  return (
    <div
      data-testid="draft-screen"
      className="pointer-events-auto fixed inset-0 z-40 overflow-y-auto bg-black/55 font-mono backdrop-blur-sm"
    >
      <div className="mx-auto flex min-h-full w-full max-w-4xl flex-col gap-5 px-4 py-6 text-white">
        <header className="flex flex-wrap items-baseline justify-between gap-2 border-b border-white/10 pb-3">
          <h1 className="text-2xl font-bold tracking-[0.3em]">LEVEL {nextLevel}</h1>
          <div className="flex items-center gap-4 text-sm">
            <span className="flex items-center gap-1" title="currency">
              <span aria-hidden="true" style={{ color: GOLD }}>●</span>
              <span className="text-white/90">{currency}</span>
            </span>
            <span style={{ color: DANGER }}>❤ {lives}</span>
          </div>
        </header>

        {cards.length === 0 ? (
          <section data-testid="draft-first-round" className="flex flex-col gap-2 py-6 text-center">
            <h2 className="text-lg font-bold tracking-[0.25em] text-white/90">A CLEAN START</h2>
            <p className="text-sm text-white/75">
              No hunters, no upgrades, no curses this round. Collect every gift and reach the beacon.
            </p>
            <p className="text-xs tracking-widest" style={{ color: MUTED }}>
              After this level you vote on who hunts you. Press CONTINUE (or Enter) when you are ready.
            </p>
          </section>
        ) : (
          <p className="text-xs tracking-widest" style={{ color: MUTED }}>
            {enemySelected}/{enemyLimit} hunters · {rewardSelected}/{rewardLimit} rewards · keys 1-9 toggle · enter continues
          </p>
        )}

        {enemyCount > 0 && (
          <section className="flex flex-col gap-3">
            <h2 className="text-sm font-bold tracking-[0.25em] text-white/85">
              WHO HUNTS YOU <span style={{ color: DANGER }}>— choose {enemyPicks}</span>
            </h2>
            <div className="grid grid-cols-1 gap-3 sm:grid-cols-2 lg:grid-cols-4">
              {cards.map((card, index) =>
                card.kind === CARD_ENEMY ? (
                  <span
                    key={`${index}:${card.id}`}
                    className={waiting ? 'block min-w-0 pointer-events-none opacity-60' : 'block min-w-0'}
                  >
                    <DraftCard
                      kind={card.kind}
                      id={card.id}
                      selected={selected.includes(index)}
                      hotkey={index + 1}
                      onToggle={() => toggle(index)}
                    />
                  </span>
                ) : null,
              )}
            </div>
          </section>
        )}

        {rewardCount > 0 && (
          <section className="flex flex-col gap-3">
            <h2 className="text-sm font-bold tracking-[0.25em] text-white/85">
              TAKE ONE FROM THE VOID <span style={{ color: MAGENTA }}>— choose {rewardPicks}</span>
            </h2>
            <div className="grid grid-cols-1 gap-3 sm:grid-cols-2 lg:grid-cols-4">
              {cards.map((card, index) =>
                card.kind === CARD_ENEMY ? null : (
                  <span
                    key={`${index}:${card.id}`}
                    className={waiting ? 'block min-w-0 pointer-events-none opacity-60' : 'block min-w-0'}
                  >
                    <DraftCard
                      kind={card.kind}
                      id={card.id}
                      selected={selected.includes(index)}
                      hotkey={index + 1}
                      onToggle={() => toggle(index)}
                    />
                  </span>
                ),
              )}
            </div>
          </section>
        )}

        <footer className="mt-auto flex flex-col items-center gap-2 pt-4">
          {waiting ? (
            <div
              data-testid="draft-waiting"
              className="rounded-lg border border-white/15 bg-[#120f1f] px-6 py-3 text-sm tracking-[0.2em] text-white/70"
            >
              Waiting for other players...
            </div>
          ) : (
            <button
              type="button"
              data-testid="draft-continue"
              disabled={!complete}
              onClick={submit}
              className={[
                'rounded-lg border px-8 py-3 text-sm font-bold tracking-[0.3em] transition',
                complete
                  ? 'border-[#ff3db8] text-[#ff3db8] hover:bg-[#ff3db8]/15 focus-visible:ring-2 focus-visible:ring-[#ff3db8]/60 focus:outline-none'
                  : 'cursor-not-allowed border-white/15 text-white/30',
              ].join(' ')}
            >
              CONTINUE
            </button>
          )}
        </footer>
      </div>
    </div>
  );
}

export function DraftScreen(props: DraftScreenProps): JSX.Element {
  // Key the inner component by the card manifest so a fresh draft (new cards
  // array with new content) resets the selection without setState in effects.
  const cardsKey = props.cards.map((c) => `${c.kind}:${c.id}`).join('|');
  return <DraftScreenInner key={`draft:${cardsKey}`} {...props} />;
}
