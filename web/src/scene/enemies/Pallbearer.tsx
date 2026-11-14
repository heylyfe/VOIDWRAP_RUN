"use client";

import { useRef } from "react";
import type { JSX } from "react";
import { useFrame } from "@react-three/fiber";
import { DoubleSide } from "three";
import type { Group, MeshStandardMaterial, PointLight } from "three";
import { ENEMY_ATTACK, ENEMY_DORMANT, ENEMY_TELEGRAPH } from "@/protocol/generated";
import type { EnemyModelProps } from "../enemyLook";
import { prog01, yawToward } from "../enemyLook";

/**
 * The Pallbearer — black-lacquered coffin-barge, brass lantern prow, hanging shroud.
 * Accent #ff3355 (deep crimson). Unit-radius model; the parent scales by body radius.
 * Idle: low bob / sway. Attack (KNEEL→LUNGE): rears up, lantern whiteouts.
 */
export function Pallbearer({ anim }: EnemyModelProps): JSX.Element {
  const root = useRef<Group>(null);
  const inner = useRef<Group>(null);
  const shroud = useRef<Group>(null);
  const seamMats = useRef<(MeshStandardMaterial | null)[]>([]);
  const lanternMat = useRef<MeshStandardMaterial>(null);
  const lanternLight = useRef<PointLight>(null);

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

    if (telegraph || attack) {
      r.rotation.y = yawToward(a.ex, a.ez, a.tx, a.tz);
    }

    if (inner.current) {
      const bobSpeed = dormant ? 0.55 : 1.35;
      inner.current.position.y =
        Math.sin(t * bobSpeed) * (dormant ? 0.05 : 0.09) + (telegraph || attack ? p * 0.32 : 0);
      inner.current.rotation.x = -(telegraph ? p * 0.55 : attack ? 0.55 : 0); // rear-up
      inner.current.rotation.z = Math.sin(t * 0.85) * (dormant ? 0.015 : 0.045);
    }
    if (shroud.current) {
      shroud.current.rotation.x = Math.sin(t * 1.7 + 1.3) * 0.24;
    }
    const ember = (0.9 + Math.sin(t * 2.1) * 0.35) * dim;
    for (const m of seamMats.current) {
      if (m) m.emissiveIntensity = ember;
    }
    if (lanternMat.current) {
      lanternMat.current.emissiveIntensity =
        (dormant ? 0.3 : 1.6) + (telegraph || attack ? p * 3.4 : 0);
    }
    if (lanternLight.current) {
      lanternLight.current.intensity = dormant ? 0.12 : 1.15 + (telegraph || attack ? p * 2.3 : 0);
    }
  });

  return (
    <group ref={root}>
      <group ref={inner}>
        {/* hull — stretched black-lacquer barge */}
        <mesh>
          <boxGeometry args={[0.95, 0.5, 2.15]} />
          <meshStandardMaterial color="#0d0d12" roughness={0.32} metalness={0.55} />
        </mesh>
        {/* coffin lid */}
        <mesh position={[0, 0.36, -0.08]}>
          <boxGeometry args={[0.72, 0.24, 1.7]} />
          <meshStandardMaterial color="#17171f" roughness={0.42} metalness={0.35} />
        </mesh>
        {/* ember-crack seams down both flanks */}
        {[0.485, -0.485].map((x, i) => (
          <mesh key={i} position={[x, 0.02, 0]}>
            <boxGeometry args={[0.02, 0.07, 1.75]} />
            <meshStandardMaterial
              ref={(m) => {
                seamMats.current[i] = m;
              }}
              color="#1a0508"
              emissive="#ff3355"
              emissiveIntensity={0.9}
              toneMapped={false}
            />
          </mesh>
        ))}
        {/* brass lantern prow */}
        <group position={[0, 0.3, 1.18]}>
          <mesh>
            <cylinderGeometry args={[0.14, 0.2, 0.42, 10]} />
            <meshStandardMaterial color="#8a6a2f" roughness={0.35} metalness={0.85} />
          </mesh>
          <mesh position={[0, 0.02, 0]}>
            <sphereGeometry args={[0.11, 10, 10]} />
            <meshStandardMaterial
              ref={lanternMat}
              color="#fff2cf"
              emissive="#ffdf9e"
              emissiveIntensity={1.6}
              toneMapped={false}
            />
          </mesh>
          <mesh position={[0, 0.29, 0]}>
            <coneGeometry args={[0.17, 0.16, 10]} />
            <meshStandardMaterial color="#8a6a2f" roughness={0.4} metalness={0.8} />
          </mesh>
          <pointLight ref={lanternLight} color="#ffd9a0" intensity={1.15} distance={6} decay={2} />
        </group>
        {/* hanging shroud stern */}
        <group ref={shroud} position={[0, 0.05, -1.04]}>
          <mesh position={[0, -0.42, 0]}>
            <planeGeometry args={[0.88, 0.95]} />
            <meshStandardMaterial color="#26262e" roughness={0.9} metalness={0.05} side={DoubleSide} />
          </mesh>
        </group>
      </group>
    </group>
  );
}
