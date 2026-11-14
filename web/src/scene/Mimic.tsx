"use client";

import { useRef } from "react";
import type { JSX } from "react";
import { useFrame } from "@react-three/fiber";
import type { Group, MeshStandardMaterial } from "three";
import type { Vec3 } from "@/protocol/generated";
import { GiftBox } from "./GiftBox";
import { COMMON } from "./theme";
import type { EnemyMode } from "./enemyBehavior";

const ORIGIN: Vec3 = { x: 0, y: 0, z: 0 };
const TEETH_X: readonly number[] = [-0.36, -0.12, 0.12, 0.36];

/**
 * Gift-mimic. While disguised it renders the exact GiftBox normal gifts use (the pool
 * slot is scaled to 1 by the parent so it is indistinguishable, light pool included).
 * When hunting (or dormant) it becomes a toothy dark box with the enemy glow.
 */
export function Mimic(props: { position: Vec3; mode: { current: EnemyMode } }): JSX.Element {
  const giftRef = useRef<Group>(null);
  const mawRef = useRef<Group>(null);
  const darkMatRef = useRef<MeshStandardMaterial>(null);
  const teethMatsRef = useRef<(MeshStandardMaterial | null)[]>([]);
  const eyeMatsRef = useRef<(MeshStandardMaterial | null)[]>([]);

  useFrame((state) => {
    const mode = props.mode.current;
    const disguised = mode === "disguised";
    const dormant = mode === "dormant";
    if (giftRef.current) giftRef.current.visible = disguised;
    if (mawRef.current) mawRef.current.visible = !disguised;
    const target = mawRef.current;
    if (target) target.rotation.y = Math.sin(state.clock.elapsedTime * (dormant ? 0.6 : 1.9)) * 0.18;
    const teethIntensity = dormant ? 0.35 : 2.1;
    for (const m of teethMatsRef.current) {
      if (m) m.emissiveIntensity = teethIntensity;
    }
    const eyeIntensity = dormant ? 0.4 : 2.6;
    for (const m of eyeMatsRef.current) {
      if (m) m.emissiveIntensity = eyeIntensity;
    }
    const darkMat = darkMatRef.current;
    if (darkMat) darkMat.emissiveIntensity = dormant ? 0.12 : 0.5;
  });

  return (
    <group position={[props.position.x, props.position.y, props.position.z]}>
      <group ref={giftRef}>
        <GiftBox position={ORIGIN} taken={false} />
      </group>
      <group ref={mawRef}>
        <mesh>
          <boxGeometry args={[1.05, 0.95, 1.05]} />
          <meshStandardMaterial
            ref={darkMatRef}
            color="#16101c"
            emissive={COMMON.enemy.glow}
            emissiveIntensity={0.5}
            roughness={0.85}
          />
        </mesh>
        {TEETH_X.map((x, i) => (
          <mesh key={`u${x}`} position={[x, 0.14, 0.48]} rotation={[Math.PI, 0, 0]}>
            <coneGeometry args={[0.085, 0.24, 4]} />
            <meshStandardMaterial
              ref={(m) => {
                teethMatsRef.current[i] = m;
              }}
              color="#f5e9ff"
              emissive="#ff4fa0"
              emissiveIntensity={2.1}
              toneMapped={false}
            />
          </mesh>
        ))}
        {TEETH_X.map((x, i) => (
          <mesh key={`l${x}`} position={[x, -0.14, 0.48]}>
            <coneGeometry args={[0.085, 0.24, 4]} />
            <meshStandardMaterial
              ref={(m) => {
                teethMatsRef.current[TEETH_X.length + i] = m;
              }}
              color="#f5e9ff"
              emissive="#ff4fa0"
              emissiveIntensity={2.1}
              toneMapped={false}
            />
          </mesh>
        ))}
        <mesh position={[-0.22, 0.33, 0.5]}>
          <boxGeometry args={[0.16, 0.08, 0.06]} />
          <meshStandardMaterial
            ref={(m) => {
              eyeMatsRef.current[0] = m;
            }}
            color="#ffd7f6"
            emissive={COMMON.enemy.glow}
            emissiveIntensity={2.6}
            toneMapped={false}
          />
        </mesh>
        <mesh position={[0.22, 0.33, 0.5]}>
          <boxGeometry args={[0.16, 0.08, 0.06]} />
          <meshStandardMaterial
            ref={(m) => {
              eyeMatsRef.current[1] = m;
            }}
            color="#ffd7f6"
            emissive={COMMON.enemy.glow}
            emissiveIntensity={2.6}
            toneMapped={false}
          />
        </mesh>
      </group>
    </group>
  );
}
