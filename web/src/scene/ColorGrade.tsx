"use client";

// Per-level screen grade: a custom postprocessing effect that tints, saturates, contrasts,
// vignettes and scanlines the whole frame, plus the r3f component that cross-fades between
// two levels' grades when the theme changes.
import { useEffect, useMemo, useRef } from "react";
import type { JSX } from "react";
import { useFrame, useThree } from "@react-three/fiber";
import { BlendFunction, Effect } from "postprocessing";
import { Color, Uniform } from "three";
import type { ScreenGrade } from "../content";

/** Seconds a theme change takes to cross-fade from the previous grade to the new one. */
const BLEND_SECONDS = 0.6;

/**
 * Grade fragment shader. It runs in linear space on the HDR input, before the composer's
 * tone mapping, so every step is a plain linear-light add or multiply.
 *
 * Per pixel, in order:
 *   1. brightness  c += uBrightness
 *   2. contrast    c = (c - 0.5) * (1 + uContrast) + 0.5, clamped to >= 0 per channel
 *   3. saturation  c = mix(vec3(luma), c, 1 + uSaturation), luma = dot(c, Rec.709 weights)
 *   4. tint        c = mix(c, c * (uTint * 2), uTintStrength), clamped to >= 0
 *                  soft-light-ish multiply: a mid-grey tint is a no-op, a bright tint lifts
 *                  the channel, a dark tint crushes it
 *   5. vignette    v = smoothstep(0.35, 1.0, distance(uv, 0.5) * 1.41) * uVignetteDarkness
 *                  c = mix(c, uVignetteColor, v)
 *   6. scanlines   c *= 1 - uScanlines * (0.5 + 0.5 * sin(uv.y * uResolutionY * PI + uTime * 2))
 *                  about one dark line per two device pixels, drifting slowly
 */
const FRAGMENT_SHADER = `
uniform vec3 uTint;
uniform float uTintStrength;
uniform float uSaturation;
uniform float uContrast;
uniform float uBrightness;
uniform vec3 uVignetteColor;
uniform float uVignetteDarkness;
uniform float uScanlines;
uniform float uTime;
uniform float uResolutionY;

void mainImage(const in vec4 inputColor, const in vec2 uv, out vec4 outputColor) {
  vec3 c = inputColor.rgb;

  c += uBrightness;

  c = max((c - 0.5) * (1.0 + uContrast) + 0.5, 0.0);

  float luma = dot(c, vec3(0.2126, 0.7152, 0.0722));
  c = mix(vec3(luma), c, 1.0 + uSaturation);

  c = max(mix(c, c * (uTint * 2.0), uTintStrength), 0.0);

  float r = distance(uv, vec2(0.5)) * 1.41;
  float v = smoothstep(0.35, 1.0, r) * uVignetteDarkness;
  c = mix(c, uVignetteColor, v);

  if (uScanlines > 0.0) {
    c *= 1.0 - uScanlines * (0.5 + 0.5 * sin(uv.y * uResolutionY * 3.14159 + uTime * 2.0));
  }

  outputColor = vec4(c, inputColor.a);
}
`;

/**
 * Full-screen colour grade. Every grade parameter is a uniform; `uTime` and `uResolutionY`
 * are driven by the component below and are not part of a `ScreenGrade`.
 *
 * The uniforms are also exposed as typed fields so callers never have to go through the
 * `Map<string, Uniform>` with casts.
 */
export class ColorGradeEffect extends Effect {
  readonly uTint: Uniform<Color>;
  readonly uTintStrength: Uniform<number>;
  readonly uSaturation: Uniform<number>;
  readonly uContrast: Uniform<number>;
  readonly uBrightness: Uniform<number>;
  readonly uVignetteColor: Uniform<Color>;
  readonly uVignetteDarkness: Uniform<number>;
  readonly uScanlines: Uniform<number>;
  readonly uTime: Uniform<number>;
  readonly uResolutionY: Uniform<number>;

