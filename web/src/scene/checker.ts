import {
  BoxGeometry,
  CanvasTexture,
  RepeatWrapping,
  SRGBColorSpace,
} from "three";
import type { Theme } from "./theme";

/** Pixel size of the generated checker canvas (2x2 checks). */
const CANVAS_SIZE = 512;
/** One check = one world unit, so half the canvas on a 512px tile. */
const CHECK_SIZE = CANVAS_SIZE / 2;
/** Per-pixel brightness noise amplitude (+/-6%). */
const NOISE_AMPLITUDE = 0.06;
/** 2px darker groove along every check border. */
const GROOVE_WIDTH = 2;
const GROOVE_ALPHA = 0.25;
/** Faint marble veins (~10% opacity). */
const VEIN_ALPHA = 0.1;
const VEIN_MIN = 3;
const VEIN_MAX = 6;

// Texture v2 grime: per-check tint jitter.
/** Per-check brightness offset range (deeper grit on the low end). */
const JITTER_MIN = -0.1;
const JITTER_MAX = 0.06;
/** Max fraction of a check's colour lerped toward the other tile colour. */
const DRIFT_MAX = 0.1;

// Texture v2 grime: scuff rings.
const SCUFF_MIN = 2;
const SCUFF_MAX = 4;
const SCUFF_ALPHA_MIN = 0.12;
const SCUFF_ALPHA_RANGE = 0.06; // tops out at 18% opacity
const SCUFF_LINE_WIDTH = 1.5;

// Texture v2 grime: hairline cracks.
const CRACK_MIN = 1;
const CRACK_MAX = 3;
const CRACK_ALPHA = 0.2;
const CRACK_LINE_WIDTH = 1;
/** How far before the check border a crack starts. */
const CRACK_START_RANGE = 24;
/** How far past the check border a crack continues. */
const CRACK_CROSS_MIN = 16;
const CRACK_CROSS_RANGE = 32;
/** Vertical wobble of a crack while crossing the border. */
const CRACK_WOBBLE = 12;

// Texture v2 grime: worn edges.
/** Width of the darkened worn band along each check edge. */
const WORN_EDGE_PIXELS = 6;
/** Darkness at the very edge, fading to 0 at WORN_EDGE_PIXELS inward. */
const WORN_EDGE_ALPHA = 0.1;

const TOP_SEED_SUFFIX = ":top";
const SIDE_SEED_SUFFIX = ":side";

export interface ThemeTexturePair {
  top: CanvasTexture | null;
  side: CanvasTexture | null;
}

/**
 * A BoxGeometry whose per-face UVs are rescaled so ONE uv unit equals ONE
 * world unit (one check per world unit with the 2-check-per-tile texture),
 * and centred on the box so checks line up with the world grid for boxes
 * whose edges sit on integer coordinates.
 */
export function worldScaledBoxGeometry(
  width: number,
  height: number,
  depth: number,
): BoxGeometry {
  const geometry = new BoxGeometry(width, height, depth);
  const uv = geometry.attributes.uv;
  if (!uv) {
    throw new Error("BoxGeometry is missing its uv attribute");
  }
  for (let face = 0; face < 6; face++) {
    const [extentU, extentV] = faceExtents(face, width, height, depth);
    for (let corner = 0; corner < 4; corner++) {
      const index = face * 4 + corner;
      const u = uv.getX(index);
      const v = uv.getY(index);
      // Old uv is in [0, 1] across the face extent; remap to world units
      // centred on the box origin: [-extent / 2, +extent / 2].
      uv.setXY(index, u * extentU - extentU / 2, v * extentV - extentV / 2);
    }
  }
  uv.needsUpdate = true;
  return geometry;
}

/** u/v world extents per BoxGeometry face (order +x, -x, +y, -y, +z, -z). */
function faceExtents(
  face: number,
  width: number,
  height: number,
  depth: number,
): [number, number] {
  switch (face) {
    case 0: // +x: u spans depth, v spans height
    case 1: // -x: u spans depth, v spans height
      return [depth, height];
    case 2: // +y: u spans width, v spans depth
    case 3: // -y: u spans width, v spans depth
      return [width, depth];
    default: // +z / -z: u spans width, v spans height
      return [width, height];
  }
}

/**
 * Grungy 2x2 checker canvas texture (each check = one world unit; pair with
 * `texture.repeat = (0.5, 0.5)` and world-scaled UVs). Deterministic for a
 * given (light, dark, seed). Returns null when there is no DOM.
 */
