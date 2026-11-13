"use client";

import { useRef } from "react";
import type { JSX } from "react";
import { useFrame } from "@react-three/fiber";
import { AdditiveBlending } from "three";
import type { Mesh, MeshStandardMaterial } from "three";
import type { Vec3 } from "@/protocol/generated";
import { COMMON } from "./theme";

interface FrameBarData {
  position: [number, number, number];
  size: [number, number, number];
}

// Thin flat frame around a 1.6-wide square on top of the base pad. The bars
// along X span the full width; the bars along Z span the gap between them so
// the corners do not double-glow. 0.08 tall, resting on the base (top at
// y = 0.2). The pillar sits on the same top surface.
const BASE_TOP_Y = 0.2;
const FRAME_WIDTH = 1.6;
const FRAME_THICKNESS = 0.08;
const FRAME_HEIGHT = 0.08;

const OPEN_PILLAR_HEIGHT = 80;
const OPEN_PILLAR_CORE_RADIUS = 0.35;
const OPEN_PILLAR_BEAM_RADIUS = 0.9;
const OPEN_PILLAR_CENTER_Y = BASE_TOP_Y + OPEN_PILLAR_HEIGHT / 2;
const CORE_EMISSIVE = 4;

const CLOSED_PILLAR_HEIGHT = 8;
const CLOSED_PILLAR_RADIUS = 0.45;
const CLOSED_PILLAR_CENTER_Y = BASE_TOP_Y + CLOSED_PILLAR_HEIGHT / 2;

const FRAME_EMISSIVE_OPEN = 1.5;
const FRAME_EMISSIVE_CLOSED = 0.35;

// Outer beam pulse (open state only): opacity 0.28..0.42, radius 1.0..1.12.
const BEAM_BASE_OPACITY = 0.28;
const BEAM_OPACITY_RANGE = 0.14;
const BEAM_SCALE_RANGE = 0.12;

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
  const beamMeshRef = useRef<Mesh>(null);
  const beamMaterialRef = useRef<MeshStandardMaterial>(null);

  useFrame((state) => {
    if (!props.open) {
      return;
    }
    const beam = beamMeshRef.current;
    const material = beamMaterialRef.current;
    if (beam === null || material === null) {
      return;
    }
    const pulse = Math.sin(state.clock.elapsedTime * 2.4) * 0.5 + 0.5;
    const scale = 1 + pulse * BEAM_SCALE_RANGE;
    beam.scale.x = scale;
    beam.scale.z = scale;
    material.opacity = BEAM_BASE_OPACITY + pulse * BEAM_OPACITY_RANGE;
  });

  const open = props.open;
  const frameColor = open ? COMMON.beacon.beam : COMMON.beacon.closed;
  const frameEmissive = open ? FRAME_EMISSIVE_OPEN : FRAME_EMISSIVE_CLOSED;

  return (
    <group position={[props.position.x, props.position.y, props.position.z]}>
      <mesh position={[0, 0.1, 0]}>
        <boxGeometry args={[1.4, 0.2, 1.4]} />
        <meshStandardMaterial
          color={open ? COMMON.beacon.beam : COMMON.beacon.closed}
          emissive={open ? COMMON.beacon.beam : COMMON.beacon.closed}
          emissiveIntensity={open ? 0.6 : 0.05}
        />
      </mesh>
      {FRAME_BARS.map((bar, index) => (
        <mesh key={index} position={bar.position}>
          <boxGeometry args={bar.size} />
          <meshStandardMaterial
            color={frameColor}
            emissive={frameColor}
            emissiveIntensity={frameEmissive}
          />
        </mesh>
      ))}
      {open ? (
        <>
          <mesh position={[0, OPEN_PILLAR_CENTER_Y, 0]}>
            <cylinderGeometry
              args={[OPEN_PILLAR_CORE_RADIUS, OPEN_PILLAR_CORE_RADIUS, OPEN_PILLAR_HEIGHT, 12]}
            />
            <meshStandardMaterial
              color={COMMON.beacon.core}
              emissive={COMMON.beacon.core}
              emissiveIntensity={CORE_EMISSIVE}
              toneMapped={false}
              fog={false}
            />
          </mesh>
          <mesh ref={beamMeshRef} position={[0, OPEN_PILLAR_CENTER_Y, 0]}>
            <cylinderGeometry
              args={[
                OPEN_PILLAR_BEAM_RADIUS,
                OPEN_PILLAR_BEAM_RADIUS,
                OPEN_PILLAR_HEIGHT,
                16,
                1,
                true,
              ]}
            />
            <meshStandardMaterial
              ref={beamMaterialRef}
              color={COMMON.beacon.beam}
              emissive={COMMON.beacon.beam}
              emissiveIntensity={1.2}
              transparent
              opacity={BEAM_BASE_OPACITY + BEAM_OPACITY_RANGE * 0.5}
              blending={AdditiveBlending}
              depthWrite={false}
              fog={false}
            />
          </mesh>
        </>
      ) : (
        <mesh position={[0, CLOSED_PILLAR_CENTER_Y, 0]}>
          <cylinderGeometry
            args={[CLOSED_PILLAR_RADIUS, CLOSED_PILLAR_RADIUS, CLOSED_PILLAR_HEIGHT, 12]}
          />
          <meshStandardMaterial
            color={COMMON.beacon.closed}
            transparent
            opacity={0.25}
            depthWrite={false}
          />
        </mesh>
      )}
    </group>
  );
}
