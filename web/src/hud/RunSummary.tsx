"use client";

import Link from "next/link";
import { useEffect } from "react";

import type { JSX } from "react";

import { ACHIEVEMENTS } from "../content";
import { causeText } from "./format";
import type { DeathCause } from "./format";

type RunSummaryProps = {
  levelReached: number;
  giftsCollected: number;
  cause: DeathCause;
  /** Cosmetic death-screen code (T-088); empty when the server did not send one. */
  code?: string;
  /** Ids of the achievements unlocked this run (T-088), in unlock order. */
  achievements?: readonly string[];
  onRestart: () => void;
};

export function RunSummary(props: RunSummaryProps): JSX.Element {
  const { levelReached, giftsCollected, cause, code, achievements, onRestart } = props;
  const badges = (achievements ?? [])
    .map((id) => ACHIEVEMENTS.find((a) => a.id === id))
    .filter((a): a is (typeof ACHIEVEMENTS)[number] => a !== undefined);

  useEffect(() => {
    const handleKeyDown = (event: KeyboardEvent): void => {
      if (event.repeat) {
        return;
      }
      if (event.key === "Enter") {
        event.preventDefault();
        onRestart();
      }
    };
    window.addEventListener("keydown", handleKeyDown);
    return () => {
      window.removeEventListener("keydown", handleKeyDown);
    };
  }, [onRestart]);

  return (
    <div
      data-testid="run-summary"
      role="dialog"
      aria-modal="true"
      aria-labelledby="run-summary-title"
      className="pointer-events-auto absolute inset-0 z-50 flex items-center justify-center bg-black/70 font-mono"
    >
      <div className="w-[min(24rem,90vw)] rounded-lg border border-white/15 bg-[#120f1f] p-6">
        <h2
          id="run-summary-title"
          className="text-center text-3xl font-bold tracking-widest text-[#ff3db8]"
        >
          RUN OVER
        </h2>
        <dl className="mt-6 space-y-2 text-lg">
          <div className="flex items-baseline justify-between gap-4">
            <dt className="text-white/70">Level reached</dt>
            <dd className="text-xl font-bold text-white">{levelReached}</dd>
          </div>
          <div className="flex items-baseline justify-between gap-4">
            <dt className="text-white/70">Gifts collected</dt>
            <dd className="text-xl font-bold text-[#ffd23d]">{giftsCollected}</dd>
          </div>
          <div className="flex items-baseline justify-between gap-4">
            <dt className="text-white/70">Cause</dt>
            <dd className="text-right text-base font-bold text-[#ff4d4d]">
              {causeText(cause)}
            </dd>
          </div>
        </dl>
        {badges.length > 0 ? (
          <ul
            data-testid="achievement-badges"
            className="mt-5 flex flex-wrap justify-center gap-2"
          >
            {badges.map((badge) => (
              <li
                key={badge.id}
                title={badge.description}
                className="rounded-full border border-[#ffd23d]/40 bg-[#ffd23d]/10 px-3 py-1 text-xs font-bold text-[#ffd23d]"
              >
                {badge.name}
              </li>
            ))}
          </ul>
        ) : null}
        {code ? (
          <div className="mt-5 rounded border border-[#38e8ff]/30 bg-[#38e8ff]/5 px-3 py-2 text-center">
            <div className="text-[0.65rem] uppercase tracking-[0.2em] text-white/50">
              Death code
            </div>
            <div
              data-testid="death-code"
              className="mt-1 select-all text-lg font-bold tracking-[0.15em] text-[#38e8ff]"
            >
              {code}
            </div>
          </div>
        ) : null}
        <button
          type="button"
          data-testid="run-again"
          autoFocus
          onClick={onRestart}
          className="mt-8 block w-full rounded bg-[#ff3db8] px-4 py-3 text-lg font-bold text-black transition-colors hover:bg-[#ff3db8]/80"
        >
          Run again
        </button>
        <Link
          href="/"
          className="mt-3 block text-center text-sm text-[#38e8ff] underline hover:text-white"
        >
          Back to title
        </Link>
      </div>
    </div>
  );
}
