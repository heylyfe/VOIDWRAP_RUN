"use client";

import { useRef } from "react";
import type { JSX } from "react";
import { useFrame } from "@react-three/fiber";
import type { Group, MeshStandardMaterial } from "three";
import { ENEMY_ATTACK, ENEMY_DORMANT, ENEMY_TELEGRAPH } from "@/protocol/generated";
import type { EnemyModelProps } from "../enemyLook";
import { prog01, yawToward } from "../enemyLook";

/**
 * Overlook — a perched, bone-pale eye high on a stalk, with a glowing mark hovering
 * above it. Accent #ecebe4 (bone) with a white-hot mark. Unit-radius model; the
 * parent scales by body radius. Idle: the head tracks the target and the mark bobs.
 * Telegraph (MARK): the iris flares as the sight line charges. Attack: the mark sears.
 */
export function Overlook({ anim }: EnemyModelProps): JSX.Element {
  const root = useRef<Group>(null);
  const inner = useRef<Group>(null);
  const head = useRef<Group>(null);
  const mark = useRef<Group>(null);
  const irisMat = useRef<MeshStandardMaterial>(null);
  const globeMat = useRef<MeshStandardMaterial>(null);
  const markMat = useRef<MeshStandardMaterial>(null);

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

    if (inner.current) {
      inner.current.rotation.z = Math.sin(t * 0.9) * 0.05;
      inner.current.rotation.x = Math.cos(t * 0.7) * 0.04;
    }
    if (head.current) {
      // The watcher always faces its quarry.
      head.current.rotation.y = yawToward(a.ex, a.ez, a.tx, a.tz);
    }
    if (globeMat.current) {
      globeMat.current.emissiveIntensity = (0.32 + Math.sin(t * 1.9) * 0.1) * dim;
    }
    if (irisMat.current) {
      irisMat.current.emissiveIntensity =
        (1.1 + Math.sin(t * 2.2) * 0.35 + (winding ? p * 3.2 : 0)) * dim;
    }
    if (mark.current) {
      mark.current.position.y = 1.44 + Math.sin(t * 1.8) * 0.06;
      mark.current.rotation.y = t * 1.6;
    }
    if (markMat.current) {
      markMat.current.emissiveIntensity =
        (1.25 + Math.sin(t * 3.1) * 0.5 + (winding ? p * 2.6 : 0)) * dim;
    }
  });

  return (
    <group ref={root}>
      <group ref={inner}>
        {/* clawed foot */}
        <mesh position={[0, -0.72, 0]}>
          <coneGeometry args={[0.3, 0.34, 10]} />
          <meshStandardMaterial color="#171410" roughness={0.7} metalness={0.3} />
        </mesh>
        {/* the stalk */}
        <mesh position={[0, -0.02, 0]}>
          <cylinderGeometry args={[0.07, 0.14, 1.5, 8]} />
          <meshStandardMaterial color="#1d1a16" roughness={0.75} metalness={0.15} />
        </mesh>
        {/* nest cup */}
        <mesh position={[0, 0.78, 0]} rotation={[Math.PI, 0, 0]}>
          <coneGeometry args={[0.3, 0.26, 10]} />
          <meshStandardMaterial color="#241f19" roughness={0.8} metalness={0.1} />
        </mesh>
        {/* the eye — high off the ground */}
        <group ref={head} position={[0, 1.0, 0]}>
          <mesh scale={[1.15, 0.8, 0.95]}>
            <sphereGeometry args={[0.34, 18, 14]} />
            <meshStandardMaterial
              ref={globeMat}
              color="#ecebe4"
              emissive="#ecebe4"
              emissiveIntensity={0.32}
              roughness={0.25}
              metalness={0.05}
            />
          </mesh>
          <mesh position={[0, 0, 0.24]} scale={[1, 1, 0.45]}>
            <sphereGeometry args={[0.17, 14, 10]} />
            <meshStandardMaterial
              ref={irisMat}
              color="#111111"
              emissive="#ffffff"
              emissiveIntensity={1.1}
              roughness={0.2}
              toneMapped={false}
            />
          </mesh>
          <mesh position={[0, 0, 0.315]}>
            <sphereGeometry args={[0.078, 10, 8]} />
            <meshStandardMaterial color="#050505" roughness={0.15} metalness={0.2} />
          </mesh>
        </group>
        {/* the glowing mark — a diamond sigil hovering over the eye */}
        <group ref={mark} position={[0, 1.44, 0]}>
          <mesh>
            <octahedronGeometry args={[0.09, 0]} />
            <meshStandardMaterial
              ref={markMat}
              color="#ffffff"
              emissive="#ffffff"
              emissiveIntensity={1.25}
              toneMapped={false}
            />
          </mesh>
        </group>
      </group>
    </group>
  );
}
