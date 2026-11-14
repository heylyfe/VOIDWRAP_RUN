"use client";

import type { JSX } from "react";

/** Countdown label from the server's collapseMs; clamps at 0:00 and rounds up. */
function formatCountdown(collapseMs: number): string {
  const total = Math.max(0, Math.ceil(collapseMs / 1000));
  const m = Math.floor(total / 60);
  const s = total % 60;
  return `${m}:${s.toString().padStart(2, "0")}`;
}

/**
 * Phase-2 HUD: collapse banner, the countdown to the end of the collapse, and the
 * golden-gift tally. Replaces the normal GameHud counter while levelPhase != LEVEL_COLLECT.
 */
export function CollapseHud(props: {
  golden: number;
  goldenTotal: number;
  collapseMs: number;
}): JSX.Element {
  const urgent = props.collapseMs > 0 && props.collapseMs <= 10_000;
  return (
    <div className="pointer-events-none absolute inset-x-0 top-0 flex flex-col items-center pt-4">
      <div
        data-testid="collapse-banner"
        className="animate-pulse rounded border border-red-500/70 bg-red-950/70 px-6 py-2 text-center font-mono text-sm font-bold tracking-widest text-red-200 shadow-[0_0_24px_rgba(255,60,40,0.35)]"
      >
        THE VOID IS COLLAPSING - grab the gold, get back to the beacon
      </div>
      <div
        data-testid="collapse-timer"
        className={`mt-2 font-mono text-4xl font-bold tabular-nums ${
          urgent ? "text-red-400" : "text-amber-300"
        }`}
      >
        {formatCountdown(props.collapseMs)}
      </div>
      <div
        data-testid="golden-count"
        className="mt-1 flex items-center gap-2 rounded bg-black/60 px-3 py-1 font-mono text-sm font-bold text-amber-200"
      >
        <span className="inline-block h-3 w-3 rotate-45 bg-amber-300 shadow-[0_0_8px_rgba(255,200,60,0.9)]" />
        GOLD {props.golden} / {props.goldenTotal}
      </div>
    </div>
  );
}
