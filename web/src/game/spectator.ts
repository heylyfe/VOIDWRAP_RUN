export interface FlyKeys {
  forward: boolean;
  back: boolean;
  left: boolean;
  right: boolean;
  up: boolean;
  down: boolean;
  fast: boolean;
}

export interface Vec {
  x: number;
  y: number;
  z: number;
}

export const FLY_SPEED = 16; // units/s
export const FAST_MULTIPLIER = 2.2;

/** Camera look direction: yaw 0 looks toward -Z, positive pitch looks DOWN. */
function lookDir(yaw: number, pitch: number): Vec {
  const cosPitch = Math.cos(pitch);
  return {
    x: -Math.sin(yaw) * cosPitch,
    y: -Math.sin(pitch),
    z: -Math.cos(yaw) * cosPitch,
  };
}

/** Strafe right vector on the horizontal plane for a given yaw. */
function rightDir(yaw: number): Vec {
  return { x: Math.cos(yaw), y: 0, z: -Math.sin(yaw) };
}

function accumulate(target: Vec, source: Vec, sign: number): void {
  target.x += source.x * sign;
  target.y += source.y * sign;
  target.z += source.z * sign;
}

/**
 * Moves `pos` (mutated) for one frame of free flight. Forward/back follow the
 * full look direction (yaw AND pitch); left/right strafe on the horizontal
 * plane; up/down move along world Y. Diagonals are normalised; dt is clamped
 * to [0, 0.1].
 */
export function flyStep(pos: Vec, keys: FlyKeys, yaw: number, pitch: number, dt: number): void {
  const clampedDt = Math.min(Math.max(dt, 0), 0.1);

  const dir: Vec = { x: 0, y: 0, z: 0 };
  const look = lookDir(yaw, pitch);
  const right = rightDir(yaw);

  if (keys.forward) accumulate(dir, look, 1);
  if (keys.back) accumulate(dir, look, -1);
  if (keys.right) accumulate(dir, right, 1);
  if (keys.left) accumulate(dir, right, -1);
  if (keys.up) dir.y += 1;
  if (keys.down) dir.y -= 1;

  const length = Math.hypot(dir.x, dir.y, dir.z);
  if (length === 0) return;

  const speed = FLY_SPEED * (keys.fast ? FAST_MULTIPLIER : 1);
  const scale = (speed * clampedDt) / length;

  pos.x += dir.x * scale;
  pos.y += dir.y * scale;
  pos.z += dir.z * scale;
}

/** Camera position for following a player: orbit `distance` units behind/above `target` along the look direction. */
export function followCamera(target: Vec, yaw: number, pitch: number, distance: number): Vec {
  const look = lookDir(yaw, pitch);
  return {
    x: target.x - look.x * distance,
    y: target.y - look.y * distance + 1.4,
    z: target.z - look.z * distance,
  };
}

/** Next index when cycling through `count` followable players; null -> 0; wraps; count 0 -> null. */
export function nextFollow(current: number | null, count: number): number | null {
  if (count <= 0) return null;
  if (current === null) return 0;
  return (current + 1) % count;
}
