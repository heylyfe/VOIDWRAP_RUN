//! Movement tuning. All durations are in physics frames (1 / PHYSICS_HZ seconds).

use protocol::PHYSICS_HZ;

/// Fixed physics timestep in seconds.
pub const DT: f32 = 1.0 / PHYSICS_HZ as f32;

/// Player collision box half-width (x and z). `ControllerState.pos` is the feet point.
pub const PLAYER_HALF_WIDTH: f32 = 0.4;
/// Player collision box height.
pub const PLAYER_HEIGHT: f32 = 1.8;

/// Ground run speed, units/s. Applied instantly (no acceleration mush).
pub const RUN_SPEED: f32 = 9.0;
/// Gravity, units/s².
pub const GRAVITY: f32 = 32.0;
/// Initial jump velocity, units/s (apex ≈ v²/2g ≈ 2.2 units).
pub const JUMP_VELOCITY: f32 = 12.0;
/// Terminal fall speed, units/s.
pub const MAX_FALL_SPEED: f32 = 40.0;

/// Frames of coyote time after walking off a ledge (~120 ms at 60 Hz).
pub const COYOTE_FRAMES: u8 = 7;
/// Frames a jump press is remembered while waiting for a landing (~100 ms).
pub const JUMP_BUFFER_FRAMES: u8 = 6;
/// Upward speed is clamped to this when jump is released early (variable height).
pub const JUMP_CUT_VELOCITY: f32 = 4.0;

/// Dash speed, units/s.
pub const DASH_SPEED: f32 = 28.0;
/// Frames a dash lasts (28 * 9 / 60 = 4.2 units of travel).
pub const DASH_FRAMES: u8 = 9;
/// Frames between dashes.
pub const DASH_COOLDOWN_FRAMES: u16 = 45;

/// Feet-to-surface distance still treated as "resting on" a solid top.
pub const GROUND_EPSILON: f32 = 0.001;

/// Defensive lower bound applied to every `Tuning` multiplier.
pub const TUNING_MUL_MIN: f32 = 0.25;
/// Defensive upper bound applied to every `Tuning` multiplier.
pub const TUNING_MUL_MAX: f32 = 3.0;
/// Defensive upper bound applied to `Tuning::air_jumps`.
pub const TUNING_AIR_JUMPS_MAX: u8 = 3;
/// Shortest dash cooldown a tuned `dash_cooldown_mul` may produce, in frames.
pub const DASH_COOLDOWN_MIN_FRAMES: u16 = 10;
/// Longest dash cooldown a tuned `dash_cooldown_mul` may produce, in frames.
pub const DASH_COOLDOWN_MAX_FRAMES: u16 = 600;

/// CLASS_GLIDER: extra air jumps on top of `Tuning::air_jumps`.
///
/// The glider's total budget is additionally capped at `TUNING_AIR_JUMPS_MAX + GLIDER_BONUS_AIR_JUMPS`.
pub const GLIDER_BONUS_AIR_JUMPS: u8 = 1;
/// CLASS_GLIDER: fall speed cap while BTN_ABILITY is held and the player is falling (units/s).
pub const GLIDE_FALL_SPEED: f32 = 3.0;
/// CLASS_GLIDER: multiplier applied to the horizontal air-speed limit while gliding.
pub const GLIDE_AIR_SPEED_MUL: f32 = 1.15;

/// CLASS_BLINKER: horizontal teleport distance (units).
pub const BLINK_DISTANCE: f32 = 6.0;
/// CLASS_BLINKER: sweep step while searching for a free position (units of travel per sample).
pub const BLINK_SWEEP_STEP: f32 = 0.25;
/// CLASS_BLINKER: frames the visual flash lasts after a blink.
pub const BLINK_FLASH_FRAMES: u8 = 6;

/// CLASS_GRAPPLER: maximum distance from the feet to a grapple target (units).
pub const GRAPPLE_RANGE: f32 = 14.0;
/// CLASS_GRAPPLER: minimum distance to a grapple target; closer candidates are rejected (units).
pub const GRAPPLE_MIN_RANGE: f32 = 3.0;
/// CLASS_GRAPPLER: distance ahead of the feet at which the aim anchor sits (units).
pub const GRAPPLE_AIM_DISTANCE: f32 = 8.0;
/// CLASS_GRAPPLER: minimum dot product between the aim and the horizontal direction to the target.
pub const GRAPPLE_MIN_DOT: f32 = 0.5;
/// CLASS_GRAPPLER: a target may sit at most this far below the feet and still be valid (units).
pub const GRAPPLE_MAX_BELOW: f32 = 1.0;
/// CLASS_GRAPPLER: height added above the chosen point to form the pull-up target (units).
pub const GRAPPLE_TARGET_HEIGHT: f32 = 1.2;
/// CLASS_GRAPPLER: pull speed along the line to the target (units/s).
pub const GRAPPLE_SPEED: f32 = 22.0;
/// CLASS_GRAPPLER: maximum number of frames a grapple lasts.
pub const GRAPPLE_MAX_FRAMES: u8 = 45;
/// CLASS_GRAPPLER: distance from the target at which the grapple considers itself arrived (units).
pub const GRAPPLE_END_DISTANCE: f32 = 1.0;

/// Lower bound applied to `Tuning::friction_mul` (the existing `TUNING_MUL_MIN` does not apply here).
pub const FRICTION_MUL_MIN: f32 = 0.1;
/// Friction: if the remaining per-frame velocity error is this small, snap to the target.
pub const GROUND_STOP_EPSILON: f32 = 1e-4;
