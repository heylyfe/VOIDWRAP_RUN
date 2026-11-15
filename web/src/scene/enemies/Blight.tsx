"use client";

import { Suspense, useRef } from "react";
import type { JSX } from "react";
import { useFrame } from "@react-three/fiber";
import { useGLTF } from "@react-three/drei";
import type { Group, Mesh, MeshStandardMaterial, Object3D } from "three";
import { ENEMY_ATTACK, ENEMY_DORMANT, ENEMY_TELEGRAPH } from "@/protocol/generated";
import type { EnemyModelProps } from "../enemyLook";
import { clamp01, enemyLookForBehavior, prog01 } from "../enemyLook";

const MODEL_URL = "/models/blight.glb";
const LOOK = enemyLookForBehavior("blight");

/** The spill lobes, matching the nodes authored in blight.py. */
const SPILLS = [
  { key: "blight_spill_1", color: "#232b12", roughness: 0.85 },
  { key: "blight_spill_2", color: "#232b12", roughness: 0.85 },
  { key: "blight_spill_3", color: "#232b12", roughness: 0.85 },
  { key: "blight_spill_4", color: "#232b12", roughness: 0.85 },
  { key: "blight_spill_5", color: "#232b12", roughness: 0.85 },
] as const;

/** The mound lobes, matching the nodes authored in blight.py. */
const MOUNDS = [
  { key: "blight_mound_main", color: "#222a16", roughness: 0.16 },
  { key: "blight_mound_r1", color: "#1f2614", roughness: 0.14 },
  { key: "blight_mound_r2", color: "#252d18", roughness: 0.14 },
  { key: "blight_mound_l1", color: "#1e2413", roughness: 0.16 },
  { key: "blight_mound_spire", color: "#28301a", roughness: 0.15 },
  { key: "blight_mound_bump", color: "#1f2614", roughness: 0.14 },
] as const;

const SORE_KEYS = ["blight_sore_0", "blight_sore_1", "blight_sore_2", "blight_sore_3"] as const;

/** A glTF node's translation as an R3F position tuple. */
function at(node: Object3D): [number, number, number] {
  return [node.position.x, node.position.y, node.position.z];
}

/**
 * Blight — a wet, lumpy mass spreading across the floor. Accent #a3c34a (sickly green).
 * Unit-radius model; the parent scales by body radius. Cue tier 3 (ambient): no
 * telegraph; the spill creeps and the sores pulse. Attack: the mound heaves and the
 * spill surges outward.
 *
 * T-160 rebuilt the silhouette: the old uniform skirt made Blight the same shape as
 * Warden (fixed-frame IoU 0.660 against a roster p90 of 0.410). It is now a low, wide,
 * asymmetric spread — flattened lobes far out on both flanks under a lopsided mound —
 * with no radial symmetry and no disc.
 *
 * The geometry is authored in `tools/blender/scripts/blight.py` and exported to
 * `web/public/models/blight.glb`; the script is the source of truth and the `.glb` is a
 * committed build artifact. Blender is not a CI dependency. The animation below is
 * unchanged in intent and drives the named nodes that script emits.
 */
export function Blight({ anim }: EnemyModelProps): JSX.Element {
  // The glb is fetched, so the first frame suspends. Without this boundary a single
  // unloaded model would blank the whole canvas.
  return (
    <Suspense fallback={null}>
      <BlightModel anim={anim} />
    </Suspense>
  );
}

