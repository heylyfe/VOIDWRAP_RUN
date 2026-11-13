export interface AvatarMotion {
  speed: number;
  vy: number;
  grounded: boolean;
  dashing: boolean;
}

export interface AvatarPose {
  leftArm: number;
  rightArm: number; // pitch (rad), + = forward swing
  leftArmRoll: number;
  rightArmRoll: number; // roll outward (rad), for jump/fall flail
  leftLeg: number;
  rightLeg: number; // pitch (rad)
  torsoPitch: number; // lean forward (+)
  bodyY: number; // vertical offset of the whole body (bob / squash compensation)
  scaleY: number;
  scaleXZ: number; // squash & stretch
}

export interface PoseState {
  phase: number;
  landTimer: number;
  wasGrounded: boolean;
  pose: AvatarPose;
}

const TAU = Math.PI * 2;
const BLEND_TAU = 0.06; // 60 ms time constant for rotations / bodyY
const SQUASH_TAU = 0.025; // squash values blend faster
const LAND_DURATION = 0.12;
const RUN_SPEED = 9;
const RUN_CADENCE = 2.6; // cycles/s at full run speed
const LEG_SWING = 0.75;
const ARM_SWING = 0.6;
const RUN_TORSO_PITCH = 0.12;
const RUN_BODY_BOB = 0.06;
const IDLE_BREATH_RATE = 1.2; // cycles/s
const IDLE_BREATH_Y = 0.02;
const JUMP_ARM_PITCH = -2.4;
const JUMP_ARM_ROLL = 0.3;
const JUMP_FRONT_LEG = 0.6;
const JUMP_BACK_LEG = -0.3;
const JUMP_TORSO_PITCH = 0.05;
const JUMP_STRETCH_Y = 1.06;
const JUMP_STRETCH_XZ = 0.96;
const FALL_ARM_ROLL = 1.1;
const FALL_FLAIL_START_VY = -14;
const FALL_FLAIL = 0.25;
const FALL_FLAIL_FAST = 0.45;
const FALL_FLAIL_RATE = 4; // cycles/s
const FALL_LEG_SPREAD = 0.35;
const FALL_TORSO_PITCH = 0.1;
const DASH_TORSO_PITCH = 0.55;
const DASH_ARM_PITCH = -1.3; // arms swept back behind the body
const DASH_FRONT_LEG = -0.5;
const DASH_BACK_LEG = -0.2;
const DASH_STRETCH_Y = 0.92;
const DASH_STRETCH_XZ = 1.1;
const LAND_SQUASH_Y = 0.82;
const LAND_SQUASH_XZ = 1.12;
const LAND_BODY_DIP = -0.08;
const MIN_RUN_SPEED = 0.5;

// Reusable scratch targets so stepPose allocates nothing per call. This module
// is used from a single animation loop per avatar; the scratch is fully
// overwritten at the start of every computeTarget call.
const target: AvatarPose = {
  leftArm: 0,
  rightArm: 0,
  leftArmRoll: 0,
  rightArmRoll: 0,
  leftLeg: 0,
  rightLeg: 0,
  torsoPitch: 0,
  bodyY: 0,
  scaleY: 1,
  scaleXZ: 1,
};

export function createPoseState(): PoseState {
  return {
    phase: 0,
    landTimer: 0,
    wasGrounded: true,
    pose: {
      leftArm: 0,
      rightArm: 0,
      leftArmRoll: 0,
      rightArmRoll: 0,
      leftLeg: 0,
      rightLeg: 0,
      torsoPitch: 0,
      bodyY: 0,
      scaleY: 1,
      scaleXZ: 1,
    },
  };
}

