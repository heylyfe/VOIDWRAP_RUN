"use client";

import { useRef } from "react";
import type { JSX } from "react";
import { useFrame } from "@react-three/fiber";
import type { Group, MeshStandardMaterial } from "three";
import { ENEMY_ATTACK, ENEMY_DORMANT, ENEMY_TELEGRAPH } from "@/protocol/generated";
import type { EnemyModelProps } from "../enemyLook";
import { clamp01, prog01, yawToward } from "../enemyLook";

const TETHER_BASE_TILT = 0.28;

/**
 * Kite — a tall thin azure spindle with a taut tether line running from its tip.
 * Accent #35a7ff (azure). Unit-radius model; the parent scales by body radius.
 * Idle: lazy sway. Telegraph (TETHER): rears back against the line, which hums and
 * brightens. Attack (SNAP): lurches forward down the line.
 */
export function Kite({ anim }: EnemyModelProps): JSX.Element {
  const root = useRef<Group>(null);
  const inner = useRef<Group>(null);
  const tether = useRef<Group>(null);
  const bodyMat = useRef<MeshStandardMaterial>(null);
  const tipMat = useRef<MeshStandardMaterial>(null);
  const tetherMat = useRef<MeshStandardMaterial>(null);
  const knotMat = useRef<MeshStandardMaterial>(null);

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

    if (winding) {
      r.rotation.y = yawToward(a.ex, a.ez, a.tx, a.tz);
    }

    if (inner.current) {
      inner.current.position.y = Math.sin(t * 1.6) * 0.06;
      if (telegraph) {
        inner.current.rotation.x = -0.35 * p; // leaning back, line under load
      } else if (attack) {
        inner.current.rotation.x = 0.5 * Math.sin(clamp01(p) * Math.PI);
      } else {
        inner.current.rotation.x = 0;
      }
      inner.current.rotation.z = Math.sin(t * 1.25) * 0.09;
    }
    if (tether.current) {
      tether.current.rotation.x = TETHER_BASE_TILT +
        (telegraph ? Math.sin(t * 32) * 0.06 * (0.3 + p) : 0);
    }
    if (bodyMat.current) {
      bodyMat.current.emissiveIntensity =
        (0.7 + Math.sin(t * 2.8) * 0.2 + (winding ? p * 2.4 : 0)) * dim;
    }
    if (tipMat.current) {
      tipMat.current.emissiveIntensity = (1.1 + (winding ? p * 2.6 : 0)) * dim;
    }
    if (tetherMat.current) {
      tetherMat.current.emissiveIntensity =
        (0.8 + Math.sin(t * 9) * 0.3 + (winding ? p * 3.4 : 0)) * dim;
    }
    if (knotMat.current) {
      knotMat.current.emissiveIntensity = (1 + (winding ? p * 3 : 0)) * dim;
    }
  });

  return (
    <group ref={root}>
      <group ref={inner}>
        {/* spindle body — a stretched octahedron */}
        <mesh position={[0, 0.35, 0]} scale={[0.85, 2.3, 0.85]}>
          <octahedronGeometry args={[0.2, 0]} />
          <meshStandardMaterial
            ref={bodyMat}
            color="#10222e"
            emissive="#35a7ff"
            emissiveIntensity={0.7}
            roughness={0.3}
            metalness={0.55}
            toneMapped={false}
          />
        </mesh>
        {/* swept side fins */}
        {[1, -1].map((side) => (
          <mesh
            key={side}
            position={[side * 0.3, 0.32, 0]}
            rotation={[0, 0, side * 0.5]}
          >
            <boxGeometry args={[0.5, 0.02, 0.16]} />
            <meshStandardMaterial color="#16293a" roughness={0.4} metalness={0.6} />
          </mesh>
        ))}
        {/* tail streamer */}
        <mesh position={[0, -0.3, 0]}>
          <coneGeometry args={[0.08, 0.45, 6]} />
          <meshStandardMaterial
            color="#16293a"
            emissive="#35a7ff"
            emissiveIntensity={0.4}
            roughness={0.4}
            metalness={0.5}
            toneMapped={false}
          />
        </mesh>
        {/* tip orb */}
        <mesh position={[0, 0.86, 0]}>
          <sphereGeometry args={[0.09, 10, 8]} />
          <meshStandardMaterial
            ref={tipMat}
            color="#bdd9ff"
            emissive="#bdd9ff"
            emissiveIntensity={1.1}
            toneMapped={false}
          />
        </mesh>
        {/* taut tether from the tip, angled back and up */}
        <group ref={tether} position={[0, 0.9, 0]} rotation={[TETHER_BASE_TILT, 0, 0]}>
          <mesh position={[0, 0.66, 0]}>
            <cylinderGeometry args={[0.018, 0.018, 1.35, 5]} />
            <meshStandardMaterial
              ref={tetherMat}
              color="#0d1a24"
              emissive="#35a7ff"
              emissiveIntensity={0.8}
              roughness={0.5}
              toneMapped={false}
            />
          </mesh>
          <mesh position={[0, 1.36, 0]}>
            <sphereGeometry args={[0.05, 8, 6]} />
            <meshStandardMaterial
              ref={knotMat}
              color="#bdd9ff"
              emissive="#bdd9ff"
              emissiveIntensity={1}
              toneMapped={false}
            />
          </mesh>
        </group>
      </group>
    </group>
  );
}
