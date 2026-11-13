"use client";

import { useRef } from "react";
import type { JSX } from "react";
import { useFrame } from "@react-three/fiber";
import { AdditiveBlending } from "three";
import type { Group } from "three";
import type { Vec3 } from "@/protocol/generated";
import { COMMON } from "./theme";

const GLOW_EMISSIVE = 0.9;
const HALO_RADIUS = 0.75;
const HALO_OPACITY = 0.12;

export function GiftBox(props: {
  position: Vec3;
  taken: boolean;
}): JSX.Element | null {
  const groupRef = useRef<Group>(null);

  useFrame((state, delta) => {
    const group = groupRef.current;
    if (group === null) {
      return;
    }
    group.rotation.y += delta * 0.9;
    group.position.y =
      props.position.y + Math.sin(state.clock.elapsedTime * 1.6) * 0.15;
  });

  if (props.taken) {
    return null;
  }

  return (
    <group
      ref={groupRef}
      position={[props.position.x, props.position.y, props.position.z]}
    >
      <mesh>
        <boxGeometry args={[0.7, 0.7, 0.7]} />
        <meshStandardMaterial
          color={COMMON.gift.body}
          emissive={COMMON.gift.glow}
          emissiveIntensity={GLOW_EMISSIVE}
          toneMapped={false}
        />
      </mesh>
      <mesh>
        <boxGeometry args={[0.73, 0.73, 0.14]} />
        <meshStandardMaterial color={COMMON.gift.ribbon} />
      </mesh>
      <mesh>
        <boxGeometry args={[0.14, 0.73, 0.73]} />
        <meshStandardMaterial color={COMMON.gift.ribbon} />
      </mesh>
      <mesh position={[0, 0.4, 0]} rotation={[0, 0, 0.7]}>
        <boxGeometry args={[0.14, 0.05, 0.05]} />
        <meshStandardMaterial color={COMMON.gift.ribbon} />
      </mesh>
      <mesh position={[0, 0.4, 0]} rotation={[0, 0, -0.7]}>
        <boxGeometry args={[0.14, 0.05, 0.05]} />
        <meshStandardMaterial color={COMMON.gift.ribbon} />
      </mesh>
      <mesh position={[0, 0.4, 0]}>
        <boxGeometry args={[0.08, 0.08, 0.08]} />
        <meshStandardMaterial color={COMMON.gift.body} />
      </mesh>
      <mesh>
        <sphereGeometry args={[HALO_RADIUS, 16, 16]} />
        <meshBasicMaterial
          color={COMMON.gift.glow}
          transparent
          opacity={HALO_OPACITY}
          blending={AdditiveBlending}
          depthWrite={false}
        />
      </mesh>
    </group>
  );
}
