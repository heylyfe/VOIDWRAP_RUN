"use client";

import { useEffect, useMemo, useRef } from "react";
import type { JSX } from "react";
import { useFrame } from "@react-three/fiber";
import {
  AdditiveBlending,
  CanvasTexture,
  CylinderGeometry,
  DynamicDrawUsage,
  Group,
  InstancedMesh,
  Matrix4,
  MeshBasicMaterial,
  MeshStandardMaterial,
  PlaneGeometry,
  SRGBColorSpace,
} from "three";
import { TILE_GONE } from "@/protocol/generated";
import type { Platform, TileState } from "@/protocol/generated";
import { computeDecor } from "./decorPlacement";
import type { Vec3Tuple } from "./platformUtils";

const scratchMatrix = new Matrix4();

const POST_HEIGHT = 1.15;
const LANTERN_Y_OFFSET = 1.17;
const GLOW_DISC_SIZE = 2.7;
const GLOW_DISC_LIFT = 0.03;

function buildRadialGlowTexture(): CanvasTexture | null {
  if (typeof document === "undefined") return null;
  const size = 128;
  const canvas = document.createElement("canvas");
  canvas.width = size;
  canvas.height = size;
  const ctx = canvas.getContext("2d");
  if (!ctx) return null;
  const g = ctx.createRadialGradient(size / 2, size / 2, 0, size / 2, size / 2, size / 2);
  g.addColorStop(0, "rgba(255, 120, 70, 0.9)");
  g.addColorStop(0.35, "rgba(255, 70, 40, 0.35)");
  g.addColorStop(1, "rgba(255, 60, 30, 0)");
  ctx.fillStyle = g;
  ctx.fillRect(0, 0, size, size);
  const tex = new CanvasTexture(canvas);
  tex.colorSpace = SRGBColorSpace;
  return tex;
}

interface DecorEntry {
  mesh: InstancedMesh;
  /** platformId per instance. */
  owner: number[];
  /** Base matrix per instance (restored when a tile recovers). */
  base: Matrix4[];
  /** Base world positions (used when zero-scaling fallen tiles). */
  basePos: Vec3Tuple[];
}

interface DecorBuilt {
  holder: Group;
  entries: DecorEntry[];
  lanternMat: MeshStandardMaterial;
  discMat: MeshBasicMaterial;
  glowTex: CanvasTexture | null;
}

function buildDecor(platforms: readonly Platform[], seed: number): DecorBuilt {
  const placement = computeDecor(platforms, seed);
  const holder = new Group();
  const entries: DecorEntry[] = [];

  const makeEntry = (
    geometry: CylinderGeometry | PlaneGeometry,
    material: MeshStandardMaterial | MeshBasicMaterial,
    transforms: Array<{ platformId: number; pos: Vec3Tuple; scale: Vec3Tuple }>,
  ): DecorEntry => {
    const count = Math.max(transforms.length, 1);
    const mesh = new InstancedMesh(geometry, material, count);
    mesh.count = transforms.length;
    mesh.frustumCulled = false;
    mesh.instanceMatrix.setUsage(DynamicDrawUsage);
    const base: Matrix4[] = [];
    const basePos: Vec3Tuple[] = [];
    const owner: number[] = [];
    transforms.forEach((t, i) => {
      scratchMatrix.makeScale(t.scale[0], t.scale[1], t.scale[2]);
      scratchMatrix.setPosition(t.pos[0], t.pos[1], t.pos[2]);
      mesh.setMatrixAt(i, scratchMatrix);
      base.push(scratchMatrix.clone());
      basePos.push([t.pos[0], t.pos[1], t.pos[2]]);
      owner.push(t.platformId);
    });
    mesh.instanceMatrix.needsUpdate = true;
    holder.add(mesh);
    return { mesh, owner, base, basePos };
  };

  // Hanging vines: tapered strands, anchored at the underside edge of platforms.
  const vineGeo = new CylinderGeometry(0.05, 0.012, 1, 5, 1, true);
  vineGeo.translate(0, -0.5, 0);
  const vineMat = new MeshStandardMaterial({
    color: "#3a5a34",
    roughness: 0.9,
    emissive: "#0d1f0a",
    emissiveIntensity: 0.5,
  });
  entries.push(
    makeEntry(
      vineGeo,
      vineMat,
      placement.vines.map((v) => {
        const thickness = 0.7 + (v.phase / (Math.PI * 2)) * 0.7;
        return {
          platformId: v.platformId,
          pos: [v.x, v.y, v.z] as Vec3Tuple,
          scale: [thickness, v.length, thickness] as Vec3Tuple,
        };
      }),
    ),
  );

  // Lantern posts: thin dark-iron columns at platform corners.
  const postGeo = new CylinderGeometry(0.05, 0.075, POST_HEIGHT, 7);
  postGeo.translate(0, POST_HEIGHT / 2, 0);
  const postMat = new MeshStandardMaterial({
    color: "#232028",
    roughness: 0.6,
    metalness: 0.35,
  });
  entries.push(
    makeEntry(
      postGeo,
      postMat,
      placement.lanterns.map((l) => ({
        platformId: l.platformId,
        pos: [l.x, l.y, l.z] as Vec3Tuple,
        scale: [1, 1, 1] as Vec3Tuple,
      })),
    ),
  );

  // Lantern bodies: emissive red paper lanterns (NO real lights — bloom handles the glow).
  const lanternGeo = new CylinderGeometry(0.17, 0.135, 0.26, 8);
  const lanternMat = new MeshStandardMaterial({
    color: "#3d0d06",
    emissive: "#ff3524",
    emissiveIntensity: 2.3,
    roughness: 0.5,
  });
  entries.push(
    makeEntry(
      lanternGeo,
      lanternMat,
      placement.lanterns.map((l) => ({
        platformId: l.platformId,
        pos: [l.x, l.y + LANTERN_Y_OFFSET, l.z] as Vec3Tuple,
        scale: [1, 1, 1] as Vec3Tuple,
      })),
    ),
  );

  // Fake light spill on the deck under each lantern (additive glow disc, no light).
  const glowTex = buildRadialGlowTexture();
  const discGeo = new PlaneGeometry(GLOW_DISC_SIZE, GLOW_DISC_SIZE);
  discGeo.rotateX(-Math.PI / 2);
  const discMat = new MeshBasicMaterial({
    map: glowTex ?? null,
    color: "#ff5a34",
    transparent: true,
    opacity: 0.5,
    blending: AdditiveBlending,
    depthWrite: false,
    fog: false,
  });
  entries.push(
    makeEntry(
      discGeo,
      discMat,
      placement.lanterns.map((l) => ({
        platformId: l.platformId,
        pos: [l.x, l.y + GLOW_DISC_LIFT, l.z] as Vec3Tuple,
        scale: [1, 1, 1] as Vec3Tuple,
      })),
    ),
  );

  return { holder, entries, lanternMat, discMat, glowTex };
}

