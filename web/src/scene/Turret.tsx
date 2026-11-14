"use client";

import { useRef } from "react";
import type { JSX } from "react";
import { useFrame } from "@react-three/fiber";
import { Vector3 } from "three";
import type { Group, MeshStandardMaterial } from "three";
import type { Vec3 } from "@/protocol/generated";
import type { GameSession } from "@/game/session";
import { COMMON } from "./theme";
import type { EnemyMode } from "./enemyBehavior";

/** Scratch vector reused every frame (no allocation). */
const SCRATCH = new Vector3();
const PULSE_SPEED = 1.6;
const DORMANT_PULSE_SPEED = 0.7;

/**
 * Squat pylon with a single eye. Tracks the local player (yaw only) and pulses slowly;
 * the pulse brightens when hunting, dims + slows when dormant.
 */
export function Turret(props: { position: Vec3; mode: { current: EnemyMode }; session: GameSession }): JSX.Element {
  const headRef = useRef<Group>(null);
  const bodyMatsRef = useRef<(MeshStandardMaterial | null)[]>(
    Array.from({ length: 3 }, () => null),
  );
  const eyeMatRef = useRef<MeshStandardMaterial>(null);

  useFrame((state) => {
    const dormant = props.mode.current === "dormant";
    const head = headRef.current;
    const ctl = props.session.playerController();
    if (head && ctl) {
      head.getWorldPosition(SCRATCH);
      const dx = ctl.pos.x - SCRATCH.x;
      const dz = ctl.pos.z - SCRATCH.z;
      if (dx * dx + dz * dz > 0.01) head.rotation.y = Math.atan2(dx, dz);
    }
    const t = state.clock.elapsedTime;
    const pulse = 0.5 + 0.5 * Math.sin(t * (dormant ? DORMANT_PULSE_SPEED : PULSE_SPEED));
    const eyeMat = eyeMatRef.current;
    if (eyeMat) eyeMat.emissiveIntensity = dormant ? 0.5 + pulse * 0.6 : 1.5 + pulse * 1.5;
    const bodyIntensity = dormant ? 0.1 : 0.3;
    for (const m of bodyMatsRef.current) {
      if (m) m.emissiveIntensity = bodyIntensity;
    }
  });

  return (
    <group position={[props.position.x, props.position.y, props.position.z]}>
      <mesh position={[0, -0.28, 0]}>
        <cylinderGeometry args={[0.62, 0.88, 0.42, 8]} />
        <meshStandardMaterial
          ref={(m) => {
            bodyMatsRef.current[0] = m;
          }}
          color="#2b2130"
          emissive={COMMON.enemy.glow}
          emissiveIntensity={0.3}
          roughness={0.6}
        />
      </mesh>
      <mesh position={[0, 0.12, 0]}>
        <cylinderGeometry args={[0.34, 0.5, 0.72, 8]} />
        <meshStandardMaterial
          ref={(m) => {
            bodyMatsRef.current[1] = m;
          }}
          color="#2b2130"
          emissive={COMMON.enemy.glow}
          emissiveIntensity={0.3}
          roughness={0.6}
        />
      </mesh>
      <group ref={headRef} position={[0, 0.55, 0]}>
        <mesh scale={[1, 0.72, 1]}>
          <sphereGeometry args={[0.46, 12, 12]} />
          <meshStandardMaterial
            ref={(m) => {
              bodyMatsRef.current[2] = m;
            }}
            color="#2b2130"
            emissive={COMMON.enemy.glow}
            emissiveIntensity={0.3}
            roughness={0.6}
          />
        </mesh>
        <mesh position={[0, 0.02, 0.36]}>
          <boxGeometry args={[0.42, 0.2, 0.14]} />
          <meshStandardMaterial
            ref={eyeMatRef}
            color="#ff9ad9"
            emissive={COMMON.enemy.glow}
            emissiveIntensity={2.2}
            toneMapped={false}
          />
        </mesh>
      </group>
    </group>
  );
}
