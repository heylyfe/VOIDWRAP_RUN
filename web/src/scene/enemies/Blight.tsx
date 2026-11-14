"use client";

import { useRef } from "react";
import type { JSX } from "react";
import { useFrame } from "@react-three/fiber";
import type { Group, Mesh, MeshStandardMaterial } from "three";
import { ENEMY_ATTACK, ENEMY_DORMANT, ENEMY_TELEGRAPH } from "@/protocol/generated";
import type { EnemyModelProps } from "../enemyLook";
import { clamp01, prog01 } from "../enemyLook";

const SORES: readonly { readonly x: number; readonly y: number; readonly z: number }[] = [
  { x: 0.28, y: -0.02, z: 0.4 },
  { x: -0.33, y: -0.05, z: 0.34 },
  { x: 0.06, y: 0.3, z: 0.28 },
  { x: -0.08, y: 0.05, z: 0.47 },
];

/**
 * Blight — a wet, lumpy mass with a spreading discoloured skirt of infected ground.
 * Accent #a3c34a (sickly green). Unit-radius model; the parent scales by body
 * radius. Cue tier 3 (ambient): no telegraph; the skirt creeps and the sores pulse.
 * Attack: the mound heaves and the skirt surges outward.
 */
export function Blight({ anim }: EnemyModelProps): JSX.Element {
  const root = useRef<Group>(null);
  const mound = useRef<Group>(null);
  const skirt = useRef<Mesh>(null);
  const moundMat = useRef<MeshStandardMaterial>(null);
  const skirtMat = useRef<MeshStandardMaterial>(null);
  const soreMats = useRef<(MeshStandardMaterial | null)[]>([]);

  useFrame((state) => {
    const r = root.current;
    if (!r || (r.parent !== null && !r.parent.visible)) return;
    const a = anim.current;
    const t = state.clock.elapsedTime;
    const dormant = a.state === ENEMY_DORMANT;
    const telegraph = a.state === ENEMY_TELEGRAPH;
    const attack = a.state === ENEMY_ATTACK;
    const p = prog01(a.prog);
    const dim = dormant ? 0.16 : 1;
    const winding = telegraph || attack;

    if (mound.current) {
      mound.current.scale.set(
        1 + Math.sin(t * 1.1 + 0.6) * 0.035,
        1 + Math.sin(t * 1.1) * 0.045,
        1 + Math.sin(t * 1.1 + 1.2) * 0.035,
      );
      mound.current.rotation.x = winding ? -0.18 * clamp01(p) : 0;
    }
    if (skirt.current) {
      const creep = 0.72 + (0.5 + 0.5 * Math.sin(t * 0.6)) * 0.3 + (winding ? p * 0.38 : 0);
      skirt.current.scale.set(creep, 1, creep);
    }
    if (moundMat.current) {
      moundMat.current.emissiveIntensity = (0.22 + 0.12 * Math.sin(t * 1.5)) * dim;
    }
    if (skirtMat.current) {
      skirtMat.current.emissiveIntensity =
        (0.32 + 0.2 * Math.sin(t * 1.7 + 0.9) + (winding ? p * 1.1 : 0)) * dim;
    }
    for (let i = 0; i < SORES.length; i += 1) {
      const m = soreMats.current[i];
      if (m) {
        m.emissiveIntensity =
          (0.75 + Math.sin(t * 2.4 + i * 2.1) * 0.4 + (winding ? p * 2 : 0)) * dim;
      }
    }
  });

  return (
    <group ref={root}>
      {/* spreading discoloured skirt — the infected floor read */}
      <mesh ref={skirt} position={[0, -0.55, 0]}>
        <cylinderGeometry args={[0.78, 1.02, 0.07, 18]} />
        <meshStandardMaterial
          ref={skirtMat}
          color="#232b12"
          emissive="#a3c34a"
          emissiveIntensity={0.35}
          transparent
          opacity={0.92}
          roughness={0.85}
          metalness={0}
        />
      </mesh>
      {/* wet lumpy mound */}
      <group ref={mound}>
        <mesh position={[0, -0.25, 0]} scale={[1.1, 0.75, 1]}>
          <sphereGeometry args={[0.5, 14, 10]} />
          <meshStandardMaterial
            ref={moundMat}
            color="#222a16"
            emissive="#a3c34a"
            emissiveIntensity={0.22}
            roughness={0.16}
            metalness={0.05}
          />
        </mesh>
        <mesh position={[0.36, -0.08, 0.2]}>
          <sphereGeometry args={[0.3, 10, 8]} />
          <meshStandardMaterial color="#1f2614" emissive="#a3c34a" emissiveIntensity={0.18} roughness={0.14} metalness={0.05} />
        </mesh>
        <mesh position={[-0.34, -0.1, 0.26]}>
          <sphereGeometry args={[0.26, 10, 8]} />
          <meshStandardMaterial color="#252d18" emissive="#a3c34a" emissiveIntensity={0.18} roughness={0.14} metalness={0.05} />
        </mesh>
        <mesh position={[0.05, 0.14, -0.3]}>
          <sphereGeometry args={[0.3, 10, 8]} />
          <meshStandardMaterial color="#1e2413" emissive="#a3c34a" emissiveIntensity={0.2} roughness={0.16} metalness={0.05} />
        </mesh>
        <mesh position={[-0.12, 0.28, 0.12]}>
          <sphereGeometry args={[0.2, 9, 8]} />
          <meshStandardMaterial color="#28301a" emissive="#a3c34a" emissiveIntensity={0.2} roughness={0.15} metalness={0.05} />
        </mesh>
        {/* sores — sickly embers under the skin */}
        {SORES.map((s, i) => (
          <mesh key={i} position={[s.x, s.y, s.z]}>
            <sphereGeometry args={[0.07, 8, 6]} />
            <meshStandardMaterial
              ref={(m) => {
                soreMats.current[i] = m;
              }}
              color="#39441c"
              emissive="#d9e89b"
              emissiveIntensity={0.75}
              toneMapped={false}
            />
          </mesh>
        ))}
      </group>
    </group>
  );
}
