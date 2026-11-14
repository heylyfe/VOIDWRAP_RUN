"use client";

import { useRef } from "react";
import type { JSX } from "react";
import { useFrame } from "@react-three/fiber";
import type { Group, MeshStandardMaterial } from "three";
import { ENEMY_ATTACK, ENEMY_DORMANT, ENEMY_TELEGRAPH } from "@/protocol/generated";
import type { EnemyModelProps } from "../enemyLook";
import { clamp01, lerp, prog01 } from "../enemyLook";

/** Relaxed ring heights, bottom to top. */
const SPREAD: readonly number[] = [-0.45, -0.25, -0.05, 0.15, 0.35, 0.55];
/** Fully wound ring heights — compressed around the middle. */
const COMPRESSED: readonly number[] = SPREAD.map((_, i) => 0.05 + (i - (SPREAD.length - 1) / 2) * 0.115);

/**
 * Coil — a wound copper spring on a steel spindle. Accent #e07a5f (burnished
 * copper). Unit-radius model; the parent scales by body radius. Idle: barely
 * breathing. Telegraph (WIND): the rings pull together, the whole coil spins up and
 * shrinks. Attack (RELEASE): the wind snaps back out in a shockwave pop.
 */
export function Coil({ anim }: EnemyModelProps): JSX.Element {
  const root = useRef<Group>(null);
  const inner = useRef<Group>(null);
  const spin = useRef<Group>(null);
  const ringRefs = useRef<(Group | null)[]>([]);
  const ringMats = useRef<(MeshStandardMaterial | null)[]>([]);
  const coreMat = useRef<MeshStandardMaterial>(null);

  useFrame((state, delta) => {
    const r = root.current;
    if (!r || (r.parent !== null && !r.parent.visible)) return;
    const a = anim.current;
    const t = state.clock.elapsedTime;
    const dormant = a.state === ENEMY_DORMANT;
    const telegraph = a.state === ENEMY_TELEGRAPH;
    const attack = a.state === ENEMY_ATTACK;
    const p = prog01(a.prog);
    const dim = dormant ? 0.2 : 1;
    const winding = telegraph || attack;
    // wind amount: wire prog while winding, a slow idle breath otherwise
    const w = winding ? p : 0.12 + 0.08 * (0.5 + 0.5 * Math.sin(t * 0.9));
    const release = attack ? Math.sin(clamp01(p) * Math.PI) : 0;

    if (spin.current) {
      spin.current.rotation.y += delta * (0.6 + w * 7);
      spin.current.scale.setScalar(Math.max(0.35, 1 - 0.28 * w + release * 0.12));
    }
    if (inner.current) {
      inner.current.position.x = attack ? Math.sin(t * 45) * 0.03 * release : 0;
    }
    for (let i = 0; i < SPREAD.length; i += 1) {
      const spread = SPREAD[i];
      const compressed = COMPRESSED[i];
      const g = ringRefs.current[i];
      const m = ringMats.current[i];
      if (g && spread !== undefined && compressed !== undefined) {
        g.position.y = lerp(spread, compressed, w);
      }
      if (m) {
        m.emissiveIntensity = (0.5 + w * 2.6 + release * 2) * dim;
      }
    }
    if (coreMat.current) {
      coreMat.current.emissiveIntensity = (0.8 + w * 3 + release * 3) * dim;
    }
  });

  return (
    <group ref={root}>
      <group ref={inner}>
        {/* base */}
        <mesh position={[0, -0.62, 0]}>
          <cylinderGeometry args={[0.55, 0.62, 0.14, 16]} />
          <meshStandardMaterial color="#23211f" roughness={0.4} metalness={0.8} />
        </mesh>
        <group ref={spin}>
          {/* central spindle */}
          <mesh position={[0, 0.05, 0]}>
            <cylinderGeometry args={[0.09, 0.09, 1.55, 8]} />
            <meshStandardMaterial color="#2b2620" roughness={0.35} metalness={0.85} />
          </mesh>
          {/* the wound spring — six rings that compress as it winds */}
          {SPREAD.map((y, i) => (
            <group
              key={i}
              ref={(g) => {
                ringRefs.current[i] = g;
              }}
              position={[0, y, 0]}
            >
              <mesh rotation={[Math.PI / 2, 0, 0]}>
                <torusGeometry args={[0.42, 0.075, 8, 22]} />
                <meshStandardMaterial
                  ref={(m) => {
                    ringMats.current[i] = m;
                  }}
                  color="#3a2a22"
                  emissive="#e07a5f"
                  emissiveIntensity={0.5}
                  roughness={0.35}
                  metalness={0.85}
                  toneMapped={false}
                />
              </mesh>
            </group>
          ))}
          {/* hot core, glimpsed through the rings */}
          <mesh position={[0, 0.05, 0]}>
            <sphereGeometry args={[0.17, 12, 10]} />
            <meshStandardMaterial
              ref={coreMat}
              color="#2a1410"
              emissive="#e07a5f"
              emissiveIntensity={0.8}
              toneMapped={false}
            />
          </mesh>
          {/* top cap */}
          <mesh position={[0, 0.72, 0]}>
            <cylinderGeometry args={[0.3, 0.3, 0.1, 12]} />
            <meshStandardMaterial color="#33291f" roughness={0.35} metalness={0.85} />
          </mesh>
        </group>
      </group>
    </group>
  );
}
