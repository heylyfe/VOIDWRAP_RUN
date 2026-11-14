// Pure routing logic shared by the enemy pool: which archetype a wire `kind` maps to,
// and which "readability mode" an enemy is in (dormant / hunting / disguised).
import { ENEMY_DISGUISED, ENEMY_DORMANT } from "../protocol/generated";
import { ENEMIES } from "../content";
import type { EnemyBehavior } from "../content";

export type EnemyMode = "dormant" | "hunting" | "disguised";

/** `kind` indexes design/content.json enemies; unknown/missing kinds behave as chasers. */
export function enemyBehaviorFor(kind: number | undefined): EnemyBehavior {
  if (kind === undefined) return "chaser";
  const card = ENEMIES[kind];
  return card?.params.behavior ?? "chaser";
}

/** Map the wire ENEMY_* state to the visual readability mode. Missing info reads as hunting. */
export function modeForState(state: number | undefined): EnemyMode {
  if (state === ENEMY_DORMANT) return "dormant";
  if (state === ENEMY_DISGUISED) return "disguised";
  return "hunting";
}
