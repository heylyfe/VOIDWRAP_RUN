"use client";

import { useRef } from "react";
import type { JSX } from "react";
import { useFrame } from "@react-three/fiber";
import type { MeshStandardMaterial } from "three";
import type { Vec3 } from "@/protocol/generated";
import { PALETTE } from "./palette";

interface FrameBarData {
  position: [number, number, number];
  size: [number, number, number];
}

// Thin flat frame around a 1.6-wide square on top of the base pad. The bars
// along X span the full width; the bars along Z span the gap between them so
// the corners do not double-glow. 0.08 tall, resting on the base (top at
// y = 0.2).
const BASE_TOP_Y = 0.2;
const FRAME_WIDTH = 1.6;
const FRAME_THICKNESS = 0.08;
const FRAME_HEIGHT = 0.08;
const FRAME_EMISSIVE_OPEN = 1.5;
const FRAME_EMISSIVE_CLOSED = 0.5;
const CLOSED_PILLAR_EMISSIVE = 0.12;

const FRAME_BARS: ReadonlyArray<FrameBarData> = [
  {
    position: [0, BASE_TOP_Y + FRAME_HEIGHT / 2, FRAME_WIDTH / 2 - FRAME_THICKNESS / 2],
    size: [FRAME_WIDTH, FRAME_HEIGHT, FRAME_THICKNESS],
  },
  {
    position: [0, BASE_TOP_Y + FRAME_HEIGHT / 2, -(FRAME_WIDTH / 2 - FRAME_THICKNESS / 2)],
    size: [FRAME_WIDTH, FRAME_HEIGHT, FRAME_THICKNESS],
  },
  {
    position: [FRAME_WIDTH / 2 - FRAME_THICKNESS / 2, BASE_TOP_Y + FRAME_HEIGHT / 2, 0],
    size: [FRAME_THICKNESS, FRAME_HEIGHT, FRAME_WIDTH - FRAME_THICKNESS * 2],
  },
  {
    position: [-(FRAME_WIDTH / 2 - FRAME_THICKNESS / 2), BASE_TOP_Y + FRAME_HEIGHT / 2, 0],
    size: [FRAME_THICKNESS, FRAME_HEIGHT, FRAME_WIDTH - FRAME_THICKNESS * 2],
  },
];

export function Beacon(props: { position: Vec3; open: boolean }): JSX.Element {
  const pillarMaterialRef = useRef<MeshStandardMaterial>(null);

  useFrame((state) => {
    const material = pillarMaterialRef.current;
    if (material === null) {
      return;
    }
    material.emissiveIntensity = props.open
      ? 1.2 + Math.sin(state.clock.elapsedTime * 3) * 0.6
      : CLOSED_PILLAR_EMISSIVE;
  });

  const accent = props.open ? PALETTE.beaconOpen : PALETTE.beaconClosed;
  const frameEmissive = props.open ? FRAME_EMISSIVE_OPEN : FRAME_EMISSIVE_CLOSED;

  return (
    <group position={[props.position.x, props.position.y, props.position.z]}>
      <mesh position={[0, 0.1, 0]}>
        <boxGeometry args={[1.4, 0.2, 1.4]} />
        <meshStandardMaterial
          color={accent}
          emissive={accent}
          emissiveIntensity={props.open ? 0.6 : 0.1}
        />
      </mesh>
      {FRAME_BARS.map((bar, index) => (
        <mesh key={index} position={bar.position}>
          <boxGeometry args={bar.size} />
          <meshStandardMaterial
            color={PALETTE.beaconOpen}
            emissive={PALETTE.beaconOpen}
            emissiveIntensity={frameEmissive}
          />
        </mesh>
      ))}
      <mesh position={[0, 3.2, 0]}>
        <boxGeometry args={[0.8, 6, 0.8]} />
        <meshStandardMaterial
          ref={pillarMaterialRef}
          color={accent}
          emissive={PALETTE.beaconOpen}
          emissiveIntensity={props.open ? 1.2 : CLOSED_PILLAR_EMISSIVE}
          transparent
          depthWrite={false}
          opacity={props.open ? 0.4 : 0.28}
        />
      </mesh>
    </group>
  );
}
