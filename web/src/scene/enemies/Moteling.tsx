"use client";

import { useEffect, useRef } from "react";
import type { JSX } from "react";
import { useFrame } from "@react-three/fiber";
import { Object3D } from "three";
import type { Group, InstancedMesh, MeshStandardMaterial } from "three";
import { ENEMY_ATTACK, ENEMY_DORMANT, ENEMY_FLAG_HEAVY, ENEMY_TELEGRAPH } from "@/protocol/generated";
import type { EnemyModelProps } from "../enemyLook";
import { clamp01, prog01 } from "../enemyLook";

const MOTES = 60;
const ORBIT_RADIUS = 0.72;

/**
 * Moteling — orbiting dust sphere around a hollow dark core.
 * Accent #ffc3d9 (pale rose) with #ff5fa8 magenta rim. Unit-radius model.
 * Idle: lazy orbit. INHALE (TELEGRAPH): radial collapse onto the core.
 * SHRIEK (ATTACK): ring snap — motes blow back outward.
 */
export function Moteling({ anim }: EnemyModelProps): JSX.Element {
  const root = useRef<Group>(null);
  const orbit = useRef<Group>(null);
  const moteRef = useRef<InstancedMesh>(null);
  const moteMat = useRef<MeshStandardMaterial>(null);
  const coreMat = useRef<MeshStandardMaterial>(null);

  // Deterministic fibonacci-sphere layout, set once. No per-frame matrix churn.
  useEffect(() => {
    const inst = moteRef.current;
    if (!inst) return;
    const dummy = new Object3D();
    const goldenAngle = Math.PI * (3 - Math.sqrt(5));
    for (let i = 0; i < MOTES; i += 1) {
      const y = 1 - (i / (MOTES - 1)) * 2;
      const r = Math.sqrt(Math.max(0, 1 - y * y));
      const theta = goldenAngle * i;
      dummy.position.set(
        Math.cos(theta) * r * ORBIT_RADIUS,
        y * ORBIT_RADIUS,
        Math.sin(theta) * r * ORBIT_RADIUS,
      );
      dummy.rotation.set(theta, theta * 1.7, 0);
      dummy.scale.setScalar(0.75 + ((i * 37) % 10) / 28);
      dummy.updateMatrix();
      inst.setMatrixAt(i, dummy.matrix);
    }
    inst.instanceMatrix.needsUpdate = true;
  }, []);

  useFrame((state, delta) => {
    const r = root.current;
    if (!r || (r.parent !== null && !r.parent.visible)) return;
    const a = anim.current;
    const dormant = a.state === ENEMY_DORMANT;
    const telegraph = a.state === ENEMY_TELEGRAPH;
    const attack = a.state === ENEMY_ATTACK;
    const heavy = (a.flags & ENEMY_FLAG_HEAVY) !== 0;
    const p = prog01(a.prog);
    const dim = dormant ? 0.18 : 1;

    if (orbit.current) {
      const spin = dormant ? 0.12 : attack ? 1.15 : 0.5;
      orbit.current.rotation.y += delta * spin;
      let vault = 1;
      if (telegraph) {
        vault = 1 - clamp01(p) * 0.66; // collapse inward
      } else if (attack) {
        vault = 0.34 + Math.sin(clamp01(p) * Math.PI) * 1.32; // ring snap outward
      }
      orbit.current.scale.setScalar(vault);
    }
    if (moteMat.current) {
      moteMat.current.emissiveIntensity =
        (telegraph || attack ? 1.5 + p * 1.6 : 1.3) * (heavy ? 1.25 : 1) * dim;
    }
    if (coreMat.current) {
      coreMat.current.emissiveIntensity =
        (0.32 + (telegraph || attack ? p * 2.6 : 0) + (heavy ? 0.25 : 0)) * dim;
    }
  });

  return (
    <group ref={root}>
      <group ref={orbit}>
        <instancedMesh ref={moteRef} args={[undefined, undefined, MOTES]} frustumCulled={false}>
          <octahedronGeometry args={[0.085, 0]} />
          <meshStandardMaterial
            ref={moteMat}
            color="#ffc3d9"
            emissive="#ffc3d9"
            emissiveIntensity={1.3}
            roughness={0.4}
            toneMapped={false}
          />
        </instancedMesh>
        {/* hollow dark core */}
        <mesh>
          <sphereGeometry args={[0.3, 16, 16]} />
          <meshStandardMaterial
            ref={coreMat}
            color="#12060c"
            emissive="#ff5fa8"
            emissiveIntensity={0.32}
            roughness={0.35}
            metalness={0.1}
          />
        </mesh>
      </group>
    </group>
  );
}
