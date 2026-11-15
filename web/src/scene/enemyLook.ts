// Pure lookup + curve helpers for the reworked enemy roster (ticket T-057a, extended in T-076).
// No three.js / React runtime imports — unit-testable in isolation.
import type { MutableRefObject } from "react";
import {
  ENEMY_ATTACK,
  ENEMY_TELEGRAPH,
} from "../protocol/generated";

/** Stable behaviour id (matches design/content.json enemy ids). */
export type EnemyBehaviorId =
  | "pallbearer"
  | "snapshrike"
  | "moteling"
  | "rift_warden"
  | "gale_wight"
  | "lantern_leech"
  | "turret"
  | "mimic"
  | "corrupter"
  | "rewind"
  | "warden"
  | "peal"
  | "kite"
  | "static"
  | "blight"
  | "metronome"
  | "overlook"
  | "chorus"
  | "coil"
  | "lure";

export interface EnemyLook {
  /** Behaviour id; also the roster order used for the wire `kind` index. */
  readonly behavior: EnemyBehaviorId;
  /** Primary accent: emissive trim + telegraph colour (design doc §2). */
  readonly accent: string;
  /** Secondary emissive: hot core / rim / inner glow. */
  readonly glow: string;
}

/** Order matches the wire `kind` index: pallbearer 0 … corrupter 8, rewind 9 … lure 19. */
export const ENEMY_LOOKS: readonly EnemyLook[] = [
  { behavior: "pallbearer", accent: "#e91e63", glow: "#ffdf9e" },
  { behavior: "snapshrike", accent: "#ff4436", glow: "#ffb36b" },
  { behavior: "moteling", accent: "#ffc3d9", glow: "#ff5fa8" },
  { behavior: "rift_warden", accent: "#b0313f", glow: "#ff5a3c" },
  { behavior: "gale_wight", accent: "#7b2ff7", glow: "#b47aff" },
  { behavior: "lantern_leech", accent: "#ff7ab8", glow: "#ffb3d1" },
  { behavior: "turret", accent: "#ff8a3c", glow: "#ffd07a" },
  { behavior: "mimic", accent: "#ff3df0", glow: "#b46bff" },
  { behavior: "corrupter", accent: "#39ffb0", glow: "#7dffb9" },
  { behavior: "rewind", accent: "#46e0ff", glow: "#cdf6ff" },
  { behavior: "warden", accent: "#f4d35e", glow: "#fdf3b8" },
  { behavior: "peal", accent: "#9d0208", glow: "#ffb0a8" },
  { behavior: "kite", accent: "#35a7ff", glow: "#bdd9ff" },
  { behavior: "static", accent: "#a8b8c8", glow: "#dde3ea" },
  { behavior: "blight", accent: "#a3c34a", glow: "#dcea9a" },
  { behavior: "metronome", accent: "#c98f3d", glow: "#ffe08a" },
  { behavior: "overlook", accent: "#ecebe4", glow: "#ffffff" },
  { behavior: "chorus", accent: "#2ec4b6", glow: "#a2eee3" },
  { behavior: "coil", accent: "#e07a5f", glow: "#ffc7a8" },
  { behavior: "lure", accent: "#a55eea", glow: "#d8b0ff" },
];

export const ENEMY_KIND_COUNT = ENEMY_LOOKS.length;

const FALLBACK_LOOK: EnemyLook = { behavior: "pallbearer", accent: "#e91e63", glow: "#ffdf9e" };

export function isEnemyKind(kind: number): boolean {
  return Number.isInteger(kind) && kind >= 0 && kind < ENEMY_LOOKS.length;
}

/** Unknown/missing kinds fall back to the pallbearer look (never crash a render). */
/**
 * Look for a behaviour id. The Blender-authored models (T-157) need their accent at
 * runtime rather than as a literal in the .tsx, so the palette table stays the single
 * source of truth for emissive trim.
 */
export function enemyLookForBehavior(behavior: EnemyBehaviorId): EnemyLook {
  return ENEMY_LOOKS.find((l) => l.behavior === behavior) ?? FALLBACK_LOOK;
}

export function enemyLookFor(kind: number | undefined): EnemyLook {
  if (kind === undefined) return FALLBACK_LOOK;
  const look = ENEMY_LOOKS[kind];
  return look ?? FALLBACK_LOOK;
}

export function isTelegraphState(state: number | undefined): boolean {
  return state === ENEMY_TELEGRAPH || state === ENEMY_ATTACK;
}

export function clamp01(v: number): number {
  if (!Number.isFinite(v)) return 0;
  return v < 0 ? 0 : v > 1 ? 1 : v;
}

/** Wire `prog` (0–255) → 0..1. */
export function prog01(prog: number | undefined): number {
  if (prog === undefined || !Number.isFinite(prog)) return 0;
  return clamp01(prog / 255);
}

export function lerp(a: number, b: number, t: number): number {
  const c = clamp01(t);
  return a + (b - a) * c;
}

/* ---------- telegraph curves ---------- */

/** Alpha ramp shared by every telegraph: dim at prog 0, solid at 255. */
export function telegraphAlpha(t01: number): number {
  return 0.3 + 0.7 * clamp01(t01);
}

