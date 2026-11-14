"use client";

import { useRef } from "react";
import type { JSX } from "react";
import { useFrame } from "@react-three/fiber";
import { AdditiveBlending } from "three";
import type { Group, Mesh, MeshBasicMaterial, MeshStandardMaterial } from "three";
import { ENEMY_ATTACK, ENEMY_DISGUISED, ENEMY_DORMANT, ENEMY_RECOVER, ENEMY_TELEGRAPH } from "@/protocol/generated";
import type { Vec3 } from "@/protocol/generated";
import type { EnemyModelProps } from "../enemyLook";
import {
  MIMIC_GIFT_SCALE,
  mimicShimmerOpacity,
  mimicShimmerVisible,
  prog01,
  yawToward,
} from "../enemyLook";
import { GiftBox } from "../GiftBox";

const ORIGIN: Vec3 = { x: 0, y: 0, z: 0 };

/**
 * False Gift — rotting-satin present, breathing ribbon, lid that splits into a
 * jaw of glass teeth. Accent #ff3df0 (magenta), violet inner glow. Unit-radius model.
 *
 * While DISGUISED it renders exactly the shared GiftBox (wrapped in a fixed 0.34/0.7
 * scale so it is never scaled by the body-radius slot), plus a periodic 0.5 s magenta
 * shimmer every 3.0 s as the only tell. HISS (TELEGRAPH): lid peels, body glitch-jitters.
 * LUNGE (ATTACK): lid fully open, jaw lurches. SPENT (RECOVER): inert wobble.
 */
