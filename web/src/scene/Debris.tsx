"use client";

import { useEffect, useMemo, useRef } from "react";
import type { JSX } from "react";
import { useFrame } from "@react-three/fiber";
import { BoxGeometry, Color, MeshStandardMaterial, Object3D } from "three";
import type { InstancedMesh } from "three";
import type { Theme } from "./theme";
import { mulberry32 } from "./rng";

export interface DebrisItem {
  position: [number, number, number];
  scale: number;
  spin: [number, number, number];
  orbitSpeed: number;
  bobPhase: number;
}

const MIN_SCALE = 0.15;
const MAX_SCALE = 0.9;
const MIN_SPIN = -1.2;
const MAX_SPIN = 1.2;
const MIN_ORBIT = -0.03;
const MAX_ORBIT = 0.03;
const BOB_AMPLITUDE = 0.6;
const BOB_SPEED = 0.8;
const CLUSTER_FRACTION = 0.3;
const CLUSTER_SPREAD = 2.0;

function clamp(value: number, min: number, max: number): number {
  return Math.min(Math.max(value, min), max);
}

/**
 * Deterministic debris layout: a thick shell of cubes around `center`, with
 * roughly 30% of them grouped into small swarms.
 */
export function debrisLayout(
  seed: number,
  count: number,
  center: { x: number; y: number; z: number },
  radius: number,
): DebrisItem[] {
  const rand = mulberry32(seed);
  const total = Math.max(0, Math.floor(count));
  const items: DebrisItem[] = [];
  if (total === 0) return items;

  const innerRadius = radius * 0.55;
  const outerRadius = radius * 1.15;
  const minY = center.y - 12;
  const maxY = center.y + 18;

  const makeItem = (x: number, y: number, z: number): DebrisItem => {
    const scaleBias = rand();
    return {
      position: [x, y, z],
      scale: MIN_SCALE + scaleBias * scaleBias * (MAX_SCALE - MIN_SCALE),
      spin: [
        MIN_SPIN + rand() * (MAX_SPIN - MIN_SPIN),
        MIN_SPIN + rand() * (MAX_SPIN - MIN_SPIN),
        MIN_SPIN + rand() * (MAX_SPIN - MIN_SPIN),
      ],
      orbitSpeed: MIN_ORBIT + rand() * (MAX_ORBIT - MIN_ORBIT),
      bobPhase: rand() * Math.PI * 2,
    };
  };

  const shellPoint = (): [number, number, number] => {
    const angle = rand() * Math.PI * 2;
    const distance = innerRadius + rand() * (outerRadius - innerRadius);
    const y = minY + rand() * (maxY - minY);
    return [center.x + Math.cos(angle) * distance, y, center.z + Math.sin(angle) * distance];
  };

  const clusterPoint = (anchor: [number, number, number]): [number, number, number] => {
    const offsetX = anchor[0] - center.x;
    const offsetZ = anchor[2] - center.z;
    const baseAngle = Math.atan2(offsetZ, offsetX);
    const baseDistance = Math.hypot(offsetX, offsetZ);
    const tangential = (rand() - 0.5) * CLUSTER_SPREAD * 2;
    const angle = baseAngle + tangential / Math.max(baseDistance, 1);
    const distance = clamp(
      baseDistance + (rand() - 0.5) * CLUSTER_SPREAD * 2,
      innerRadius,
      outerRadius,
    );
    const y = clamp(anchor[1] + (rand() - 0.5) * CLUSTER_SPREAD * 2, minY, maxY);
    return [center.x + Math.cos(angle) * distance, y, center.z + Math.sin(angle) * distance];
  };

  const clusterTarget = Math.round(total * CLUSTER_FRACTION);
  while (items.length < clusterTarget) {
    const anchor = shellPoint();
    const clusterSize = 3 + Math.floor(rand() * 4);
    const remaining = Math.min(clusterSize, clusterTarget - items.length, total - items.length);
    for (let i = 0; i < remaining; i += 1) {
      const point = i === 0 ? anchor : clusterPoint(anchor);
      items.push(makeItem(point[0], point[1], point[2]));
    }
  }

  while (items.length < total) {
    const point = shellPoint();
    items.push(makeItem(point[0], point[1], point[2]));
  }

  return items;
}

export function Debris(props: {
  theme: Theme;
  center: { x: number; y: number; z: number };
  radius: number;
  count?: number;
  seed?: number;
}): JSX.Element {
  const { theme, center, radius, count = 140, seed = 1337 } = props;
  const { x: centerX, y: centerY, z: centerZ } = center;

  const layout = useMemo(
    () => debrisLayout(seed, count, { x: centerX, y: centerY, z: centerZ }, radius),
    [seed, count, centerX, centerY, centerZ, radius],
  );

  const geometry = useMemo(() => new BoxGeometry(1, 1, 1), []);

  const material = useMemo(
    () =>
      new MeshStandardMaterial({
        color: new Color(theme.debris),
        roughness: 0.6,
        metalness: 0.2,
        emissive: new Color(theme.accent),
        emissiveIntensity: 0.05,
      }),
    [theme.debris, theme.accent],
  );

  const dummy = useMemo(() => new Object3D(), []);
  const meshRef = useRef<InstancedMesh>(null);

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

  useFrame((state) => {
    const mesh = meshRef.current;
    if (!mesh) return;
    const time = state.clock.elapsedTime;

    for (let i = 0; i < layout.length; i += 1) {
      const item = layout[i];
      if (!item) continue;

      const angle = item.orbitSpeed * time;
      const cos = Math.cos(angle);
      const sin = Math.sin(angle);
      const offsetX = item.position[0] - centerX;
      const offsetZ = item.position[2] - centerZ;

      dummy.position.set(
        centerX + offsetX * cos - offsetZ * sin,
        item.position[1] + Math.sin(time * BOB_SPEED + item.bobPhase) * BOB_AMPLITUDE,
        centerZ + offsetX * sin + offsetZ * cos,
      );
      dummy.rotation.set(item.spin[0] * time, item.spin[1] * time, item.spin[2] * time);
      dummy.scale.setScalar(item.scale);
      dummy.updateMatrix();
      mesh.setMatrixAt(i, dummy.matrix);
    }

    mesh.instanceMatrix.needsUpdate = true;
  });

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
