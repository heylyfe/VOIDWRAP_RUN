"use client";

import { useRef } from "react";
import type { JSX } from "react";
import { useFrame } from "@react-three/fiber";
import { DoubleSide } from "three";
import type { Group, Mesh, MeshBasicMaterial, MeshStandardMaterial } from "three";
import { ENEMY_ATTACK, ENEMY_DORMANT, ENEMY_FLAG_HEAVY, ENEMY_TELEGRAPH } from "@/protocol/generated";
import type { EnemyModelProps } from "../enemyLook";
import { clamp01, prog01, yawToward } from "../enemyLook";

/**
 * Gale Wight — tattered manta of storm-grey cloth, ribbed throat sac, cyclone-eye.
 * Accent #d94fff (magenta-violet), arcs #b47aff. Unit-radius model.
 * Idle: banking circles with folded wings. VOLLEY (TELEGRAPH): sac inflates,
 * lightning crawls. ATTACK: sac bursts and deflates; the shell itself is drawn by Projectiles.
 */
export function GaleWight({ anim }: EnemyModelProps): JSX.Element {
  const root = useRef<Group>(null);
  const inner = useRef<Group>(null);
  const wingL = useRef<Mesh>(null);
  const wingR = useRef<Mesh>(null);
  const sac = useRef<Mesh>(null);
  const sacMat = useRef<MeshStandardMaterial>(null);
  const eyeMat = useRef<MeshStandardMaterial>(null);
  const arcL = useRef<Mesh>(null);
  const arcR = useRef<Mesh>(null);
  const arcMatL = useRef<MeshBasicMaterial>(null);
  const arcMatR = useRef<MeshBasicMaterial>(null);

  useFrame((state) => {
    const r = root.current;
    if (!r || (r.parent !== null && !r.parent.visible)) return;
    const a = anim.current;
    const t = state.clock.elapsedTime;
    const dormant = a.state === ENEMY_DORMANT;
    const telegraph = a.state === ENEMY_TELEGRAPH;
    const attack = a.state === ENEMY_ATTACK;
    const heavy = (a.flags & ENEMY_FLAG_HEAVY) !== 0;
    const p = prog01(a.prog);
    const dim = dormant ? 0.15 : 1;

    if (telegraph || attack) {
      r.rotation.y = yawToward(a.ex, a.ez, a.tx, a.tz);
    }

    // banking idle — levels out while winding up a volley
    const bank = Math.sin(t * 1.05) * 0.22 * (telegraph || attack ? 0.25 : 1);
    if (inner.current) {
      inner.current.rotation.z = bank;
      inner.current.position.y = Math.sin(t * 1.3) * 0.13;
      inner.current.rotation.y = Math.sin(t * 0.8) * 0.18;
    }

    const fold = Math.sin(t * 1.05 + 0.6) * 0.16 - (telegraph || attack ? p * 0.22 : 0);
    if (wingL.current) wingL.current.rotation.z = 0.42 + fold;
    if (wingR.current) wingR.current.rotation.z = -0.42 - fold;

    // sac inflates during the mark, bursts on release
    let sacScale = 1;
    if (telegraph) sacScale = 1 + p * 0.95 + (heavy ? 0.15 : 0);
    else if (attack) sacScale = p < 0.3 ? 1.95 - (p / 0.3) * 1.05 : 0.9;
    if (sac.current) sac.current.scale.setScalar(sacScale);
    if (sacMat.current) {
      sacMat.current.emissiveIntensity = dormant ? 0.25 : attack ? 3.6 : 1.5 + p * 2.4;
    }
    if (eyeMat.current) {
      eyeMat.current.emissiveIntensity = dormant ? 0.4 : attack ? 4.6 : 2.1 + p * 2.1;
    }
    const crackle = telegraph || attack;
    if (arcL.current) arcL.current.visible = crackle;
    if (arcR.current) arcR.current.visible = crackle;
    if (arcMatL.current) arcMatL.current.opacity = Math.sin(t * 31) > 0 ? 0.95 : 0.22;
    if (arcMatR.current) arcMatR.current.opacity = Math.sin(t * 27 + 1.4) > 0 ? 0.95 : 0.22;
    void dim;
    void clamp01;
  });

  return (
    <group ref={root}>
      <group ref={inner}>
        {/* centre sail — flattened 4-sided cone, apex forward */}
        <mesh position={[0, 0.08, 0.1]} rotation={[Math.PI / 2, 0, Math.PI / 4]} scale={[1.2, 0.72, 1]}>
          <coneGeometry args={[0.62, 1.35, 4]} />
          <meshStandardMaterial color="#4d5464" roughness={0.85} metalness={0.1} flatShading emissive="#b47aff" emissiveIntensity={0.12} />
        </mesh>
        {/* wings — two angled cloth planes with tattered tips */}
        <mesh ref={wingL} position={[-0.6, 0.16, -0.02]} rotation={[0.1, 0.25, 0.42]}>
          <planeGeometry args={[1.15, 0.74]} />
          <meshStandardMaterial color="#565e70" roughness={0.9} metalness={0.05} side={DoubleSide} />
        </mesh>
        <mesh ref={wingR} position={[0.6, 0.16, -0.02]} rotation={[0.1, -0.25, -0.42]}>
          <planeGeometry args={[1.15, 0.74]} />
          <meshStandardMaterial color="#565e70" roughness={0.9} metalness={0.05} side={DoubleSide} />
        </mesh>
        {/* ribbed throat sac */}
        <mesh ref={sac} position={[0, -0.22, 0.52]}>
          <torusGeometry args={[0.22, 0.12, 8, 18]} />
          <meshStandardMaterial
            ref={sacMat}
            color="#3a2b4d"
            emissive="#d94fff"
            emissiveIntensity={1.5}
            roughness={0.5}
            toneMapped={false}
          />
        </mesh>
        {/* cyclone-eye */}
        <mesh position={[0, 0.1, 0.72]}>
          <sphereGeometry args={[0.14, 12, 12]} />
          <meshStandardMaterial
            ref={eyeMat}
            color="#1c1030"
            emissive="#b47aff"
            emissiveIntensity={2.1}
            toneMapped={false}
          />
        </mesh>
        {/* lightning slivers under the chin — visible while charging/firing */}
        <mesh ref={arcL} position={[-0.16, -0.3, 0.62]} rotation={[0, 0, 0.6]} visible={false}>
          <boxGeometry args={[0.42, 0.03, 0.03]} />
          <meshBasicMaterial ref={arcMatL} color="#b47aff" transparent opacity={0.8} blending={2} depthWrite={false} toneMapped={false} />
        </mesh>
        <mesh ref={arcR} position={[0.16, -0.3, 0.62]} rotation={[0, 0, -0.6]} visible={false}>
          <boxGeometry args={[0.42, 0.03, 0.03]} />
          <meshBasicMaterial ref={arcMatR} color="#b47aff" transparent opacity={0.8} blending={2} depthWrite={false} toneMapped={false} />
        </mesh>
      </group>
    </group>
  );
}
