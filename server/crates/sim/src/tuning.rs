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