function computeTarget(m: AvatarMotion, state: PoseState): number {
  target.leftArm = 0;
  target.rightArm = 0;
  target.leftArmRoll = 0;
  target.rightArmRoll = 0;
  target.leftLeg = 0;
  target.rightLeg = 0;
  target.torsoPitch = 0;
  target.bodyY = 0;
  target.scaleY = 1;
  target.scaleXZ = 1;

  if (m.dashing) {
    // Dash overrides everything: strong lean, arms swept back, trailing legs.
    target.torsoPitch = DASH_TORSO_PITCH;
    target.leftArm = DASH_ARM_PITCH;
    target.rightArm = DASH_ARM_PITCH;
    target.leftLeg = DASH_FRONT_LEG;
    target.rightLeg = DASH_BACK_LEG;
    target.scaleY = DASH_STRETCH_Y;
    target.scaleXZ = DASH_STRETCH_XZ;
    return 0; // phase frozen while dashing
  }

  if (m.grounded) {
    if (m.speed >= MIN_RUN_SPEED) {
      const rate = (m.speed / RUN_SPEED) * RUN_CADENCE;
      const amp = Math.min(Math.abs(m.speed) / RUN_SPEED, 1);
      const s = Math.sin(state.phase * TAU);
      target.leftLeg = s * LEG_SWING * amp;
      target.rightLeg = -s * LEG_SWING * amp;
      target.leftArm = -s * ARM_SWING * amp;
      target.rightArm = s * ARM_SWING * amp;
      target.torsoPitch = RUN_TORSO_PITCH * amp;
      target.bodyY = Math.abs(s) * RUN_BODY_BOB * amp; // two bounces per cycle
      return rate;
    }
    // Idle: subtle breathing. Always advance the phase from the raw rate so a
    // freshly spawned (phase === 0) avatar starts breathing immediately.
    target.bodyY = Math.sin(state.phase * TAU) * IDLE_BREATH_Y;
    return IDLE_BREATH_RATE;
  }

  // Airborne: the shared phase oscillator drives the fall flail rate.
  if (m.vy > 1) {
    // Jump rising: arms overhead, legs tucked, slight stretch.
    target.leftArm = JUMP_ARM_PITCH;
    target.rightArm = JUMP_ARM_PITCH;
    target.leftArmRoll = JUMP_ARM_ROLL;
    target.rightArmRoll = JUMP_ARM_ROLL;
    target.leftLeg = JUMP_FRONT_LEG;
    target.rightLeg = JUMP_BACK_LEG;
    target.torsoPitch = JUMP_TORSO_PITCH;
    target.scaleY = JUMP_STRETCH_Y;
    target.scaleXZ = JUMP_STRETCH_XZ;
  } else {
    // Apex / falling: arms out to the sides, flailing; more flail when fast.
    const flail = m.vy < FALL_FLAIL_START_VY ? FALL_FLAIL_FAST : FALL_FLAIL;
    const f = Math.sin(state.phase * TAU);
    target.leftArm = f * flail;
    target.rightArm = -f * flail;
    target.leftArmRoll = FALL_ARM_ROLL;
    target.rightArmRoll = FALL_ARM_ROLL;
    target.leftLeg = FALL_LEG_SPREAD;
    target.rightLeg = -FALL_LEG_SPREAD;
    target.torsoPitch = FALL_TORSO_PITCH;
  }
  return FALL_FLAIL_RATE;
}

/** Advance animation by dt seconds toward the target pose for this motion; mutates and returns state.pose. */
export function stepPose(state: PoseState, m: AvatarMotion, dtRaw: number): AvatarPose {
  const dt = Math.min(0.1, Math.max(0, dtRaw));

  // Detect the landing frame: grounded flips back to true after being false.
  if (m.grounded && !state.wasGrounded) {
    state.landTimer = LAND_DURATION;
  }
  state.wasGrounded = m.grounded;

  const phaseRate = computeTarget(m, state);
  // Wrap phase into [0, 1) so long sessions keep float precision; every
  // consumer reads it through sin(phase * TAU), so wrapping is invisible.
  state.phase = (state.phase + phaseRate * dt) % 1;
  if (state.phase < 0) {
    state.phase += 1;
  }

  const k = 1 - Math.exp(-dt / BLEND_TAU);
  const k2 = 1 - Math.exp(-dt / SQUASH_TAU);
  const p = state.pose;
  p.leftArm += (target.leftArm - p.leftArm) * k;
  p.rightArm += (target.rightArm - p.rightArm) * k;
  p.leftArmRoll += (target.leftArmRoll - p.leftArmRoll) * k;
  p.rightArmRoll += (target.rightArmRoll - p.rightArmRoll) * k;
  p.leftLeg += (target.leftLeg - p.leftLeg) * k;
  p.rightLeg += (target.rightLeg - p.rightLeg) * k;
  p.torsoPitch += (target.torsoPitch - p.torsoPitch) * k;
  p.bodyY += (target.bodyY - p.bodyY) * k;
  p.scaleY += (target.scaleY - p.scaleY) * k2;
  p.scaleXZ += (target.scaleXZ - p.scaleXZ) * k2;

  // Timed land squash, applied directly (not blended) so it always reads clearly.
  state.landTimer = Math.max(0, state.landTimer - dt);
  if (state.landTimer > 0 && !m.dashing) {
    const prog = 1 - state.landTimer / LAND_DURATION;
    const easeOut = 1 - (1 - prog) * (1 - prog);
    p.scaleY = LAND_SQUASH_Y + (1 - LAND_SQUASH_Y) * easeOut;
    p.scaleXZ = LAND_SQUASH_XZ + (1 - LAND_SQUASH_XZ) * easeOut;
    p.bodyY += LAND_BODY_DIP * (1 - easeOut);
  }

  return p;
}
