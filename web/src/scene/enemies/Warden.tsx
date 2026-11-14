"use client";

import { useRef } from "react";
import type { JSX } from "react";
import { useFrame } from "@react-three/fiber";
import type { Group, MeshStandardMaterial } from "three";
import { ENEMY_ATTACK, ENEMY_DORMANT, ENEMY_TELEGRAPH } from "@/protocol/generated";
import type { EnemyModelProps } from "../enemyLook";
import { prog01, yawToward } from "../enemyLook";

/**
 * Warden — a squat grave-post with a wide flat guard ring at its base; the ring IS
 * the model. Accent #f4d35e (ward gold). Unit-radius model; the parent scales by
 * body radius. Idle: the ring breathes. Telegraph (GUARD): ring flares and swells.
 * Attack: rears back as the ward shoves.
 */
export function Warden({ anim }: EnemyModelProps): JSX.Element {
  const root = useRef<Group>(null);
  const inner = useRef<Group>(null);
  const ring = useRef<Group>(null);
  const ringMat = useRef<MeshStandardMaterial>(null);
  const eyeMat = useRef<MeshStandardMaterial>(null);

  useFrame((state) => {
    const r = root.current;
    if (!r || (r.parent !== null && !r.parent.visible)) return;
    const a = anim.current;
    const t = state.clock.elapsedTime;
    const dormant = a.state === ENEMY_DORMANT;
    const telegraph = a.state === ENEMY_TELEGRAPH;
    const attack = a.state === ENEMY_ATTACK;
    const p = prog01(a.prog);
    const dim = dormant ? 0.18 : 1;
    const winding = telegraph || attack;

    if (winding) {
      r.rotation.y = yawToward(a.ex, a.ez, a.tx, a.tz);
    }

    if (inner.current) {
      inner.current.position.y = Math.sin(t * 1.4) * 0.05;
      inner.current.rotation.x = attack ? -p * 0.16 : 0;
      inner.current.rotation.z = Math.sin(t * 0.9) * 0.02;
    }
    if (ring.current) {
      const swell = winding
        ? 1 + p * 0.18 + Math.sin(t * 9) * 0.04 * p
        : 1 + Math.sin(t * 2.2) * 0.02;
      ring.current.scale.set(swell, 1, swell);
    }
    if (ringMat.current) {
      ringMat.current.emissiveIntensity =
        (1.15 + Math.sin(t * 2.2) * 0.35 + (winding ? p * 2.6 : 0)) * dim;
    }
    if (eyeMat.current) {
      eyeMat.current.emissiveIntensity =
        ((dormant ? 0.35 : 1.4) + (winding ? p * 2 : 0)) * dim;
    }
  });

  return (
    <group ref={root}>
      <group ref={inner}>
        {/* squat post */}
        <mesh position={[0, 0.05, 0]}>
          <cylinderGeometry args={[0.32, 0.44, 1.1, 12]} />
          <meshStandardMaterial color="#1c1a17" roughness={0.62} metalness={0.45} />
        </mesh>
        {/* foot */}
        <mesh position={[0, -0.5, 0]}>
          <cylinderGeometry args={[0.58, 0.62, 0.12, 16]} />
          <meshStandardMaterial color="#141311" roughness={0.55} metalness={0.5} />
        </mesh>
        {/* cap + finial */}
        <mesh position={[0, 0.7, 0]}>
          <coneGeometry args={[0.3, 0.3, 12]} />
          <meshStandardMaterial color="#26231f" roughness={0.45} metalness={0.6} />
        </mesh>
        <mesh position={[0, 0.9, 0]}>
          <sphereGeometry args={[0.08, 10, 8]} />
          <meshStandardMaterial
            ref={eyeMat}
            color="#f4d35e"
            emissive="#f4d35e"
            emissiveIntensity={1.4}
            toneMapped={false}
          />
        </mesh>
        {/* slit eye — cold sentry light */}
        <mesh position={[0, 0.34, 0.33]}>
          <boxGeometry args={[0.34, 0.045, 0.03]} />
          <meshStandardMaterial
            color="#2a2410"
            emissive="#fdf3b8"
            emissiveIntensity={1.1}
            toneMapped={false}
          />
        </mesh>
        {/* the guard ring — wide, flat, at the base */}
        <group ref={ring} position={[0, -0.44, 0]}>
          <mesh rotation={[Math.PI / 2, 0, 0]}>
            <torusGeometry args={[0.82, 0.085, 10, 28]} />
            <meshStandardMaterial
              ref={ringMat}
              color="#3a2f10"
              emissive="#f4d35e"
              emissiveIntensity={1.15}
              roughness={0.35}
              metalness={0.7}
              toneMapped={false}
            />
          </mesh>
        </group>
      </group>
    </group>
  );
}
