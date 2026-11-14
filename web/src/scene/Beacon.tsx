"use client";

import { useRef } from "react";
import type { JSX } from "react";
import { useFrame } from "@react-three/fiber";
import { AdditiveBlending } from "three";
import type { Mesh, MeshStandardMaterial } from "three";
import type { Vec3 } from "@/protocol/generated";
import { LightPool } from "./LightPool";
import { COMMON } from "./theme";

// Dormant state: cold grey brazier on the spawn hub dais (phase 1).
const STONE = "#5d616b";
const STONE_DARK = "#41454e";
const BOWL = "#34373f";
const COALS_DARK = "#23242a";
const COALS_ASH = "#16171d";
const FRAME_GREY = "#6a6e78";
const FRAME_GREY_EMISSIVE = 0.12;

const OPEN_PILLAR_HEIGHT = 90;
const OPEN_PILLAR_CORE_RADIUS = 0.34;
const OPEN_PILLAR_BEAM_RADIUS = 0.95;
const PILLAR_BASE_Y = 0.66;
const OPEN_PILLAR_CENTER_Y = PILLAR_BASE_Y + OPEN_PILLAR_HEIGHT / 2;
const CORE_EMISSIVE = 4;

const FRAME_EMISSIVE_OPEN = 1.6;

// Outer beam pulse (open state only): opacity 0.28..0.42, radius scale 1.0..1.12.
const BEAM_BASE_OPACITY = 0.28;
const BEAM_OPACITY_RANGE = 0.14;
const BEAM_SCALE_RANGE = 0.12;

// Fake light spill around an open beacon's base (no real lights).
const POOL_RADIUS = 4.5;
const POOL_INTENSITY = 0.55;

const FRAME_WIDTH = 1.6;
const FRAME_THICKNESS = 0.08;
const FRAME_HEIGHT = 0.08;

interface FrameBarData {
  position: [number, number, number];
  size: [number, number, number];
}

const FRAME_BARS: ReadonlyArray<FrameBarData> = [
  {
    position: [0, 0.22, FRAME_WIDTH / 2 - FRAME_THICKNESS / 2],
    size: [FRAME_WIDTH, FRAME_HEIGHT, FRAME_THICKNESS],
  },
  {
    position: [0, 0.22, -(FRAME_WIDTH / 2 - FRAME_THICKNESS / 2)],
    size: [FRAME_WIDTH, FRAME_HEIGHT, FRAME_THICKNESS],
  },
  {
    position: [FRAME_WIDTH / 2 - FRAME_THICKNESS / 2, 0.22, 0],
    size: [FRAME_THICKNESS, FRAME_HEIGHT, FRAME_WIDTH - FRAME_THICKNESS * 2],
  },
  {
    position: [-(FRAME_WIDTH / 2 - FRAME_THICKNESS / 2), 0.22, 0],
    size: [FRAME_THICKNESS, FRAME_HEIGHT, FRAME_WIDTH - FRAME_THICKNESS * 2],
  },
];

export function Beacon(props: { position: Vec3; open: boolean }): JSX.Element {
  const beamMeshRef = useRef<Mesh>(null);
  const beamMaterialRef = useRef<MeshStandardMaterial>(null);

  useFrame((state) => {
    if (!props.open) return;
    const beam = beamMeshRef.current;
    const material = beamMaterialRef.current;
    if (beam === null || material === null) return;
    const pulse = Math.sin(state.clock.elapsedTime * 2.4) * 0.5 + 0.5;
    const scale = 1 + pulse * BEAM_SCALE_RANGE;
    beam.scale.x = scale;
    beam.scale.z = scale;
    material.opacity = BEAM_BASE_OPACITY + pulse * BEAM_OPACITY_RANGE;
  });

  const open = props.open;
  const frameColor = open ? COMMON.beacon.beam : FRAME_GREY;
  const frameEmissive = open ? FRAME_EMISSIVE_OPEN : FRAME_GREY_EMISSIVE;

  return (
    <group position={[props.position.x, props.position.y, props.position.z]}>
      {/* Dais slab */}
      <mesh position={[0, 0.09, 0]}>
        <boxGeometry args={[1.8, 0.18, 1.8]} />
        <meshStandardMaterial color={STONE_DARK} roughness={0.9} />
      </mesh>
      {/* Corner frame bars around the dais */}
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
      {/* Brazier stem */}
      <mesh position={[0, 0.32, 0]}>
        <cylinderGeometry args={[0.26, 0.38, 0.3, 12]} />
        <meshStandardMaterial color={STONE} roughness={0.85} />
      </mesh>
      {/* Brazier bowl */}
      <mesh position={[0, 0.52, 0]}>
        <cylinderGeometry args={[0.66, 0.32, 0.26, 12]} />
        <meshStandardMaterial color={BOWL} roughness={0.6} metalness={0.35} />
      </mesh>
      {/* Coals: grey ash while dormant, roaring ember when ignited */}
      <mesh position={[0, PILLAR_BASE_Y - 0.03, 0]}>
        <cylinderGeometry args={[0.56, 0.56, 0.1, 12]} />
        {open ? (
          <meshStandardMaterial
            color="#ffca66"
            emissive="#ff8a2a"
            emissiveIntensity={2.6}
            toneMapped={false}
          />
        ) : (
          <meshStandardMaterial
            color={COALS_DARK}
            emissive={COALS_ASH}
            emissiveIntensity={0.4}
            roughness={1}
          />
        )}
      </mesh>
      {open ? (
        <>
          <group position={[0, PILLAR_BASE_Y, 0]}>
            <LightPool color={COMMON.beacon.beam} radius={POOL_RADIUS} intensity={POOL_INTENSITY} />
          </group>
          {/* Core pillar */}
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
          {/* Outer glow beam (pulsing) */}
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
      ) : null}
    </group>
  );
}
