import { describe, expect, it, vi } from "vitest";
import type { ScreenGrade } from "../content";
import { applyGrade, ColorGradeEffect } from "./ColorGrade";

// ColorGrade.tsx also exports the r3f component; the effect itself is pure three +
// postprocessing, so stub the fiber hooks out and keep the test in the node environment.
vi.mock("@react-three/fiber", () => ({
  useFrame: (): void => undefined,
  useThree: (): { size: { height: number } } => ({ size: { height: 1080 } }),
}));

const FLUXBLOOM: ScreenGrade = {
  tint: "#e13ad0",
  tint_strength: 0.28,
  saturation: 0.25,
  contrast: 0.08,
  brightness: 0.02,
  vignette_color: "#290a33",
  vignette_darkness: 0.55,
  aberration: 0.0018,
  grain: 0.02,
  scanlines: 0.08,
};

const NEUTRAL: ScreenGrade = {
  tint: "#ffffff",
  tint_strength: 0,
  saturation: 0,
  contrast: 0,
  brightness: 0,
  vignette_color: "#000000",
  vignette_darkness: 0.4,
  aberration: 0.001,
  grain: 0.02,
  scanlines: 0,
};

const UNIFORM_NAMES: readonly string[] = [
  "uBrightness",
  "uContrast",
  "uResolutionY",
  "uSaturation",
  "uScanlines",
  "uTime",
  "uTint",
  "uTintStrength",
  "uVignetteColor",
  "uVignetteDarkness",
];

describe("ColorGradeEffect", () => {
  it("declares one uniform per grade parameter plus time and resolution", () => {
    const effect = new ColorGradeEffect();
    expect([...effect.uniforms.keys()].sort()).toEqual([...UNIFORM_NAMES]);
  });
});

describe("applyGrade", () => {
  it("writes every grade value onto the effect uniforms", () => {
    const effect = new ColorGradeEffect();
    applyGrade(effect, FLUXBLOOM);

    expect(effect.uTint.value.getHexString()).toBe("e13ad0");
    expect(effect.uTintStrength.value).toBe(0.28);
    expect(effect.uSaturation.value).toBe(0.25);
    expect(effect.uContrast.value).toBe(0.08);
    expect(effect.uBrightness.value).toBe(0.02);
    expect(effect.uVignetteColor.value.getHexString()).toBe("290a33");
    expect(effect.uVignetteDarkness.value).toBe(0.55);
    expect(effect.uScanlines.value).toBe(0.08);
  });

  it("overwrites a previous grade and leaves time and resolution untouched", () => {
    const effect = new ColorGradeEffect();
    effect.uTime.value = 12.5;
    effect.uResolutionY.value = 720;

    applyGrade(effect, FLUXBLOOM);
    applyGrade(effect, NEUTRAL);

    expect(effect.uTime.value).toBe(12.5);
    expect(effect.uResolutionY.value).toBe(720);
    expect(effect.uTint.value.getHexString()).toBe("ffffff");
    expect(effect.uTintStrength.value).toBe(0);
    expect(effect.uSaturation.value).toBe(0);
    expect(effect.uContrast.value).toBe(0);
    expect(effect.uBrightness.value).toBe(0);
    expect(effect.uVignetteColor.value.getHexString()).toBe("000000");
    expect(effect.uVignetteDarkness.value).toBe(0.4);
    expect(effect.uScanlines.value).toBe(0);
  });
});
