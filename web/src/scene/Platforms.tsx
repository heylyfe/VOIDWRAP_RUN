"use client";

import { useEffect, useMemo, useRef } from "react";
import type { JSX } from "react";
import { useFrame } from "@react-three/fiber";
import {
  BoxGeometry,
  CanvasTexture,
  Color,
  MeshStandardMaterial,
  RepeatWrapping,
  SRGBColorSpace,
} from "three";
import type { Group, Material, MeshStandardMaterial as MeshStandardMaterialType, Texture } from "three";
import { PLATFORM_CRUMBLE, TILE_GONE, TILE_WARN } from "@/protocol/generated";
import type { Platform, TileState } from "@/protocol/generated";
import { themeTextures, worldScaledBoxGeometry } from "./checker";
import { iceTint } from "./iceTint";
import type { Theme } from "./theme";

interface EdgeBarData {
  size: [number, number, number];
  position: [number, number, number];
}

interface PlatformBoxData {
  id: number;
  kind: number;
  center: [number, number, number];
  size: [number, number, number];
  edgeBars: EdgeBarData[];
}

interface ThemeMaterials {
  /** One material per BoxGeometry face, in face order: +x, -x, +y, -y, +z, -z. */
  byFace: [Material, Material, Material, Material, Material, Material];
  /** Materials broken out so crumble platforms can reuse them per-platform. */
  side: Material;
  bottom: Material;
  /** Glowing perimeter trim material. */
  edgeTrim: Material;
  /** Base (already ice-tinted when applicable) hex colour for crumble tops. */
  crumbleBase: string;
  ghostColor: string;
  /** Every material created for the current theme, for disposal. */
  all: readonly Material[];
}

const EDGE_BAR_THICKNESS = 0.08;
const EDGE_BAR_HEIGHT = 0.08;
const EDGE_BAR_OVERHANG = 0.04;
const EDGE_BAR_EMISSIVE = 1.4;
const SIDE_TINT = "#9a9a9a";
const TOP_ROUGHNESS = 0.85;
const SIDE_ROUGHNESS = 0.9;
const BOTTOM_ROUGHNESS = 1;
const ICE_ROUGHNESS = 0.16;
const ICE_EMISSIVE = "#33506b";
const ICE_EMISSIVE_INTENSITY = 0.3;
const CRUMBLE_SHADE = 0.55;
const CRUMBLE_ROUGHNESS = 0.8;
const CRUMBLE_IDLE_EMISSIVE = 0.14;
const WARN_EMISSIVE = "#ff5a2a";
const SHAKE_AMP = 0.055;
const GONE_GHOST_OPACITY = 0.07;

// Four thin bars along the top perimeter. The bars along X straddle the
// top edge (overhang EDGE_BAR_OVERHANG on each side); the bars along Z
// span exactly the gap between them so nothing overlaps at the corners.
function toEdgeBars(size: readonly [number, number, number]): EdgeBarData[] {
  const [width, height, depth] = size;
  const halfWidth = width / 2;
  const halfDepth = depth / 2;
  const barY = height / 2 + EDGE_BAR_HEIGHT / 2;
  const longLength = width + EDGE_BAR_OVERHANG * 2;
  const shortLength = depth - EDGE_BAR_OVERHANG * 2;
  return [
    {
      size: [longLength, EDGE_BAR_HEIGHT, EDGE_BAR_THICKNESS],
      position: [0, barY, halfDepth],
    },
    {
      size: [longLength, EDGE_BAR_HEIGHT, EDGE_BAR_THICKNESS],
      position: [0, barY, -halfDepth],
    },
    {
      size: [EDGE_BAR_THICKNESS, EDGE_BAR_HEIGHT, shortLength],
      position: [halfWidth, barY, 0],
    },
    {
      size: [EDGE_BAR_THICKNESS, EDGE_BAR_HEIGHT, shortLength],
      position: [-halfWidth, barY, 0],
    },
  ];
}

function toBoxData(platform: Platform): PlatformBoxData {
  const { min, max } = platform.bounds;
  const size: [number, number, number] = [
    max.x - min.x,
    max.y - min.y,
    max.z - min.z,
  ];
  const center: [number, number, number] = [
    (min.x + max.x) / 2,
    (min.y + max.y) / 2,
    (min.z + max.z) / 2,
  ];
  return {
    id: platform.id,
    kind: platform.kind,
    center,
    size,
    edgeBars: toEdgeBars(size),
  };
}

/** Multiply a hex colour by `factor` (used to darken crumble tops so they read as fragile). */
function shadeColor(hex: string, factor: number): string {
  const c = new Color(hex);
  c.multiplyScalar(factor);
  return `#${c.getHexString()}`;
}

