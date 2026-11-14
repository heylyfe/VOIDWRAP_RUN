// Pure routing logic shared by the enemy pool: which roster model a wire `kind` maps
// to, and which readability mode an enemy is in (dormant / hunting / disguised).
// Behaviour ids now come from enemyLook.ts (single source of truth for the 9-kind roster).
import { ENEMY_DISGUISED, ENEMY_DORMANT } from "../protocol/generated";
import { enemyLookFor } from "./enemyLook";
import type { EnemyBehaviorId } from "./enemyLook";

export type EnemyMode = "dormant" | "hunting" | "disguised";
export type EnemyBehavior = EnemyBehaviorId;

/** `kind` indexes the roster order (see enemyLook.ENEMY_LOOKS); unknown kinds fall back to pallbearer. */
export function enemyBehaviorFor(kind: number | undefined): EnemyBehavior {
  return enemyLookFor(kind).behavior;
}

/** Map the wire ENEMY_* state to the visual readability mode. Missing info reads as hunting. */
export function modeForState(state: number | undefined): EnemyMode {
  if (state === ENEMY_DORMANT) return "dormant";
  if (state === ENEMY_DISGUISED) return "disguised";
  return "hunting";
}
