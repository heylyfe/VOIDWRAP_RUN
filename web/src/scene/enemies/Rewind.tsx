"use client";

import { useRef } from "react";
import type { JSX } from "react";
import { useFrame } from "@react-three/fiber";
import type { Group, MeshStandardMaterial } from "three";
import { ENEMY_ATTACK, ENEMY_DORMANT, ENEMY_TELEGRAPH } from "@/protocol/generated";
import type { EnemyModelProps } from "../enemyLook";
import { clamp01, prog01, yawToward } from "../enemyLook";

const ECHOS = 4;

/**
 * Rewind — a thin replay ghost: a translucent body trailing a short ribbon of its
 * own past positions (the echoes). Accent #46e0ff (temporal cyan). Unit-radius model;
 * the parent scales by body radius. Idle: slow bob and echo drift.
 * Telegraph (SNAP back): the body trembles and the echoes flare. Attack: snap-back lurch.
 */
export function Rewind({ anim }: EnemyModelProps): JSX.Element {
  const root = useRef<Group>(null);
  const inner = useRef<Group>(null);
  const bodyMat = useRef<MeshStandardMaterial>(null);
  const headMat = useRef<MeshStandardMaterial>(null);
  const echoRefs = useRef<(Group | null)[]>([]);
  const echoMats = useRef<(MeshStandardMaterial | null)[]>([]);

  useFrame((state) => {
    const r = root.current;
    if (!r || (r.parent !== null && !r.parent.visible)) return;
    const a = anim.current;
    const t = state.clock.elapsedTime;
    const dormant = a.state === ENEMY_DORMANT;
    const telegraph = a.state === ENEMY_TELEGRAPH;
    const attack = a.state === ENEMY_ATTACK;
    const p = prog01(a.prog);
    const dim = dormant ? 0.14 : 1;
    const winding = telegraph || attack;

    if (winding) {
      r.rotation.y = yawToward(a.ex, a.ez, a.tx, a.tz);
    }

    if (inner.current) {
      inner.current.position.x = telegraph ? Math.sin(t * 36) * 0.05 * (0.35 + p) : 0;
      inner.current.position.y = Math.sin(t * 1.9) * 0.07;
      inner.current.position.z = attack ? -Math.sin(clamp01(p) * Math.PI) * 0.45 : 0;
      inner.current.rotation.z = telegraph
        ? Math.sin(t * 41 + 0.7) * 0.06 * (0.3 + p)
        : Math.sin(t * 1.1) * 0.04;
    }
    const heat = 0.55 + Math.sin(t * 3.1) * 0.25 + (winding ? p * 2.3 : 0);
    if (bodyMat.current) bodyMat.current.emissiveIntensity = heat * dim;
    if (headMat.current) headMat.current.emissiveIntensity = (heat + 0.5) * dim;
    for (let i = 0; i < ECHOS; i += 1) {
      const g = echoRefs.current[i];
      const m = echoMats.current[i];
      const lag = i + 1;
      if (g) {
        const drift = Math.sin(t * 2.3 - lag * 0.9);
        g.position.z = -0.3 - lag * 0.21 - drift * 0.05;
        g.position.y = 0.15 + Math.sin(t * 2.3 - lag * 0.9 + 1.1) * 0.05;
        const s = 1 - lag * 0.16;
        g.scale.set(s * 0.75, s * 1.45, s * 0.75);
      }
      if (m) {
        m.opacity = clamp01((0.42 - lag * 0.09) * (winding ? 1 + p * 1.4 : 1)) * dim;
        m.emissiveIntensity = (0.9 + (winding ? p * 2 : 0)) * dim;
      }
    }
  });

  return (
    <group ref={root}>
      <group ref={inner}>
        {/* trunk — a tall tapered shade */}
        <mesh position={[0, 0.35, 0]}>
          <cylinderGeometry args={[0.15, 0.34, 1.5, 10]} />
          <meshStandardMaterial
            ref={bodyMat}
            color="#0e2a33"
            emissive="#46e0ff"
            emissiveIntensity={0.6}
            transparent
            opacity={0.62}
            roughness={0.3}
            metalness={0.05}
            toneMapped={false}
            depthWrite={false}
          />
        </mesh>
        {/* head */}
        <mesh position={[0, 1.18, 0]}>
          <sphereGeometry args={[0.17, 12, 10]} />
          <meshStandardMaterial
            ref={headMat}
            color="#12333d"
            emissive="#cdf6ff"
            emissiveIntensity={1.1}
            transparent
            opacity={0.7}
            roughness={0.25}
            toneMapped={false}
            depthWrite={false}
          />
        </mesh>
        {/* tattered tail */}
        <mesh position={[0, -0.62, 0]} rotation={[Math.PI, 0, 0]}>
          <coneGeometry args={[0.3, 0.75, 10]} />
          <meshStandardMaterial
            color="#0e2a33"
            emissive="#46e0ff"
            emissiveIntensity={0.45}
            transparent
            opacity={0.5}
            roughness={0.4}
            toneMapped={false}
            depthWrite={false}
          />
        </mesh>
        {/* ribbon of past positions — trailing echoes */}
        {Array.from({ length: ECHOS }, (_, i) => (
          <group
            key={i}
            ref={(g) => {
              echoRefs.current[i] = g;
            }}
            position={[0, 0.15, -0.3 - (i + 1) * 0.21]}
          >
            <mesh>
              <octahedronGeometry args={[0.17, 0]} />
              <meshStandardMaterial
                ref={(m) => {
                  echoMats.current[i] = m;
                }}
                color="#12333d"
                emissive="#46e0ff"
                emissiveIntensity={0.9}
                transparent
                opacity={0.33}
                roughness={0.4}
                toneMapped={false}
                depthWrite={false}
              />
            </mesh>
          </group>
        ))}
      </group>
    </group>
  );
}