export function FalseGift({ anim }: EnemyModelProps): JSX.Element {
  const root = useRef<Group>(null);
  const disguised = useRef<Group>(null);
  const jaw = useRef<Group>(null);
  const lidL = useRef<Mesh>(null);
  const lidR = useRef<Mesh>(null);
  const teeth = useRef<Group>(null);
  const glow = useRef<Mesh>(null);
  const glowMat = useRef<MeshBasicMaterial>(null);
  const shimmer = useRef<Mesh>(null);
  const shimmerMat = useRef<MeshBasicMaterial>(null);
  const ribbonMats = useRef<(MeshStandardMaterial | null)[]>([]);
  const jawGlowMat = useRef<MeshStandardMaterial>(null);

  useFrame((state) => {
    const r = root.current;
    if (!r || (r.parent !== null && !r.parent.visible)) return;
    const a = anim.current;
    const t = state.clock.elapsedTime;
    const p = prog01(a.prog);
    const isDisguised = a.state === ENEMY_DISGUISED;
    const dormant = a.state === ENEMY_DORMANT;
    const telegraph = a.state === ENEMY_TELEGRAPH;
    const attack = a.state === ENEMY_ATTACK;
    const spent = a.state === ENEMY_RECOVER;
    const dim = dormant ? 0.18 : 1;

    if (disguised.current) disguised.current.visible = isDisguised;
    if (jaw.current) jaw.current.visible = !isDisguised;

    if (isDisguised) {
      // periodic 0.5 s magenta shimmer — the only tell
      const on = mimicShimmerVisible(t);
      if (shimmer.current) shimmer.current.visible = on;
      if (shimmerMat.current) shimmerMat.current.opacity = mimicShimmerOpacity(t) * 0.55;
      return;
    }

    if (telegraph || attack) {
      r.rotation.y = yawToward(a.ex, a.ez, a.tx, a.tz);
      // glitch jitter while hissing
      const j = 0.03 * (0.35 + p);
      r.position.set(
        Math.sin(t * 39.7) * Math.sin(t * 23.3) * j,
        Math.sin(t * 31.1 + 1.7) * j * 0.6,
        Math.sin(t * 44.3 + 0.6) * j,
      );
    } else {
      r.position.set(0, 0, 0);
    }

    const open = telegraph ? p : attack ? 1 : spent ? 0.22 : 0.04;
    if (lidL.current) lidL.current.rotation.z = open * 1.02;
    if (lidR.current) lidR.current.rotation.z = -open * 1.02;
    if (teeth.current) teeth.current.scale.set(1, 1, 1 + (attack ? p * 0.55 : 0));
    if (glow.current) glow.current.scale.setScalar(0.32 + open * 0.62);
    if (glowMat.current) glowMat.current.opacity = open * 0.72;
    if (jawGlowMat.current) jawGlowMat.current.emissiveIntensity = (0.9 + open * 1.6) * dim;
    // breathing ribbon
    const breathe = 1 + Math.sin(t * 2.2) * 0.07;
    for (const m of ribbonMats.current) {
      if (m) m.emissiveIntensity = (0.55 + Math.sin(t * 2.2) * 0.22) * dim * breathe;
    }
    // SPENT wobble — inert, harmless
    r.rotation.z = spent ? Math.sin(t * 3.1) * 0.1 : 0;
  });

  return (
    <group ref={root}>
      {/* DISGUISED — identical to a normal gift (0.34 u) + shimmer */}
      <group ref={disguised}>
        <group scale={MIMIC_GIFT_SCALE}>
          <GiftBox position={ORIGIN} taken={false} />
        </group>
        <mesh ref={shimmer} position={[0, 0.05, 0]} visible={false}>
          <boxGeometry args={[0.46, 0.44, 0.46]} />
          <meshBasicMaterial
            ref={shimmerMat}
            color="#ff3df0"
            transparent
            opacity={0}
            blending={AdditiveBlending}
            depthWrite={false}
            toneMapped={false}
          />
        </mesh>
      </group>

      {/* JAW FORM — rotting satin body, split glass-teeth lid */}
      <group ref={jaw} visible={false}>
        {/* body */}
        <mesh position={[0, -0.08, 0]}>
          <boxGeometry args={[0.82, 0.62, 0.82]} />
          <meshStandardMaterial color="#4a1f33" roughness={0.62} metalness={0.15} />
        </mesh>
        {/* breathing ribbon cross */}
        {[0, Math.PI / 2].map((rot, i) => (
          <mesh key={i} position={[0, -0.08, 0]} rotation={[0, rot, 0]}>
            <boxGeometry args={[0.86, 0.66, 0.16]} />
            <meshStandardMaterial
              ref={(m) => {
                ribbonMats.current[i] = m;
              }}
              color="#2b071f"
              emissive="#ff3df0"
              emissiveIntensity={0.55}
              toneMapped={false}
            />
          </mesh>
        ))}
        {/* violet inner glow — scales with lid opening */}
        <mesh ref={glow} position={[0, 0.24, 0]}>
          <sphereGeometry args={[1, 10, 10]} />
          <meshBasicMaterial
            ref={glowMat}
            color="#a64dff"
            transparent
            opacity={0}
            blending={AdditiveBlending}
            depthWrite={false}
            toneMapped={false}
          />
        </mesh>
        {/* jaw of glass teeth — lower row */}
        <group ref={teeth} position={[0, 0.06, 0]}>
          {[-0.28, -0.14, 0, 0.14, 0.28].map((x, i) => (
            <mesh key={`b${i}`} position={[x, -0.02, 0.3]} rotation={[Math.PI, 0, 0]}>
              <coneGeometry args={[0.055, 0.22, 5]} />
              <meshStandardMaterial
                ref={i === 0 ? jawGlowMat : undefined}
                color="#cfa6ff"
                emissive="#b46bff"
                emissiveIntensity={1.35}
                roughness={0.12}
              />
            </mesh>
          ))}
          {[-0.28, -0.14, 0, 0.14, 0.28].map((x, i) => (
            <mesh key={`t${i}`} position={[x, 0.14, 0.3]}>
              <coneGeometry args={[0.05, 0.2, 5]} />
              <meshStandardMaterial color="#cfa6ff" emissive="#b46bff" emissiveIntensity={1.2} roughness={0.12} />
            </mesh>
          ))}
        </group>
        {/* lid halves — peel upward on HISS/LUNGE */}
        <mesh ref={lidL} position={[-0.205, 0.32, 0]}>
          <boxGeometry args={[0.41, 0.13, 0.8]} />
          <meshStandardMaterial color="#3a1626" roughness={0.55} metalness={0.2} emissive="#ff3df0" emissiveIntensity={0.12} />
        </mesh>
        <mesh ref={lidR} position={[0.205, 0.32, 0]}>
          <boxGeometry args={[0.41, 0.13, 0.8]} />
          <meshStandardMaterial color="#3a1626" roughness={0.55} metalness={0.2} emissive="#ff3df0" emissiveIntensity={0.12} />
        </mesh>
      </group>
    </group>
  );
}
