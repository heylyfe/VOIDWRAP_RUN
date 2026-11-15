"use client";

import { Suspense, useRef } from "react";
import type { JSX } from "react";
import { useFrame } from "@react-three/fiber";
import { useGLTF } from "@react-three/drei";
import type { Group, Mesh, MeshStandardMaterial } from "three";
import { ENEMY_ATTACK, ENEMY_DORMANT, ENEMY_TELEGRAPH } from "@/protocol/generated";
import type { EnemyModelProps } from "../enemyLook";
import { clamp01, enemyLookForBehavior, prog01, yawToward } from "../enemyLook";

const MODEL_URL = "/models/peal.glb";
const LOOK = enemyLookForBehavior("peal");

/** A glTF node's translation as an R3F position tuple. */
function at(node: Mesh): [number, number, number] {
  return [node.position.x, node.position.y, node.position.z];
}

/**
 * Peal — a wide squat bronze bell whose two shell halves swing apart at the lip as
 * the ring charges. Accent #9d0208 (oxblood bell-mouth).
 *
 * T-157 rebuilt the silhouette: the old tall column read as the same shape as Coil
 * (fixed-frame IoU 0.780 against a roster p90 of 0.432). It is now a squat flared
 * trapezoid with the clapper hanging clear below the lip, so the outline has a
 * bottom-centre tail nothing else in the roster has.
 *
 * The geometry is authored in `tools/blender/scripts/peal.py` and exported to
 * `web/public/models/peal.glb`; the script is the source of truth and the `.glb` is a
 * committed build artifact. Blender is not a CI dependency. The animation below is
 * unchanged in intent and drives the named nodes that script emits.
 */
export function Peal({ anim }: EnemyModelProps): JSX.Element {
  // The glb is fetched, so the first frame suspends. Without this boundary a single
  // unloaded model would blank the whole canvas.
  return (
    <Suspense fallback={null}>
      <PealModel anim={anim} />
    </Suspense>
  );
}

function PealModel({ anim }: EnemyModelProps): JSX.Element {
  const { nodes } = useGLTF(MODEL_URL);
  const shellL = nodes.peal_shell_l as Mesh;
  const shellR = nodes.peal_shell_r as Mesh;
  const clapper = nodes.peal_clapper as Mesh;
  const lip = nodes.peal_lip as Mesh;
  const glow = nodes.peal_glow as Mesh;
  const crown = nodes.peal_crown as Mesh;

  const root = useRef<Group>(null);
  const inner = useRef<Group>(null);
  const shellLRef = useRef<Group>(null);
  const shellRRef = useRef<Group>(null);
  const clapperRef = useRef<Group>(null);
  const lipRef = useRef<Mesh>(null);
  const lipMat = useRef<MeshStandardMaterial>(null);
  const glowMat = useRef<MeshStandardMaterial>(null);
  const shellMats = useRef<(MeshStandardMaterial | null)[]>([]);

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
    const winding = telegraph || attack;

    if (winding) {
      r.rotation.y = yawToward(a.ex, a.ez, a.tx, a.tz);
    }

    // How open the mouth is: telegraph winds it open; attack clangs open then shut.
    const open = telegraph ? p : attack ? Math.sin(clamp01(p) * Math.PI) : 0;

    if (shellLRef.current) shellLRef.current.rotation.z = -open * 0.42;
    if (shellRRef.current) shellRRef.current.rotation.z = open * 0.42;
    if (inner.current) {
      inner.current.position.y = Math.sin(t * 1.7) * 0.06;
      inner.current.rotation.x = winding ? -p * 0.22 : 0;
      inner.current.rotation.z = Math.sin(t * 0.8) * 0.03;
    }
    if (clapperRef.current) {
      clapperRef.current.rotation.z =
        Math.sin(t * (winding ? 11 : 1.6)) * (winding ? 0.35 + p * 0.75 : 0.12);
    }
    if (lipRef.current) {
      // The lip ring lies in the XZ plane after the glTF Y-up conversion.
      const s = 1 + open * 0.18;
      lipRef.current.scale.set(s, 1, s);
    }
    if (lipMat.current) {
      lipMat.current.emissiveIntensity = (0.9 + Math.sin(t * 2.6) * 0.25 + open * 3.2) * dim;
    }
    if (glowMat.current) {
      glowMat.current.emissiveIntensity = (1 + open * 2.6) * dim;
    }
    for (const m of shellMats.current) {
      if (m) m.emissiveIntensity = (0.18 + open * 1.1) * dim;
    }
  });

  const shellMaterial = (slot: number) => (
    <meshStandardMaterial
      ref={(m) => {
        shellMats.current[slot] = m;
      }}
      color="#a8845a"
      emissive={LOOK.accent}
      emissiveIntensity={0.18}
      roughness={0.45}
      metalness={0.55}
    />
  );

  return (
    <group ref={root}>
      <group ref={inner}>
        <mesh geometry={crown.geometry} position={at(crown)}>
          <meshStandardMaterial
            color="#9d0208"
            emissive={LOOK.accent}
            emissiveIntensity={2.5}
            roughness={0.45}
            metalness={0.55}
            toneMapped={false}
          />
        </mesh>
        {/* two shell halves, pivoting at the crown so the lip splays open */}
        <group ref={shellLRef} position={at(shellL)}>
          <mesh geometry={shellL.geometry}>{shellMaterial(0)}</mesh>
        </group>
        <group ref={shellRRef} position={at(shellR)}>
          <mesh geometry={shellR.geometry}>{shellMaterial(1)}</mesh>
        </group>
        {/* lip ring — the mouth rim */}
        <mesh ref={lipRef} geometry={lip.geometry} position={at(lip)}>
          <meshStandardMaterial
            ref={lipMat}
            color="#7a6242"
            emissive={LOOK.accent}
            emissiveIntensity={0.9}
            roughness={0.55}
            metalness={0.45}
            toneMapped={false}
          />
        </mesh>
        {/* inner mouth glow */}
        <mesh geometry={glow.geometry} position={at(glow)}>
          <meshStandardMaterial
            ref={glowMat}
            color="#ffd9a0"
            emissive={LOOK.glow}
            emissiveIntensity={1}
            toneMapped={false}
          />
        </mesh>
        {/* clapper — hangs clear of the lip, which is what gives the outline its tail */}
        <group ref={clapperRef} position={at(clapper)}>
          <mesh geometry={clapper.geometry}>
            <meshStandardMaterial color="#7a6242" roughness={0.55} metalness={0.45} />
          </mesh>
        </group>
      </group>
    </group>
  );
}
