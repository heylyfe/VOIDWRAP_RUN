'use client';

// A single draft card tile. Looks its content up from the manifest; unknown
// wire ids degrade to a muted "Unknown card" tile that stays clickable-safe.
import type { JSX } from 'react';
import type { EnemyCard, ModCard } from '../content';
import { CARD_CURSE, CARD_ENEMY, CARD_UPGRADE, lookupCard } from '../content';

const DANGER = '#ff4d4d';
const CYAN = '#38e8ff';
const MAGENTA = '#ff3db8';
const MUTED = 'rgba(255,255,255,0.55)';
const PIP_EMPTY = 'rgba(255,255,255,0.14)';

function ThreatPips(props: { threat: number }): JSX.Element {
  const clamped = Math.max(0, Math.min(5, Math.round(props.threat)));
  return (
    <div className="flex items-center gap-1" aria-label={`threat ${clamped} of 5`}>
      {[0, 1, 2, 3, 4].map((pip) => (
        <span
          key={pip}
          aria-hidden="true"
          className="inline-block h-2.5 w-2.5 rounded-full"
          style={{ backgroundColor: pip < clamped ? DANGER : PIP_EMPTY }}
        />
      ))}
      <span className="ml-1 text-[10px] tracking-widest" style={{ color: MUTED }}>
        THREAT {clamped}/5
      </span>
    </div>
  );
}

function EnemyBody(props: { card: EnemyCard }): JSX.Element {
  const { card } = props;
  return (
    <>
      <span className="text-base font-bold text-white">{card.name}</span>
      <span className="text-xs italic" style={{ color: MUTED }}>{card.flavor}</span>
      <span className="text-xs leading-snug text-white/80">{card.behavior}</span>
      <ThreatPips threat={card.threat} />
    </>
  );
}

function UpgradeBody(props: { card: ModCard }): JSX.Element {
  const { card } = props;
  return (
    <>
      <span className="text-base font-bold text-white">{card.name}</span>
      <span className="text-sm font-semibold" style={{ color: CYAN }}>{card.effect}</span>
      <span className="text-xs italic" style={{ color: MUTED }}>{card.flavor}</span>
    </>
  );
}

function CurseBody(props: { card: ModCard }): JSX.Element {
  const { card } = props;
  return (
    <>
      <span className="text-base font-bold text-white">{card.name}</span>
      <span className="text-sm font-semibold" style={{ color: '#ff9d9d' }}>{card.effect}</span>
      <span className="text-xs italic" style={{ color: MUTED }}>{card.flavor}</span>
      <ThreatPips threat={card.threat ?? 0} />
    </>
  );
}

export function DraftCard(props: {
  kind: number;
  id: string;
  selected: boolean;
  hotkey: number;
  onToggle: () => void;
}): JSX.Element {
  const { kind, id, selected, hotkey, onToggle } = props;
  const resolved = lookupCard(kind, id);

  let accent = 'rgba(255,255,255,0.35)';
  let label = 'CARD';
  let body: JSX.Element;
  if (resolved === null) {
    body = (
      <>
        <span className="text-base font-bold text-white/60">Unknown card</span>
        <span className="text-xs italic" style={{ color: 'rgba(255,255,255,0.4)' }}>
          its content is missing from the manifest
        </span>
      </>
    );
  } else if (resolved.kind === CARD_ENEMY) {
    accent = DANGER;
    label = 'HUNTER';
    body = <EnemyBody card={resolved.card} />;
  } else if (resolved.kind === CARD_UPGRADE) {
    accent = CYAN;
    label = 'UPGRADE';
    body = <UpgradeBody card={resolved.card} />;
  } else {
    accent = DANGER;
    label = resolved.card.greater === true ? 'GREATER CURSE' : 'CURSE';
    body = <CurseBody card={resolved.card} />;
  }

  const isGreaterCurse =
    resolved !== null && resolved.kind === CARD_CURSE && resolved.card.greater === true;

  const rarityBadge =
    resolved !== null && resolved.kind === CARD_UPGRADE
      ? (() => {
          const rare = resolved.card.rarity === 'rare';
          return (
            <span
              className="rounded border px-1 py-px text-[9px] tracking-widest"
              style={{
                color: rare ? MAGENTA : 'rgba(255,255,255,0.6)',
                borderColor: rare ? `${MAGENTA}88` : 'rgba(255,255,255,0.25)',
              }}
            >
              {resolved.card.rarity.toUpperCase()}
            </span>
          );
        })()
      : null;

  return (
    <button
      type="button"
      aria-pressed={selected}
      onClick={onToggle}
      className={[
        'relative flex w-60 flex-col gap-2 rounded-lg border p-4 text-left font-mono leading-snug transition',
        'bg-[#120f1f]/90 focus:outline-none focus-visible:ring-2 focus-visible:ring-white/40',
        selected ? 'scale-[1.03] border-[3px]' : 'border border-white/15',
      ].join(' ')}
      style={{
        borderColor: selected ? accent : undefined,
        boxShadow: isGreaterCurse
          ? `0 0 14px ${DANGER}88, inset 0 0 10px ${DANGER}33`
          : selected
            ? `0 0 12px ${accent}66`
            : undefined,
      }}
    >
      <span
        className="absolute right-2 top-2 flex h-5 w-5 items-center justify-center rounded border text-[10px]"
        style={{
          color: selected ? accent : MUTED,
          borderColor: selected ? accent : 'rgba(255,255,255,0.25)',
          backgroundColor: 'rgba(0,0,0,0.6)',
        }}
        aria-hidden="true"
      >
        {hotkey}
      </span>

      <span
        className="flex items-center gap-2 pr-6 text-[10px] font-bold tracking-[0.25em]"
        style={{ color: accent }}
      >
        {label}
        {rarityBadge}
        {selected ? <span aria-hidden="true">✓</span> : null}
      </span>

      {body}
    </button>
  );
}
