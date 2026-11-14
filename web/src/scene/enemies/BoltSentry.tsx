"use client";

import { useRef } from "react";
import type { JSX } from "react";
import { useFrame } from "@react-three/fiber";
import type { Group, Mesh, MeshBasicMaterial, MeshStandardMaterial } from "three";
import { ENEMY_ATTACK, ENEMY_DORMANT, ENEMY_RECOVER, ENEMY_TELEGRAPH } from "@/protocol/generated";
import type { EnemyModelProps } from "../enemyLook";
import { prog01, yawToward } from "../enemyLook";

/**
 * Bolt Sentry — tripod brass mortar, single ocular lens, coiled springs, steam wisps.
 * Accent #ff8a3c (ember-orange), gold core #ffd07a. Unit-radius model.
 * Stationary; idle irises, attack is a white-hot lens flare + recoil kick.
 */
export function BoltSentry({ anim }: EnemyModelProps): JSX.Element {
  const root = useRef<Group>(null);
  const recoil = useRef<Group>(null);
  const lens = useRef<Mesh>(null);
  const lensMat = useRef<MeshStandardMaterial>(null);
  const wispA = useRef<Mesh>(null);
  const wispB = useRef<Mesh>(null);
  const wispMatA = useRef<MeshBasicMaterial>(null);
  const wispMatB = useRef<MeshBasicMaterial>(null);

  useFrame((state) => {
    const r = root.current;
    if (!r || (r.parent !== null && !r.parent.visible)) return;
    const a = anim.current;
    const t = state.clock.elapsedTime;
    const dormant = a.state === ENEMY_DORMANT;
    const telegraph = a.state === ENEMY_TELEGRAPH;
    const attack = a.state === ENEMY_ATTACK;
    const venting = a.state === ENEMY_RECOVER;
    const p = prog01(a.prog);
    const dim = dormant ? 0.15 : 1;

    if (telegraph || attack) {
      r.rotation.y = yawToward(a.ex, a.ez, a.tx, a.tz);
    } else if (!dormant) {
      r.rotation.y = Math.sin(t * 0.6) * 0.22; // slow scan
    }

    if (recoil.current) {
      const shudder = telegraph ? Math.sin(t * 55) * 0.008 * p : 0;
      recoil.current.position.z = attack ? -0.17 * Math.max(0, 1 - p * 2.2) : shudder;
    }
    if (lens.current) {
      lens.current.scale.setScalar(telegraph ? 1 + p * 0.6 : attack ? 1.55 : 1);
    }
    if (lensMat.current) {
      lensMat.current.emissiveIntensity = dormant
        ? 0.3
        : attack
          ? 5.2 * Math.max(0, 1 - p * 1.6) + 1
          : telegraph
            ? 1.25 + p * 3.6
            : 1.15 + Math.sin(t * 2.3) * 0.35;
    }
    // steam wisps — faster while venting after a shot
    const wispGain = venting ? 1.8 : 1;
    const wa = Math.max(0, Math.sin(t * 1.7)) * 0.55 * wispGain * dim;
    const wb = Math.max(0, Math.sin(t * 1.7 + 1.9)) * 0.45 * wispGain * dim;
    if (wispMatA.current) wispMatA.current.opacity = wa;
    if (wispMatB.current) wispMatB.current.opacity = wb;
    if (wispA.current) wispA.current.scale.setScalar(0.6 + Math.max(0, Math.sin(t * 1.7)) * 1.1);
    if (wispB.current) wispB.current.scale.setScalar(0.5 + Math.max(0, Math.sin(t * 1.7 + 1.9)) * 0.9);
  });

  const legAngles = [0, (Math.PI * 2) / 3, (Math.PI * 4) / 3];

  return (
    <group ref={root}>
      {/* tripod legs — splayed brass cones */}
      {legAngles.map((ang) => (
        <group key={ang} rotation={[0, ang, 0]}>
          <mesh position={[0.34, -0.62, 0]} rotation={[0, 0, 0.45]}>
            <coneGeometry args={[0.09, 1.1, 6]} />
            <meshStandardMaterial color="#5e4a2c" roughness={0.4} metalness={0.8} />
          </mesh>
        </group>
      ))}
      {/* hub */}
      <mesh position={[0, -0.12, 0]}>
        <cylinderGeometry args={[0.17, 0.21, 0.42, 10]} />
        <meshStandardMaterial color="#5e4a2c" roughness={0.35} metalness={0.85} />
      </mesh>
      {/* barrel group — recoils on FIRE */}
      <group ref={recoil} position={[0, 0.22, 0]} rotation={[0.55, 0, 0]}>
        {/* coiled springs at the barrel base */}
        {[0.02, 0.16, 0.3].map((y, i) => (
          <mesh key={i} position={[0, y, 0]}>
            <torusGeometry args={[0.19 - i * 0.015, 0.032, 6, 14]} />
            <meshStandardMaterial color="#3c3c46" roughness={0.35} metalness={0.9} />
          </mesh>
        ))}
        {/* mortar barrel */}
        <mesh position={[0, 0.28, 0]}>
          <cylinderGeometry args={[0.16, 0.24, 0.72, 12]} />
          <meshStandardMaterial color="#6e5230" roughness={0.3} metalness={0.88} />
        </mesh>
        {/* muzzle rim */}
        <mesh position={[0, 0.66, 0]}>
          <torusGeometry args={[0.16, 0.04, 6, 14]} />
          <meshStandardMaterial color="#8a6a3a" roughness={0.3} metalness={0.9} />
        </mesh>
        {/* ocular lens */}
        <mesh ref={lens} position={[0, 0.68, 0]}>
          <cylinderGeometry args={[0.13, 0.13, 0.06, 14]} />
          <meshStandardMaterial
            ref={lensMat}
            color="#2a1c08"
            emissive="#ffd07a"
            emissiveIntensity={1.15}
            toneMapped={false}
          />
        </mesh>
      </group>
      {/* steam wisps above the vent */}
      <mesh ref={wispA} position={[0.3, 0.42, -0.18]}>
        <sphereGeometry args={[0.09, 6, 6]} />
        <meshBasicMaterial ref={wispMatA} color="#ffffff" transparent opacity={0.3} depthWrite={false} />
      </mesh>
      <mesh ref={wispB} position={[-0.26, 0.5, -0.22]}>
        <sphereGeometry args={[0.075, 6, 6]} />
        <meshBasicMaterial ref={wispMatB} color="#ffffff" transparent opacity={0.25} depthWrite={false} />
      </mesh>
    </group>
  );
}
