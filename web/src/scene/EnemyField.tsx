"use client";

import { useMemo, useRef } from "react";
import type { JSX } from "react";
import { useFrame } from "@react-three/fiber";
import type { Group } from "three";
import type { Vec3 } from "@/protocol/generated";
import type { GameSession } from "@/game/session";
import { Chaser } from "./Chaser";
import { Turret } from "./Turret";
import { Mimic } from "./Mimic";
import { Corrupter } from "./Corrupter";
import { enemyBehaviorFor, modeForState } from "./enemyBehavior";
import type { EnemyMode } from "./enemyBehavior";

export const MAX_ENEMIES = 8;

const ORIGIN: Vec3 = { x: 0, y: 0, z: 0 };

interface SlotRefs {
  outer: Group | null;
  chaser: Group | null;
  turret: Group | null;
  mimic: Group | null;
  corrupter: Group | null;
}

function emptySlot(): SlotRefs {
  return { outer: null, chaser: null, turret: null, mimic: null, corrupter: null };
}

function setVisible(g: Group | null, visible: boolean): void {
  if (g) g.visible = visible;
}

/**
 * Pooled enemy slots (fixed size, visibility toggled — never re-created).
 * Every slot pre-builds all four archetype models and switches which one is
 * visible per frame from the server's ENEMY_* state and the content kind.
 * The slot is scaled by the server's hit-sphere radius; a disguised mimic is
 * scaled to 1 so it is indistinguishable from a normal gift.
 */
export function EnemyField({ session }: { session: GameSession }): JSX.Element {
  const slots = useRef<SlotRefs[]>([]);
  if (slots.current.length === 0) {
    slots.current = Array.from({ length: MAX_ENEMIES }, emptySlot);
  }
  const modes = useMemo(() => Array.from({ length: MAX_ENEMIES }, () => ({ current: "hunting" as EnemyMode })), []);

  useFrame(() => {
    const now = performance.now();
    let i = 0;
    for (const [id, o] of session.enemyPositions(now)) {
      if (i >= MAX_ENEMIES) break;
      const slot = slots.current[i];
      if (!slot) {
        i += 1;
        continue;
      }
      const info = session.enemyInfo.get(id);
      const mode = modeForState(info?.state);
      const behavior = enemyBehaviorFor(info?.kind);
      const modeRef = modes[i];
      if (modeRef) modeRef.current = mode;
      const outer = slot.outer;
      if (outer) {
        outer.visible = true;
        outer.position.set(o.x, o.y, o.z);
        const disguised = behavior === "mimic" && mode === "disguised";
        outer.scale.setScalar(disguised ? 1 : session.enemyRadius.get(id) ?? 0.5);
      }
      setVisible(slot.chaser, behavior === "chaser");
      setVisible(slot.turret, behavior === "turret");
      setVisible(slot.mimic, behavior === "mimic");
      setVisible(slot.corrupter, behavior === "corrupter");
      i += 1;
    }
    for (; i < MAX_ENEMIES; i += 1) {
      const slot = slots.current[i];
      setVisible(slot?.outer ?? null, false);
    }
  });

  return (
    <group>
      {Array.from({ length: MAX_ENEMIES }, (_, i) => {
        const mode = modes[i] ?? { current: "hunting" as EnemyMode };
        return (
          <group
            key={i}
            ref={(g) => {
              const slot = slots.current[i];
              if (slot) slot.outer = g;
            }}
            visible={false}
          >
            <group
              ref={(g) => {
                const slot = slots.current[i];
                if (slot) slot.chaser = g;
              }}
            >
              <Chaser position={ORIGIN} mode={mode} />
            </group>
            <group
              ref={(g) => {
                const slot = slots.current[i];
                if (slot) slot.turret = g;
              }}
            >
              <Turret position={ORIGIN} mode={mode} session={session} />
            </group>
            <group
              ref={(g) => {
                const slot = slots.current[i];
                if (slot) slot.mimic = g;
              }}
            >
              <Mimic position={ORIGIN} mode={mode} />
            </group>
            <group
              ref={(g) => {
                const slot = slots.current[i];
                if (slot) slot.corrupter = g;
              }}
            >
              <Corrupter position={ORIGIN} mode={mode} />
            </group>
          </group>
        );
      })}
    </group>
  );
}
