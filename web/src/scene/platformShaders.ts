// Pure GLSL chunks and header builders for the platform shaders.
// No React imports so vitest can string-check the generated GLSL directly.
import { Color } from "three";

export type UniformValue = { value: unknown };
export type UniformMap = Record<string, UniformValue>;

/** Resolved (already ice-tinted) surface colors; see resolveSurfacePalette in Platforms.tsx. */
export interface SurfacePalette {
  light: string;
  side: string;
  dark: string;
  edge: string;
}

const CHECKER_CELL = 1.5;
const ICE_EMISSIVE = "#33506b";
const ICE_EMISSIVE_INTENSITY = 0.3;
const CHECKER_B_SHADE = 0.6;
const CRUMBLE_SHADE = 0.72;
const CRUMBLE_IDLE_EMISSIVE = 0.22;
const WARN_AMBER = "#ffb13c";
const WARN_RED = "#ff4436";
const EDGE_WIDTH = 0.11;
const EDGE_INTENSITY = 1.5;

function shadeColor(hex: string, factor: number): string {
  const c = new Color(hex);
  c.multiplyScalar(factor);
  return `#${c.getHexString()}`;
}

const ATTRIBUTE_DECLARATIONS = "attribute vec4 aState;\nattribute vec2 aSlope;";

// Varyings must be declared with identical types in BOTH stages.
const VARYING_DECLARATIONS = `varying vec4 vStateF;
varying vec3 vWorldPos;
varying vec3 vLocalPos;
varying vec3 vScaleW;
varying vec3 vNrmW;
varying vec2 vSlope;`;

/**
 * GLSL type of a uniform value: numbers become `float`, three.js Colors and any
 * {r,g,b} object become `vec3`. Throws for anything else so a bad uniform fails
 * at material build time instead of at WebGL compile time.
 */
function glslTypeOf(name: string, value: unknown): string {
  if (typeof value === "number") return "float";
  if (typeof value === "object" && value !== null) {
    const v = value as Record<string, unknown>;
    if (
      typeof v.r === "number" &&
      typeof v.g === "number" &&
      typeof v.b === "number"
    ) {
      return "vec3";
    }
  }
  throw new Error(`Cannot infer GLSL type for uniform "${name}"`);
}

/** Declarations for every uniform in the map, sorted by name (deterministic output). */
export function uniformDeclarations(uniforms: UniformMap): string {
  return Object.entries(uniforms)
    .sort(([a], [b]) => (a < b ? -1 : a > b ? 1 : 0))
    .map(([name, u]) => `uniform ${glslTypeOf(name, u.value)} ${name};`)
    .join("\n");
}

/** Vertex header: built-in include + per-instance attribute + the varyings. */
export function buildVertexHeader(): string {
  return ["#include <common>", ATTRIBUTE_DECLARATIONS, VARYING_DECLARATIONS].join("\n");
}

/** Fragment header: built-in include + a declaration for every uniform in the map + the varyings. */
export function buildFragmentHeader(uniforms: UniformMap): string {
  return ["#include <common>", uniformDeclarations(uniforms), VARYING_DECLARATIONS].join("\n");
}

// ---------------------------------------------------------------------------
// Vertex injection: world position, unit-box local pos, instance scale,
// axis normal and the per-instance state vector (warn, fretted, fade, pillar).
// ---------------------------------------------------------------------------
export const VERTEX_BEGIN = `#include <begin_vertex>
vStateF = aState;
vSlope = aSlope;
vLocalPos = position;
vec4 vwIp = vec4(transformed, 1.0);
vec3 vwScale = vec3(1.0);
#ifdef USE_INSTANCING
vwIp = instanceMatrix * vwIp;
vwScale = vec3(length(instanceMatrix[0].xyz), length(instanceMatrix[1].xyz), length(instanceMatrix[2].xyz));
#endif
vScaleW = vwScale;
vWorldPos = (modelMatrix * vwIp).xyz;
vec3 vwN = objectNormal;
#ifdef USE_INSTANCING
vwN = mat3(instanceMatrix) * vwN;
#endif
vNrmW = normalize(mat3(modelMatrix) * vwN);`;