export function makeCheckerTexture(
  light: string,
  dark: string,
  seed: number,
): CanvasTexture | null {
  if (typeof document === "undefined") {
    return null;
  }
  const canvas = document.createElement("canvas");
  canvas.width = CANVAS_SIZE;
  canvas.height = CANVAS_SIZE;
  const ctx = canvas.getContext("2d");
  if (!ctx) {
    return null;
  }
  const rand = mulberry32(seed);

  drawChecks(ctx, light, dark);
  applyCheckJitter(ctx, rand, light, dark);
  drawScuffRings(ctx, rand);
  drawCracks(ctx, rand);
  drawWornEdges(ctx);
  drawGrooves(ctx);
  drawVeins(ctx, rand);
  applyNoise(ctx, rand);

  const texture = new CanvasTexture(canvas);
  texture.wrapS = RepeatWrapping;
  texture.wrapT = RepeatWrapping;
  texture.colorSpace = SRGBColorSpace;
  texture.anisotropy = 8;
  texture.generateMipmaps = true;
  texture.repeat.set(0.5, 0.5);
  texture.needsUpdate = true;
  return texture;
}

const textureCache = new Map<string, ThemeTexturePair>();

/** Top/side checker textures for a theme, memoised per theme.id. */
export function themeTextures(theme: Theme): ThemeTexturePair {
  const cached = textureCache.get(theme.id);
  if (cached) {
    return cached;
  }
  const pair: ThemeTexturePair = {
    top: makeCheckerTexture(
      theme.tile.light,
      theme.tile.dark,
      stringSeed(theme.id + TOP_SEED_SUFFIX),
    ),
    side: makeCheckerTexture(
      theme.tile.side,
      theme.tile.dark,
      stringSeed(theme.id + SIDE_SEED_SUFFIX),
    ),
  };
  textureCache.set(theme.id, pair);
  return pair;
}

function drawChecks(
  ctx: CanvasRenderingContext2D,
  light: string,
  dark: string,
): void {
  ctx.fillStyle = light;
  ctx.fillRect(0, 0, CANVAS_SIZE, CANVAS_SIZE);
  ctx.fillStyle = dark;
  ctx.fillRect(CHECK_SIZE, 0, CHECK_SIZE, CHECK_SIZE);
  ctx.fillRect(0, CHECK_SIZE, CHECK_SIZE, CHECK_SIZE);
}

/**
 * Per-check tint jitter: each of the 4 checks gets its own deterministic
 * brightness offset in [JITTER_MIN, JITTER_MAX] plus a slight colour drift
 * toward the other tile colour, for mottled, uneven stone.
 */
function applyCheckJitter(
  ctx: CanvasRenderingContext2D,
  rand: () => number,
  light: string,
  dark: string,
): void {
  const lightRGB = parseRGB(light);
  const darkRGB = parseRGB(dark);
  const image = ctx.getImageData(0, 0, CANVAS_SIZE, CANVAS_SIZE);
  const data = image.data;
  // Check order: 0 = top-left (light), 1 = top-right (dark),
  // 2 = bottom-left (dark), 3 = bottom-right (light).
  for (let check = 0; check < 4; check++) {
    const brightness = 1 + (JITTER_MIN + rand() * (JITTER_MAX - JITTER_MIN));
    const drift = rand() * DRIFT_MAX;
    const base: [number, number, number] | null =
      check === 0 || check === 3 ? lightRGB : darkRGB;
    const other: [number, number, number] | null =
      check === 0 || check === 3 ? darkRGB : lightRGB;
    if (base === null) {
      continue;
    }
    const target = other === null ? base : other;
    const x0 = (check % 2) * CHECK_SIZE;
    const y0 = Math.floor(check / 2) * CHECK_SIZE;
    for (let y = y0; y < y0 + CHECK_SIZE; y++) {
      for (let x = x0; x < x0 + CHECK_SIZE; x++) {
        const i = (y * CANVAS_SIZE + x) * 4;
        const r = data[i];
        const g = data[i + 1];
        const b = data[i + 2];
        if (r === undefined || g === undefined || b === undefined) {
          continue; // Unreachable for a full-canvas ImageData.
        }
        data[i] = clampByte(
          r * brightness * (1 - drift) + target[0] * drift,
        );
        data[i + 1] = clampByte(
          g * brightness * (1 - drift) + target[1] * drift,
        );
        data[i + 2] = clampByte(
          b * brightness * (1 - drift) + target[2] * drift,
        );
      }
    }
  }
  ctx.putImageData(image, 0, 0);
}

/**
 * 2-4 thin partial "scuff ring" ellipse strokes, darker than the tile, for
 * the scribbled-ring wear visible on the reference floor.
 */
