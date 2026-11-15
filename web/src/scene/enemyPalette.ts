// Perceptual distinctness for the enemy accent palette (ticket T-145).
//
// The roster gives every enemy an `accent` (emissive trim + telegraph colour) and a
// `glow` (hot core). Those two hexes are the only thing that separates one enemy from
// another at gameplay distance, where the silhouette is a few pixels tall and the fog
// is already eating the edges. Two accents that are close in colour read as the same
// enemy; an accent close to the fog colour reads as no enemy at all.
//
// "Close" has to mean perceptual distance, not RGB distance: #ff3355 and #e63946 are
// 0x0D apart in red but only 10.5 apart in CIELAB, which is well inside the range where
// two colours look identical side by side. This module converts to CIELAB and measures
// CIE76 delta-E, which is crude but monotonic with perceived difference and needs no
// dependency.
//
// No three.js / React imports: unit-testable in isolation, same as `enemyLook.ts`.

/** A colour as CIELAB: [L, a, b]. L is lightness 0-100; a and b are chroma axes. */
export type Lab = readonly [number, number, number];

/** Minimum CIE76 delta-E between any two enemy accents. */
export const ACCENT_MIN_DELTA_E = 20;

/**
 * Minimum CIE76 delta-E between an enemy accent and any theme's fog colour.
 *
 * Fog is what the enemy is seen *through*, so this is the readability floor: below it
 * the accent and the fog sit at the same lightness and hue and the model dissolves.
 */
export const FOG_MIN_DELTA_E = 30;

function srgbToLinear(channel: number): number {
  const c = channel / 255;
  return c <= 0.04045 ? c / 12.92 : Math.pow((c + 0.055) / 1.055, 2.4);
}

/** Parse `#rrggbb` into CIELAB (D65 white point). Throws on anything else. */
export function hexToLab(hex: string): Lab {
  const m = /^#([0-9a-fA-F]{6})$/.exec(hex);
  if (!m) throw new Error(`not a #rrggbb colour: ${hex}`);
  const n = parseInt(m[1] as string, 16);
  const r = srgbToLinear((n >> 16) & 0xff);
  const g = srgbToLinear((n >> 8) & 0xff);
  const b = srgbToLinear(n & 0xff);

  const x = (r * 0.4124 + g * 0.3576 + b * 0.1805) / 0.95047;
  const y = r * 0.2126 + g * 0.7152 + b * 0.0722;
  const z = (r * 0.0193 + g * 0.1192 + b * 0.9505) / 1.08883;

  const f = (t: number) => (t > 0.008856 ? Math.cbrt(t) : 7.787 * t + 16 / 116);
  const fx = f(x);
  const fy = f(y);
  const fz = f(z);

  return [116 * fy - 16, 500 * (fx - fy), 200 * (fy - fz)];
}

/** CIE76 delta-E: Euclidean distance in CIELAB. */
export function deltaE(a: string, b: string): number {
  const [l1, a1, b1] = hexToLab(a);
  const [l2, a2, b2] = hexToLab(b);
  return Math.hypot(l1 - l2, a1 - a2, b1 - b2);
}