  constructor() {
    const uTint = new Uniform(new Color(0xffffff));
    const uTintStrength = new Uniform(0);
    const uSaturation = new Uniform(0);
    const uContrast = new Uniform(0);
    const uBrightness = new Uniform(0);
    const uVignetteColor = new Uniform(new Color(0x000000));
    const uVignetteDarkness = new Uniform(0);
    const uScanlines = new Uniform(0);
    const uTime = new Uniform(0);
    const uResolutionY = new Uniform(1080);

    super("ColorGradeEffect", FRAGMENT_SHADER, {
      blendFunction: BlendFunction.NORMAL,
      uniforms: new Map<string, Uniform>([
        ["uTint", uTint],
        ["uTintStrength", uTintStrength],
        ["uSaturation", uSaturation],
        ["uContrast", uContrast],
        ["uBrightness", uBrightness],
        ["uVignetteColor", uVignetteColor],
        ["uVignetteDarkness", uVignetteDarkness],
        ["uScanlines", uScanlines],
        ["uTime", uTime],
        ["uResolutionY", uResolutionY],
      ]),
    });

    this.uTint = uTint;
    this.uTintStrength = uTintStrength;
    this.uSaturation = uSaturation;
    this.uContrast = uContrast;
    this.uBrightness = uBrightness;
    this.uVignetteColor = uVignetteColor;
    this.uVignetteDarkness = uVignetteDarkness;
    this.uScanlines = uScanlines;
    this.uTime = uTime;
    this.uResolutionY = uResolutionY;
  }
}

/**
 * Push a grade onto an effect. Allocation free apart from the string parsing inside
 * `Color.set`; `uTime` and `uResolutionY` are left alone because they are not grade data.
 */
export function applyGrade(effect: ColorGradeEffect, grade: ScreenGrade): void {
  effect.uTint.value.set(grade.tint);
  effect.uTintStrength.value = grade.tint_strength;
  effect.uSaturation.value = grade.saturation;
  effect.uContrast.value = grade.contrast;
  effect.uBrightness.value = grade.brightness;
  effect.uVignetteColor.value.set(grade.vignette_color);
  effect.uVignetteDarkness.value = grade.vignette_darkness;
  effect.uScanlines.value = grade.scanlines;
}

/** The numeric grade parameters, kept in a preallocated shape so blending never allocates. */
interface GradeNumbers {
  tintStrength: number;
  saturation: number;
  contrast: number;
  brightness: number;
  vignetteDarkness: number;
  scanlines: number;
}

/**
 * Everything the frame loop needs, created once per mounted component and reached through a
 * ref so `useFrame` never mutates a value that was created during render.
 */
interface GradeRuntime {
  readonly effect: ColorGradeEffect;
  applied: boolean;
  blending: boolean;
  elapsed: number;
  readonly startTint: Color;
  readonly targetTint: Color;
  readonly startVignette: Color;
  readonly targetVignette: Color;
  readonly startNumbers: GradeNumbers;
  readonly targetNumbers: GradeNumbers;
}

function emptyNumbers(): GradeNumbers {
  return {
    tintStrength: 0,
    saturation: 0,
    contrast: 0,
    brightness: 0,
    vignetteDarkness: 0,
    scanlines: 0,
  };
}

function createRuntime(effect: ColorGradeEffect): GradeRuntime {
  return {
    effect,
    applied: false,
    blending: false,
    elapsed: 0,
    startTint: new Color(),
    targetTint: new Color(),
    startVignette: new Color(),
    targetVignette: new Color(),
    startNumbers: emptyNumbers(),
    targetNumbers: emptyNumbers(),
  };
}

function lerp(from: number, to: number, t: number): number {
  return from + (to - from) * t;
}

/**
 * Snapshot the current uniforms as the blend start, jump the uniforms to `grade`, then keep
 * that as the blend target. `useFrame` walks the uniforms from start to target over
 * `BLEND_SECONDS`, so the jump is never visible.
 */
