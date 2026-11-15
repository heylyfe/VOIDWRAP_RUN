import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";
import { ENEMY_LOOKS } from "./enemyLook";
import { ACCENT_MIN_DELTA_E, FOG_MIN_DELTA_E, deltaE, hexToLab } from "./enemyPalette";
import { THEMES } from "./theme";

const here = dirname(fileURLToPath(import.meta.url));

describe("enemyPalette", () => {
  it("converts sRGB to CIELAB on the D65 white point", () => {
    // Reference values: pure white is L*=100 with zero chroma; pure black is L*=0.
    const white = hexToLab("#ffffff");
    expect(white[0]).toBeCloseTo(100, 1);
    expect(white[1]).toBeCloseTo(0, 1);
    expect(white[2]).toBeCloseTo(0, 1);

    const black = hexToLab("#000000");
    expect(black[0]).toBeCloseTo(0, 1);

    // Mid grey is L*~53.6, not 50: sRGB is gamma-encoded.
    expect(hexToLab("#808080")[0]).toBeCloseTo(53.6, 0);
  });

  it("rejects anything that is not #rrggbb", () => {
    expect(() => hexToLab("ff3355")).toThrow(/not a #rrggbb colour/);
    expect(() => hexToLab("#fff")).toThrow(/not a #rrggbb colour/);
  });

  it("measures delta-E as Euclidean CIELAB distance", () => {
    expect(deltaE("#ff3355", "#ff3355")).toBe(0);
    // Symmetric, and larger for a bigger perceived difference.
    expect(deltaE("#ff3355", "#e63946")).toBeCloseTo(deltaE("#e63946", "#ff3355"), 10);
    expect(deltaE("#000000", "#ffffff")).toBeGreaterThan(deltaE("#000000", "#808080"));
  });
});

describe("enemy accent palette", () => {
  it("has one look per enemy and no duplicate accents", () => {
    expect(ENEMY_LOOKS.length).toBe(20);
    const accents = ENEMY_LOOKS.map((l) => l.accent);
    expect(new Set(accents).size).toBe(accents.length);
    const glows = ENEMY_LOOKS.map((l) => l.glow);
    expect(new Set(glows).size).toBe(glows.length);
  });

  it(`keeps every pair of accents at least ${ACCENT_MIN_DELTA_E} delta-E apart`, () => {
    const tooClose: string[] = [];
    for (let i = 0; i < ENEMY_LOOKS.length; i++) {
      for (let j = i + 1; j < ENEMY_LOOKS.length; j++) {
        const a = ENEMY_LOOKS[i]!;
        const b = ENEMY_LOOKS[j]!;
        const d = deltaE(a.accent, b.accent);
        if (d < ACCENT_MIN_DELTA_E) {
          tooClose.push(`${a.behavior} ${a.accent} vs ${b.behavior} ${b.accent}: ${d.toFixed(1)}`);
        }
      }
    }
    expect(tooClose).toEqual([]);
  });

  it(`keeps every accent at least ${FOG_MIN_DELTA_E} delta-E from every theme's fog`, () => {
    const washedOut: string[] = [];
    for (const look of ENEMY_LOOKS) {
      for (const theme of THEMES) {
        const d = deltaE(look.accent, theme.fog.color);
        if (d < FOG_MIN_DELTA_E) {
          washedOut.push(`${look.behavior} ${look.accent} vs ${theme.id} fog ${theme.fog.color}: ${d.toFixed(1)}`);
        }
      }
    }
    expect(washedOut).toEqual([]);
  });
});

describe("model files agree with the look table", () => {
  // The accents are duplicated: `ENEMY_LOOKS` holds the table, and each model hardcodes
  // its accent as an `emissive` literal. Nothing forced the two to agree, so a palette
  // change could land in the table and never reach the screen. This reads the model
  // sources and asserts each one renders its own accent.
  const modelFiles = (): string[] => {
    const src = readFileSync(join(here, "EnemyField.tsx"), "utf8");
    const start = src.indexOf("export const MODEL_COMPONENTS");
    expect(start).toBeGreaterThan(-1);
    const block = src.slice(start, src.indexOf("];", start));
    return [...block.matchAll(/^\s{2}([A-Z][A-Za-z]+),$/gm)].map((m) => m[1] as string);
  };

  it("has one model file per look, in the same order", () => {
    expect(modelFiles().length).toBe(ENEMY_LOOKS.length);
  });

  // A model may light itself with either its accent (the trim colour) or its glow (the
  // hot core) -- RiftWarden and BoltSentry both use the glow, which is a legitimate
  // choice. What is never legitimate is rendering *another* enemy's accent, or rendering
  // no palette colour at all, because then the table and the screen have drifted apart.
  const renderedHexes = (file: string): string[] => {
    const src = readFileSync(join(here, "enemies", `${file}.tsx`), "utf8");
    return [...src.matchAll(/(?:emissive|color)="(#[0-9a-f]{6})"/g)].map((m) => m[1] as string);
  };

  it("lights each enemy with its own accent or glow", () => {
    const files = modelFiles();
    const unwired: string[] = [];
    ENEMY_LOOKS.forEach((look, i) => {
      const file = files[i];
      if (!file) return;
      const hexes = renderedHexes(file);
      if (!hexes.includes(look.accent) && !hexes.includes(look.glow)) {
        unwired.push(`${look.behavior} (${file}.tsx) renders neither accent ${look.accent} nor glow ${look.glow}`);
      }
    });
    expect(unwired).toEqual([]);
  });

  it("never renders another enemy's accent", () => {
    const files = modelFiles();
    const accentOwner = new Map(ENEMY_LOOKS.map((l) => [l.accent, l.behavior]));
    const stolen: string[] = [];
    ENEMY_LOOKS.forEach((look, i) => {
      const file = files[i];
      if (!file) return;
      for (const hex of renderedHexes(file)) {
        const owner = accentOwner.get(hex);
        if (owner && owner !== look.behavior) {
          stolen.push(`${look.behavior} (${file}.tsx) renders ${hex}, which is ${owner}'s accent`);
        }
      }
    });
    expect(stolen).toEqual([]);
  });
});
