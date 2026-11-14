"use client";

import { useEffect, useMemo, useRef } from "react";
import type { JSX } from "react";
import { useFrame } from "@react-three/fiber";
import { AdditiveBlending, MeshBasicMaterial } from "three";
import type { Group } from "three";
import type { GameSession } from "@/game/session";

export const MAX_PROJECTILES = 64;

/**
 * Pooled falling gale_wight shells (turret bolts share the same pool and visual):
 * a hot glowing shell with a vertical ion streak trailing above it, since shells
 * drop ~12 u onto their marked circle. Positions come from the newest server
 * snapshot; the streak is static-length and always points up.
 */
export function Projectiles({ session }: { session: GameSession }): JSX.Element {
  const slots = useRef<(Group | null)[]>(Array.from({ length: MAX_PROJECTILES }, () => null));
  const coreMat = useMemo(() => new MeshBasicMaterial({ color: "#fff2ff", toneMapped: false }), []);
  const shellMat = useMemo(
    () =>
      new MeshBasicMaterial({
        color: "#d94fff",
        transparent: true,
        opacity: 0.9,
        blending: AdditiveBlending,
        depthWrite: false,
        toneMapped: false,
      }),
    [],
  );
  const haloMat = useMemo(
    () =>
      new MeshBasicMaterial({
        color: "#b47aff",
        transparent: true,
        opacity: 0.42,
        blending: AdditiveBlending,
        depthWrite: false,
        toneMapped: false,
      }),
    [],
  );
  const streakMat = useMemo(
    () =>
      new MeshBasicMaterial({
        color: "#d94fff",
        transparent: true,
        opacity: 0.34,
        blending: AdditiveBlending,
        depthWrite: false,
        toneMapped: false,
      }),
    [],
  );
  const streakCoreMat = useMemo(
    () =>
      new MeshBasicMaterial({
        color: "#eec4ff",
        transparent: true,
        opacity: 0.62,
        blending: AdditiveBlending,
        depthWrite: false,
        toneMapped: false,
      }),
    [],
  );
  useEffect(
    () => () => {
      coreMat.dispose();
      shellMat.dispose();
      haloMat.dispose();
      streakMat.dispose();
      streakCoreMat.dispose();
    },
    [coreMat, shellMat, haloMat, streakMat, streakCoreMat],
  );

  useFrame(() => {
    const now = performance.now();
    let i = 0;
    for (const p of session.projectilePositions(now).values()) {
      if (i >= MAX_PROJECTILES) break;
      const slot = slots.current[i];
      if (slot) {
        slot.visible = true;
        slot.position.set(p.x, p.y, p.z);
      }
      i += 1;
    }
    for (; i < MAX_PROJECTILES; i += 1) {
      const slot = slots.current[i];
      if (slot) slot.visible = false;
    }
  });

  return (
    <group>
      {Array.from({ length: MAX_PROJECTILES }, (_, i) => (
        <group
          key={i}
          ref={(g) => {
            slots.current[i] = g;
          }}
          visible={false}
        >
          {/* shell core + glow */}
          <mesh material={coreMat}>
            <sphereGeometry args={[0.15, 10, 10]} />
          </mesh>
          <mesh material={shellMat}>
            <sphereGeometry args={[0.25, 10, 10]} />
          </mesh>
          <mesh material={haloMat}>
            <sphereGeometry args={[0.4, 8, 8]} />
          </mesh>
          {/* vertical streak above the falling shell */}
          <mesh material={streakMat} position={[0, 0.95, 0]}>
            <cylinderGeometry args={[0.06, 0.17, 1.7, 6, 1, true]} />
          </mesh>
          <mesh material={streakCoreMat} position={[0, 0.7, 0]}>
            <cylinderGeometry args={[0.032, 0.09, 1.1, 6, 1, true]} />
          </mesh>
        </group>
      ))}
    </group>
  );
}
