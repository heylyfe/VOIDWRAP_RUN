"use client";

import { useEffect, useMemo, useRef } from "react";
import type { JSX } from "react";
import { useFrame } from "@react-three/fiber";
import { DoubleSide, Shape, ShapeGeometry } from "three";
import type { Group, MeshStandardMaterial } from "three";
import { ENEMY_ATTACK, ENEMY_DORMANT, ENEMY_TELEGRAPH } from "@/protocol/generated";
import type { EnemyModelProps } from "../enemyLook";
import { prog01 } from "../enemyLook";

const STAR_POINTS = 5;
const STAR_OUTER = 0.62;
const STAR_INNER = 0.27;

/** Five-pointed star outline, drawn in the shape's XY plane. */
function starShape(): Shape {
  const shape = new Shape();
  for (let i = 0; i < STAR_POINTS * 2; i += 1) {
    const radius = i % 2 === 0 ? STAR_OUTER : STAR_INNER;
    const angle = (i / (STAR_POINTS * 2)) * Math.PI * 2 + Math.PI / 2;
    const x = Math.cos(angle) * radius;
    const y = Math.sin(angle) * radius;
    if (i === 0) shape.moveTo(x, y);
    else shape.lineTo(x, y);
  }
  shape.closePath();
  return shape;
}

/**
 * Lure — the owner's star-shaped fog creature: a flat five-point star silhouette
 * sitting in a purple fog pool, pulsing slowly so it reads as a gift at distance.
 * Accent #a55eea (lure purple). Unit-radius model; the parent scales by body
 * radius. Deliberately flat and low. Telegraph (the false floor shimmering):
 * the pool and star brighten; attack: a full sear before it drops you.
 */
export function Lure({ anim }: EnemyModelProps): JSX.Element {
  const root = useRef<Group>(null);
  const starSpin = useRef<Group>(null);
  const poolMat = useRef<MeshStandardMaterial>(null);
  const innerPoolMat = useRef<MeshStandardMaterial>(null);
  const starMat = useRef<MeshStandardMaterial>(null);
  const coreMat = useRef<MeshStandardMaterial>(null);
  const starGeo = useMemo(() => new ShapeGeometry(starShape(), 4), []);

  useEffect(
    () => () => {
      starGeo.dispose();
    },
    [starGeo],
  );

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
    const pulse = 0.5 + 0.5 * Math.sin(t * 1.5);

    if (poolMat.current) {
      poolMat.current.opacity = (0.3 + pulse * 0.22 + (winding ? p * 0.3 : 0)) * dim;
      poolMat.current.emissiveIntensity = (0.4 + pulse * 0.35 + (winding ? p * 1.6 : 0)) * dim;
    }
    if (innerPoolMat.current) {
      innerPoolMat.current.opacity = (0.34 + pulse * 0.26 + (winding ? p * 0.32 : 0)) * dim;
      innerPoolMat.current.emissiveIntensity = (0.55 + pulse * 0.45 + (winding ? p * 2 : 0)) * dim;
    }
    if (starSpin.current) {
      starSpin.current.rotation.y = t * 0.15;
      const s = 1 + Math.sin(t * 1.5) * 0.055 + (winding ? p * 0.12 : 0);
      starSpin.current.scale.set(s, s, 1);
    }
    if (starMat.current) {
      starMat.current.emissiveIntensity = (0.85 + pulse * 0.75 + (winding ? p * 2.6 : 0)) * dim;
    }
    if (coreMat.current) {
      coreMat.current.emissiveIntensity = (1.3 + pulse * 1.1 + (winding ? p * 3.2 : 0)) * dim;
    }
  });

  return (
    <group ref={root}>
      {/* the purple fog pool — flat on the ground */}
      <mesh position={[0, 0.004, 0]} rotation={[-Math.PI / 2, 0, 0]}>
        <circleGeometry args={[0.95, 36]} />
        <meshStandardMaterial
          ref={poolMat}
          color="#241033"
          emissive="#a55eea"
          emissiveIntensity={0.4}
          transparent
          opacity={0.34}
          depthWrite={false}
          side={DoubleSide}
        />
      </mesh>
      <mesh position={[0, 0.012, 0]} rotation={[-Math.PI / 2, 0, 0]}>
        <circleGeometry args={[0.55, 28]} />
        <meshStandardMaterial
          ref={innerPoolMat}
          color="#2e1545"
          emissive="#a55eea"
          emissiveIntensity={0.55}
          transparent
          opacity={0.4}
          depthWrite={false}
          side={DoubleSide}
        />
      </mesh>
      {/* the five-point star silhouette — flat, slow, gift-like */}
      <group ref={starSpin} position={[0, 0.03, 0]}>
        <mesh geometry={starGeo} rotation={[-Math.PI / 2, 0, 0]}>
          <meshStandardMaterial
            ref={starMat}
            color="#2a1240"
            emissive="#a55eea"
            emissiveIntensity={0.85}
            roughness={0.35}
            metalness={0.1}
            side={DoubleSide}
            toneMapped={false}
            depthWrite={false}
          />
        </mesh>
      </group>
      {/* bright core — the false gift's shimmer */}
      <mesh position={[0, 0.045, 0]} rotation={[-Math.PI / 2, 0, 0]}>
        <circleGeometry args={[0.15, 20]} />
        <meshStandardMaterial
          ref={coreMat}
          color="#d8b0ff"
          emissive="#d8b0ff"
          emissiveIntensity={1.3}
          transparent
          opacity={0.9}
          toneMapped={false}
          depthWrite={false}
          side={DoubleSide}
        />
      </mesh>
    </group>
  );
}