function BlightModel({ anim }: EnemyModelProps): JSX.Element {
  const { nodes } = useGLTF(MODEL_URL);
  const spill = nodes.blight_spill as Object3D;
  const mound = nodes.blight_mound as Object3D;
  const spillParts = SPILLS.map((s) => ({ ...s, node: nodes[s.key] as Mesh }));
  const moundParts = MOUNDS.map((m) => ({ ...m, node: nodes[m.key] as Mesh }));
  const soreParts = SORE_KEYS.map((key) => ({ key, node: nodes[key] as Mesh }));
  const core = nodes.blight_core as Mesh;

  const root = useRef<Group>(null);
  const spillRef = useRef<Group>(null);
  const moundRef = useRef<Group>(null);
  const spillMats = useRef<(MeshStandardMaterial | null)[]>([]);
  const moundMats = useRef<(MeshStandardMaterial | null)[]>([]);
  const soreMats = useRef<(MeshStandardMaterial | null)[]>([]);
  const coreMat = useRef<MeshStandardMaterial>(null);

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

    if (moundRef.current) {
      moundRef.current.scale.set(
        1 + Math.sin(t * 1.1 + 0.6) * 0.035,
        1 + Math.sin(t * 1.1) * 0.045,
        1 + Math.sin(t * 1.1 + 1.2) * 0.035,
      );
      moundRef.current.rotation.x = winding ? -0.18 * clamp01(p) : 0;
    }
    if (spillRef.current) {
      const creep = 0.72 + (0.5 + 0.5 * Math.sin(t * 0.6)) * 0.3 + (winding ? p * 0.38 : 0);
      spillRef.current.scale.set(creep, 1, creep);
    }
    for (const m of moundMats.current) {
      if (m) m.emissiveIntensity = (0.22 + 0.12 * Math.sin(t * 1.5)) * dim;
    }
    for (const m of spillMats.current) {
      if (m) {
        m.emissiveIntensity =
          (0.32 + 0.2 * Math.sin(t * 1.7 + 0.9) + (winding ? p * 1.1 : 0)) * dim;
      }
    }
    for (let i = 0; i < SORE_KEYS.length; i += 1) {
      const m = soreMats.current[i];
      if (m) {
        m.emissiveIntensity =
          (0.75 + Math.sin(t * 2.4 + i * 2.1) * 0.4 + (winding ? p * 2 : 0)) * dim;
      }
    }
    if (coreMat.current) {
      coreMat.current.emissiveIntensity =
        (1.6 + Math.sin(t * 2.4) * 0.5 + (winding ? p * 2.4 : 0)) * dim;
    }
  });

  return (
    <group ref={root}>
      {/* spreading discoloured spill — the infected floor read */}
      <group ref={spillRef} position={at(spill)}>
        {spillParts.map(({ key, color, roughness, node }, i) => (
          <mesh key={key} geometry={node.geometry} position={at(node)}>
            <meshStandardMaterial
              ref={(m) => {
                spillMats.current[i] = m;
              }}
              color={color}
              emissive={LOOK.accent}
              emissiveIntensity={0.35}
              transparent
              opacity={0.92}
              roughness={roughness}
              metalness={0}
            />
          </mesh>
        ))}
      </group>
      {/* wet lumpy mound — lopsided, no radial symmetry */}
      <group ref={moundRef} position={at(mound)}>
        {moundParts.map(({ key, color, roughness, node }, i) => (
          <mesh key={key} geometry={node.geometry} position={at(node)}>
            <meshStandardMaterial
              ref={(m) => {
                moundMats.current[i] = m;
              }}
              color={color}
              emissive={LOOK.accent}
              emissiveIntensity={0.22}
              roughness={roughness}
              metalness={0.05}
            />
          </mesh>
        ))}
      </group>
      {/* sores — sickly embers under the skin */}
      {soreParts.map(({ key, node }, i) => (
        <mesh key={key} geometry={node.geometry} position={at(node)}>
          <meshStandardMaterial
            ref={(m) => {
              soreMats.current[i] = m;
            }}
            color="#39441c"
            emissive={LOOK.glow}
            emissiveIntensity={0.75}
            toneMapped={false}
          />
        </mesh>
      ))}
      {/* the core — a hot ember buried in the mound */}
      <mesh geometry={core.geometry} position={at(core)}>
        <meshStandardMaterial
          ref={coreMat}
          color="#a3c34a"
          emissive={LOOK.accent}
          emissiveIntensity={1.6}
          roughness={0.3}
          metalness={0.1}
          toneMapped={false}
        />
      </mesh>
    </group>
  );
}
