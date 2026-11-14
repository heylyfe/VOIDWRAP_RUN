"use client";

import { useRef } from "react";
import type { JSX } from "react";
import { useFrame } from "@react-three/fiber";
import type { Group, MeshStandardMaterial } from "three";
import { ENEMY_ATTACK, ENEMY_DORMANT, ENEMY_TELEGRAPH } from "@/protocol/generated";
import type { EnemyModelProps } from "../enemyLook";
import { prog01 } from "../enemyLook";

const THREAD_COUNT = 5;

/**
 * Crumble Wisp — roiling black gauze knot, ember-green heart, trailing threads.
 * Accent #39ffb0 (sickly emerald) on a green-black core. Unit-radius model.
 * Idle: thread lashes. Fretting (TELEGRAPH/ATTACK): the heart flares and the
 * threads lash faster. The fretted-floor vein decals belong to the tile system.
 */
export function CrumbleWisp({ anim }: EnemyModelProps): JSX.Element {
  const root = useRef<Group>(null);
  const core = useRef<Group>(null);
  const heart = useRef<MeshStandardMaterial>(null);
  const coreMat = useRef<MeshStandardMaterial>(null);
  const pivots = useRef<(Group | null)[]>([]);

  useFrame((state, delta) => {
    const r = root.current;
    if (!r || (r.parent !== null && !r.parent.visible)) return;
    const a = anim.current;
    const t = state.clock.elapsedTime;
    const dormant = a.state === ENEMY_DORMANT;
    const telegraph = a.state === ENEMY_TELEGRAPH;
    const attack = a.state === ENEMY_ATTACK;
    const p = prog01(a.prog);
    const dim = dormant ? 0.16 : 1;
    const lashSpeed = attack ? 4.6 : telegraph ? 3.4 : 2.6;

    if (core.current) {
      core.current.rotation.y += delta * (dormant ? 0.2 : 0.75);
      core.current.scale.setScalar(
        1 + Math.sin(t * 2.3) * 0.06 + Math.sin(t * 5.1 + 0.9) * 0.035 + (telegraph ? p * 0.12 : 0),
      );
    }
    if (heart.current) {
      heart.current.emissiveIntensity =
        (1.9 + Math.sin(t * 3.2) * 0.5 + (telegraph || attack ? p * 2.3 : 0)) * dim;
    }
    if (coreMat.current) {
      coreMat.current.emissiveIntensity = (0.42 + (telegraph || attack ? p * 1.7 : 0)) * dim;
    }
    for (let i = 0; i < THREAD_COUNT; i += 1) {
      const pivot = pivots.current[i];
      if (pivot) pivot.rotation.x = Math.sin(t * lashSpeed + i * 1.7) * 0.32;
    }
  });

  return (
    <group ref={root}>
      <group ref={core}>
        {/* roiling black gauze knot */}
        <mesh>
          <icosahedronGeometry args={[0.44, 0]} />
          <meshStandardMaterial
            ref={coreMat}
            color="#08130c"
            emissive="#39ffb0"
            emissiveIntensity={0.42}
            flatShading
            roughness={0.55}
            metalness={0.1}
          />
        </mesh>
        {/* ember-green heart */}
        <mesh>
          <sphereGeometry args={[0.17, 10, 10]} />
          <meshStandardMaterial
            ref={heart}
            color="#0d2b1a"
            emissive="#39ffb0"
            emissiveIntensity={1.9}
            toneMapped={false}
          />
        </mesh>
      </group>
      {/* trailing threads lashing outward-down */}
      {Array.from({ length: THREAD_COUNT }, (_, i) => {
        const angle = (i / THREAD_COUNT) * Math.PI * 2;
        return (
          <group key={i} rotation={[0, angle, 0]}>
            <group
              ref={(g) => {
                pivots.current[i] = g;
              }}
              position={[0, -0.28, 0.3]}
            >
              <mesh position={[0, -0.4, 0.12]} rotation={[0.35, 0, 0]}>
                <cylinderGeometry args={[0.018, 0.008, 0.78, 4]} />
                <meshStandardMaterial color="#0f2418" emissive="#39ffb0" emissiveIntensity={0.5} roughness={0.6} />
              </mesh>
            </group>
          </group>
        );
      })}
    </group>
  );
}
