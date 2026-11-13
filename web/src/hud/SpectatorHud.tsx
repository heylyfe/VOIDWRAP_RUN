"use client";

import type { JSX } from "react";

const REASON_TEXT: Record<"beacon" | "dead", string> = {
  beacon: "You reached the beacon",
  dead: "You were caught",
};

export function SpectatorHud(props: {
  reason: "beacon" | "dead";
  playersLeft: number;
  following: string | null;
}): JSX.Element {
  const { reason, playersLeft, following } = props;
  const noun = playersLeft === 1 ? "player" : "players";

  return (
    <div
      data-testid="spectator-hud"
      className="pointer-events-none absolute inset-0 z-30 flex flex-col items-center justify-between font-mono"
    >
      <div className="mt-4 rounded border border-white/15 bg-black/60 px-5 py-3 text-center">
        <div className="text-2xl font-bold tracking-[0.3em]" style={{ color: "#38e8ff" }}>
          SPECTATING
        </div>
        <div className="mt-1 text-sm text-white/85">{REASON_TEXT[reason]}</div>
        <div className="mt-0.5 text-xs text-white/60">
          {playersLeft} {noun} still in the level
        </div>
        {following !== null && (
          <div className="mt-1 text-xs" style={{ color: "#38e8ff" }}>
            Following {following}
          </div>
        )}
      </div>
      <div className="mb-4 rounded border border-white/15 bg-black/60 px-3 py-1.5 text-xs text-white/70">
        WASD fly · Space up · C down · Shift fast · F follow next player · G free camera
      </div>
    </div>
  );
}
