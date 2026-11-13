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
