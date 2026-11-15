"use client";

import { Suspense, useRef } from "react";
import type { JSX } from "react";
import { useFrame } from "@react-three/fiber";
import { useGLTF } from "@react-three/drei";
import type { Group, Mesh, MeshStandardMaterial } from "three";
import { ENEMY_ATTACK, ENEMY_DORMANT, ENEMY_TELEGRAPH } from "@/protocol/generated";
import type { EnemyModelProps } from "../enemyLook";
import { clamp01, enemyLookForBehavior, lerp, prog01 } from "../enemyLook";

const MODEL_URL = "/models/coil.glb";
const LOOK = enemyLookForBehavior("coil");

/** Relaxed ring heights, bottom to top — the authored positions in coil.py. */
const SPREAD: readonly number[] = [-0.523, -0.273, -0.023, 0.227, 0.477];
/** Fully wound ring heights — compressed around the middle. */
const COMPRESSED: readonly number[] = SPREAD.map((_, i) => (i - (SPREAD.length - 1) / 2) * 0.115);

/** A glTF node's translation as an R3F position tuple. */
function at(node: Mesh): [number, number, number] {
  return [node.position.x, node.position.y, node.position.z];
}

/**
 * Coil — a wound copper spring on a steel spindle. Accent #e07a5f (burnished copper).
 *
 * T-157 rebuilt the silhouette: it used to be a rounded column that read as the same
 * shape as Peal (fixed-frame IoU 0.780 against a roster p90 of 0.432). It is now a
 * 1.20-wide flat base under a 0.56-wide column 1.70 tall — a "T" against Peal's squat
 * trapezoid.
 *
 * The geometry is authored in `tools/blender/scripts/coil.py` and exported to
 * `web/public/models/coil.glb`; the script is the source of truth and the `.glb` is a
 * committed build artifact. Blender is not a CI dependency. The animation below is
 * unchanged in intent and drives the named nodes that script emits.
 */
export function Coil({ anim }: EnemyModelProps): JSX.Element {
  return (
    <Suspense fallback={null}>
      <CoilModel anim={anim} />
    </Suspense>
  );
}

function CoilModel({ anim }: EnemyModelProps): JSX.Element {
  const { nodes } = useGLTF(MODEL_URL);
  const base = nodes.coil_base as Mesh;
  const core = nodes.coil_core as Mesh;
  const cap = nodes.coil_cap as Mesh;
  const rings = SPREAD.map((_, i) => nodes[`coil_ring_${i}`] as Mesh);

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
    // Wind amount: wire prog while winding, a slow idle breath otherwise.
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
      if (m) m.emissiveIntensity = (0.5 + w * 2.6 + release * 2) * dim;
    }
    if (coreMat.current) {
      coreMat.current.emissiveIntensity = (0.8 + w * 3 + release * 3) * dim;
    }
  });

  return (
    <group ref={root}>
      <group ref={inner}>
        {/* the base stays put; the spindle, rings and cap spin together */}
        <mesh geometry={base.geometry} position={at(base)}>
          <meshStandardMaterial color="#22242a" roughness={0.5} metalness={0.7} />
        </mesh>
        <group ref={spin}>
          <mesh geometry={core.geometry} position={at(core)}>
            <meshStandardMaterial
              ref={coreMat}
              color="#ffb59a"
              emissive={LOOK.glow}
              emissiveIntensity={0.8}
              toneMapped={false}
            />
          </mesh>
          {rings.map((ring, i) => (
            <group
              key={i}
              ref={(g) => {
                ringRefs.current[i] = g;
              }}
              position={at(ring)}
            >
              <mesh geometry={ring.geometry}>
                <meshStandardMaterial
                  ref={(m) => {
                    ringMats.current[i] = m;
                  }}
                  color="#8a8f9a"
                  emissive={LOOK.accent}
                  emissiveIntensity={0.5}
                  roughness={0.4}
                  metalness={0.6}
                  toneMapped={false}
                />
              </mesh>
            </group>
          ))}
          <mesh geometry={cap.geometry} position={at(cap)}>
            <meshStandardMaterial
              color="#e07a5f"
              emissive={LOOK.accent}
              emissiveIntensity={3}
              roughness={0.4}
              metalness={0.6}
              toneMapped={false}
            />
          </mesh>
        </group>
      </group>
    </group>
  );
}