/** Warn-flash (amber->red) + fretted-vein blocks shared by surface and pad shaders. */
export const GLSL_WARN_FRET = `
{
  float fade = vStateF.z;
  float topF = step(0.5, vNrmW.y);
  float warn = vStateF.x;
  float fret = vStateF.y;
  if (warn > 0.5) {
    float b = 0.5 + 0.5 * sin(uTime * 9.0);
    totalEmissiveRadiance += mix(uWarnAmber, uWarnRed, b) * (1.1 + 1.6 * b);
  }
  if (fret > 0.5) {
    vec2 fp = vWorldPos.xz * 2.6;
    float v1 = 1.0 - smoothstep(0.0, 0.16, abs(sin(fp.x + sin(fp.y * 1.7) * 1.6)));
    float v2 = 1.0 - smoothstep(0.0, 0.16, abs(sin(fp.y * 1.3 + sin(fp.x * 1.9) * 1.6)));
    float vein = clamp(v1 + v2, 0.0, 1.0);
    float crawl = 0.65 + 0.35 * sin(uTime * 0.9 + fp.x * 0.6 + fp.y * 0.4);
    totalEmissiveRadiance += vec3(0.10, 1.0, 0.55) * vein * (0.5 + 0.5 * crawl) * 0.9;
  }
  if (uIce > 0.5 && topF > 0.5) {
    totalEmissiveRadiance += uIceEmissive * ${ICE_EMISSIVE_INTENSITY};
  }
  totalEmissiveRadiance *= fade;
}`;

export const SURFACE_FRAGMENT_COLOR = `#include <color_fragment>
{
  float fade = vStateF.z;
  float topF = step(0.5, vNrmW.y);
  float botF = step(0.5, -vNrmW.y);
  vec3 base;
  if (vStateF.w > 0.5) {
    // Tall dark stone pillar: no checker, subtle streaks and horizontal bands.
    float s = sin(vWorldPos.x * 4.7 + vWorldPos.y * 0.9) * sin(vWorldPos.z * 4.3 - vWorldPos.y * 0.8);
    float bands = sin(vWorldPos.y * 2.6);
    base = uPillarColor * (0.82 + 0.18 * s + 0.08 * bands);
  } else if (topF > 0.5) {
    // World-space checkerboard: instances tile seamlessly without per-platform UVs.
    //
    // A ramp's top face is inclined, so world XZ is a projection of it: along the
    // slope axis the pattern is compressed by cos(theta) and the cells read as
    // stretched by 1/cos(theta). Dividing that axis by cos(theta) restores arc
    // length, so the cells stay square on the incline and line up with the flat
    // path pieces at both ends. Flat platforms carry aSlope = (0, 0) and are
    // untouched.
    vec2 chkPos = vWorldPos.xz;
    if (vSlope.y > 0.0) {
      float slopeCos = max(vSlope.y, 0.05);
      if (vSlope.x < 0.5) {
        chkPos.x /= slopeCos;
      } else {
        chkPos.y /= slopeCos;
      }
    }
    vec2 cell = floor(chkPos / ${CHECKER_CELL});
    float chk = mod(cell.x + cell.y, 2.0);
    base = mix(uCheckerA, uCheckerB, chk);
    if (uCrumble > 0.5) {
      base *= ${CRUMBLE_SHADE};
      float r1 = abs(sin(vWorldPos.x * 1.9 + sin(vWorldPos.z * 2.3) * 1.7));
      float r2 = abs(sin(vWorldPos.z * 1.7 + sin(vWorldPos.x * 2.1) * 1.9));
      float crack = (1.0 - smoothstep(0.0, 0.09, r1)) + (1.0 - smoothstep(0.0, 0.09, r2));
      base *= 1.0 - clamp(crack, 0.0, 1.0) * 0.5;
    }
  } else if (botF > 0.5) {
    base = uBottomColor;
  } else {
    base = uSideColor;
  }
  if (vStateF.y > 0.5) {
    base = mix(base, vec3(0.03, 0.10, 0.055), 0.8);
  }
  diffuseColor.rgb = base * fade;
}`;