function makeTopMaterial(baseHex: string, map: Texture | null, icy: boolean): MeshStandardMaterial {
  if (map) {
    return new MeshStandardMaterial({
      map,
      color: icy ? iceTint("#ffffff") : "#ffffff",
      roughness: icy ? ICE_ROUGHNESS : TOP_ROUGHNESS,
      emissive: icy ? ICE_EMISSIVE : "#000000",
      emissiveIntensity: icy ? ICE_EMISSIVE_INTENSITY : 0,
    });
  }
  return new MeshStandardMaterial({
    color: icy ? iceTint(baseHex) : baseHex,
    roughness: icy ? ICE_ROUGHNESS : TOP_ROUGHNESS,
    emissive: icy ? ICE_EMISSIVE : "#000000",
    emissiveIntensity: icy ? ICE_EMISSIVE_INTENSITY : 0,
  });
}

/** Deterministic cracked-stone canvas texture for crumbling platform tops (built once). */
function createCrackTexture(baseHex: string): CanvasTexture | null {
  if (typeof document === "undefined") return null;
  const size = 256;
  const canvas = document.createElement("canvas");
  canvas.width = size;
  canvas.height = size;
  const ctx = canvas.getContext("2d");
  if (!ctx) return null;
  // Deterministic LCG so every client sees the same cracks.
  let seed = 9001;
  const rnd = (): number => {
    seed = (Math.imul(seed, 1664525) + 1013904223) >>> 0;
    return seed / 4294967296;
  };
  ctx.fillStyle = baseHex;
  ctx.fillRect(0, 0, size, size);
  for (let i = 0; i < 40; i += 1) {
    ctx.fillStyle = `rgba(0, 0, 0, ${0.08 + rnd() * 0.12})`;
    ctx.beginPath();
    ctx.arc(rnd() * size, rnd() * size, 12 + rnd() * 42, 0, Math.PI * 2);
    ctx.fill();
  }
  for (let i = 0; i < 14; i += 1) {
    ctx.strokeStyle = "rgba(12, 9, 8, 0.9)";
    ctx.lineWidth = 1.5 + rnd() * 2.5;
    ctx.beginPath();
    let x = rnd() * size;
    let y = rnd() * size;
    let angle = rnd() * Math.PI * 2;
    ctx.moveTo(x, y);
    const segments = 5 + Math.floor(rnd() * 5);
    for (let j = 0; j < segments; j += 1) {
      angle += (rnd() - 0.5) * 1.4;
      const len = 18 + rnd() * 30;
      x += Math.cos(angle) * len;
      y += Math.sin(angle) * len;
      ctx.lineTo(x, y);
    }
    ctx.stroke();
  }
  const tex = new CanvasTexture(canvas);
  tex.wrapS = RepeatWrapping;
  tex.wrapT = RepeatWrapping;
  tex.colorSpace = SRGBColorSpace;
  tex.anisotropy = 4;
  return tex;
}

/** Materials shared by every platform of a theme (textures are module-cached). */
function createThemeMaterials(theme: Theme, icy: boolean): ThemeMaterials {
  const textures = themeTextures(theme);
  const top = makeTopMaterial(theme.tile.light, textures.top ?? null, icy);
  const side = new MeshStandardMaterial(
    textures.side
      ? { map: textures.side, color: icy ? iceTint(SIDE_TINT) : SIDE_TINT, roughness: SIDE_ROUGHNESS }
      : { color: icy ? iceTint(theme.tile.side) : theme.tile.side, roughness: SIDE_ROUGHNESS },
  );
  const bottom = new MeshStandardMaterial({
    color: icy ? iceTint(theme.tile.dark) : theme.tile.dark,
    roughness: BOTTOM_ROUGHNESS,
  });
  const edgeTrim = new MeshStandardMaterial({
    color: icy ? iceTint(theme.tile.edge) : theme.tile.edge,
    emissive: icy ? iceTint(theme.tile.edge) : theme.tile.edge,
    emissiveIntensity: EDGE_BAR_EMISSIVE,
    roughness: icy ? 0.2 : 0.4,
  });
  return {
    // BoxGeometry face order: +x, -x, +y, -y, +z, -z.
    byFace: [side, side, top, bottom, side, side],
    side,
    bottom,
    edgeTrim,
    crumbleBase: shadeColor(theme.tile.light, CRUMBLE_SHADE),
    ghostColor: theme.tile.edge,
    all: [top, side, bottom, edgeTrim],
  };
}

function disposeMaterials(materials: ThemeMaterials): void {
  for (const material of materials.all) {
    material.dispose();
  }
}

