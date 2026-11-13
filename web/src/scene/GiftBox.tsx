"use client";

import { useRef } from "react";
import type { JSX } from "react";
import { useFrame } from "@react-three/fiber";
import { AdditiveBlending } from "three";
import type { Group } from "three";
import type { Vec3 } from "@/protocol/generated";
import { LightPool } from "./LightPool";
import { COMMON } from "./theme";

const GLOW_EMISSIVE = 0.45;
const HALO_RADIUS = 0.6;
const HALO_OPACITY = 0.06;

/** The gift centre floats this far above the platform's top surface. */
const GIFT_LIFT = 1;
/** Fake light spill on the tiles below the gift. */
const POOL_RADIUS = 1.8;
const POOL_INTENSITY = 0.35;

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
    // The parent group already carries the gift's base position; bobbing is
    // a relative offset so the light pool below stays put.
    group.position.y = Math.sin(state.clock.elapsedTime * 1.6) * 0.15;
  });

  if (props.taken) {
    return null;
  }

  return (
    <group position={[props.position.x, props.position.y, props.position.z]}>
      <group position={[0, -GIFT_LIFT, 0]}>
        <LightPool
          color={COMMON.gift.glow}
          radius={POOL_RADIUS}
          intensity={POOL_INTENSITY}
        />
      </group>
      <group ref={groupRef}>
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
    </group>
  );
}
