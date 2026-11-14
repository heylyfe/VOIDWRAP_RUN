"use client";

import { useRef } from "react";
import type { JSX } from "react";
import { useFrame } from "@react-three/fiber";
import type { Group, Mesh, MeshStandardMaterial, PointLight } from "three";
import { ENEMY_ATTACK, ENEMY_DORMANT, ENEMY_TELEGRAPH } from "@/protocol/generated";
import type { EnemyModelProps } from "../enemyLook";
import { prog01, yawToward } from "../enemyLook";

/**
 * Lantern Leech — translucent sac hung from a paper-lantern disc, dangling threads.
 * Accent #ff7ab8 (ember-pink), hot core #ffb3d1. Unit-radius model. Reads as a floating lamp.
 * Idle: slow lantern swing. SIPHON (TELEGRAPH): sac dilates, core flares.
 * GORGE (ATTACK): rears back, then strikes.
 */
export function LanternLeech({ anim }: EnemyModelProps): JSX.Element {
  const root = useRef<Group>(null);
  const swing = useRef<Group>(null);
  const sac = useRef<Mesh>(null);
  const sacMat = useRef<MeshStandardMaterial>(null);
  const coreMat = useRef<MeshStandardMaterial>(null);
  const discMat = useRef<MeshStandardMaterial>(null);
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

    if (telegraph || attack) {
      r.rotation.y = yawToward(a.ex, a.ez, a.tx, a.tz);
    }

    if (swing.current) {
      if (attack) {
        // rear back, then lunge forward
        swing.current.rotation.x =
          p < 0.35 ? -0.55 * (p / 0.35) : -0.55 + ((p - 0.35) / 0.65) * 0.95;
      } else {
        swing.current.rotation.x = Math.sin(t * 1.02 + 0.8) * 0.09;
      }
      swing.current.rotation.z = telegraph || attack ? 0 : Math.sin(t * 1.35) * 0.14;
    }
    if (sac.current) {
      const dilate = telegraph ? 1 + p * 0.4 : attack ? 1 + Math.sin(p * Math.PI) * 0.42 : 1 + Math.sin(t * 2.2) * 0.035;
      sac.current.scale.set(0.42 * dilate, 0.58 * dilate, 0.42 * dilate);
    }
    if (sacMat.current) {
      sacMat.current.opacity = dormant ? 0.28 : 0.58;
      sacMat.current.emissiveIntensity = dormant ? 0.15 : 0.85 + (telegraph ? p * 1.3 : 0);
    }
    if (coreMat.current) {
      coreMat.current.emissiveIntensity = dormant ? 0.3 : 2.3 + (telegraph ? p * 2.4 : attack ? 2.2 : 0);
    }
    if (discMat.current) {
      discMat.current.emissiveIntensity = dormant ? 0.08 : 0.55 + (telegraph ? p * 0.9 : 0);
    }
    if (light.current) {
      light.current.intensity = dormant ? 0.1 : 0.95 + (telegraph || attack ? p * 1.5 : Math.sin(t * 1.35) * 0.15);
    }
  });

  const threadAngles = [0.6, 2.2, 3.8, 5.3];

  return (
    <group ref={root}>
      <group ref={swing}>
        {/* paper-lantern disc overhead */}
        <mesh position={[0, 0.46, 0]}>
          <cylinderGeometry args={[0.5, 0.58, 0.16, 14]} />
          <meshStandardMaterial
            ref={discMat}
            color="#f2e3c4"
            emissive="#ffb3d1"
            emissiveIntensity={0.55}
            roughness={0.6}
          />
        </mesh>
        <mesh position={[0, 0.46, 0]}>
          <torusGeometry args={[0.55, 0.025, 6, 20]} />
          <meshStandardMaterial color="#caa6b0" emissive="#ff7ab8" emissiveIntensity={0.8} toneMapped={false} />
        </mesh>
        {/* dangling threads from the disc rim */}
        {threadAngles.map((ang) => (
          <mesh
            key={ang}
            position={[Math.cos(ang) * 0.42, 0.18, Math.sin(ang) * 0.42]}
            rotation={[0.18, 0, Math.cos(ang) * 0.22]}
          >
            <cylinderGeometry args={[0.012, 0.012, 0.48, 4]} />
            <meshStandardMaterial color="#caa6b0" roughness={0.8} />
          </mesh>
        ))}
        {/* translucent sac */}
        <mesh ref={sac} position={[0, -0.28, 0]} scale={[0.42, 0.58, 0.42]}>
          <sphereGeometry args={[1, 14, 14]} />
          <meshStandardMaterial
            ref={sacMat}
            color="#ff7ab8"
            emissive="#ff7ab8"
            emissiveIntensity={0.85}
            transparent
            opacity={0.58}
            depthWrite={false}
            roughness={0.2}
          />
        </mesh>
        {/* hot core */}
        <mesh position={[0, -0.28, 0]}>
          <sphereGeometry args={[0.15, 10, 10]} />
          <meshStandardMaterial
            ref={coreMat}
            color="#fff0f6"
            emissive="#ffb3d1"
            emissiveIntensity={2.3}
            toneMapped={false}
          />
        </mesh>
        {/* two threads trailing from the sac tip */}
        {[0.35, -0.35].map((x) => (
          <mesh key={x} position={[x, -0.78, 0]} rotation={[0, 0, x * 0.4]}>
            <cylinderGeometry args={[0.01, 0.01, 0.42, 4]} />
            <meshStandardMaterial color="#caa6b0" roughness={0.8} />
          </mesh>
        ))}
      </group>
      <pointLight ref={light} color="#ffb3d1" intensity={0.95} distance={5} decay={2} />
    </group>
  );
}