function beginBlend(runtime: GradeRuntime, grade: ScreenGrade): void {
  const effect = runtime.effect;

  runtime.startTint.copy(effect.uTint.value);
  runtime.startVignette.copy(effect.uVignetteColor.value);
  runtime.startNumbers.tintStrength = effect.uTintStrength.value;
  runtime.startNumbers.saturation = effect.uSaturation.value;
  runtime.startNumbers.contrast = effect.uContrast.value;
  runtime.startNumbers.brightness = effect.uBrightness.value;
  runtime.startNumbers.vignetteDarkness = effect.uVignetteDarkness.value;
  runtime.startNumbers.scanlines = effect.uScanlines.value;

  applyGrade(effect, grade);

  runtime.targetTint.copy(effect.uTint.value);
  runtime.targetVignette.copy(effect.uVignetteColor.value);
  runtime.targetNumbers.tintStrength = effect.uTintStrength.value;
  runtime.targetNumbers.saturation = effect.uSaturation.value;
  runtime.targetNumbers.contrast = effect.uContrast.value;
  runtime.targetNumbers.brightness = effect.uBrightness.value;
  runtime.targetNumbers.vignetteDarkness = effect.uVignetteDarkness.value;
  runtime.targetNumbers.scanlines = effect.uScanlines.value;

  runtime.elapsed = 0;
  runtime.blending = true;
}

export interface ColorGradeProps {
  grade: ScreenGrade;
}

/**
 * Renders the grade effect into the surrounding `EffectComposer` and keeps it in sync with
 * `grade`. A theme change cross-fades over `BLEND_SECONDS`; the first grade is applied
 * instantly so a level never fades in from the neutral default.
 */
export function ColorGrade({ grade }: ColorGradeProps): JSX.Element {
  const effect = useMemo(() => new ColorGradeEffect(), []);
  const runtimeRef = useRef<GradeRuntime | null>(null);
  const height = useThree((state) => state.size.height);

  useEffect(() => {
    const runtime = createRuntime(effect);
    runtimeRef.current = runtime;
    return () => {
      runtimeRef.current = null;
      effect.dispose();
    };
  }, [effect]);

  useEffect(() => {
    const runtime = runtimeRef.current;
    if (runtime === null) return;
    if (!runtime.applied) {
      runtime.applied = true;
      applyGrade(runtime.effect, grade);
      return;
    }
    beginBlend(runtime, grade);
  }, [grade]);

  useFrame((state, delta) => {
    const runtime = runtimeRef.current;
    if (runtime === null) return;

    const fx = runtime.effect;
    fx.uTime.value = state.clock.elapsedTime;
    fx.uResolutionY.value = height;

    if (!runtime.blending) return;

    runtime.elapsed += delta;
    const progress = Math.min(runtime.elapsed / BLEND_SECONDS, 1);
    const t = progress * progress * (3 - 2 * progress);

    fx.uTintStrength.value = lerp(
      runtime.startNumbers.tintStrength,
      runtime.targetNumbers.tintStrength,
      t,
    );
    fx.uSaturation.value = lerp(
      runtime.startNumbers.saturation,
      runtime.targetNumbers.saturation,
      t,
    );
    fx.uContrast.value = lerp(
      runtime.startNumbers.contrast,
      runtime.targetNumbers.contrast,
      t,
    );
    fx.uBrightness.value = lerp(
      runtime.startNumbers.brightness,
      runtime.targetNumbers.brightness,
      t,
    );
    fx.uVignetteDarkness.value = lerp(
      runtime.startNumbers.vignetteDarkness,
      runtime.targetNumbers.vignetteDarkness,
      t,
    );
    fx.uScanlines.value = lerp(
      runtime.startNumbers.scanlines,
      runtime.targetNumbers.scanlines,
      t,
    );

    fx.uTint.value.copy(runtime.startTint).lerp(runtime.targetTint, t);
    fx.uVignetteColor.value.copy(runtime.startVignette).lerp(runtime.targetVignette, t);

    if (progress >= 1) runtime.blending = false;
  });

  return <primitive object={effect} />;
}
