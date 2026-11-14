"use client";

import { useRef } from "react";
import type { JSX } from "react";
import { useFrame } from "@react-three/fiber";
import type { Group, MeshStandardMaterial } from "three";
import { ENEMY_ATTACK, ENEMY_DORMANT, ENEMY_TELEGRAPH } from "@/protocol/generated";
import type { EnemyModelProps } from "../enemyLook";
import { clamp01, prog01 } from "../enemyLook";

/** Swing rate of the arm, rad/s. */
const BEAT = 2.6;

/**
 * Metronome — an upright brass arm on a pyramid plinth; the arm sweeps the floor's
 * safe half back and forth. Accent #c98f3d (warm brass). Unit-radius model; the
 * parent scales by body radius. Idle: slow, drowsy swing. Telegraph (the 0.5 s
 * warning): the weight and tip flash white-hot. Attack: a hard downbeat thud.
 */
export function Metronome({ anim }: EnemyModelProps): JSX.Element {
  const root = useRef<Group>(null);
  const inner = useRef<Group>(null);
  const armPivot = useRef<Group>(null);
  const armMat = useRef<MeshStandardMaterial>(null);
  const flashMats = useRef<(MeshStandardMaterial | null)[]>([]);

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
    const beat = dormant ? 0.9 : BEAT;

    const swing = Math.sin(t * beat);
    if (armPivot.current) {
      armPivot.current.rotation.z = swing * (dormant ? 0.28 : 0.55);
    }
    if (inner.current) {
      inner.current.position.y = attack ? -Math.sin(clamp01(p) * Math.PI) * 0.05 : 0;
      inner.current.rotation.y = Math.sin(t * 0.6) * 0.05;
    }
    const beatPulse = Math.pow(Math.abs(Math.sin(t * beat)), 8) * 1.5;
    for (const m of flashMats.current) {
      if (m) {
        m.emissiveIntensity = winding
          ? (1.4 + p * 3.4) * dim
          : (0.75 + beatPulse) * dim;
      }
    }
    if (armMat.current) {
      armMat.current.emissiveIntensity =
        (0.5 + (winding ? p * 1.8 : beatPulse * 0.4)) * dim;
    }
  });

  return (
    <group ref={root}>
      <group ref={inner}>
        {/* base slab */}
        <mesh position={[0, -0.75, 0]}>
          <boxGeometry args={[0.95, 0.12, 0.95]} />
          <meshStandardMaterial color="#1a1410" roughness={0.5} metalness={0.3} />
        </mesh>
        {/* pyramid plinth */}
        <mesh position={[0, -0.3, 0]}>
          <coneGeometry args={[0.55, 0.85, 4]} />
          <meshStandardMaterial color="#241a12" roughness={0.55} metalness={0.15} />
        </mesh>
        {/* pivoting arm */}
        <group ref={armPivot} position={[0, -0.05, 0]}>
          {/* arm */}
          <mesh position={[0, 0.65, 0]}>
            <boxGeometry args={[0.07, 1.35, 0.05]} />
            <meshStandardMaterial
              ref={armMat}
              color="#8a6a2f"
              emissive="#c98f3d"
              emissiveIntensity={0.5}
              roughness={0.35}
              metalness={0.85}
              toneMapped={false}
            />
          </mesh>
          {/* sliding weight */}
          <mesh position={[0, 0.95, 0]}>
            <boxGeometry args={[0.17, 0.17, 0.09]} />
            <meshStandardMaterial
              ref={(m) => {
                flashMats.current[0] = m;
              }}
              color="#a87f3a"
              emissive="#ffe08a"
              emissiveIntensity={0.75}
              roughness={0.3}
              metalness={0.85}
              toneMapped={false}
            />
          </mesh>
          {/* tip */}
          <mesh position={[0, 1.36, 0]}>
            <sphereGeometry args={[0.06, 8, 8]} />
            <meshStandardMaterial
              ref={(m) => {
                flashMats.current[1] = m;
              }}
              color="#ffe08a"
              emissive="#ffe08a"
              emissiveIntensity={0.75}
              toneMapped={false}
            />
          </mesh>
        </group>
      </group>
    </group>
  );
}
