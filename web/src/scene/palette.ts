const PLATFORM_TOP_COLORS = ["#3b2a6b", "#1f4b7a", "#5a1f5e", "#20565a"] as const;

export const PALETTE = {
  voidBackground: "#07060d",
  platformTopColors: PLATFORM_TOP_COLORS,
  edgeGlowCyan: "#38e8ff",
  edgeGlowMagenta: "#ff3db8",
  giftGold: "#ffd23d",
  ribbonMagenta: "#ff3db8",
  beaconOpen: "#38e8ff",
  beaconClosed: "#2a3550",
  chaserRed: "#ff4d4d",
  playerBody: "#ff3db8",
  playerHeadHands: "#ffd9b8",
  playerLegs: "#2b2b4a",
  playerFace: "#07060d",
} as const;

const TOP_COLOR_COUNT = PLATFORM_TOP_COLORS.length;

/**
 * Deterministically pick a platform top colour from the platform id.
 * Safe for negative ids: the result of `id % 4` is normalised into [0, 3].
 */
export function platformColor(id: number): string {
  const normalized = ((id % TOP_COLOR_COUNT) + TOP_COLOR_COUNT) % TOP_COLOR_COUNT;
  return PALETTE.platformTopColors[normalized] ?? PLATFORM_TOP_COLORS[0];
}
