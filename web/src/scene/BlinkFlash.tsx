"use client";

import { useRef } from "react";
import type { JSX } from "react";
import { useFrame } from "@react-three/fiber";
import { AdditiveBlending } from "three";
import type { Group, MeshBasicMaterial } from "three";
import { CLASS_BULWARK } from "@/protocol/generated";
import type { GameSession } from "@/game/session";
import { classColor } from "./classColors";

const MAX_FLASHES = 4;
const FLASH_TIME = 0.45;

interface FlashSlot {
  x: number;
  y: number;
  z: number;
  t0: number;
  active: boolean;
}

/**
 * Blinker ability: when the server's abilityActiveFrames jumps upward (a blink just
 * started), spawn a brief expanding afterimage sphere at the position the player occupied
 * on the previous frame (the blink origin). Small fixed pool, no per-frame allocation.
 */
export function BlinkFlash({ session }: { session: GameSession }): JSX.Element {
  const meshes = useRef<(Group | null)[]>(Array.from({ length: MAX_FLASHES }, () => null));
  const slots = useRef<FlashSlot[]>(Array.from({ length: MAX_FLASHES }, () => ({ x: 0, y: 0, z: 0, t0: 0, active: false })));
  const prevFrames = useRef(0);
  const lastPos = useRef({ x: 0, y: 0, z: 0, ok: false });
  const flashMats = useRef<(MeshBasicMaterial | null)[]>(
    Array.from({ length: MAX_FLASHES }, () => null),
  );
  const color = classColor(CLASS_BULWARK);

  useFrame((state) => {
    const t = state.clock.elapsedTime;
    const c = session.playerController();
    if (c) {
      const prev = prevFrames.current;
      if (c.class === CLASS_BULWARK && c.abilityActiveFrames > prev && lastPos.current.ok) {
        let idx = -1;
        for (let i = 0; i < MAX_FLASHES; i += 1) {
          const sl = slots.current[i];
          if (sl && !sl.active) {
            idx = i;
            break;
          }
        }
        if (idx === -1) idx = 0;
        const target = slots.current[idx];
        if (target) {
          target.x = lastPos.current.x;
          target.y = lastPos.current.y;
          target.z = lastPos.current.z;
          target.t0 = t;
          target.active = true;
        }
      }
      prevFrames.current = c.abilityActiveFrames;
      const lp = lastPos.current;
      lp.x = c.pos.x;
      lp.y = c.pos.y;
      lp.z = c.pos.z;
      lp.ok = true;
    }
    for (let i = 0; i < MAX_FLASHES; i += 1) {
      const slot = slots.current[i];
      const g = meshes.current[i];
      if (!slot || !g) continue;
      if (!slot.active) {
        g.visible = false;
        continue;
      }
      const age = t - slot.t0;
      if (age >= FLASH_TIME) {
        slot.active = false;
        g.visible = false;
        continue;
      }
      const sc = 0.5 + age * 4.5;
      g.visible = true;
      g.position.set(slot.x, slot.y + 0.9, slot.z);
      g.scale.set(sc, sc, sc);
      const mat = flashMats.current[i];
      if (mat) mat.opacity = 0.85 * (1 - age / FLASH_TIME);
    }
  });

  return (
    <group>
      {Array.from({ length: MAX_FLASHES }, (_, i) => (
        <group
          key={i}
          ref={(g) => {
            meshes.current[i] = g;
          }}
          visible={false}
        >
          <mesh>
            <sphereGeometry args={[0.5, 12, 12]} />
            <meshBasicMaterial
              ref={(m) => {
                flashMats.current[i] = m;
              }}
              color={color}
              transparent
              opacity={0.85}
              blending={AdditiveBlending}
              depthWrite={false}
              toneMapped={false}
            />
          </mesh>
        </group>
      ))}
    </group>
  );
}
