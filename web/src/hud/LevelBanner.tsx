"use client";

import { useEffect, useState } from "react";

import type { JSX } from "react";

const FADE_MS = 1500;
const HIDE_MS = 2000;

type BannerPhase = "shown" | "faded" | "hidden";

type LevelBannerProps = {
  level: number;
};

export function LevelBanner(props: LevelBannerProps): JSX.Element | null {
  const { level } = props;
  const [shownFor, setShownFor] = useState<number>(level);
  const [phase, setPhase] = useState<BannerPhase>("shown");

  // Adjust state while rendering (recommended React pattern): when the level
  // prop changes, re-arm the banner synchronously. The effect below never
  // calls setState synchronously, only inside its timers.
  if (level !== shownFor) {
    setShownFor(level);
    setPhase("shown");
  }

  useEffect(() => {
    const fadeTimer = window.setTimeout(() => setPhase("faded"), FADE_MS);
    const hideTimer = window.setTimeout(() => setPhase("hidden"), HIDE_MS);
    return () => {
      window.clearTimeout(fadeTimer);
      window.clearTimeout(hideTimer);
    };
  }, [shownFor]);

  if (phase === "hidden") {
    return null;
  }

  return (
    <div
      data-testid="level-banner"
      className={`pointer-events-none absolute left-1/2 top-1/2 -translate-x-1/2 -translate-y-1/2 whitespace-nowrap font-mono text-6xl font-bold tracking-widest text-white transition-opacity duration-500 [text-shadow:0_0_24px_#ff3db8,0_0_6px_#07060d] ${
        phase === "faded" ? "opacity-0" : "opacity-100"
      }`}
    >
      LEVEL {level}
    </div>
  );
}
