"use client";

import { useRef } from "react";
import type { JSX } from "react";
import { useFrame } from "@react-three/fiber";
import type { MeshStandardMaterial } from "three";
import type { Vec3 } from "@/protocol/generated";
import { PALETTE } from "./palette";

export function Beacon(props: { position: Vec3; open: boolean }): JSX.Element {
  const pillarMaterialRef = useRef<MeshStandardMaterial>(null);

  useFrame((state) => {
    const material = pillarMaterialRef.current;
    if (material === null) {
      return;
    }
    if (!props.open) {
      material.emissiveIntensity = 0;
      return;
    }
    material.emissiveIntensity =
      1.2 + Math.sin(state.clock.elapsedTime * 3) * 0.6;
  });

  const accent = props.open ? PALETTE.beaconOpen : PALETTE.beaconClosed;

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
      <mesh position={[0, 3.2, 0]}>
        <boxGeometry args={[0.8, 6, 0.8]} />
        <meshStandardMaterial
          ref={pillarMaterialRef}
          color={accent}
          emissive={accent}
          emissiveIntensity={props.open ? 1.2 : 0}
          transparent
          depthWrite={false}
          opacity={props.open ? 0.35 : 0.12}
        />
      </mesh>
    </group>
  );
}
