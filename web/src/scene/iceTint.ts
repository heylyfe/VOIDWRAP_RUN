// Pure colour helper for the ice event: blends a platform colour towards a pale blue
// and lifts every channel so icy platforms read as glossy/specular.
const ICE = { r: 186, g: 224, b: 244 } as const;
const BLEND = 0.5;
const LIFT = 24;

function channel(v: number): number {
  return Math.min(255, Math.max(0, Math.round(v)));
}

function toHex(v: number): string {
  return channel(v).toString(16).padStart(2, "0");
}

export function iceTint(baseColor: string): string {
  const raw = baseColor.trim().replace(/^#/, "");
  const norm = raw.length === 3 ? raw.split("").map((c) => c + c).join("") : raw;
  if (!/^[0-9a-fA-F]{6}$/.test(norm)) {
    throw new Error(`iceTint: expected a hex colour like "#rrggbb", got "${baseColor}"`);
  }
  const r = parseInt(norm.slice(0, 2), 16);
  const g = parseInt(norm.slice(2, 4), 16);
  const b = parseInt(norm.slice(4, 6), 16);
  const mix = (c: number, t: number) => channel(c + (t - c) * BLEND + LIFT);
  return `#${toHex(mix(r, ICE.r))}${toHex(mix(g, ICE.g))}${toHex(mix(b, ICE.b))}`;
}
