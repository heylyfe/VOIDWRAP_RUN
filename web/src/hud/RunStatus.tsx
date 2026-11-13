'use client';

// Compact run-status panel pinned top-right above the WebGL canvas:
// currency, lives, and one chip per drafted enemy/curse/upgrade. Only the
// chips take pointer events; tooltips are CSS-only (hover + keyboard focus).
import type { JSX } from 'react';
import { CARD_CURSE, CARD_ENEMY, CARD_UPGRADE, lookupCard } from '../content';

const GOLD = '#ffd23d';
const DANGER = '#ff4d4d';
const CYAN = '#38e8ff';
const CURSE = '#c25555';

interface ChipData {
  // "chipKey" rather than "key": React never passes `key` to a component, so a
  // data field must not share that name.
  chipKey: string;
  label: string;
  detail: string;
  color: string;
  background: string;
  borderColor: string;
}

function buildChip(kind: number, id: string, color: string, background: string, borderColor: string): ChipData {
  const resolved = lookupCard(kind, id);
  if (resolved === null) {
    return {
      chipKey: `${kind}:${id}`,
      label: id,
      detail: 'Unknown card',
      color,
      background,
      borderColor,
    };
  }
  return {
    chipKey: `${kind}:${id}`,
    label: resolved.card.name,
    detail: resolved.kind === CARD_ENEMY ? resolved.card.behavior : resolved.card.effect,
    color,
    background,
    borderColor,
  };
}

function StatusChip(props: Omit<ChipData, "chipKey">): JSX.Element {
  const { label, detail, color, background, borderColor } = props;
  return (
    <span
      tabIndex={0}
      className="group pointer-events-auto relative rounded border px-2 py-0.5 text-[11px] tracking-wide focus:outline-none focus-visible:ring-1 focus-visible:ring-white/40"
      style={{ color, backgroundColor: background, borderColor }}
    >
      {label}
      <span
        role="tooltip"
        className="pointer-events-none absolute right-0 top-full z-50 mt-1 w-48 rounded border border-white/20 bg-black/95 p-2 text-[11px] leading-snug text-white/90 opacity-0 shadow-lg transition-opacity group-hover:opacity-100 group-focus-visible:opacity-100"
      >
        {detail}
      </span>
    </span>
  );
}

export function RunStatus(props: {
  currency: number;
  lives: number;
  enemies: readonly string[];
  curses: readonly string[];
  upgrades: readonly string[];
}): JSX.Element {
  const { currency, lives, enemies, curses, upgrades } = props;

  const chips: ChipData[] = [
    ...enemies.map((id) =>
      buildChip(CARD_ENEMY, id, DANGER, 'rgba(255,77,77,0.12)', 'rgba(255,77,77,0.45)'),
    ),
    ...curses.map((id) =>
      buildChip(CARD_CURSE, id, CURSE, 'rgba(140,30,40,0.25)', 'rgba(140,30,40,0.6)'),
    ),
    ...upgrades.map((id) =>
      buildChip(CARD_UPGRADE, id, CYAN, 'rgba(56,232,255,0.12)', 'rgba(56,232,255,0.45)'),
    ),
  ];

  const hearts =
    lives > 0 && lives <= 5 ? '❤'.repeat(lives) : `x ${lives}`;

  return (
    <div className="pointer-events-none fixed right-3 top-3 z-30 w-64 rounded-lg border border-white/15 bg-black/60 p-3 font-mono text-sm text-white/90 backdrop-blur-sm">
      <div className="flex items-center justify-between">
        <span data-testid="currency" className="flex items-center gap-1.5" title="golden gifts">
          <span aria-hidden="true" style={{ color: GOLD }}>●</span>
          <span className="text-white">{currency}</span>
        </span>
        <span style={{ color: DANGER }} title="lives">{hearts}</span>
      </div>
      {chips.length > 0 && <div className="mt-2 flex flex-wrap gap-1">{chips.map((chip) => (
        <StatusChip
          key={chip.chipKey}
          label={chip.label}
          detail={chip.detail}
          color={chip.color}
          background={chip.background}
          borderColor={chip.borderColor}
        />
      ))}</div>}
    </div>
  );
}