function drawScuffRings(ctx: CanvasRenderingContext2D, rand: () => number): void {
  const count = SCUFF_MIN + Math.floor(rand() * (SCUFF_MAX - SCUFF_MIN + 1));
  for (let i = 0; i < count; i++) {
    const alpha = SCUFF_ALPHA_MIN + rand() * SCUFF_ALPHA_RANGE;
    const cx = rand() * CANVAS_SIZE;
    const cy = rand() * CANVAS_SIZE;
    const radiusX = CHECK_SIZE * (0.12 + rand() * 0.33);
    const radiusY = radiusX * (0.35 + rand() * 0.65);
    const rotation = rand() * Math.PI * 2;
    const start = rand() * Math.PI * 2;
    const span = Math.PI * (0.4 + rand() * 1.1);
    ctx.lineWidth = SCUFF_LINE_WIDTH;
    ctx.strokeStyle = `rgba(0, 0, 0, ${alpha.toFixed(3)})`;
    ctx.beginPath();
    ctx.ellipse(cx, cy, radiusX, radiusY, rotation, start, start + span);
    ctx.stroke();
  }
}

/**
 * 1-3 jagged hairline cracks that start inside one check and wander across
 * a check border, so every crack visibly spans two checks.
 */
function drawCracks(ctx: CanvasRenderingContext2D, rand: () => number): void {
  const count = CRACK_MIN + Math.floor(rand() * (CRACK_MAX - CRACK_MIN + 1));
  ctx.lineWidth = CRACK_LINE_WIDTH;
  ctx.strokeStyle = `rgba(0, 0, 0, ${CRACK_ALPHA})`;
  for (let i = 0; i < count; i++) {
    const vertical = rand() < 0.5;
    const alongStart = rand() * CANVAS_SIZE;
    const acrossStart = rand() * CRACK_START_RANGE;
    const crossPast = CRACK_CROSS_MIN + rand() * CRACK_CROSS_RANGE;
    const steps = 6 + Math.floor(rand() * 5);
    ctx.beginPath();
    for (let step = 0; step <= steps; step++) {
      const t = step / steps;
      // Offset relative to the check border at CHECK_SIZE: starts
      // `acrossStart` inside the first check, ends `crossPast` beyond it.
      const offset = -acrossStart + t * (acrossStart + crossPast);
      const along = alongStart + (rand() * 2 - 1) * CRACK_WOBBLE;
      const x = vertical ? CHECK_SIZE + offset : along;
      const y = vertical ? along : CHECK_SIZE + offset;
      if (step === 0) {
        ctx.moveTo(x, y);
      } else {
        ctx.lineTo(x, y);
      }
    }
    ctx.stroke();
  }
}

/**
 * Darken the outer WORN_EDGE_PIXELS of every check with a gradient fading
 * from WORN_EDGE_ALPHA at the edge to transparent, so checks read as worn
 * stone blocks instead of flat prints.
 */
function drawWornEdges(ctx: CanvasRenderingContext2D): void {
  for (let check = 0; check < 4; check++) {
    const x0 = (check % 2) * CHECK_SIZE;
    const y0 = Math.floor(check / 2) * CHECK_SIZE;
    const top = y0;
    const bottom = y0 + CHECK_SIZE;
    const left = x0;
    const right = x0 + CHECK_SIZE;
    const w = WORN_EDGE_PIXELS;
    ctx.fillStyle = edgeGradient(ctx, left, top, left, top + w);
    ctx.fillRect(left, top, CHECK_SIZE, w);
    ctx.fillStyle = edgeGradient(ctx, left, bottom, left, bottom - w);
    ctx.fillRect(left, bottom - w, CHECK_SIZE, w);
    ctx.fillStyle = edgeGradient(ctx, left, top, left + w, top);
    ctx.fillRect(left, top, w, CHECK_SIZE);
    ctx.fillStyle = edgeGradient(ctx, right, top, right - w, top);
    ctx.fillRect(right - w, top, w, CHECK_SIZE);
  }
}

/** Gradient from a worn dark edge fading to fully transparent inward. */
function edgeGradient(
  ctx: CanvasRenderingContext2D,
  x1: number,
  y1: number,
  x2: number,
  y2: number,
): CanvasGradient {
  const gradient = ctx.createLinearGradient(x1, y1, x2, y2);
  gradient.addColorStop(0, `rgba(0, 0, 0, ${WORN_EDGE_ALPHA})`);
  gradient.addColorStop(1, "rgba(0, 0, 0, 0)");
  return gradient;
}

