import type { JSX } from "react";

import { giftsLabel } from "./format";

type GameHudProps = {
  level: number;
  giftsRemaining: number;
  giftsTotal: number;
};

export function GameHud(props: GameHudProps): JSX.Element {
  const { level, giftsRemaining, giftsTotal } = props;
  const beaconOpen: boolean = giftsRemaining === 0 && giftsTotal > 0;

  return (
    <div className="pointer-events-none absolute left-1/2 top-3 -translate-x-1/2 font-mono">
      <div className="flex items-center gap-5 rounded-md border border-white/15 bg-black/60 px-5 py-2">
        <div className="whitespace-nowrap text-sm tracking-widest text-white/70">
          LEVEL{" "}
          <span data-testid="level-number" className="text-xl font-bold text-white">
            {level}
          </span>
        </div>
        <div className="flex items-center gap-2 whitespace-nowrap">
          <span aria-hidden="true" className="inline-block h-3 w-3 bg-[#ffd23d]" />
          <span
            data-testid="gifts-remaining"
            className="text-xl font-bold text-white"
          >
            {giftsLabel(giftsRemaining, giftsTotal)}
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
  );
}
