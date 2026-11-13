"use client";

import { useRef } from "react";
import type { JSX } from "react";
import { useFrame } from "@react-three/fiber";
import type { Group, Mesh } from "three";
import type { Vec3 } from "@/protocol/generated";
import { PALETTE } from "./palette";

const SHARD_OFFSETS: ReadonlyArray<readonly [number, number, number]> = [
  [0.95, 0.25, 0],
  [-0.95, 0.25, 0],
  [0, 0.95, 0.35],
  [0, -0.95, -0.35],
];

export function Chaser(props: { position: Vec3 }): JSX.Element {
  const coreRef = useRef<Mesh>(null);
  const shardsRef = useRef<Group>(null);

  useFrame((state, delta) => {
    const core = coreRef.current;
    if (core !== null) {
      core.rotation.y += delta * 2.5;
    }
    const shards = shardsRef.current;
    if (shards !== null) {
      shards.rotation.y -= delta * 1.8;
      shards.rotation.x = Math.sin(state.clock.elapsedTime * 1.3) * 0.35;
    }
  });

  return (
    <group position={[props.position.x, props.position.y, props.position.z]}>
      <mesh ref={coreRef}>
        <octahedronGeometry args={[0.6]} />
        <meshStandardMaterial
          color={PALETTE.chaserRed}
          emissive={PALETTE.chaserRed}
          emissiveIntensity={1.4}
        />
      </mesh>
      <group ref={shardsRef}>
        {SHARD_OFFSETS.map((offset, index) => (
          <mesh key={index} position={offset as [number, number, number]}>
            <octahedronGeometry args={[0.14]} />
            <meshStandardMaterial
              color={PALETTE.chaserRed}
              emissive={PALETTE.chaserRed}
              emissiveIntensity={1.8}
            />
          </mesh>
        ))}
      </group>
    </group>
  );
}
