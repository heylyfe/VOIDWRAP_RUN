"use client";

import type { JSX } from "react";
import { AdditiveBlending, CanvasTexture, SRGBColorSpace } from "three";

/** Pixel size of the shared radial-gradient light texture. */
const TEXTURE_SIZE = 128;
/** Flat disc tessellation. */
const DISC_SEGMENTS = 24;
/** How far the disc hovers above the surface it sits on. */
const LIFT = 0.03;

/**
 * Shared radial-gradient CanvasTexture (white centre fading smoothly to
 * transparent at the edge). Built once at module level; returns null when
 * there is no DOM (tests, SSR), in which case pools render a plain
 * transparent disc instead.
 */
function createRadialPoolTexture(): CanvasTexture | null {
  if (typeof document === "undefined") {
    return null;
  }
  const canvas = document.createElement("canvas");
  canvas.width = TEXTURE_SIZE;
  canvas.height = TEXTURE_SIZE;
  const ctx = canvas.getContext("2d");
  if (!ctx) {
    return null;
  }
  const half = TEXTURE_SIZE / 2;
  const gradient = ctx.createRadialGradient(half, half, 0, half, half, half);
  gradient.addColorStop(0, "rgba(255, 255, 255, 1)");
  gradient.addColorStop(0.4, "rgba(255, 255, 255, 0.45)");
  gradient.addColorStop(0.75, "rgba(255, 255, 255, 0.15)");
  gradient.addColorStop(1, "rgba(255, 255, 255, 0)");
  ctx.fillStyle = gradient;
  ctx.fillRect(0, 0, TEXTURE_SIZE, TEXTURE_SIZE);
  const texture = new CanvasTexture(canvas);
  texture.colorSpace = SRGBColorSpace;
  texture.needsUpdate = true;
  return texture;
}

const POOL_TEXTURE: CanvasTexture | null = createRadialPoolTexture();

/**
 * A fake coloured light spill on the ground: a flat additive disc. No real
 * point light is created (performance); the radial texture fakes the falloff.
 * Place this component at the surface height; it lifts itself LIFT above it.
 */
export function LightPool(props: {
  color: string;
  radius: number;
  intensity: number;
}): JSX.Element {
  return (
    <mesh position={[0, LIFT, 0]} rotation={[-Math.PI / 2, 0, 0]}>
      <circleGeometry args={[props.radius, DISC_SEGMENTS]} />
      <meshBasicMaterial
        map={POOL_TEXTURE ?? undefined}
        color={props.color}
        transparent
        opacity={props.intensity}
        blending={AdditiveBlending}
        depthWrite={false}
        toneMapped={false}
      />
    </mesh>
  );
}
