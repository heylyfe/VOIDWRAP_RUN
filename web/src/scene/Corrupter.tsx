"use client";

import { useRef } from "react";
import type { JSX } from "react";
import { useFrame } from "@react-three/fiber";
import { AdditiveBlending } from "three";
import type { Group, MeshStandardMaterial } from "three";
import type { Vec3 } from "@/protocol/generated";
import type { EnemyMode } from "./enemyBehavior";

interface Spike {
  readonly pos: readonly [number, number, number];
  readonly rot: readonly [number, number, number];
  readonly scale: number;
}

const SPIKES: readonly Spike[] = [
  { pos: [0.62, 0.08, 0.1], rot: [0, 0, -1.15], scale: 1.15 },
  { pos: [-0.58, -0.12, 0.18], rot: [0.2, 0.3, 1.05], scale: 0.95 },
  { pos: [0.22, 0.18, -0.6], rot: [1.15, 0, 0.25], scale: 1.05 },
  { pos: [-0.15, 0.3, 0.6], rot: [-1.05, 0.2, -0.2], scale: 0.9 },
  { pos: [0.35, -0.35, 0.4], rot: [-0.6, 0, -0.8], scale: 0.8 },
  { pos: [-0.4, 0.32, -0.35], rot: [0.7, 0.4, 0.75], scale: 0.85 },
  { pos: [0.05, 0.68, -0.1], rot: [0.15, 0, 0.1], scale: 0.75 },
];

interface Mote {
  readonly radius: number;
  readonly y: number;
  readonly phase: number;
  readonly speed: number;
}

const MOTES: readonly Mote[] = [
  { radius: 0.72, y: 0.35, phase: 0.0, speed: 0.9 },
  { radius: 0.95, y: -0.15, phase: 0.9, speed: 0.7 },
  { radius: 1.1, y: 0.55, phase: 1.8, speed: 0.55 },
  { radius: 0.8, y: -0.5, phase: 2.6, speed: 0.8 },
  { radius: 1.0, y: 0.1, phase: 3.4, speed: 0.65 },
  { radius: 0.68, y: 0.65, phase: 4.2, speed: 0.85 },
  { radius: 1.15, y: -0.35, phase: 5.0, speed: 0.5 },
  { radius: 0.88, y: 0.0, phase: 5.8, speed: 0.75 },
];

/** Ragged dark-cored wisp with orbiting motes. Dormant: dimmed, slow drift. */
export function Corrupter(props: { position: Vec3; mode: { current: EnemyMode } }): JSX.Element {
  const wispRef = useRef<Group>(null);
  const motesRef = useRef<Group>(null);
  const coreMatRef = useRef<MeshStandardMaterial>(null);
  const spikeMatsRef = useRef<(MeshStandardMaterial | null)[]>([]);
  const moteMatsRef = useRef<(MeshStandardMaterial | null)[]>([]);

  useFrame((state) => {
    const t = state.clock.elapsedTime;
    const dormant = props.mode.current === "dormant";
    const speedMul = dormant ? 0.35 : 1;
    const wisp = wispRef.current;
    if (wisp) wisp.rotation.y = t * (dormant ? 0.3 : 0.9);
    const motes = motesRef.current;
    if (motes) {
      for (let i = 0; i < MOTES.length; i += 1) {
        const m = MOTES[i];
        const child = motes.children[i];
        if (!m || !child) continue;
        const a = t * m.speed * speedMul + m.phase;
        child.position.set(
          Math.cos(a) * m.radius,
          m.y + Math.sin(t * 1.4 * speedMul + m.phase) * 0.14,
          Math.sin(a) * m.radius,
        );
      }
    }
    const coreMat = coreMatRef.current;
    if (coreMat) coreMat.emissiveIntensity = dormant ? 0.32 : 1.1;
    const spikeIntensity = dormant ? 0.18 : 0.75;
    for (const m of spikeMatsRef.current) {
      if (m) m.emissiveIntensity = spikeIntensity;
    }
    const moteOpacity = dormant ? 0.22 : 0.72;
    for (const m of moteMatsRef.current) {
      if (m) m.opacity = moteOpacity;
    }
  });

  return (
    <group position={[props.position.x, props.position.y, props.position.z]}>
      <group ref={wispRef}>
        <mesh>
          <dodecahedronGeometry args={[0.58, 0]} />
          <meshStandardMaterial
            ref={coreMatRef}
            color="#171020"
            emissive="#8a3df0"
            emissiveIntensity={1.1}
            roughness={0.9}
          />
        </mesh>
        {SPIKES.map((s, i) => (
          <mesh
            key={i}
            position={s.pos as [number, number, number]}
            rotation={s.rot as [number, number, number]}
            scale={[s.scale, s.scale * 1.6, s.scale]}
          >
            <octahedronGeometry args={[0.16, 0]} />
            <meshStandardMaterial
              ref={(m) => {
                spikeMatsRef.current[i] = m;
              }}
              color="#1d1526"
              emissive="#6a2bd8"
              emissiveIntensity={0.75}
              roughness={0.85}
            />
          </mesh>
        ))}
      </group>
      <group ref={motesRef}>
        {MOTES.map((m, i) => (
          <mesh key={i}>
            <octahedronGeometry args={[0.07, 0]} />
            <meshStandardMaterial
              ref={(mm) => {
                moteMatsRef.current[i] = mm;
              }}
              color="#b07cff"
              emissive="#b07cff"
              emissiveIntensity={1.6}
              transparent
              opacity={0.72}
              blending={AdditiveBlending}
              depthWrite={false}
              toneMapped={false}
            />
          </mesh>
        ))}
      </group>
    </group>
  );
}
