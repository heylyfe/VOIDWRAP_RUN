"use client";

import { useRef } from "react";
import type { JSX } from "react";
import { useFrame } from "@react-three/fiber";
import { DoubleSide } from "three";
import type { Group, MeshStandardMaterial, PointLight } from "three";
import { ENEMY_ATTACK, ENEMY_DORMANT, ENEMY_TELEGRAPH } from "@/protocol/generated";
import type { EnemyModelProps } from "../enemyLook";
import { SNAPSHRIKE_LOCK_PROG, prog01, yawToward } from "../enemyLook";

/**
 * Snapshrike — two angled chitin shards, scythe forelimbs, spark tail.
 * Accent #ff4436 (scarlet). Unit-radius model; the parent scales by body radius.
 * Idle: shiver + tail whip. LOCK: compresses; the frozen end of the lock flares the
 * eyes. DASH (ATTACK): springs forward, over-stretched.
 */
export function Snapshrike({ anim }: EnemyModelProps): JSX.Element {
  const root = useRef<Group>(null);
  const body = useRef<Group>(null);
  const tail = useRef<Group>(null);
  const sparks = useRef<Group>(null);
  const eyeMats = useRef<(MeshStandardMaterial | null)[]>([]);
  const light = useRef<PointLight>(null);

  useFrame((state) => {
    const r = root.current;
    if (!r || (r.parent !== null && !r.parent.visible)) return;
    const a = anim.current;
    const t = state.clock.elapsedTime;
    const dormant = a.state === ENEMY_DORMANT;
    const telegraph = a.state === ENEMY_TELEGRAPH;
    const attack = a.state === ENEMY_ATTACK;
    const p = prog01(a.prog);
    const locked = p >= SNAPSHRIKE_LOCK_PROG;
    const dim = dormant ? 0.15 : 1;

    if (telegraph || attack) {
      r.rotation.y = yawToward(a.ex, a.ez, a.tx, a.tz);
    }

    if (body.current) {
      body.current.position.y = Math.sin(t * (dormant ? 0.7 : 2.1)) * 0.06;
      if (attack) {
        body.current.scale.set(0.92, 0.9, 1.45); // spring release
      } else if (telegraph) {
        const squash = 1 - p * 0.24;
        body.current.scale.set(squash, 1 + p * 0.32, squash); // compress
      } else {
        body.current.scale.set(1, 1, 1);
      }
    }
    // shiver — amplitude climbs with lock progress
    r.rotation.z = Math.sin(t * 13.7) * 0.022 * (1 + p * 2.2);
    if (tail.current) {
      tail.current.rotation.y = Math.sin(t * (attack ? 7.5 : 3.4)) * (attack ? 0.72 : 0.46);
    }
    if (sparks.current) {
      sparks.current.scale.setScalar(1 + Math.sin(t * 9.3) * 0.14);
    }
    const eye = dormant ? 0.4 : attack ? 4.2 : telegraph ? (locked ? 5 : 1.6 + p * 1.6) : 1.7;
    for (const m of eyeMats.current) {
      if (m) m.emissiveIntensity = eye * dim;
    }
    if (light.current) {
      light.current.intensity = dormant ? 0.1 : 0.85 + (telegraph ? p * 0.8 : 0) + (attack ? 1.1 : 0);
    }
  });

  return (
    <group ref={root}>
      {/* shard body — two angled chitin cones */}
      <group ref={body}>
        <mesh position={[0.15, 0.08, 0.02]} rotation={[Math.PI / 2, 0.42, 0]}>
          <coneGeometry args={[0.3, 1.28, 6]} />
          <meshStandardMaterial color="#231014" roughness={0.55} metalness={0.35} flatShading emissive="#ff4436" emissiveIntensity={0.14} />
        </mesh>
        <mesh position={[-0.15, 0.08, 0.02]} rotation={[Math.PI / 2, -0.42, 0]}>
          <coneGeometry args={[0.3, 1.28, 6]} />
          <meshStandardMaterial color="#1b0d10" roughness={0.6} metalness={0.35} flatShading emissive="#ff4436" emissiveIntensity={0.14} />
        </mesh>
        {/* eyes */}
        {[0.13, -0.13].map((x, i) => (
          <mesh key={i} position={[x, 0.22, 0.66]}>
            <sphereGeometry args={[0.05, 6, 6]} />
            <meshStandardMaterial
              ref={(m) => {
                eyeMats.current[i] = m;
              }}
              color="#2a050313"
              emissive="#ff4436"
              emissiveIntensity={1.7}
              toneMapped={false}
            />
          </mesh>
        ))}
        {/* scythe forelimbs */}
        {[1, -1].map((side) => (
          <mesh
            key={side}
            position={[side * 0.44, -0.08, 0.42]}
            rotation={[0.12, -side * 0.6, -side * 0.38]}
          >
            <planeGeometry args={[0.22, 0.78]} />
            <meshStandardMaterial color="#3a3a44" roughness={0.25} metalness={0.85} emissive="#ff4436" emissiveIntensity={0.4} side={DoubleSide} />
          </mesh>
        ))}
      </group>
      {/* comet tail of sparks */}
      <group ref={tail} position={[0, 0.12, -0.62]}>
        <group ref={sparks}>
          <mesh position={[0, 0, 0]}>
            <sphereGeometry args={[0.09, 6, 6]} />
            <meshBasicMaterial color="#ffb36b" toneMapped={false} />
          </mesh>
          <mesh position={[0, 0.02, -0.26]}>
            <sphereGeometry args={[0.065, 6, 6]} />
            <meshBasicMaterial color="#ff8a5a" toneMapped={false} />
          </mesh>
          <mesh position={[0, 0.05, -0.5]}>
            <sphereGeometry args={[0.04, 6, 6]} />
            <meshBasicMaterial color="#ff4436" toneMapped={false} />
          </mesh>
        </group>
      </group>
      <pointLight ref={light} color="#ff5544" intensity={0.85} distance={4} decay={2} />
    </group>
  );
}
