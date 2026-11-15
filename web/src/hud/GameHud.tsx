import type { CSSProperties, JSX } from "react";

import { giftsLabel } from "./format";
import { COMMON } from "../scene/theme";

const COUNTER_TEXT_SHADOW: CSSProperties["textShadow"] =
  "0 2px 10px rgba(0, 0, 0, 0.9), 0 0 4px rgba(0, 0, 0, 0.9)";

type RadarHudState = {
  giftArrow: boolean;
  enemyRadar: boolean;
  hazardRadar: boolean;
  pingReady: boolean;
};

type GameHudProps = {
  level: number;
  giftsRemaining: number;
  giftsTotal: number;
  radar?: RadarHudState;
};

type RadarChip = {
  key: Exclude<keyof RadarHudState, "pingReady">;
  label: string;
};

const RADAR_CHIPS: readonly RadarChip[] = [
  { key: "giftArrow", label: "COMPASS" },
  { key: "enemyRadar", label: "PING" },
  { key: "hazardRadar", label: "GROUND" },
];

export function GameHud(props: GameHudProps): JSX.Element {
  const { level, giftsRemaining, giftsTotal, radar } = props;
  const beaconOpen: boolean = giftsRemaining === 0 && giftsTotal > 0;
  const collected: number = Math.min(
    Math.max(giftsTotal - giftsRemaining, 0),
    giftsTotal,
  );
  const ownedChips: RadarChip[] = radar
    ? RADAR_CHIPS.filter((chip) => radar[chip.key])
    : [];

  return (
    <div className="pointer-events-none absolute inset-0 font-mono">
      <div className="absolute left-1/2 top-3 -translate-x-1/2">
        <div className="flex items-center justify-center gap-5 rounded-md border border-white/15 bg-black/60 px-5 py-2">
          <div className="whitespace-nowrap text-sm tracking-widest text-white/70">
            LEVEL{" "}
            <span data-testid="level-number" className="text-xl font-bold text-white">
              {level}
            </span>
          </div>
        </div>
        {beaconOpen ? (
          <div
            data-testid="beacon-open"
            className="mt-1 text-center text-sm font-bold tracking-widest text-[#38e8ff]"
          >
            BEACON OPEN
          </div>
        ) : null}
      </div>
      <div className="absolute bottom-10 left-1/2 flex -translate-x-1/2 items-center gap-3">
        <span
          aria-hidden="true"
          className="inline-block h-4 w-4"
          // Normal gifts are black now, so the swatch needs its glow ring to stay
          // visible against the dark HUD panel.
          style={{
            backgroundColor: COMMON.gift.body,
            boxShadow: `inset 0 0 0 2px ${COMMON.gift.glow}`,
          }}
        />
        <span
          data-testid="gift-counter"
          className="whitespace-nowrap text-5xl font-bold"
          style={{ color: COMMON.ui.counter, textShadow: COUNTER_TEXT_SHADOW }}
        >
          {collected} / {giftsTotal}
        </span>
      </div>
      {radar !== undefined && ownedChips.length > 0 ? (
        <div
          data-testid="radar-chips"
          className="absolute bottom-3 left-1/2 flex -translate-x-1/2 items-center gap-2"
        >
          {ownedChips.map((chip) => {
            const dimmed: boolean =
              chip.key === "enemyRadar" && !radar.pingReady;
            return (
              <span
                key={chip.key}
                data-testid={`radar-chip-${chip.label.toLowerCase()}`}
                className={`rounded-sm border border-white/15 bg-black/60 px-2 py-0.5 text-xs tracking-widest ${
                  dimmed ? "text-white/40" : "text-white/80"
                }`}
              >
                {chip.label}
              </span>
            );
          })}
        </div>
      ) : null}
      <p data-testid="gifts-remaining" className="sr-only">
        {giftsLabel(giftsRemaining, giftsTotal)}
      </p>
    </div>
  );
}