function PlatformMesh(props: {
  box: PlatformBoxData;
  materials: ThemeMaterials;
  tileState: number | null;
  icy: boolean;
}): JSX.Element {
  const { box, materials, tileState, icy } = props;
  const crumble = box.kind === PLATFORM_CRUMBLE;
  const warn = tileState === TILE_WARN;
  const gone = tileState === TILE_GONE;
  // World-scaled UVs are memoised per platform box; disposed on change/unmount.
  const geometry = useMemo(
    () => worldScaledBoxGeometry(box.size[0], box.size[1], box.size[2]),
    [box.size],
  );
  useEffect(() => () => geometry.dispose(), [geometry]);

  // Crumble platforms get their own cracked top material so TILE_WARN can flash it.
  const ownTop = useMemo(() => {
    if (!crumble) return null;
    const base = icy ? iceTint(materials.crumbleBase) : materials.crumbleBase;
    const map = createCrackTexture(base);
    const mat = new MeshStandardMaterial({
      color: map ? "#ffffff" : base,
      map,
      roughness: icy ? ICE_ROUGHNESS : CRUMBLE_ROUGHNESS,
      emissive: WARN_EMISSIVE,
      emissiveIntensity: CRUMBLE_IDLE_EMISSIVE,
    });
    return { mat, map };
  }, [crumble, icy, materials.crumbleBase]);
  useEffect(
    () => () => {
      ownTop?.mat.dispose();
      ownTop?.map?.dispose();
    },
    [ownTop],
  );
  // The crack material is hook-created once per identity change; relay it through a
  // ref (assigned in an effect) so per-frame emissive tweaks never touch a hook value.
  const ownTopMatRef = useRef<MeshStandardMaterialType | null>(null);
  useEffect(() => {
    ownTopMatRef.current = ownTop?.mat ?? null;
    return () => {
      ownTopMatRef.current = null;
    };
  }, [ownTop]);
  const byFace = useMemo(() => {
    if (!ownTop) return materials.byFace;
    const faces: [Material, Material, Material, Material, Material, Material] = [
      materials.side,
      materials.side,
      ownTop.mat,
      materials.bottom,
      materials.side,
      materials.side,
    ];
    return faces;
  }, [ownTop, materials]);

  const innerRef = useRef<Group>(null);
  const phase = useMemo(() => (box.id * 1.61803398875) % (Math.PI * 2), [box.id]);

  useFrame((state) => {
    if (!crumble && !warn) return;
    const t = state.clock.elapsedTime;
    const group = innerRef.current;
    if (group) {
      if (warn) {
        group.position.set(
          box.center[0] + Math.sin(t * 34 + phase) * SHAKE_AMP,
          box.center[1],
          box.center[2] + Math.cos(t * 27 + phase) * SHAKE_AMP,
        );
      } else if (
        group.position.x !== box.center[0] ||
        group.position.y !== box.center[1] ||
        group.position.z !== box.center[2]
      ) {
        group.position.set(box.center[0], box.center[1], box.center[2]);
      }
    }
    const ownTopMat = ownTopMatRef.current;
    if (ownTopMat) {
      ownTopMat.emissiveIntensity = warn
        ? 1.05 + Math.sin(t * 9 + phase) * 0.9
        : CRUMBLE_IDLE_EMISSIVE;
    }
  });

  return (
    <>
      <group ref={innerRef} position={box.center} visible={!gone}>
        <mesh geometry={geometry} material={byFace} />
        {box.edgeBars.map((bar, index) => (
          <EdgeTrimBar key={index} bar={bar} material={materials.edgeTrim} />
        ))}
      </group>
      {/* Faint ghost outline where a crumbled platform used to be. */}
      <mesh geometry={geometry} position={box.center} visible={gone}>
        <meshBasicMaterial
          color={materials.ghostColor}
          wireframe
          transparent
          opacity={GONE_GHOST_OPACITY}
          depthWrite={false}
        />
      </mesh>
    </>
  );
}

function EdgeTrimBar(props: { bar: EdgeBarData; material: Material }): JSX.Element {
  const [x, y, z] = props.bar.size;
  // World-scaled UVs do not matter for the flat trim bars, so a plain box does.
  const geometry = useMemo(() => new BoxGeometry(x, y, z), [x, y, z]);
  useEffect(() => () => geometry.dispose(), [geometry]);
  return (
    <mesh geometry={geometry} position={props.bar.position} material={props.material} />
  );
}

export function Platforms(props: {
  platforms: readonly Platform[];
  theme: Theme;
  tiles: readonly TileState[];
  icy: boolean;
}): JSX.Element {
  const boxes = useMemo(() => props.platforms.map(toBoxData), [props.platforms]);
  const tileById = useMemo(() => {
    const m = new Map<number, number>();
    for (const tile of props.tiles) m.set(tile.id, tile.state);
    return m;
  }, [props.tiles]);
  // Materials are shared between all platforms and memoised per theme.
  const materials = useMemo(
    () => createThemeMaterials(props.theme, props.icy),
    [props.theme, props.icy],
  );
  useEffect(() => () => disposeMaterials(materials), [materials]);

  return (
    <group>
      {boxes.map((box) => (
        <PlatformMesh
          key={box.id}
          box={box}
          materials={materials}
          tileState={tileById.get(box.id) ?? null}
          icy={props.icy}
        />
      ))}
    </group>
  );
}
