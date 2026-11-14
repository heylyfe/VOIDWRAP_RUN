"use client";

import { useRef } from "react";
import type { JSX } from "react";
import { useFrame } from "@react-three/fiber";
import type { Group, MeshStandardMaterial, PointLight } from "three";
import { ENEMY_ATTACK, ENEMY_DORMANT, ENEMY_TELEGRAPH } from "@/protocol/generated";
import type { EnemyModelProps } from "../enemyLook";
import { prog01, yawToward } from "../enemyLook";

/**
 * Rift Warden — two free-standing doorframe slabs + floating lintel, carved storm-runes.
 * Rust-maroon #b0313f with magma-crack runes #ff5a3c. Unit-radius model.
 * Idle: slabs drift, runes pulse slow. IGNITE (TELEGRAPH): runes blaze, doorway widens.
 * SHOVE (ATTACK): violent white-out flash.
 */
export function RiftWarden({ anim }: EnemyModelProps): JSX.Element {
  const root = useRef<Group>(null);
  const slabL = useRef<Group>(null);
  const slabR = useRef<Group>(null);
  const lintel = useRef<Group>(null);
  const runeMats = useRef<(MeshStandardMaterial | null)[]>([]);
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
    const dim = dormant ? 0.15 : 1;

    if (telegraph || attack) {
      r.rotation.y = yawToward(a.ex, a.ez, a.tx, a.tz);
    }

    const spread = telegraph || attack ? 0.55 + p * 0.13 : 0.55 + Math.sin(t * 0.8) * 0.06;
    if (slabL.current) slabL.current.position.x = -spread;
    if (slabR.current) slabR.current.position.x = spread;
    if (lintel.current) {
      lintel.current.position.y = 1.02 + Math.sin(t * 1.15 + 0.7) * 0.07 + (attack ? Math.sin(p * Math.PI) * 0.12 : 0);
      lintel.current.rotation.z = Math.sin(t * 0.9) * 0.05;
    }
    const swell = attack ? 1 + Math.sin(p * Math.PI) * 0.13 : 1;
    r.scale.setScalar(swell);

    const rune = dormant ? 0.25 : telegraph || attack ? 1.3 + p * 3.2 : 1.05 + Math.sin(t * 1.6) * 0.38;
    for (const m of runeMats.current) {
      if (m) m.emissiveIntensity = rune;
    }
    if (light.current) {
      light.current.intensity = dormant ? 0.1 : 0.9 + (telegraph ? p * 1.6 : attack ? Math.sin(p * Math.PI) * 2 : 0);
    }
    void dim;
  });

  const slab = (side: 1 | -1, idx: number): JSX.Element => (
    <group ref={side === -1 ? slabL : slabR} position={[side * 0.55, 0, 0]}>
      {/* rust-iron slab with dried-blood patina */}
      <mesh>
        <boxGeometry args={[0.34, 1.95, 0.26]} />
        <meshStandardMaterial color="#3a2420" roughness={0.72} metalness={0.55} />
      </mesh>
      {/* carved storm-rune — magma crack */}
      {[0.55, 0, -0.55].map((y, j) => (
        <mesh key={j} position={[0, y, 0.135]}>
          <planeGeometry args={[0.17, 0.3]} />
          <meshStandardMaterial
            ref={(m) => {
              runeMats.current[idx * 3 + j] = m;
            }}
            color="#1c0c08"
            emissive="#ff5a3c"
            emissiveIntensity={1.05}
            toneMapped={false}
          />
        </mesh>
      ))}
    </group>
  );

  return (
    <group ref={root}>
      {slab(-1, 0)}
      {slab(1, 1)}
      {/* floating lintel with a storm-rune */}
      <group ref={lintel} position={[0, 1.02, 0]}>
        <mesh>
          <boxGeometry args={[1.48, 0.3, 0.3]} />
          <meshStandardMaterial color="#432a24" roughness={0.7} metalness={0.55} />
        </mesh>
        <mesh position={[0, 0, 0.155]}>
          <planeGeometry args={[1.1, 0.14]} />
          <meshStandardMaterial
            ref={(m) => {
              runeMats.current[6] = m;
            }}
            color="#1c0c08"
            emissive="#ff5a3c"
            emissiveIntensity={1.05}
            toneMapped={false}
          />
        </mesh>
      </group>
      <pointLight ref={light} color="#ff5a3c" intensity={0.9} distance={4} decay={2} />
    </group>
  );
}
