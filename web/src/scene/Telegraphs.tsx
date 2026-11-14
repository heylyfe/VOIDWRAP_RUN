"use client";

import { useEffect, useMemo, useRef } from "react";
import type { JSX } from "react";
import { useFrame } from "@react-three/fiber";
import { AdditiveBlending, CircleGeometry, Color, DoubleSide, MeshBasicMaterial, PlaneGeometry, RingGeometry } from "three";
import type { Group, Mesh } from "three";
import type { GameSession } from "@/game/session";
import { ENEMY_ATTACK } from "@/protocol/generated";
import {
  CORRUPTER_AURA_RADIUS,
  ENEMY_LOOKS,
  GALE_WIGHT_BLAST_RADIUS,
  LEECH_FLARE_RADIUS,
  MIMIC_HISS_RADIUS,
  PALLBEARER_LUNGE_RADIUS,
  RIFT_WARDEN_SHOVE_RADIUS,
  SNAPSHRIKE_DASH_HALF_WIDTH,
  TURRET_AIM_WIDTH,
  KITE_STRIKE_RADIUS,
  KITE_TETHER_WIDTH,
  METRONOME_LANE_WIDTH,
  OVERLOOK_SIGHT_WIDTH,
  PEAL_ZONE_MAX,
  WARDEN_GUARD_RADIUS,
  COIL_SHOCKWAVE_RADIUS,
  coilWindRadius,
  enemyLookFor,
  expandingRingRadius,
  isTelegraphState,
  motelingRingRadius,
  prog01,
  rewindMarkRadius,
  snapshrikeAlpha,
  telegraphAlpha,
} from "./enemyLook";

export const MAX_TELEGRAPHS = 24;

interface TelegraphSlot {
  outer: Group | null;
  ring: Mesh | null;
  disc: Mesh | null;
  line: Mesh | null;
  ringMat: MeshBasicMaterial;
  discMat: MeshBasicMaterial;
  lineMat: MeshBasicMaterial;
}

function createSlot(): TelegraphSlot {
  const base = {
    transparent: true,
    opacity: 0,
    blending: AdditiveBlending,
    depthWrite: false,
    toneMapped: false,
    side: DoubleSide,
  } as const;
  return {
    outer: null,
    ring: null,
    disc: null,
    line: null,
    ringMat: new MeshBasicMaterial({ ...base, color: "#ffffff" }),
    discMat: new MeshBasicMaterial({ ...base, color: "#ffffff" }),
    lineMat: new MeshBasicMaterial({ ...base, color: "#ffffff" }),
  };
}

/**
 * Pooled telegraph drawings, one slot per enemy (max 24). Everything is additive,
 * bright, and tinted with the enemy's accent colour; alpha and radius are driven by
 * the wire `prog` (0..255). Slots exist only while the enemy is TELEGRAPH/ATTACK —
 * otherwise hidden. `target` is the telegraph anchor from the server payload.
 * The switch covers all twenty roster ids.
 */