function disposeBuilt(built: DecorBuilt): void {
  for (const entry of built.entries) {
    entry.mesh.geometry.dispose();
    const material = entry.mesh.material;
    if (Array.isArray(material)) {
      for (const m of material) m.dispose();
    } else {
      material.dispose();
    }
  }
  built.glowTex?.dispose();
}

/** Zero-scale decor whose platform is TILE_GONE; restore the rest. */
function applyTiles(built: DecorBuilt, tiles: readonly TileState[] | undefined): void {
  if (!tiles) return;
  const gone = new Set<number>();
  for (const t of tiles) if (t.state === TILE_GONE) gone.add(t.id);
  for (const entry of built.entries) {
    for (let i = 0; i < entry.owner.length; i += 1) {
      const owner = entry.owner[i];
      const pos = entry.basePos[i];
      const base = entry.base[i];
      if (owner === undefined || pos === undefined || base === undefined) continue;
      if (gone.has(owner)) {
        scratchMatrix.makeScale(0.0001, 0.0001, 0.0001);
        scratchMatrix.setPosition(pos[0], pos[1], pos[2]);
        entry.mesh.setMatrixAt(i, scratchMatrix);
      } else {
        entry.mesh.setMatrixAt(i, base);
      }
    }
    entry.mesh.instanceMatrix.needsUpdate = true;
  }
}

/**
 * Decoration (no collision). Hanging vines under walkway edges and red glowing
 * lantern lights on posts at some platform corners. Instanced meshes,
 * deterministic from the level seed, density capped (see decorPlacement.ts),
 * NO real lights — emissive + bloom only.
 */
export function Decor(props: {
  platforms: readonly Platform[];
  seed: number;
  /** Optional: when a platform's tile is TILE_GONE its decoration is hidden. */
  tiles?: readonly TileState[];
}): JSX.Element {
  const { platforms, seed, tiles } = props;
  const built = useMemo(() => buildDecor(platforms, seed), [platforms, seed]);
  const builtRef = useRef<DecorBuilt | null>(null);
  useEffect(() => {
    builtRef.current = built;
  }, [built]);
  useEffect(() => () => disposeBuilt(built), [built]);
  useEffect(() => {
    const current = builtRef.current;
    if (current) applyTiles(current, tiles);
  }, [built, tiles]);

  useFrame((state) => {
    const current = builtRef.current;
    if (!current) return;
    const t = state.clock.elapsedTime;
    current.lanternMat.emissiveIntensity = 2.3 + Math.sin(t * 2.3) * 0.55;
    current.discMat.opacity = 0.42 + Math.sin(t * 2.3 + 1.0) * 0.14;
  });

  return <primitive object={built.holder} />;
}
