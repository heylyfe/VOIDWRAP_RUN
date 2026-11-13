"use client";

import { useEffect, useLayoutEffect, useMemo, useRef } from "react";
import type { JSX } from "react";
import { Color, MeshStandardMaterial, Object3D, OctahedronGeometry } from "three";
import type { InstancedMesh } from "three";
import type { Platform } from "@/protocol/generated";
import type { Theme } from "./theme";
import { mulberry32 } from "./rng";

export interface CrystalSpot {
  position: [number, number, number];
  scale: number;
  tilt: [number, number];
  yaw: number;
}

const MIN_CLUSTER_SIZE = 3;
const MAX_CLUSTER_SIZE = 7;
const MIN_CRYSTAL_SCALE = 0.25;
const MAX_CRYSTAL_SCALE = 1.1;
const MAX_TILT = 0.5;

/** Hanging clusters: anchored below the underside, staggered further down. */
const HANGING_DROP = 0.18;
const HANGING_STAGGER = 0.24;
const HANGING_SPREAD = 0.3;
const HANGING_INSET = 0.45;

/** Standing clusters: pushed just past a corner, never back over the top. */
const STANDING_OFFSET = 0.4;
const STANDING_SPREAD = 0.18;
const STANDING_VERTICAL_SPREAD = 0.2;
const STANDING_LIFT = 0.12;
const STANDING_HEADROOM = 0.12;

const PLACEMENT_ATTEMPTS = 8;
const DEFAULT_PER_PLATFORM = 2;

const CRYSTAL_WIDTH = 0.35;
const CRYSTAL_HEIGHT = 1.4;
const EMISSIVE_INTENSITY = 1.6;
const CRYSTAL_ROUGHNESS = 0.3;
/** Slack used when deciding whether a spot sits under a platform. */
const HANGING_FOOTPRINT_MARGIN = 0.3;

function insideFootprint(x: number, z: number, platform: Platform): boolean {
  const { min, max } = platform.bounds;
  return x >= min.x && x <= max.x && z >= min.z && z <= max.z;
}

/**
 * A position is blocked when it sits inside a platform footprint at or above
 * that platform's underside - that is, on the walkable top or inside the block.
 */
function isBlocked(
  position: readonly [number, number, number],
  platforms: readonly Platform[],
): boolean {
  for (const platform of platforms) {
    if (position[1] < platform.bounds.min.y) continue;
    if (insideFootprint(position[0], position[2], platform)) return true;
  }
  return false;
}

function hangingAnchor(rand: () => number, platform: Platform): [number, number, number] {
  const { min, max } = platform.bounds;
  const width = Math.max(max.x - min.x, 0);
  const depth = Math.max(max.z - min.z, 0);
  const insetX = Math.min(HANGING_INSET, width * 0.25);
  const insetZ = Math.min(HANGING_INSET, depth * 0.25);
  return [
    min.x + insetX + rand() * Math.max(width - insetX * 2, 0),
    min.y - HANGING_DROP,
    min.z + insetZ + rand() * Math.max(depth - insetZ * 2, 0),
  ];
}

function standingAnchor(rand: () => number, platform: Platform): [number, number, number] {
  const { min, max } = platform.bounds;
  const x = rand() < 0.5 ? min.x - STANDING_OFFSET : max.x + STANDING_OFFSET;
  const z = rand() < 0.5 ? min.z - STANDING_OFFSET : max.z + STANDING_OFFSET;
  const low = min.y + STANDING_LIFT;
  const high = max.y - STANDING_HEADROOM;
  const y = high > low ? low + rand() * (high - low) : low;
  return [x, y, z];
}

function pickAnchor(
  rand: () => number,
  platform: Platform,
  hanging: boolean,
  platforms: readonly Platform[],
): [number, number, number] | null {
  for (let attempt = 0; attempt < PLACEMENT_ATTEMPTS; attempt += 1) {
    const candidate = hanging ? hangingAnchor(rand, platform) : standingAnchor(rand, platform);
    if (!isBlocked(candidate, platforms)) return candidate;
  }
  return null;
}

function placeSpot(
  rand: () => number,
  anchor: readonly [number, number, number],
  spread: number,
  verticalSpread: number,
  downward: boolean,
  platforms: readonly Platform[],
): [number, number, number] {
  for (let attempt = 0; attempt < PLACEMENT_ATTEMPTS; attempt += 1) {
    const jitter = rand() * verticalSpread;
    const candidate: [number, number, number] = [
      anchor[0] + (rand() - 0.5) * spread * 2,
      downward ? anchor[1] - jitter : anchor[1] + jitter - verticalSpread * 0.5,
      anchor[2] + (rand() - 0.5) * spread * 2,
    ];
    if (!isBlocked(candidate, platforms)) return candidate;
  }
  // The anchor itself is always valid, so this keeps the cluster size intact.
  return [anchor[0], anchor[1], anchor[2]];
}