/** 2px darker groove along every check border, wrap-safe. */
function drawGrooves(ctx: CanvasRenderingContext2D): void {
  ctx.fillStyle = `rgba(0, 0, 0, ${GROOVE_ALPHA})`;
  ctx.fillRect(0, 0, CANVAS_SIZE, GROOVE_WIDTH);
  ctx.fillRect(0, CHECK_SIZE - GROOVE_WIDTH / 2, CANVAS_SIZE, GROOVE_WIDTH);
  ctx.fillRect(0, 0, GROOVE_WIDTH, CANVAS_SIZE);
  ctx.fillRect(CHECK_SIZE - GROOVE_WIDTH / 2, 0, GROOVE_WIDTH, CANVAS_SIZE);
}

/** 3-6 faint thin bezier "marble" veins, wrapped so they tile seamlessly. */
function drawVeins(ctx: CanvasRenderingContext2D, rand: () => number): void {
  ctx.strokeStyle = `rgba(0, 0, 0, ${VEIN_ALPHA})`;
  const count = VEIN_MIN + Math.floor(rand() * (VEIN_MAX - VEIN_MIN + 1));
  for (let i = 0; i < count; i++) {
    ctx.lineWidth = 1 + rand() * 1.5;
    const horizontal = rand() < 0.5;
    const p0 = rand() * CANVAS_SIZE;
    const p1 = rand() * CANVAS_SIZE;
    const c1 = rand() * CANVAS_SIZE;
    const c2 = rand() * CANVAS_SIZE;
    const draw = (offset: number): void => {
      ctx.beginPath();
      if (horizontal) {
        ctx.moveTo(offset, p0);
        ctx.bezierCurveTo(
          offset + CANVAS_SIZE / 3,
          c1,
          offset + (2 * CANVAS_SIZE) / 3,
          c2,
          offset + CANVAS_SIZE,
          p1,
        );
      } else {
        ctx.moveTo(p0, offset);
        ctx.bezierCurveTo(
          c1,
          offset + CANVAS_SIZE / 3,
          c2,
          offset + (2 * CANVAS_SIZE) / 3,
          p1,
          offset + CANVAS_SIZE,
        );
      }
      ctx.stroke();
    };
    draw(-CANVAS_SIZE);
    draw(0);
    draw(CANVAS_SIZE);
  }
}

/** Deterministic per-pixel brightness noise of +/-6% over the whole tile. */
function applyNoise(ctx: CanvasRenderingContext2D, rand: () => number): void {
  const image = ctx.getImageData(0, 0, CANVAS_SIZE, CANVAS_SIZE);
  const data = image.data;
  for (let i = 0; i < data.length; i += 4) {
    const r = data[i];
    const g = data[i + 1];
    const b = data[i + 2];
    if (r === undefined || g === undefined || b === undefined) {
      continue; // Unreachable for a full-canvas ImageData.
    }
    const factor = 1 + (rand() * 2 - 1) * NOISE_AMPLITUDE;
    data[i] = scaleChannel(r, factor);
    data[i + 1] = scaleChannel(g, factor);
    data[i + 2] = scaleChannel(b, factor);
  }
  ctx.putImageData(image, 0, 0);
}

function scaleChannel(value: number, factor: number): number {
  return clampByte(value * factor);
}

function clampByte(value: number): number {
  return Math.max(0, Math.min(255, Math.round(value)));
}

/**
 * Parse a "#rgb" or "#rrggbb" hex colour into [r, g, b]. Returns null for
 * anything else (in which case jitter callers skip the colour drift).
 */
function parseRGB(color: string): [number, number, number] | null {
  const value = color.trim();
  const digit = (text: string): number => {
    const parsed = parseInt(text, 16);
    return Number.isNaN(parsed) ? 0 : parsed;
  };
  if (value.length === 7 && value.charAt(0) === "#") {
    return [
      digit(value.slice(1, 3)),
      digit(value.slice(3, 5)),
      digit(value.slice(5, 7)),
    ];
  }
  if (value.length === 4 && value.charAt(0) === "#") {
    return [
      digit(value.slice(1, 2) + value.slice(1, 2)),
      digit(value.slice(2, 3) + value.slice(2, 3)),
      digit(value.slice(3, 4) + value.slice(3, 4)),
    ];
  }
  return null;
}

/** Small deterministic PRNG (mulberry32). */
function mulberry32(seed: number): () => number {
  let state = seed >>> 0;
  return () => {
    state = (state + 0x6d2b79f5) >>> 0;
    let t = state;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

/** FNV-1a style stable string hash -> uint32, for per-theme texture seeds. */
function stringSeed(text: string): number {
  let hash = 2166136261;
  for (let i = 0; i < text.length; i++) {
    hash ^= text.charCodeAt(i);
    hash = Math.imul(hash, 16777619);
  }
  return hash >>> 0;
}
