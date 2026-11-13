"use client";

import { useMemo } from "react";
import type { JSX } from "react";
import type { Platform } from "@/protocol/generated";
import { PALETTE, platformColor } from "./palette";

interface PlatformBoxData {
  id: number;
  center: [number, number, number];
  size: [number, number, number];
  top: string;
  accent: string;
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
              emissiveIntensity={0.15}
            />
          </mesh>
          <mesh position={[0, box.size[1] / 2 + 0.015, 0]}>
            <boxGeometry
              args={[
                box.size[0] + 0.06,
                0.06,
                box.size[2] + 0.06,
              ]}
            />
            <meshStandardMaterial
              color={box.accent}
              emissive={box.accent}
              emissiveIntensity={1.2}
            />
          </mesh>
        </group>
      ))}
    </group>
  );
}
