"use client";

import { useRef } from "react";
import type { JSX } from "react";
import { useFrame } from "@react-three/fiber";
import type { Group, Mesh, MeshStandardMaterial } from "three";
import { DoubleSide } from "three";
import { ENEMY_ATTACK, ENEMY_DORMANT, ENEMY_TELEGRAPH } from "@/protocol/generated";
import type { EnemyModelProps } from "../enemyLook";
import { clamp01, prog01, yawToward } from "../enemyLook";

const SHELL_H = 1.15;

/**
 * Peal — a bronze bell whose two shell halves swing open at the lip as the ring
 * charges. Accent #e63946 (bell-mouth red). Unit-radius model; the parent scales by
 * body radius. Idle: slow sway. Telegraph (PEAL): lip opens and the mouth glows.
 * Attack (the ring): the shell clangs through a full open-shut arc.
 */
export function Peal({ anim }: EnemyModelProps): JSX.Element {
  const root = useRef<Group>(null);
  const inner = useRef<Group>(null);
  const leftHalf = useRef<Group>(null);
  const rightHalf = useRef<Group>(null);
  const clapper = useRef<Group>(null);
  const lipMesh = useRef<Mesh>(null);
  const lipMat = useRef<MeshStandardMaterial>(null);
  const glowMat = useRef<MeshStandardMaterial>(null);
  const shellMats = useRef<(MeshStandardMaterial | null)[]>([]);

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

    // How open the mouth is: telegraph winds it open; attack clangs open→shut.
    const open = telegraph
      ? p
      : attack
        ? Math.sin(clamp01(p) * Math.PI)
        : 0;

    if (leftHalf.current) leftHalf.current.rotation.z = -open * 0.42;
    if (rightHalf.current) rightHalf.current.rotation.z = open * 0.42;
    if (inner.current) {
      inner.current.position.y = Math.sin(t * 1.7) * 0.06;
      inner.current.rotation.x = winding ? -p * 0.22 : 0;
      inner.current.rotation.z = Math.sin(t * 0.8) * 0.03;
    }
    if (clapper.current) {
      clapper.current.rotation.z = Math.sin(t * (winding ? 11 : 1.6)) *
        (winding ? 0.35 + p * 0.75 : 0.12);
    }
    if (lipMesh.current) {
      const s = 1 + open * 0.18;
      lipMesh.current.scale.set(s, s, 1);
    }
    if (lipMat.current) {
      lipMat.current.emissiveIntensity =
        (0.9 + Math.sin(t * 2.6) * 0.25 + open * 3.2) * dim;
    }
    if (glowMat.current) {
      glowMat.current.opacity = (0.18 + open * 0.72) * dim;
      glowMat.current.emissiveIntensity = (1 + open * 2.6) * dim;
    }
    for (const m of shellMats.current) {
      if (m) m.emissiveIntensity = (0.18 + open * 1.1) * dim;
    }
  });

  const shellMaterial = (slot: number) => (
    <meshStandardMaterial
      ref={(m) => {
        shellMats.current[slot] = m;
      }}
      color="#2a2118"
      emissive="#e63946"
      emissiveIntensity={0.18}
      roughness={0.35}
      metalness={0.75}
      side={DoubleSide}
    />
  );

  return (
    <group ref={root}>
      <group ref={inner}>
        {/* crown */}
        <mesh position={[0, 0.66, 0]}>
          <cylinderGeometry args={[0.09, 0.14, 0.22, 10]} />
          <meshStandardMaterial color="#3a2c1c" roughness={0.4} metalness={0.8} />
        </mesh>
        <mesh position={[0, 0.82, 0]}>
          <sphereGeometry args={[0.085, 10, 8]} />
          <meshStandardMaterial color="#4a3823" roughness={0.35} metalness={0.85} />
        </mesh>
        {/* two shell halves, pivoting at the crown so the lip splays open */}
        <group ref={leftHalf} position={[0, 0.56, 0]}>
          <mesh position={[0, -SHELL_H / 2, 0]}>
            <cylinderGeometry args={[0.24, 0.62, SHELL_H, 12, 1, true, 0, Math.PI]} />
            {shellMaterial(0)}
          </mesh>
        </group>
        <group ref={rightHalf} position={[0, 0.56, 0]}>
          <mesh position={[0, -SHELL_H / 2, 0]}>
            <cylinderGeometry args={[0.24, 0.62, SHELL_H, 12, 1, true, Math.PI, Math.PI]} />
            {shellMaterial(1)}
          </mesh>
        </group>
        {/* lip ring — the mouth rim */}
        <mesh ref={lipMesh} position={[0, 0.56 - SHELL_H, 0]} rotation={[Math.PI / 2, 0, 0]}>
          <torusGeometry args={[0.62, 0.075, 10, 24]} />
          <meshStandardMaterial
            ref={lipMat}
            color="#33241a"
            emissive="#e63946"
            emissiveIntensity={0.9}
            roughness={0.3}
            metalness={0.8}
            toneMapped={false}
          />
        </mesh>
        {/* inner mouth glow */}
        <mesh position={[0, 0.56 - SHELL_H + 0.02, 0]} rotation={[Math.PI / 2, 0, 0]}>
          <circleGeometry args={[0.5, 20]} />
          <meshStandardMaterial
            ref={glowMat}
            color="#1a0505"
            emissive="#ffb0a8"
            emissiveIntensity={1}
            transparent
            opacity={0.2}
            side={DoubleSide}
            toneMapped={false}
            depthWrite={false}
          />
        </mesh>
        {/* clapper */}
        <group ref={clapper} position={[0, 0.45, 0]}>
          <mesh position={[0, -0.375, 0]}>
            <cylinderGeometry args={[0.035, 0.035, 0.75, 6]} />
            <meshStandardMaterial color="#20160e" roughness={0.45} metalness={0.7} />
          </mesh>
          <mesh position={[0, -0.78, 0]}>
            <sphereGeometry args={[0.14, 10, 8]} />
            <meshStandardMaterial color="#241a10" roughness={0.4} metalness={0.75} />
          </mesh>
        </group>
      </group>
    </group>
  );
}