/**
 * Deterministic crystal clusters around `platforms`. Every cluster either hangs
 * under a platform (strictly below `bounds.min.y`) or stands just outside a
 * corner of it, between its bottom and its top. Nothing is ever placed on or
 * above a walkable top.
 */
export function crystalLayout(
  seed: number,
  platforms: readonly Platform[],
  perPlatform: number,
): CrystalSpot[] {
  const spots: CrystalSpot[] = [];
  const maxClusters = Math.max(0, Math.floor(perPlatform));
  if (maxClusters === 0 || platforms.length === 0) return spots;

  const rand = mulberry32(seed);

  for (const platform of platforms) {
    const clusterCount = Math.floor(rand() * (maxClusters + 1));
    for (let cluster = 0; cluster < clusterCount; cluster += 1) {
      const hanging = rand() < 0.5;
      const size =
        MIN_CLUSTER_SIZE + Math.floor(rand() * (MAX_CLUSTER_SIZE - MIN_CLUSTER_SIZE + 1));
      const anchor = pickAnchor(rand, platform, hanging, platforms);
      if (!anchor) continue;

      for (let i = 0; i < size; i += 1) {
        const position =
          i === 0
            ? anchor
            : placeSpot(
                rand,
                anchor,
                hanging ? HANGING_SPREAD : STANDING_SPREAD,
                hanging ? HANGING_STAGGER : STANDING_VERTICAL_SPREAD,
                hanging,
                platforms,
              );

        spots.push({
          position,
          scale: MIN_CRYSTAL_SCALE + rand() * (MAX_CRYSTAL_SCALE - MIN_CRYSTAL_SCALE),
          tilt: [(rand() - 0.5) * 2 * MAX_TILT, (rand() - 0.5) * 2 * MAX_TILT],
          yaw: rand() * Math.PI * 2,
        });
      }
    }
  }

  return spots;
}

/**
 * A spot hangs when it sits below a platform's underside and under (or just
 * past) its footprint. Hanging crystals are flipped so their tip points down.
 */
function isHangingSpot(spot: CrystalSpot, platforms: readonly Platform[]): boolean {
  for (const platform of platforms) {
    const { min, max } = platform.bounds;
    if (spot.position[1] >= min.y) continue;
    if (spot.position[0] < min.x - HANGING_FOOTPRINT_MARGIN) continue;
    if (spot.position[0] > max.x + HANGING_FOOTPRINT_MARGIN) continue;
    if (spot.position[2] < min.z - HANGING_FOOTPRINT_MARGIN) continue;
    if (spot.position[2] > max.z + HANGING_FOOTPRINT_MARGIN) continue;
    return true;
  }
  return false;
}

export function Crystals(props: {
  theme: Theme;
  platforms: readonly Platform[];
  seed: number;
}): JSX.Element {
  const { theme, platforms, seed } = props;

  const layout = useMemo(
    () => crystalLayout(seed, platforms, DEFAULT_PER_PLATFORM),
    [seed, platforms],
  );

  const geometry = useMemo(() => {
    const crystal = new OctahedronGeometry(1, 0);
    crystal.scale(CRYSTAL_WIDTH, CRYSTAL_HEIGHT, CRYSTAL_WIDTH);
    return crystal;
  }, []);

  const material = useMemo(
    () =>
      new MeshStandardMaterial({
        color: new Color(theme.tile.edge),
        emissive: new Color(theme.accent),
        emissiveIntensity: EMISSIVE_INTENSITY,
        roughness: CRYSTAL_ROUGHNESS,
        flatShading: true,
        toneMapped: false,
      }),
    [theme.tile.edge, theme.accent],
  );

  const meshRef = useRef<InstancedMesh>(null);

  // Static instances: written once per layout, never touched per frame.
  useLayoutEffect(() => {
    const mesh = meshRef.current;
    if (!mesh) return;

    const dummy = new Object3D();
    for (let i = 0; i < layout.length; i += 1) {
      const spot = layout[i];
      if (!spot) continue;

      const hanging = isHangingSpot(spot, platforms);
      dummy.position.set(spot.position[0], spot.position[1], spot.position[2]);
      dummy.rotation.set(
        hanging ? Math.PI + spot.tilt[0] : spot.tilt[0],
        spot.yaw,
        spot.tilt[1],
      );
      dummy.scale.setScalar(spot.scale);
      dummy.updateMatrix();
      mesh.setMatrixAt(i, dummy.matrix);
    }

    mesh.count = layout.length;
    mesh.instanceMatrix.needsUpdate = true;
    mesh.computeBoundingSphere();
  }, [layout, platforms]);

  useEffect(
    () => () => {
      geometry.dispose();
    },
    [geometry],
  );

  useEffect(
    () => () => {
      material.dispose();
    },
    [material],
  );

  return (
    <instancedMesh
      ref={meshRef}
      args={[undefined, undefined, layout.length]}
      geometry={geometry}
      material={material}
      frustumCulled={false}
    />
  );
}
