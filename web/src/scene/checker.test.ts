import { BoxGeometry } from "three";
import { describe, expect, it } from "vitest";
import { makeCheckerTexture, themeTextures, worldScaledBoxGeometry } from "./checker";
import { themeForLevel } from "./theme";

interface UvRange {
  minU: number;
  maxU: number;
  spanU: number;
  spanV: number;
}

function faceUvRange(geometry: BoxGeometry, face: number): UvRange {
  const uv = geometry.attributes.uv;
  if (!uv) {
    throw new Error("expected BoxGeometry to have a uv attribute");
  }
  let minU = Number.POSITIVE_INFINITY;
  let maxU = Number.NEGATIVE_INFINITY;
  let minV = Number.POSITIVE_INFINITY;
  let maxV = Number.NEGATIVE_INFINITY;
  for (let i = face * 4; i < face * 4 + 4; i++) {
    const u = uv.getX(i);
    const v = uv.getY(i);
    minU = Math.min(minU, u);
    maxU = Math.max(maxU, u);
    minV = Math.min(minV, v);
    maxV = Math.max(maxV, v);
  }
  return { minU, maxU, spanU: maxU - minU, spanV: maxV - minV };
}

describe("worldScaledBoxGeometry", () => {
  it("maps one uv unit to one world unit on every face", () => {
    const geometry = worldScaledBoxGeometry(4, 1, 2);

    // +y face (index 2): u spans width (4), v spans depth (2).
    const top = faceUvRange(geometry, 2);
    expect(top.spanU).toBeCloseTo(4);
    expect(top.spanV).toBeCloseTo(2);

    // +x face (index 0): u spans depth (2), v spans height (1).
    const side = faceUvRange(geometry, 0);
    expect(side.spanU).toBeCloseTo(2);
    expect(side.spanV).toBeCloseTo(1);

    geometry.dispose();
  });

  it("centres each face's uvs on the box so checks align to the world grid", () => {
    const geometry = worldScaledBoxGeometry(4, 1, 2);

    // +y face: u should range from -2 to +2 (half its width extent on each side).
    const top = faceUvRange(geometry, 2);
    expect(top.minU).toBeCloseTo(-2);
    expect(top.maxU).toBeCloseTo(2);

    // +x face: u should range from -1 to +1 (half its depth extent on each side).
    const side = faceUvRange(geometry, 0);
    expect(side.minU).toBeCloseTo(-1);
    expect(side.maxU).toBeCloseTo(1);

    geometry.dispose();
  });
});

describe("makeCheckerTexture", () => {
  it("returns null in the node test environment (no DOM)", () => {
    expect(makeCheckerTexture("#ffffff", "#000000", 1)).toBeNull();
  });
});

describe("themeTextures", () => {
  it("returns the same memoised object for the same theme id", () => {
    const theme = themeForLevel(1);
    const first = themeTextures(theme);
    const second = themeTextures(theme);
    expect(second).toBe(first);
  });

  it("returns null textures without a DOM", () => {
    const pair = themeTextures(themeForLevel(1));
    expect(pair.top).toBeNull();
    expect(pair.side).toBeNull();
  });
});