/** Moteling (Stillness Shrieker) INHALE: warning ring starts at orbit radius and collapses onto the victim. */
export const MOTELING_RING_START = 5.0;
export const MOTELING_RING_END = 0.45;
export function motelingRingRadius(t01: number): number {
  return lerp(MOTELING_RING_START, MOTELING_RING_END, t01);
}

/** Rings that grow outward as an attack resolves (shriek snap / warden shove / fret spread). */
export function expandingRingRadius(t01: number, maxRadius: number): number {
  return Math.max(0.25, clamp01(t01) * maxRadius);
}

/** Fixed threat radii from the design doc (§2). */
export const PALLBEARER_LUNGE_RADIUS = 1.1;
export const GALE_WIGHT_BLAST_RADIUS = 2.1;
export const RIFT_WARDEN_SHOVE_RADIUS = 3.5;
export const SNAPSHRIKE_DASH_HALF_WIDTH = 0.7;
export const TURRET_AIM_WIDTH = 0.1;
export const LEECH_FLARE_RADIUS = 0.9;
export const MIMIC_HISS_RADIUS = 0.95;
export const CORRUPTER_AURA_RADIUS = 2.0;

/* ---------- telegraph radii for the eleven new entries (T-076) ---------- */

/** Rewind (SNAP): the return mark shrinks onto the remembered point as prog fills. */
export const REWIND_MARK_START = 1.7;
export const REWIND_MARK_END = 0.35;
export function rewindMarkRadius(t01: number): number {
  return lerp(REWIND_MARK_START, REWIND_MARK_END, t01);
}

/** Warden (GUARD): fixed ring around the guarded gift. */
export const WARDEN_GUARD_RADIUS = 2.1;

/** Peal (PEAL): each ring widens the zone out to this radius. */
export const PEAL_ZONE_MAX = 3.2;

/** Kite (TETHER): thin tether line; the strike marker at the anchor. */
export const KITE_TETHER_WIDTH = 0.12;
export const KITE_STRIKE_RADIUS = 0.85;

/** Overlook (MARK): sight line width to the victim. */
export const OVERLOOK_SIGHT_WIDTH = 0.14;

/** Coil (WIND): the ring tightens while winding; the release is a shockwave. */
export const COIL_WIND_START = 1.6;
export const COIL_WIND_END = 0.55;
export function coilWindRadius(t01: number): number {
  return lerp(COIL_WIND_START, COIL_WIND_END, t01);
}
export const COIL_SHOCKWAVE_RADIUS = 2.6;

/** Metronome (BEAT): the swing lane across the floor. */
export const METRONOME_LANE_WIDTH = 2.3;

/** Orbit Dasher (snapshrike) freezes its line for the last 0.35 s of the 0.75 s LOCK window. */
export const SNAPSHRIKE_LOCK_PROG = 0.4 / 0.75; // ≈ 0.533
/** Alpha for the LOCK line: ramps while tracking, snaps solid once the line freezes. */
export function snapshrikeAlpha(t01: number): number {
  const t = clamp01(t01);
  return t >= SNAPSHRIKE_LOCK_PROG ? 1 : telegraphAlpha(t) * 0.85;
}

/** MIMIC CACHED tell: a 0.5 s magenta shimmer every 3.0 s. Purely client-clock driven. */
export const MIMIC_SHIMMER_PERIOD = 3.0;
export const MIMIC_SHIMMER_WINDOW = 0.5;
function shimmerPhase(timeS: number): number {
  return ((timeS % MIMIC_SHIMMER_PERIOD) + MIMIC_SHIMMER_PERIOD) % MIMIC_SHIMMER_PERIOD;
}
export function mimicShimmerVisible(timeS: number): boolean {
  if (!Number.isFinite(timeS)) return false;
  return shimmerPhase(timeS) < MIMIC_SHIMMER_WINDOW;
}
/** 0 → peak → 0 across the shimmer window; 0 outside it. */
export function mimicShimmerOpacity(timeS: number): number {
  if (!mimicShimmerVisible(timeS)) return 0;
  return Math.sin((shimmerPhase(timeS) / MIMIC_SHIMMER_WINDOW) * Math.PI) * 0.9;
}

/** Gifts are 0.34 u; the shared GiftBox still draws a 0.7 u box, so the False Gift
 *  wraps it in a fixed scale wrapper (independent of the body-radius scale). */
/** EnemyField draws a disguised slot at scale 1 and GiftBox is already the real gift size (0.34 u). */
export const MIMIC_GIFT_SCALE = 1;

/** Yaw that faces a model (+Z forward) from (fx,fz) toward (tx,tz). */
export function yawToward(fx: number, fz: number, tx: number, tz: number): number {
  return Math.atan2(tx - fx, tz - fz);
}

/** Shared per-slot animation payload the parent writes every frame (no re-render).
 *  `tx/ty/tz` is the telegraph anchor copy, `ex/ey/ez` the enemy's world position. */
export interface EnemyAnim {
  state: number;
  prog: number;
  flags: number;
  ex: number;
  ey: number;
  ez: number;
  tx: number;
  ty: number;
  tz: number;
}

/** Stable across frames — parent mutates fields, never replaces the object. */
export type EnemyAnimRef = MutableRefObject<EnemyAnim>;

export interface EnemyModelProps {
  anim: EnemyAnimRef;
}
