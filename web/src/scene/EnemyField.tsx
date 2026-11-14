"use client";

import { useRef } from "react";
import type { JSX } from "react";
import type { ComponentType } from "react";
import { useFrame } from "@react-three/fiber";
import type { Group } from "three";
import { ENEMY_DISGUISED, ENEMY_HUNTING } from "@/protocol/generated";
import type { GameSession } from "@/game/session";
import { Pallbearer } from "./enemies/Pallbearer";
import { Snapshrike } from "./enemies/Snapshrike";
import { Moteling } from "./enemies/Moteling";
import { RiftWarden } from "./enemies/RiftWarden";
import { GaleWight } from "./enemies/GaleWight";
import { LanternLeech } from "./enemies/LanternLeech";
import { BoltSentry } from "./enemies/BoltSentry";
import { FalseGift } from "./enemies/FalseGift";
import { CrumbleWisp } from "./enemies/CrumbleWisp";
import { Rewind } from "./enemies/Rewind";
import { Warden } from "./enemies/Warden";
import { Peal } from "./enemies/Peal";
import { Kite } from "./enemies/Kite";
import { Static } from "./enemies/Static";
import { Blight } from "./enemies/Blight";
import { Metronome } from "./enemies/Metronome";
import { Overlook } from "./enemies/Overlook";
import { Chorus } from "./enemies/Chorus";
import { Coil } from "./enemies/Coil";
import { Lure } from "./enemies/Lure";
import { Telegraphs } from "./Telegraphs";
import { enemyBehaviorFor } from "./enemyBehavior";
import type { EnemyAnim, EnemyAnimRef, EnemyModelProps } from "./enemyLook";
import { ENEMY_KIND_COUNT } from "./enemyLook";

export const MAX_ENEMIES = 24;

/** One model per wire `kind` index, in design/content.json roster order. */
export const MODEL_COMPONENTS: readonly ComponentType<EnemyModelProps>[] = [
  Pallbearer,
  Snapshrike,
  Moteling,
  RiftWarden,
  GaleWight,
  LanternLeech,
  BoltSentry,
  FalseGift,
  CrumbleWisp,
  Rewind,
  Warden,
  Peal,
  Kite,
  Static,
  Blight,
  Metronome,
  Overlook,
  Chorus,
  Coil,
  Lure,
];

interface SlotRefs {
  outer: Group | null;
  models: (Group | null)[];
}

function emptySlot(): SlotRefs {
  return { outer: null, models: Array.from({ length: ENEMY_KIND_COUNT }, () => null) };
}

/**
 * Pooled enemy slots (fixed size, visibility toggled — never re-created).
 * Each slot pre-builds all twenty roster models and shows the one matching the
 * server's `kind`. The slot is scaled by the server's hit-sphere radius; a
 * disguised False Gift is left unscaled so it is indistinguishable from a gift.
 * Telegraph drawings live in <Telegraphs /> (own pool, same size).
 * Props are unchanged: { session }.
 */
export function EnemyField({ session }: { session: GameSession }): JSX.Element {
  const slots = useRef<SlotRefs[]>([]);
  if (slots.current.length === 0) {
    slots.current = Array.from({ length: MAX_ENEMIES }, emptySlot);
  }
  const anims = useRef<EnemyAnim[]>([]);
  if (anims.current.length === 0) {
    anims.current = Array.from({ length: MAX_ENEMIES }, () => ({
      state: ENEMY_HUNTING,
      prog: 0,
      flags: 0,
      ex: 0,
      ey: 0,
      ez: 0,
      tx: 0,
      ty: 0,
      tz: 0,
    }));
  }
  // Stable `{ current }` wrappers for the models (EnemyModelProps.anim is a ref-style object).
  const animRefs = useRef<EnemyAnimRef[]>([]);
  if (animRefs.current.length === 0) {
    animRefs.current = anims.current.map((a) => ({ current: a }));
  }

  useFrame(() => {
    const now = performance.now();
    let i = 0;
    for (const [id, o] of session.enemyPositions(now)) {
      if (i >= MAX_ENEMIES) break;
      const slot = slots.current[i];
      const anim = anims.current[i];
      if (!slot || !anim) {
        i += 1;
        continue;
      }
      const info = session.enemyInfo.get(id);
      const state = info?.state ?? ENEMY_HUNTING;
      const kind = info?.kind ?? 0;
      anim.state = state;
      anim.prog = info?.prog ?? 0;
      anim.flags = info?.flags ?? 0;
      anim.ex = o.x;
      anim.ey = o.y;
      anim.ez = o.z;
      anim.tx = info?.target.x ?? 0;
      anim.ty = info?.target.y ?? 0;
      anim.tz = info?.target.z ?? 0;

      const outer = slot.outer;
      if (outer) {
        outer.visible = true;
        outer.position.set(o.x, o.y, o.z);
        const disguised = enemyBehaviorFor(kind) === "mimic" && state === ENEMY_DISGUISED;
        outer.scale.setScalar(disguised ? 1 : session.enemyRadius.get(id) ?? 0.5);
      }
      for (let k = 0; k < ENEMY_KIND_COUNT; k += 1) {
        const m = slot.models[k];
        if (m) m.visible = k === kind;
      }
      i += 1;
    }
    for (; i < MAX_ENEMIES; i += 1) {
      const slot = slots.current[i];
      if (slot && slot.outer) slot.outer.visible = false;
    }
  });

  return (
    <group>
      {Array.from({ length: MAX_ENEMIES }, (_, i) => {
        const anim = animRefs.current[i];
        if (!anim) return null;
        return (
          <group
            key={i}
            ref={(g) => {
              const slot = slots.current[i];
              if (slot) slot.outer = g;
            }}
            visible={false}
          >
            {MODEL_COMPONENTS.map((Model, k) => (
              <group
                key={k}
                ref={(g) => {
                  const slot = slots.current[i];
                  if (slot) slot.models[k] = g;
                }}
                visible={false}
              >
                <Model anim={anim} />
              </group>
            ))}
          </group>
        );
      })}
      <Telegraphs session={session} />
    </group>
  );
}
