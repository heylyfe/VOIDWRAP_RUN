// Keyboard + mouse input. Movement keys are turned into a world-space wish direction using the
// camera yaw (so the shared simulation never needs trigonometry) and quantised to i8.
import { BTN_ABILITY, BTN_INTERACT, BTN_JUMP } from "@/protocol/generated";

export interface KeyState {
  forward: boolean;
  back: boolean;
  left: boolean;
  right: boolean;
  jump: boolean;
  ability: boolean;
  interact: boolean;
  /** Spectator flight only: descend. */
  down: boolean;
}

export const emptyKeys = (): KeyState => ({
  forward: false,
  back: false,
  left: false,
  right: false,
  jump: false,
  ability: false,
  interact: false,
  down: false,
});

const BINDINGS: Record<string, keyof KeyState> = {
  KeyW: "forward",
  ArrowUp: "forward",
  KeyS: "back",
  ArrowDown: "back",
  KeyA: "left",
  ArrowLeft: "left",
  KeyD: "right",
  ArrowRight: "right",
  Space: "jump",
  ShiftLeft: "ability",
  ShiftRight: "ability",
  KeyE: "interact",
  KeyC: "down",
  ControlLeft: "down",
};

export function bindingFor(code: string): keyof KeyState | undefined {
  return BINDINGS[code];
}

/**
 * Camera convention: yaw 0 looks toward -Z; positive yaw turns left (counter-clockwise from above).
 * Returns the quantised world-space wish direction, each axis in [-127, 127].
 */
export function wishFromKeys(keys: KeyState, yaw: number): { moveX: number; moveZ: number } {
  const f = (keys.forward ? 1 : 0) - (keys.back ? 1 : 0);
  const r = (keys.right ? 1 : 0) - (keys.left ? 1 : 0);
  if (f === 0 && r === 0) return { moveX: 0, moveZ: 0 };
  const sin = Math.sin(yaw);
  const cos = Math.cos(yaw);
  // forward = (-sin, -cos), right = (cos, -sin)
  let x = -sin * f + cos * r;
  let z = -cos * f - sin * r;
  const n = Math.hypot(x, z);
  x /= n;
  z /= n;
  const q = (v: number) => Math.max(-127, Math.min(127, Math.round(v * 127))) || 0; // `|| 0` folds -0
  return { moveX: q(x), moveZ: q(z) };
}

export function buttonsFromKeys(keys: KeyState, rightMouse: boolean): number {
  return (keys.jump ? BTN_JUMP : 0) | (keys.ability || rightMouse ? BTN_ABILITY : 0) | (keys.interact ? BTN_INTERACT : 0);
}
