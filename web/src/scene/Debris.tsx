"use client";

import { useEffect, useMemo, useRef } from "react";
import type { JSX } from "react";
import { useFrame } from "@react-three/fiber";
import {
  BufferAttribute,
  Color,
  IcosahedronGeometry,
  MeshStandardMaterial,
  Object3D,
} from "three";
import type { BufferGeometry, InstancedMesh } from "three";
import type { Theme } from "./theme";
import { hashString, mulberry32 } from "./rng";

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

/** Seed of the single shared rock silhouette; every Debris instance reuses it. */
const ROCK_JITTER_SEED = 0x5eed;
const ROCK_MIN_FACTOR = 0.7;
const ROCK_MAX_FACTOR = 1.15;
/** Positions are quantised before hashing so duplicated vertices agree. */
const ROCK_QUANTISATION = 1000;

/** Per-instance axis stretch so no two rocks share the same silhouette. */
const MIN_AXIS_SCALE = 0.75;
const MAX_AXIS_SCALE = 1.25;

const RIM_STRENGTH = 0.6;

const RIM_FRAGMENT_PARS = `
uniform vec3 uRimColor;
uniform float uRimStrength;
`;

const RIM_FRAGMENT_APPLY = `
  {
    vec3 rimViewDirection = normalize( vViewPosition );
    float rimFresnel = pow( 1.0 - clamp( dot( normal, rimViewDirection ), 0.0, 1.0 ), 3.0 );
    outgoingLight += uRimColor * rimFresnel * uRimStrength;
  }
`;

function clamp(value: number, min: number, max: number): number {
  return Math.min(Math.max(value, min), max);
}

/** Deterministic float in [0, 1) derived from a string key. */
function unitHash(key: string): number {
  return hashString(key) / 4294967296;
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

/**
 * One shared jagged rock: an icosahedron whose vertices are pushed in and out
 * by a per-position hash. `IcosahedronGeometry` is non-indexed, so the same
 * corner appears once per adjacent face; hashing the quantised position (not
 * the vertex index) gives every copy the same factor, which keeps the faces
 * welded and the silhouette crack-free.
 */
function buildRockGeometry(): BufferGeometry {
  const geometry = new IcosahedronGeometry(1, 0);
  const attribute = geometry.getAttribute("position");
  if (!(attribute instanceof BufferAttribute)) {
    throw new Error("IcosahedronGeometry is missing a position attribute");
  }

  const factors = new Map<string, number>();
  for (let i = 0; i < attribute.count; i += 1) {
    const x = attribute.getX(i);
    const y = attribute.getY(i);
    const z = attribute.getZ(i);
    const key = [
      Math.round(x * ROCK_QUANTISATION),
      Math.round(y * ROCK_QUANTISATION),
      Math.round(z * ROCK_QUANTISATION),
    ].join(":");

    let factor = factors.get(key);
    if (factor === undefined) {
      factor =
        ROCK_MIN_FACTOR +
        unitHash(`rock:${ROCK_JITTER_SEED}:${key}`) * (ROCK_MAX_FACTOR - ROCK_MIN_FACTOR);
      factors.set(key, factor);
    }

    attribute.setXYZ(i, x * factor, y * factor, z * factor);
  }

  attribute.needsUpdate = true;
  geometry.computeVertexNormals();
  return geometry;
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

  const geometry = useMemo(() => buildRockGeometry(), []);

  // Non-uniform per-instance stretch, derived from the item index only, so the
  // rocks differ without any per-frame randomness or allocation.
  const axisScales = useMemo(() => {
    const values = new Float32Array(layout.length * 3);
    for (let i = 0; i < layout.length; i += 1) {
      values[i * 3] = MIN_AXIS_SCALE + unitHash(`rock-x:${i}`) * (MAX_AXIS_SCALE - MIN_AXIS_SCALE);
      values[i * 3 + 1] =
        MIN_AXIS_SCALE + unitHash(`rock-y:${i}`) * (MAX_AXIS_SCALE - MIN_AXIS_SCALE);
      values[i * 3 + 2] =
        MIN_AXIS_SCALE + unitHash(`rock-z:${i}`) * (MAX_AXIS_SCALE - MIN_AXIS_SCALE);
    }
    return values;
  }, [layout]);

  const material = useMemo(() => {
    const rimColor = new Color(theme.accent);
    const rockMaterial = new MeshStandardMaterial({
      color: new Color(theme.debris),
      roughness: 0.9,
      flatShading: true,
    });

    // Fresnel rim light: bright accent on the silhouette edges, nothing on the
    // faces pointing at the camera. Injected into the standard material so the
    // rocks keep their normal lighting and fog.
    rockMaterial.onBeforeCompile = (shader) => {
      shader.uniforms.uRimColor = { value: rimColor };
      shader.uniforms.uRimStrength = { value: RIM_STRENGTH };
      shader.fragmentShader = shader.fragmentShader
        .replace("#include <common>", `#include <common>\n${RIM_FRAGMENT_PARS}`)
        .replace("#include <opaque_fragment>", `${RIM_FRAGMENT_APPLY}\n#include <opaque_fragment>`);
    };

    return rockMaterial;
  }, [theme.debris, theme.accent]);

  const dummyRef = useRef<Object3D>(new Object3D());
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
    const dummy = dummyRef.current;
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
      dummy.scale.set(
        item.scale * (axisScales[i * 3] ?? 1),
        item.scale * (axisScales[i * 3 + 1] ?? 1),
        item.scale * (axisScales[i * 3 + 2] ?? 1),
      );
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
