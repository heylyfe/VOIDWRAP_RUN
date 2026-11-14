"use client";

import { useRef } from "react";
import type { JSX } from "react";
import { useFrame } from "@react-three/fiber";
import { AdditiveBlending } from "three";
import type { Group, Mesh, MeshStandardMaterial } from "three";
import type { Vec3 } from "@/protocol/generated";
import { COMMON } from "./theme";
import type { EnemyMode } from "./enemyBehavior";

// Unit-radius model: the parent scales it by the enemy's body radius from the server, so the
// solid core is exactly the body the hit sphere is derived from (sim::rules::enemy_hit_radius).
const SHARD_OFFSETS: ReadonlyArray<readonly [number, number, number]> = [
  [1.3, 0.3, 0],
  [-1.3, 0.3, 0],
  [0, 1.3, 0.45],
  [0, -1.3, -0.45],
];

const CORE_EMISSIVE = 2.2;
const DORMANT_CORE_EMISSIVE = 0.85;
const SHARD_EMISSIVE = 0.8;
const DORMANT_SHARD_EMISSIVE = 0.3;
const AURA_RADIUS = 1.45;
const AURA_BASE_OPACITY = 0.07;
const DORMANT_AURA_OPACITY = 0.03;
const AURA_SCALE_RANGE = 0.06;

export function Chaser(props: { position: Vec3; mode: { current: EnemyMode } }): JSX.Element {
  const coreRef = useRef<Mesh>(null);
  const coreMaterialRef = useRef<MeshStandardMaterial>(null);
  const shardsRef = useRef<Group>(null);
  const auraMeshRef = useRef<Mesh>(null);
  const auraMaterialRef = useRef<MeshStandardMaterial>(null);
  const shardMaterialsRef = useRef<(MeshStandardMaterial | null)[]>([]);

  useFrame((state, delta) => {
    const dormant = props.mode.current === "dormant";
    const t = state.clock.elapsedTime;
    const ease = dormant ? 0.35 : 1;
    const core = coreRef.current;
    if (core !== null) {
      core.rotation.y += delta * 2.5 * ease;
    }
    const shards = shardsRef.current;
    if (shards !== null) {
      shards.rotation.y -= delta * 1.8 * ease;
      shards.rotation.x = Math.sin(t * 1.3 * ease) * 0.35;
    }
    const aura = auraMeshRef.current;
    if (aura !== null) {
      const scale = 1 + Math.sin(t * 2.1 * ease) * AURA_SCALE_RANGE;
      aura.scale.set(scale, scale, scale);
    }
    const auraMaterial = auraMaterialRef.current;
    if (auraMaterial !== null) {
      auraMaterial.opacity =
        (dormant ? DORMANT_AURA_OPACITY : AURA_BASE_OPACITY) +
        Math.sin(t * 2.1 * ease) * AURA_SCALE_RANGE * 0.5;
    }
    const coreMaterial = coreMaterialRef.current;
    if (coreMaterial !== null) {
      coreMaterial.emissiveIntensity = dormant ? DORMANT_CORE_EMISSIVE : CORE_EMISSIVE;
    }
    const shardIntensity = dormant ? DORMANT_SHARD_EMISSIVE : SHARD_EMISSIVE;
    for (const m of shardMaterialsRef.current) {
      if (m) m.emissiveIntensity = shardIntensity;
    }
  });

  return (
    <group position={[props.position.x, props.position.y, props.position.z]}>
      <mesh ref={coreRef}>
        <icosahedronGeometry args={[1, 0]} />
        <meshStandardMaterial
          ref={coreMaterialRef}
          color={COMMON.enemy.body}
          emissive={COMMON.enemy.glow}
          emissiveIntensity={CORE_EMISSIVE}
          toneMapped={false}
        />
      </mesh>
      <group ref={shardsRef}>
        {SHARD_OFFSETS.map((offset, index) => (
          <mesh key={index} position={offset as [number, number, number]}>
            <octahedronGeometry args={[0.18]} />
            <meshStandardMaterial
              ref={(m) => {
                shardMaterialsRef.current[index] = m;
              }}
              color={COMMON.enemy.body}
              emissive={COMMON.enemy.glow}
              emissiveIntensity={SHARD_EMISSIVE}
              toneMapped={false}
            />
          </mesh>
        ))}
      </group>
      <mesh ref={auraMeshRef}>
        <sphereGeometry args={[AURA_RADIUS, 16, 16]} />
        <meshStandardMaterial
          ref={auraMaterialRef}
          color={COMMON.enemy.glow}
          transparent
          opacity={AURA_BASE_OPACITY}
          blending={AdditiveBlending}
          depthWrite={false}
        />
      </mesh>
    </group>
  );
}
