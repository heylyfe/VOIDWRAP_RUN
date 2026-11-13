"use client";

import { useMemo } from "react";
import type { JSX } from "react";
import type { Platform } from "@/protocol/generated";
import { PALETTE, platformColor } from "./palette";

interface EdgeBarData {
  size: [number, number, number];
  position: [number, number, number];
}

interface PlatformBoxData {
  id: number;
  center: [number, number, number];
  size: [number, number, number];
  top: string;
  accent: string;
  edgeBars: EdgeBarData[];
}

const TOP_EMISSIVE = 0.08;
const EDGE_BAR_THICKNESS = 0.08;
const EDGE_BAR_HEIGHT = 0.08;
const EDGE_BAR_OVERHANG = 0.04;
const EDGE_BAR_EMISSIVE = 1.0;

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
    top: platformColor(platform.id),
    accent:
      platform.id % 2 === 0 ? PALETTE.edgeGlowCyan : PALETTE.edgeGlowMagenta,
    edgeBars: toEdgeBars(size),
  };
}

export function Platforms(props: {
  platforms: readonly Platform[];
}): JSX.Element {
  const boxes = useMemo(() => props.platforms.map(toBoxData), [props.platforms]);

  return (
    <group>
      {boxes.map((box) => (
        <group key={box.id} position={box.center}>
          <mesh>
            <boxGeometry args={box.size} />
            <meshStandardMaterial
              color={box.top}
              emissive={box.top}
              emissiveIntensity={TOP_EMISSIVE}
            />
          </mesh>
          {box.edgeBars.map((bar, index) => (
            <mesh key={index} position={bar.position}>
              <boxGeometry args={bar.size} />
              <meshStandardMaterial
                color={box.accent}
                emissive={box.accent}
                emissiveIntensity={EDGE_BAR_EMISSIVE}
              />
            </mesh>
          ))}
        </group>
      ))}
    </group>
  );
}
