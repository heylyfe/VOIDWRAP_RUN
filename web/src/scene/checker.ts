import {
  BoxGeometry,
  CanvasTexture,
  RepeatWrapping,
  SRGBColorSpace,
} from "three";
import type { Theme } from "./theme";

/** Pixel size of the generated checker canvas (2x2 checks). */
const CANVAS_SIZE = 256;
/** One check = one world unit, so half the canvas on a 256px tile. */
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
  return Math.max(0, Math.min(255, Math.round(value * factor)));
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
