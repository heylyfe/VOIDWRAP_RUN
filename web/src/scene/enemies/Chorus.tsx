"use client";

import { useRef } from "react";
import type { JSX } from "react";
import { useFrame } from "@react-three/fiber";
import type { Group, MeshStandardMaterial } from "three";
import { ENEMY_ATTACK, ENEMY_DORMANT, ENEMY_TELEGRAPH } from "@/protocol/generated";
import type { EnemyModelProps } from "../enemyLook";
import { prog01 } from "../enemyLook";

interface Pipe {
  readonly x: number;
  readonly z: number;
  readonly height: number;
  readonly radius: number;
  readonly tilt: number;
  readonly phase: number;
}

/** Seven organ pipes on a shallow arc, tallest in the middle. Deterministic. */
const PIPES: readonly Pipe[] = [
  { x: -0.45, z: 0.056, height: 0.72, radius: 0.075, tilt: 0.2, phase: 0.0 },
  { x: -0.3, z: 0.104, height: 0.95, radius: 0.085, tilt: 0.135, phase: 0.9 },
  { x: -0.15, z: 0.152, height: 1.18, radius: 0.095, tilt: 0.068, phase: 1.8 },
  { x: 0.0, z: 0.2, height: 1.32, radius: 0.105, tilt: 0.0, phase: 2.7 },
  { x: 0.15, z: 0.152, height: 1.18, radius: 0.095, tilt: -0.068, phase: 3.6 },
  { x: 0.3, z: 0.104, height: 0.95, radius: 0.085, tilt: -0.135, phase: 4.5 },
  { x: 0.45, z: 0.056, height: 0.72, radius: 0.075, tilt: -0.2, phase: 5.4 },
];

const VOICES: readonly { readonly x: number; readonly y: number; readonly z: number; readonly phase: number }[] = [
  { x: -0.2, y: 0.62, z: 0.1, phase: 0.0 },
  { x: 0.05, y: 0.82, z: 0.3, phase: 2.1 },
  { x: 0.28, y: 0.58, z: 0.18, phase: 4.2 },
];

/**
 * Chorus — a cluster of seven organ pipes murmuring on a shared plinth, with three
 * loose voices drifting above. Accent #2ec4b6 (organ teal). Unit-radius model; the
 * parent scales by body radius. Cue tier 3 (ambient): no telegraph; each pipe
 * breathes on its own phase. Attack: the whole choir swells.
 */
export function Chorus({ anim }: EnemyModelProps): JSX.Element {
  const root = useRef<Group>(null);
  const pipeRefs = useRef<(Group | null)[]>([]);
  const slitMats = useRef<(MeshStandardMaterial | null)[]>([]);
  const voiceRefs = useRef<(Group | null)[]>([]);
  const voiceMats = useRef<(MeshStandardMaterial | null)[]>([]);

  useFrame((state) => {
    const r = root.current;
    if (!r || (r.parent !== null && !r.parent.visible)) return;
    const a = anim.current;
    const t = state.clock.elapsedTime;
    const dormant = a.state === ENEMY_DORMANT;
    const winding = a.state === ENEMY_TELEGRAPH || a.state === ENEMY_ATTACK;
    const p = prog01(a.prog);
    const dim = dormant ? 0.16 : 1;

    for (let i = 0; i < PIPES.length; i += 1) {
      const pipe = PIPES[i];
      if (!pipe) continue;
      const g = pipeRefs.current[i];
      if (g) {
        g.scale.y = 1 + Math.sin(t * 2.6 + pipe.phase) * 0.055 + (winding ? p * 0.06 : 0);
      }
      const m = slitMats.current[i];
      if (m) {
        m.emissiveIntensity =
          (0.55 + 0.4 * Math.sin(t * 3.2 + pipe.phase * 1.3) + (winding ? p * 1.8 : 0)) * dim;
      }
    }
    for (let i = 0; i < VOICES.length; i += 1) {
      const voice = VOICES[i];
      if (!voice) continue;
      const g = voiceRefs.current[i];
      if (g) {
        g.position.y = voice.y + Math.sin(t * 1.7 + voice.phase) * 0.07;
        g.rotation.y = t * 0.9 + voice.phase;
      }
      const m = voiceMats.current[i];
      if (m) {
        m.emissiveIntensity =
          (0.9 + 0.5 * Math.sin(t * 2.9 + voice.phase) + (winding ? p * 2.4 : 0)) * dim;
      }
    }
  });

  return (
    <group ref={root}>
      {/* shared plinth */}
      <mesh position={[0, -0.7, 0]}>
        <cylinderGeometry args={[0.62, 0.72, 0.22, 14]} />
        <meshStandardMaterial color="#211a20" roughness={0.6} metalness={0.35} />
      </mesh>
      {/* the pipes, fanned on an arc */}
      {PIPES.map((pipe, i) => (
        <group
          key={i}
          ref={(g) => {
            pipeRefs.current[i] = g;
          }}
          position={[pipe.x, -0.59, pipe.z]}
          rotation={[0, 0, pipe.tilt]}
        >
          <mesh position={[0, pipe.height / 2, 0]}>
            <cylinderGeometry args={[pipe.radius, pipe.radius * 1.08, pipe.height, 10]} />
            <meshStandardMaterial color="#2e2a33" roughness={0.38} metalness={0.75} />
          </mesh>
          {/* mouth slit */}
          <mesh position={[0, pipe.height * 0.52, pipe.radius + 0.004]}>
            <boxGeometry args={[0.035, pipe.height * 0.45, 0.02]} />
            <meshStandardMaterial
              ref={(m) => {
                slitMats.current[i] = m;
              }}
              color="#0c1a18"
              emissive="#2ec4b6"
              emissiveIntensity={0.55}
              toneMapped={false}
            />
          </mesh>
        </group>
      ))}
      {/* loose voices above the pipes */}
      {VOICES.map((voice, i) => (
        <group
          key={i}
          ref={(g) => {
            voiceRefs.current[i] = g;
          }}
          position={[voice.x, voice.y, voice.z]}
        >
          <mesh>
            <octahedronGeometry args={[0.085, 0]} />
            <meshStandardMaterial
              ref={(m) => {
                voiceMats.current[i] = m;
              }}
              color="#123a36"
              emissive="#2ec4b6"
              emissiveIntensity={0.9}
              roughness={0.4}
              toneMapped={false}
            />
          </mesh>
        </group>
      ))}
    </group>
  );
}