export const SURFACE_FRAGMENT_EMISSIVE = `#include <emissivemap_fragment>
{
  float fade = vStateF.z;
  float topF = step(0.5, vNrmW.y);
  if (topF > 0.5 && vStateF.w <= 0.5) {
    vec2 off = vLocalPos.xz * vScaleW.xz;
    vec2 halfE = vScaleW.xz * 0.5;
    vec2 dvec = halfE - abs(off);
    float dEdge = min(dvec.x, dvec.y);
    float edgeMask = 1.0 - smoothstep(0.0, ${EDGE_WIDTH}, dEdge);
    totalEmissiveRadiance += uEdgeColor * edgeMask * ${EDGE_INTENSITY};
  }
  if (uCrumble > 0.5) {
    totalEmissiveRadiance += uWarnAmber * ${CRUMBLE_IDLE_EMISSIVE};
  }
  ${GLSL_WARN_FRET}
}`;

export const PAD_FRAGMENT_COLOR = `#include <color_fragment>
{
  float fade = vStateF.z;
  vec3 base = uPadBase;
  if (vStateF.y > 0.5) {
    base = mix(base, vec3(0.03, 0.10, 0.055), 0.8);
  }
  diffuseColor.rgb = base * fade;
}`;

export const PAD_FRAGMENT_EMISSIVE = `#include <emissivemap_fragment>
{
  float fade = vStateF.z;
  float topF = step(0.5, vNrmW.y);
  if (topF > 0.5) {
    float pulse = 0.5 + 0.5 * sin(uTime * 2.6);
    float r = length(vLocalPos.xz);
    float ringR = 0.26 + 0.05 * pulse;
    float ring = 1.0 - smoothstep(0.0, 0.035, abs(r - ringR));
    // Upward chevron (points toward local +Z), bobbing with the pulse.
    vec2 cp = vec2(vLocalPos.x, vLocalPos.z + 0.06 * sin(uTime * 2.6));
    float chevD = abs(cp.y + abs(cp.x) * 0.85 - 0.04);
    float chev = (1.0 - smoothstep(0.02, 0.055, chevD))
      * (1.0 - smoothstep(0.12, 0.2, abs(cp.x)))
      * step(-0.24, cp.y) * (1.0 - step(0.14, cp.y));
    totalEmissiveRadiance += uPadGlow * (ring * (1.3 + 1.4 * pulse) + chev * (2.0 + 0.8 * pulse) + 0.08);
  }
  ${GLSL_WARN_FRET}
}`;

/** Surface-material uniforms (normal + crumble). `palette` is the already ice-tinted tile palette. */
export function buildSurfaceUniforms(
  palette: SurfacePalette,
  icy: boolean,
  isCrumble: boolean,
  timeUniform: { value: number },
): UniformMap {
  return {
    uTime: timeUniform,
    uCheckerA: { value: new Color(palette.light) },
    uCheckerB: { value: new Color(shadeColor(palette.light, CHECKER_B_SHADE)) },
    uSideColor: { value: new Color(palette.side) },
    uBottomColor: { value: new Color(palette.dark) },
    uPillarColor: { value: new Color(shadeColor(palette.side, 0.5)) },
    uEdgeColor: { value: new Color(palette.edge) },
    uWarnAmber: { value: new Color(WARN_AMBER) },
    uWarnRed: { value: new Color(WARN_RED) },
    uIceEmissive: { value: new Color(ICE_EMISSIVE) },
    uIce: { value: icy ? 1 : 0 },
    uCrumble: { value: isCrumble ? 1 : 0 },
  };
}

/** Jump-pad uniforms. */
export function buildPadUniforms(timeUniform: { value: number }): UniformMap {
  return {
    uTime: timeUniform,
    uPadBase: { value: new Color("#20242c") },
    uPadGlow: { value: new Color("#ffc24b") },
    uWarnAmber: { value: new Color(WARN_AMBER) },
    uWarnRed: { value: new Color(WARN_RED) },
    uIce: { value: 0 },
    uIceEmissive: { value: new Color(ICE_EMISSIVE) },
  };
}
