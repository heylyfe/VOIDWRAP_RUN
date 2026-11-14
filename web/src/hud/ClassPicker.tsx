'use client';

// Four class cards for the first-round lobby. Keys 1-4 pick a class while the
// picker is mounted (it is only rendered in the first-round lobby). Cards
// fill their grid cell and never overlap.
import { useEffect } from 'react';
import type { JSX } from 'react';
import { CLASSES } from '../content';

const MAGENTA = '#ff3db8';
const CYAN = '#38e8ff';
const MUTED = 'rgba(255,255,255,0.55)';

export function ClassPicker(props: {
  selected: number;
  onChoose: (cls: number) => void;
  disabled?: boolean;
}): JSX.Element {
  const { selected, onChoose, disabled = false } = props;

  useEffect(() => {
    if (disabled) {
      return;
    }
    const onKeyDown = (event: KeyboardEvent): void => {
      if (event.repeat) {
        return;
      }
      if (event.ctrlKey || event.metaKey || event.altKey) {
        return;
      }
      if (event.target instanceof HTMLElement) {
        const tag = event.target.tagName;
        if (tag === 'INPUT' || tag === 'TEXTAREA' || tag === 'SELECT') {
          return;
        }
      }
      const digit = Number.parseInt(event.key, 10);
      if (!Number.isNaN(digit) && digit >= 1 && digit <= CLASSES.length) {
        onChoose(digit - 1);
      }
    };
    window.addEventListener('keydown', onKeyDown);
    return () => {
      window.removeEventListener('keydown', onKeyDown);
    };
  }, [disabled, onChoose]);

  return (
    <section data-testid="class-picker" className="flex flex-col gap-3">
      <div className="flex flex-wrap items-baseline justify-between gap-2">
        <h2 className="text-sm font-bold tracking-[0.25em] text-white/85">CHOOSE YOUR CLASS</h2>
        <p className="text-[10px] tracking-widest" style={{ color: MUTED }}>
          keys 1-{CLASSES.length}, or click
        </p>
      </div>
      <div className="grid grid-cols-1 gap-3 sm:grid-cols-2 lg:grid-cols-4">
        {CLASSES.map((cls, index) => {
          const isSelected = selected === index;
          return (
            <button
              key={cls.id}
              type="button"
              aria-pressed={isSelected}
              disabled={disabled}
              onClick={() => onChoose(index)}
              className={[
                'relative flex h-full w-full min-w-0 flex-col gap-1.5 break-words rounded-lg border p-4 text-left font-mono leading-snug transition',
                'bg-[#120f1f]/90 focus:outline-none focus-visible:ring-2 focus-visible:ring-white/40',
                disabled ? 'cursor-not-allowed opacity-60' : '',
                isSelected ? 'scale-[1.03] border-[3px]' : 'border border-white/15',
              ].join(' ')}
              style={{
                borderColor: isSelected ? MAGENTA : undefined,
                boxShadow: isSelected ? `0 0 12px ${MAGENTA}66` : undefined,
              }}
            >
              <span
                aria-hidden="true"
                className="absolute right-2 top-2 flex h-5 w-5 items-center justify-center rounded border text-[10px]"
                style={{
                  color: isSelected ? MAGENTA : MUTED,
                  borderColor: isSelected ? MAGENTA : 'rgba(255,255,255,0.25)',
                  backgroundColor: 'rgba(0,0,0,0.6)',
                }}
              >
                {index + 1}
              </span>

              <span className="pr-6 text-base font-bold text-white">{cls.name}</span>
              <span className="text-xs italic" style={{ color: MUTED }}>{cls.flavor}</span>

              <span className="mt-1 text-[9px] font-bold tracking-[0.25em]" style={{ color: CYAN }}>
                ABILITY
              </span>
              <span className="text-xs leading-snug text-white/85">{cls.ability}</span>

              <span className="mt-1 text-[9px] font-bold tracking-[0.25em]" style={{ color: CYAN }}>
                CONTROLS
              </span>
              <span className="text-xs leading-snug text-white/85">{cls.controls}</span>

              {isSelected ? (
                <span
                  className="mt-auto pt-2 text-[10px] font-bold tracking-[0.3em]"
                  style={{ color: MAGENTA }}
                >
                  CHOSEN ✓
                </span>
              ) : null}
            </button>
          );
        })}
      </div>
    </section>
  );
}
