"use client";

import { Suspense, useRef } from "react";
import type { JSX } from "react";
import { useFrame } from "@react-three/fiber";
import { useGLTF } from "@react-three/drei";
import type { Group, Mesh, MeshStandardMaterial } from "three";
import { ENEMY_ATTACK, ENEMY_DORMANT, ENEMY_TELEGRAPH } from "@/protocol/generated";
import type { EnemyModelProps } from "../enemyLook";
import { enemyLookForBehavior, prog01, yawToward } from "../enemyLook";

const MODEL_URL = "/models/warden.glb";
const LOOK = enemyLookForBehavior("warden");

/** A glTF node's translation as an R3F position tuple. */
function at(node: Mesh): [number, number, number] {
  return [node.position.x, node.position.y, node.position.z];
}

/**
 * Warden — a tall grave-post with a ward halo held high on a spike; the halo
 * IS the model. Accent #f4d35e (ward gold). Unit-radius model; the parent scales by
 * body radius. Idle: the halo breathes. Telegraph (GUARD): halo flares and swells.
 * Attack: rears back as the ward shoves.
 *
 * T-160 rebuilt the silhouette: the old base ring made Warden the same shape as
 * Blight (fixed-frame IoU 0.660 against a roster p90 of 0.410). It is now a tall
 * narrow post threaded by a spike with a flat halo held high — a vertical read
 * with a distinctive top, not a cone on a plate.
 *
 * The geometry is authored in `tools/blender/scripts/warden.py` and exported to
 * `web/public/models/warden.glb`; the script is the source of truth and the `.glb` is a
 * committed build artifact. Blender is not a CI dependency. The animation below is
 * unchanged in intent and drives the named nodes that script emits.
 */
export function Warden({ anim }: EnemyModelProps): JSX.Element {
  // The glb is fetched, so the first frame suspends. Without this boundary a single
  // unloaded model would blank the whole canvas.
  return (
    <Suspense fallback={null}>
      <WardenModel anim={anim} />
    </Suspense>
  );
}

function WardenModel({ anim }: EnemyModelProps): JSX.Element {
  const { nodes } = useGLTF(MODEL_URL);
  const post = nodes.warden_post as Mesh;
  const finial = nodes.warden_finial as Mesh;
  const halo = nodes.warden_halo as Mesh;
  const eye = nodes.warden_eye as Mesh;

  const root = useRef<Group>(null);
  const inner = useRef<Group>(null);
  const haloRef = useRef<Group>(null);
  const haloMat = useRef<MeshStandardMaterial>(null);
  const finialMat = useRef<MeshStandardMaterial>(null);

  useFrame((state) => {
    const r = root.current;
    if (!r || (r.parent !== null && !r.parent.visible)) return;
    const a = anim.current;
    const t = state.clock.elapsedTime;
    const dormant = a.state === ENEMY_DORMANT;
    const telegraph = a.state === ENEMY_TELEGRAPH;
    const attack = a.state === ENEMY_ATTACK;
    const p = prog01(a.prog);
    const dim = dormant ? 0.18 : 1;
    const winding = telegraph || attack;

    if (winding) {
      r.rotation.y = yawToward(a.ex, a.ez, a.tx, a.tz);
    }

    if (inner.current) {
      inner.current.position.y = Math.sin(t * 1.4) * 0.05;
      inner.current.rotation.x = attack ? -p * 0.16 : 0;
      inner.current.rotation.z = Math.sin(t * 0.9) * 0.02;
    }
    if (haloRef.current) {
      const swell = winding
        ? 1 + p * 0.18 + Math.sin(t * 9) * 0.04 * p
        : 1 + Math.sin(t * 2.2) * 0.02;
      haloRef.current.scale.set(swell, 1, swell);
    }
    if (haloMat.current) {
      haloMat.current.emissiveIntensity =
        (1.15 + Math.sin(t * 2.2) * 0.35 + (winding ? p * 2.6 : 0)) * dim;
    }
    if (finialMat.current) {
      finialMat.current.emissiveIntensity =
        ((dormant ? 0.35 : 1.4) + (winding ? p * 2 : 0)) * dim;
    }
  });

  return (
    <group ref={root}>
      <group ref={inner}>
        {/* post, plinth and capital — one revolved profile */}
        <mesh geometry={post.geometry} position={at(post)}>
          <meshStandardMaterial color="#1c1a17" roughness={0.62} metalness={0.45} />
        </mesh>
        {/* ward finial — the spear light */}
        <mesh geometry={finial.geometry} position={at(finial)}>
          <meshStandardMaterial
            ref={finialMat}
            color="#f4d35e"
            emissive={LOOK.accent}
            emissiveIntensity={1.4}
            roughness={0.35}
            metalness={0.6}
            toneMapped={false}
          />
        </mesh>
        {/* the ward halo — held high on the spike */}
        <group ref={haloRef} position={at(halo)}>
          <mesh geometry={halo.geometry}>
            <meshStandardMaterial
              ref={haloMat}
              color="#3a2f10"
              emissive={LOOK.accent}
              emissiveIntensity={1.15}
              roughness={0.35}
              metalness={0.7}
              toneMapped={false}
            />
          </mesh>
        </group>
        {/* slit eye — cold sentry light */}
        <mesh geometry={eye.geometry} position={at(eye)}>
          <meshStandardMaterial
            color="#2a2410"
            emissive={LOOK.glow}
            emissiveIntensity={1.1}
            toneMapped={false}
          />
        </mesh>
      </group>
    </group>
  );
}