export function Telegraphs({ session }: { session: GameSession }): JSX.Element {
  const slots = useRef<TelegraphSlot[]>([]);
  if (slots.current.length === 0) {
    slots.current = Array.from({ length: MAX_TELEGRAPHS }, createSlot);
  }

  const ringGeo = useMemo(() => new RingGeometry(0.86, 1, 48), []);
  const discGeo = useMemo(() => new CircleGeometry(1, 40), []);
  const lineGeo = useMemo(() => new PlaneGeometry(1, 1), []);
  const palette = useMemo(() => ENEMY_LOOKS.map((l) => new Color(l.accent)), []);
  const fallbackColor = useMemo(() => new Color("#ffffff"), []);

  useEffect(
    () => () => {
      for (const slot of slots.current) {
        slot.ringMat.dispose();
        slot.discMat.dispose();
        slot.lineMat.dispose();
      }
      ringGeo.dispose();
      discGeo.dispose();
      lineGeo.dispose();
    },
    [ringGeo, discGeo, lineGeo],
  );

  // Ribbon lines orient with Euler order Y→X→Z (yaw then pitch). Set once.
  useEffect(() => {
    for (const slot of slots.current) {
      if (slot.line) slot.line.rotation.order = "YXZ";
    }
  }, []);

  useFrame((state) => {
    const now = performance.now();
    const t = state.clock.elapsedTime;
    let i = 0;
    for (const [id, o] of session.enemyPositions(now)) {
      if (i >= MAX_TELEGRAPHS) break;
      const slot = slots.current[i];
      if (!slot || !slot.outer) {
        i += 1;
        continue;
      }
      const info = session.enemyInfo.get(id);
      if (!info || !isTelegraphState(info.state)) {
        slot.outer.visible = false;
        i += 1;
        continue;
      }

      const kind = info.kind;
      const look = enemyLookFor(kind);
      const col =
        kind !== undefined && kind >= 0 && kind < palette.length
          ? palette[kind] ?? fallbackColor
          : fallbackColor;

      const attack = info.state === ENEMY_ATTACK;
      const p = prog01(info.prog);
      const anchorX = info.target.x;
      const anchorY = info.target.y;
      const anchorZ = info.target.z;

      let ringVisible = false;
      let discVisible = false;
      let lineVisible = false;
      let ringX = anchorX;
      let ringY = anchorY + 0.06;
      let ringZ = anchorZ;
      let ringRadius = 1;
      let ringAlpha = telegraphAlpha(p);
      let discRadius = 1;
      let discAlpha = 0;
      let lineWidth = 0.2;
      let lineAlpha = telegraphAlpha(p);

      switch (look.behavior) {
        case "pallbearer":
          // KNEEL: straight line barge→victim + ring under the locked point
          lineVisible = true;
          lineWidth = 0.2;
          lineAlpha = attack ? 0.55 * (1 - p) : telegraphAlpha(p);
          ringVisible = true;
          ringRadius = PALLBEARER_LUNGE_RADIUS;
          ringAlpha = telegraphAlpha(p);
          break;
        case "snapshrike":
          // LOCK: full-width dash wedge, snapping brighter once the line freezes
          lineVisible = true;
          if (attack) {
            lineWidth = SNAPSHRIKE_DASH_HALF_WIDTH;
            lineAlpha = 0.6 * (1 - p);
          } else {
            lineWidth = SNAPSHRIKE_DASH_HALF_WIDTH * 2;
            lineAlpha = snapshrikeAlpha(p);
          }
          break;
        case "moteling":
          // INHALE: collapsing ring around the marked player; SHRIEK: ring snap
          ringVisible = true;
          if (attack) {
            ringRadius = expandingRingRadius(p, 4.0);
            ringAlpha = 0.9 * (1 - p);
          } else {
            ringRadius = motelingRingRadius(p);
            ringAlpha = 0.35 + 0.65 * p;
          }
          ringY = anchorY + 0.09;
          discVisible = true;
          discRadius = ringRadius;
          discAlpha = ringAlpha * 0.16;
          break;
        case "rift_warden":
          // shove radius — 3.5 u ring centred on the warden itself
          ringVisible = true;
          ringX = o.x;
          ringY = o.y + 0.06;
          ringZ = o.z;
          if (attack) {
            ringRadius = expandingRingRadius(p, RIFT_WARDEN_SHOVE_RADIUS);
            ringAlpha = 0.95 * (1 - p);
          } else {
            ringRadius = RIFT_WARDEN_SHOVE_RADIUS;
            ringAlpha = 0.3 + 0.7 * p;
          }
          break;
        case "gale_wight":
          // blast circle persists on the ground through MARK → LOCK → FALL
          ringVisible = true;
          ringRadius = GALE_WIGHT_BLAST_RADIUS;
          ringAlpha = attack ? 0.85 * (1 - p) + 0.15 : telegraphAlpha(p);
          discVisible = true;
          discRadius = GALE_WIGHT_BLAST_RADIUS;
          discAlpha = attack ? 0.1 * (1 - p) : 0.14 + 0.12 * p;
          break;
        case "lantern_leech":
          // path to the siphoned pickup + destination flare
          lineVisible = true;
          lineWidth = 0.3;
          lineAlpha = attack ? 0.5 * (1 - p) : telegraphAlpha(p);
          ringVisible = true;
          ringRadius = LEECH_FLARE_RADIUS + p * 0.7;
          ringAlpha = attack ? 0.8 * (1 - p) + 0.2 : 0.35 + 0.65 * p;
          break;
        case "turret":
          // thin aiming line to the predicted intercept; FIRE flashes bright then dies
          lineVisible = true;
          if (attack) {
            lineWidth = 0.34;
            lineAlpha = Math.max(0, 1 - p * 2.5);
          } else {
            lineWidth = TURRET_AIM_WIDTH;
            lineAlpha = telegraphAlpha(p);
          }
          break;
        case "mimic":
          // HISS glitch — jittered magenta ring on the gift itself
          ringVisible = true;
          ringX = o.x;
          ringY = o.y + 0.06;
          ringZ = o.z;
          ringRadius = MIMIC_HISS_RADIUS * (1 + 0.1 * Math.sin(t * 37 + i * 2.39));
          ringAlpha =
            (Math.sin(t * 29 + i * 1.7) > 0 ? 0.95 : 0.28) *
            (attack ? Math.max(0.15, 1 - p) : telegraphAlpha(p));
          if (attack) {
            lineVisible = true;
            lineWidth = 0.5;
            lineAlpha = 0.6 * (1 - p);
          }
          break;
        case "corrupter":
          // fretting spread — veins crawling outward from the marked tile
          ringVisible = true;
          ringRadius = expandingRingRadius(p, CORRUPTER_AURA_RADIUS);
          ringAlpha = attack ? 0.7 * (1 - p) : 0.3 + 0.6 * p;
          discVisible = true;
          discRadius = ringRadius;
          discAlpha = ringAlpha * 0.2;
          break;
        case "rewind":
          // SNAP: the return mark shrinks onto the remembered point
          ringVisible = true;
          if (attack) {
            ringRadius = expandingRingRadius(p, 1.4);
            ringAlpha = 0.85 * (1 - p);
          } else {
            ringRadius = rewindMarkRadius(p);
            ringAlpha = telegraphAlpha(p);
          }
          discVisible = true;
          discRadius = ringRadius;
          discAlpha = ringAlpha * 0.22;
          break;
        case "warden":
          // GUARD: ring around the guarded gift (anchor = the gift)
          ringVisible = true;
          ringRadius = WARDEN_GUARD_RADIUS;
          ringAlpha = attack ? 0.9 * (1 - p) : 0.3 + 0.7 * p;
          discVisible = true;
          discRadius = WARDEN_GUARD_RADIUS;
          discAlpha = attack ? 0.12 * (1 - p) : 0.1 + 0.12 * p;
          break;
        case "peal":
          // PEAL: the bell's ring, expanding from the bell itself
          ringVisible = true;
          ringX = o.x;
          ringY = o.y + 0.06;
          ringZ = o.z;
          ringRadius = expandingRingRadius(p, PEAL_ZONE_MAX);
          ringAlpha = attack ? 0.9 * (1 - p) : telegraphAlpha(p);
          discVisible = true;
          discRadius = ringRadius;
          discAlpha = ringAlpha * 0.18;
          break;
        case "kite":
          // TETHER: line to the tethered player + strike marker at the anchor
          lineVisible = true;
          lineWidth = KITE_TETHER_WIDTH;
          lineAlpha = attack ? 0.1 + 0.9 * (1 - p) : 0.25 + 0.75 * p;
          ringVisible = true;
          ringRadius = KITE_STRIKE_RADIUS;
          ringAlpha = attack ? 0.85 * (1 - p) : telegraphAlpha(p);
          break;
        case "overlook":
          // MARK: the sight line to the victim + a marker on the marked player
          lineVisible = true;
          lineWidth = OVERLOOK_SIGHT_WIDTH;
          lineAlpha = attack ? 0.5 * (1 - p) : telegraphAlpha(p);
          ringVisible = true;
          ringRadius = 0.7;
          ringAlpha = attack ? 0.8 * (1 - p) : telegraphAlpha(p);
          break;
        case "coil":
          // WIND: ring tightens while winding; RELEASE: expanding shockwave
          ringVisible = true;
          ringX = o.x;
          ringY = o.y + 0.06;
          ringZ = o.z;
          if (attack) {
            ringRadius = expandingRingRadius(p, COIL_SHOCKWAVE_RADIUS);
            ringAlpha = 0.95 * (1 - p);
          } else {
            ringRadius = coilWindRadius(p);
            ringAlpha = telegraphAlpha(p);
          }
          discVisible = true;
          discRadius = ringRadius;
          discAlpha = ringAlpha * 0.16;
          break;
        case "metronome":
          // BEAT: the swing lane across the floor
          lineVisible = true;
          lineWidth = METRONOME_LANE_WIDTH;
          lineAlpha = attack ? 0.7 * (1 - p) : 0.3 + 0.6 * p;
          break;
        // Static, Blight, Chorus and Lure are cue_tier 3 (ambient, ENEMY_REGISTRY §2):
        // their cue is a persistent body/aura/fog tell on the model itself, never a
        // floor decal — no ring, disc or line by design. They deliberately draw nothing.
        case "static":
        case "blight":
        case "chorus":
        case "lure":
          break;
      }

      slot.outer.visible = true;

      if (slot.ring) {
        slot.ring.visible = ringVisible;
        if (ringVisible) {
          slot.ring.position.set(ringX, ringY, ringZ);
          slot.ring.scale.set(ringRadius, ringRadius, 1);
          slot.ringMat.opacity = ringAlpha;
          slot.ringMat.color.copy(col);
        }
      }
      if (slot.disc) {
        slot.disc.visible = discVisible;
        if (discVisible) {
          slot.disc.position.set(ringX, ringY - 0.015, ringZ);
          slot.disc.scale.set(discRadius, discRadius, 1);
          slot.discMat.opacity = discAlpha;
          slot.discMat.color.copy(col);
        }
      }
      if (slot.line) {
        slot.line.visible = lineVisible;
        if (lineVisible) {
          const dx = anchorX - o.x;
          const dy = anchorY - o.y;
          const dz = anchorZ - o.z;
          const h = Math.hypot(dx, dz);
          const len = Math.hypot(h, dy);
          if (len < 0.05) {
            slot.line.visible = false;
          } else {
            slot.line.position.set((o.x + anchorX) / 2, (o.y + anchorY) / 2, (o.z + anchorZ) / 2);
            slot.line.rotation.y = Math.atan2(dx, dz);
            slot.line.rotation.x = Math.atan2(h, dy);
            slot.line.scale.set(lineWidth, len, 1);
            slot.lineMat.opacity = lineAlpha;
            slot.lineMat.color.copy(col);
          }
        }
      }
      i += 1;
    }
    for (; i < MAX_TELEGRAPHS; i += 1) {
      const slot = slots.current[i];
      if (slot && slot.outer) slot.outer.visible = false;
    }
  });

  return (
    <group>
      {slots.current.map((slot, i) => (
        <group
          key={i}
          ref={(g) => {
            slot.outer = g;
          }}
          visible={false}
        >
          <mesh
            ref={(m) => {
              slot.ring = m;
            }}
            geometry={ringGeo}
            material={slot.ringMat}
            rotation={[-Math.PI / 2, 0, 0]}
          />
          <mesh
            ref={(m) => {
              slot.disc = m;
            }}
            geometry={discGeo}
            material={slot.discMat}
            rotation={[-Math.PI / 2, 0, 0]}
          />
          <mesh
            ref={(m) => {
              slot.line = m;
            }}
            geometry={lineGeo}
            material={slot.lineMat}
          />
        </group>
      ))}
    </group>
  );
}
