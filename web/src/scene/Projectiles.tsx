"use client";

import { useEffect, useMemo, useRef } from "react";
import type { JSX } from "react";
import { useFrame } from "@react-three/fiber";
import { AdditiveBlending, MeshBasicMaterial } from "three";
import type { Group } from "three";
import type { GameSession } from "@/game/session";
import { COMMON } from "./theme";

export const MAX_PROJECTILES = 64;

/**
 * Pooled enemy bolts. Fixed-size pool of groups, toggled visible; positions come from
 * the newest server snapshot (lightly extrapolated), same policy as enemies.
 * Bright toneMapped=false materials so bloom picks them up.
 */
export function Projectiles({ session }: { session: GameSession }): JSX.Element {
  const slots = useRef<(Group | null)[]>(Array.from({ length: MAX_PROJECTILES }, () => null));
  const coreMat = useMemo(() => new MeshBasicMaterial({ color: "#ffe0f4", toneMapped: false }), []);
  const haloMat = useMemo(
    () =>
      new MeshBasicMaterial({
        color: COMMON.enemy.glow,
        transparent: true,
        opacity: 0.4,
        blending: AdditiveBlending,
        depthWrite: false,
        toneMapped: false,
      }),
    [],
  );
  useEffect(() => () => coreMat.dispose(), [coreMat]);
  useEffect(() => () => haloMat.dispose(), [haloMat]);

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
          <mesh material={coreMat}>
            <sphereGeometry args={[0.13, 8, 8]} />
          </mesh>
          <mesh material={haloMat}>
            <sphereGeometry args={[0.3, 8, 8]} />
          </mesh>
        </group>
      ))}
    </group>
  );
}
