// Typed access to design/palette.json (the single source of truth for colours).
import palette from "../../../design/palette.json";

export interface Theme {
  id: string;
  name: string;
  mood: string;
  sky: { zenith: string; horizon: string; cloud: string; cloudDark: string };
  fog: { color: string; near: number; far: number };
  tile: { light: string; dark: string; side: string; edge: string };
  accent: string;
  debris: string;
}

export interface CommonColors {
  gift: { body: string; ribbon: string; glow: string };
  beacon: { core: string; beam: string; closed: string };
  enemy: { body: string; glow: string; hazardRing: string };
  player: { body: string; skin: string; legs: string; face: string };
  ui: { panel: string; panelTint: string; text: string; counter: string; danger: string };
}

export const THEMES: readonly Theme[] = palette.themes;
export const COMMON: CommonColors = palette.common;

/** Theme for level `n` (1-based); level 0 or negatives behave like level 1. */
export function themeForLevel(n: number): Theme {
  const count = THEMES.length;
  const i = Number.isFinite(n) && n >= 1 ? (Math.floor(n) - 1) % count : 0;
  const theme = THEMES[i] ?? THEMES[0];
  if (!theme) throw new Error("design/palette.json has no themes");
  return theme;
}
