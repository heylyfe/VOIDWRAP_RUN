"use client";

import { useRef } from "react";
import type { JSX } from "react";
import { useFrame } from "@react-three/fiber";
import type { Group, Mesh, MeshStandardMaterial } from "three";
import { ENEMY_ATTACK, ENEMY_DORMANT, ENEMY_TELEGRAPH } from "@/protocol/generated";
import type { EnemyModelProps } from "../enemyLook";
import { prog01 } from "../enemyLook";

interface Puff {
  readonly radius: number;
  readonly size: number;
  readonly phase: number;
  readonly speed: number;
}

const PUFFS: readonly Puff[] = [
  { radius: 0.46, size: 0.5, phase: 0.0, speed: 0.24 },
  { radius: 0.55, size: 0.42, phase: 1.7, speed: -0.18 },
  { radius: 0.38, size: 0.36, phase: 3.1, speed: 0.31 },
  { radius: 0.62, size: 0.3, phase: 4.6, speed: -0.27 },
];

/**
 * Static — a low fog-grey lump that is nearly invisible; the fog patch is the read.
 * Accent #a8b8c8 (cold steel grey). Unit-radius model; the parent scales by body radius.
 * Cue tier 3 (ambient): no telegraph; the fog slowly churns and pinprick eyes
 * brighten while it is tracking. Attack: fog thickens and the eyes flare.
 */
export function Static({ anim }: EnemyModelProps): JSX.Element {
  const root = useRef<Group>(null);
  const lump = useRef<Mesh>(null);
  const puffRefs = useRef<(Group | null)[]>([]);
  const puffMats = useRef<(MeshStandardMaterial | null)[]>([]);
  const slitMats = useRef<(MeshStandardMaterial | null)[]>([]);

  useFrame((state) => {
    const r = root.current;
    if (!r || (r.parent !== null && !r.parent.visible)) return;
    const a = anim.current;
    const t = state.clock.elapsedTime;
    const dormant = a.state === ENEMY_DORMANT;
    const agitated = a.state === ENEMY_TELEGRAPH || a.state === ENEMY_ATTACK;
    const p = prog01(a.prog);
    const dim = dormant ? 0.1 : 1;
    const fogGain = agitated ? 1 + p * 0.9 : 1;

    if (lump.current) {
      lump.current.scale.set(
        1.15 + Math.sin(t * 0.9) * 0.03,
        0.6 + Math.sin(t * 1.3 + 1) * 0.03,
        1.0 + Math.cos(t * 0.8) * 0.03,
      );
    }
    for (let i = 0; i < PUFFS.length; i += 1) {
      const puff = PUFFS[i];
      if (!puff) continue;
      const g = puffRefs.current[i];
      const m = puffMats.current[i];
      const angle = t * puff.speed + puff.phase;
      if (g) {
        g.position.set(
          Math.cos(angle) * puff.radius,
          -0.3 + Math.sin(t * 0.8 + puff.phase * 2) * 0.05,
          Math.sin(angle) * puff.radius,
        );
      }
      if (m) {
        m.opacity = (0.16 + 0.07 * Math.sin(t * 1.3 + puff.phase * 1.9)) * dim * fogGain;
      }
    }
    for (const m of slitMats.current) {
      if (m) m.emissiveIntensity = ((agitated ? 1.6 + p * 1.6 : 0.55) * dim);
    }
  });

  return (
    <group ref={root}>
      {/* the lump — barely there */}
      <mesh ref={lump} position={[0, -0.35, 0]} scale={[1.15, 0.6, 1]}>
        <sphereGeometry args={[0.55, 12, 9]} />
        <meshStandardMaterial
          color="#33383f"
          emissive="#a8b8c8"
          emissiveIntensity={0.12}
          roughness={0.95}
          metalness={0.02}
        />
      </mesh>
      <mesh position={[0.42, -0.42, 0.18]} scale={[1, 0.7, 0.9]}>
        <sphereGeometry args={[0.34, 10, 8]} />
        <meshStandardMaterial
          color="#2b3036"
          emissive="#a8b8c8"
          emissiveIntensity={0.08}
          roughness={0.97}
          metalness={0.02}
        />
      </mesh>
      {/* pinprick eyes — the only sure tell */}
      {[0.16, -0.16].map((x, i) => (
        <mesh key={i} position={[x, -0.28, 0.43]}>
          <sphereGeometry args={[0.035, 8, 6]} />
          <meshStandardMaterial
            ref={(m) => {
              slitMats.current[i] = m;
            }}
            color="#dde3ea"
            emissive="#dde3ea"
            emissiveIntensity={0.55}
            toneMapped={false}
          />
        </mesh>
      ))}
      {/* the fog patch — the real silhouette */}
      {PUFFS.map((puff, i) => (
        <group
          key={i}
          ref={(g) => {
            puffRefs.current[i] = g;
          }}
          position={[Math.cos(puff.phase) * puff.radius, -0.3, Math.sin(puff.phase) * puff.radius]}
        >
          <mesh scale={[1.35, 0.65, 1.35]}>
            <sphereGeometry args={[puff.size, 10, 8]} />
            <meshStandardMaterial
              ref={(m) => {
                puffMats.current[i] = m;
              }}
              color="#aeb6bf"
              emissive="#dde3ea"
              emissiveIntensity={0.25}
              transparent
              opacity={0.17}
              roughness={1}
              metalness={0}
              depthWrite={false}
            />
          </mesh>
        </group>
      ))}
    </group>
  );
}
