'use client';

// Lobby shop row shown below the draft cards from level 2 on. The server
// sends the wire offer; details come from design/content.json. Buying sends
// one item id; the server answers with a fresh Shop + RunState + Notice.
import type { JSX } from 'react';
import type { ShopItem } from '../protocol/generated';
import { buyBlockedReason, resolveShopItems } from './shop';

const GOLD = '#ffd23d';
const DANGER = '#ff4d6d';
const MUTED = 'rgba(255,255,255,0.55)';

export function ShopPanel(props: {
  /** The wire offer as sent by the server; details are looked up by id. */
  items: readonly ShopItem[];
  currency: number;
  disabled?: boolean;
  onBuy: (itemId: string) => void;
}): JSX.Element {
  const { items, currency, disabled = false, onBuy } = props;
  const resolved = resolveShopItems(items);

  return (
    <section data-testid="shop-panel" className="flex flex-col gap-3">
      <div className="flex flex-wrap items-baseline justify-between gap-2">
        <h2 className="text-sm font-bold tracking-[0.25em] text-white/85">LOBBY SHOP</h2>
        <p className="flex items-center gap-1 text-xs text-white/70" title="team gold">
          <span aria-hidden="true" style={{ color: GOLD }}>●</span>
          <span className="text-white/90">{currency}</span>
          <span style={{ color: MUTED }}> team gold</span>
        </p>
      </div>

      {resolved.length === 0 ? (
        <p className="text-xs text-white/55">Nothing in stock right now.</p>
      ) : (
        <div className="grid grid-cols-1 gap-3 sm:grid-cols-2 lg:grid-cols-4">
          {resolved.map((item) => {
            const reason = buyBlockedReason(item, currency);
            const blocked = reason !== null || disabled;
            return (
              <div
                key={item.id}
                className="flex h-full w-full min-w-0 flex-col gap-1.5 break-words rounded-lg border border-white/15 bg-[#120f1f]/90 p-4 font-mono leading-snug"
              >
                <span className="text-sm font-bold text-white">{item.name}</span>
                <span className="text-xs leading-snug" style={{ color: GOLD }}>{item.effect}</span>
                <span className="text-[11px] italic text-white/50">{item.flavor}</span>

                <div className="mt-auto flex items-center justify-between gap-2 pt-2">
                  <span className="flex items-center gap-1 text-sm" title="cost">
                    <span aria-hidden="true" style={{ color: GOLD }}>●</span>
                    <span className="font-bold text-white/90">{item.cost}</span>
                  </span>
                  <span className="text-[10px] tracking-widest text-white/55">
                    {item.stock > 0 ? `${item.stock} LEFT` : 'SOLD OUT'}
                  </span>
                </div>

                <button
                  type="button"
                  disabled={blocked}
                  onClick={() => onBuy(item.id)}
                  className={[
                    'mt-2 rounded-lg border px-4 py-2 text-xs font-bold tracking-[0.3em] transition focus:outline-none focus-visible:ring-2 focus-visible:ring-white/40',
                    blocked
                      ? 'cursor-not-allowed border-white/15 text-white/30'
                      : 'border-[#ffd23d] text-[#ffd23d] hover:bg-[#ffd23d]/15',
                  ].join(' ')}
                >
                  BUY
                </button>
                {reason !== null ? (
                  <p
                    className="text-[11px] leading-snug"
                    style={{ color: reason === 'Sold out' ? MUTED : DANGER }}
                  >
                    {reason}
                  </p>
                ) : null}
              </div>
            );
          })}
        </div>
      )}
    </section>
  );
}
