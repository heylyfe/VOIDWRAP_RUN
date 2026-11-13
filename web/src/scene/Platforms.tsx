"use client";

import { useEffect, useMemo } from "react";
import type { JSX } from "react";
import { BoxGeometry, MeshStandardMaterial } from "three";
import type { Material } from "three";
import type { Platform } from "@/protocol/generated";
import { themeTextures, worldScaledBoxGeometry } from "./checker";
import type { Theme } from "./theme";

interface EdgeBarData {
  size: [number, number, number];
  position: [number, number, number];
}

interface PlatformBoxData {
  id: number;
  center: [number, number, number];
  size: [number, number, number];
  edgeBars: EdgeBarData[];
}

interface ThemeMaterials {
  /** One material per BoxGeometry face, in face order: +x, -x, +y, -y, +z, -z. */
  byFace: [Material, Material, Material, Material, Material, Material];
  /** Glowing perimeter trim material. */
  edgeTrim: Material;
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
    center,
    size,
    edgeBars: toEdgeBars(size),
  };
}

/** Materials shared by every platform of a theme (textures are module-cached). */
function createThemeMaterials(theme: Theme): ThemeMaterials {
  const textures = themeTextures(theme);
  const top = new MeshStandardMaterial(
    textures.top
      ? { map: textures.top, roughness: TOP_ROUGHNESS }
      : { color: theme.tile.light, roughness: TOP_ROUGHNESS },
  );
  const side = new MeshStandardMaterial(
    textures.side
      ? { map: textures.side, color: SIDE_TINT, roughness: SIDE_ROUGHNESS }
      : { color: theme.tile.side, roughness: SIDE_ROUGHNESS },
  );
  const bottom = new MeshStandardMaterial({
    color: theme.tile.dark,
    roughness: BOTTOM_ROUGHNESS,
  });
  const edgeTrim = new MeshStandardMaterial({
    color: theme.tile.edge,
    emissive: theme.tile.edge,
    emissiveIntensity: EDGE_BAR_EMISSIVE,
  });
  return {
    // BoxGeometry face order: +x, -x, +y, -y, +z, -z.
    byFace: [side, side, top, bottom, side, side],
    edgeTrim,
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
}): JSX.Element {
  const { box, materials } = props;
  // World-scaled UVs are memoised per platform box; disposed on change/unmount.
  const geometry = useMemo(
    () => worldScaledBoxGeometry(box.size[0], box.size[1], box.size[2]),
    [box.size],
  );
  useEffect(() => () => geometry.dispose(), [geometry]);
  return (
    <group position={box.center}>
      <mesh geometry={geometry} material={materials.byFace} />
      {box.edgeBars.map((bar, index) => (
        <EdgeTrimBar key={index} bar={bar} material={materials.edgeTrim} />
      ))}
    </group>
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
}): JSX.Element {
  const boxes = useMemo(() => props.platforms.map(toBoxData), [props.platforms]);
  // Materials are shared between all platforms and memoised per theme.
  const materials = useMemo(() => createThemeMaterials(props.theme), [props.theme]);
  useEffect(() => () => disposeMaterials(materials), [materials]);

  return (
    <group>
      {boxes.map((box) => (
        <PlatformMesh key={box.id} box={box} materials={materials} />
      ))}
    </group>
  );
}
