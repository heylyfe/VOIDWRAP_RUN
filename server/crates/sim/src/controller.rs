//! Kinematic character controller.
//!
//! M1 controller: instant run speed (ground and air), gravity with terminal
//! velocity, coyote time, jump buffering, variable jump height (jump cut) and a
//! dash class ability. Collision is axis-separated AABB against [`World`].
//!
//! M2 classes (`ControllerState.class`, CLASS_*): CLASS_STRIKER dashes (the M1
//! ability), CLASS_GLIDER glides, CLASS_BULWARK blinks and CLASS_GRAPPLER
//! grapples. Unknown values behave like CLASS_STRIKER. Glide: holding
//! BTN_ABILITY while airborne and falling caps the fall speed at
//! `GLIDE_FALL_SPEED`, multiplies the horizontal air-speed limit by
//! `GLIDE_AIR_SPEED_MUL`, and grants one extra air jump (total still capped at
//! `TUNING_AIR_JUMPS_MAX + GLIDER_BONUS_AIR_JUMPS`). Blink: a BTN_ABILITY press
//! teleports horizontally up to `BLINK_DISTANCE` (never through a solid); the
//! blink frame applies no gravity and performs no normal horizontal movement,
//! so the teleported distance is exact. Grapple: a BTN_ABILITY press pulls the
//! player toward an automatically picked target point.
//!
//! [`step_with`] applies a [`Tuning`] on top of that (upgrades and curses): run,
//! jump, gravity and dash-speed multipliers, a dash-cooldown multiplier, extra
//! air jumps, and a ground `friction_mul` that scales per-frame ground
//! acceleration and braking (air control is not affected). [`step`] is the same
//! step with the neutral default tuning.
//!
//! [`step`] is the contract shared by the authoritative server and the WASM
//! client prediction. Both must produce bit-identical results, so the step path
//! only uses `+ - * /`, `sqrt`, `abs`, `min`, `max`, `clamp` and comparisons: no
//! transcendental functions, no randomness, no time, no allocation.
//!
//! Step order (identical on server and client):
//!   1. timers & edge detection (+ grapple cancel on a BTN_JUMP rising edge)
//!   2. class ability start (dash / blink / grapple)
//!   3. jump (buffer + coyote, then air jumps)
//!   4. vertical velocity (grapple pull / dash / gravity / jump cut / glide cap; skipped on the blink frame)
//!   5. horizontal velocity (grapple pull / dash / ground friction / glider boost; skipped on the blink frame)
//!   6. collide Y, then X, then Z (the blink frame moves on Y only)
//!   7. grounded & coyote refresh, air jump reset, ability timer tick
//!   8. `prev_buttons = input.buttons`
//!
//! Horizontal and vertical velocity were computed in the opposite order in M1;
//! the two are independent there so all M1 behaviour is preserved.
//! Ability frame counters follow the dash convention: a counter is set when the
//! ability starts and decremented later in the same step, so the firing step
//! ends with `counter == START - 1`.

use crate::tuning::*;
use crate::world::{World, overlaps};
use protocol::{
    Aabb, BTN_ABILITY, BTN_JUMP, CLASS_BULWARK, CLASS_GLIDER, CLASS_GRAPPLER, CLASS_STRIKER, ControllerState,
    InputFrame, Tuning, Vec3,
};

/// A fresh controller standing at `pos`.
pub fn spawn_state(pos: Vec3) -> ControllerState {
    ControllerState {
        pos,
        vel: Vec3::ZERO,
        grounded: false,
        coyote_frames: 0,
        jump_buffer_frames: 0,
        prev_buttons: 0,
        air_jumps_used: 0,
        ability_cooldown_frames: 0,
        ability_active_frames: 0,
        ability_dir: Vec3::ZERO,
        class: protocol::CLASS_STRIKER,
        ward_frames: 0,
        stagger_frames: 0,
    }
}

/// The player's collision box for a given feet position.
pub fn player_box(pos: &Vec3) -> Aabb {
    Aabb {
        min: Vec3::new(pos.x - PLAYER_HALF_WIDTH, pos.y, pos.z - PLAYER_HALF_WIDTH),
        max: Vec3::new(
            pos.x + PLAYER_HALF_WIDTH,
            pos.y + PLAYER_HEIGHT,
            pos.z + PLAYER_HALF_WIDTH,
        ),
    }
}

/// Normalised wish direction from the quantised input (length ≤ 1).
pub fn wish_dir(input: &InputFrame) -> (f32, f32) {
    // Clamp -128 to -127 so the range is symmetric.
    let x = f32::from(input.move_x.max(-127)) / 127.0;
    let z = f32::from(input.move_z.max(-127)) / 127.0;
    let len_sq = x * x + z * z;
    if len_sq > 1.0 {
        let len = len_sq.sqrt();
        (x / len, z / len)
    } else {
        (x, z)
    }
}

/// Advance one fixed physics frame of [`DT`] seconds with the neutral tuning.
///
/// Thin wrapper over [`step_with`] so every existing caller keeps working; with
/// the default tuning the result is bit-identical to the pre-tuning controller.
pub fn step(state: &mut ControllerState, input: &InputFrame, world: &World) {
    step_with(state, input, world, &Tuning::default());
}

/// How `state.class` selects the BTN_ABILITY behaviour. Unknown values behave like CLASS_STRIKER.
fn effective_class(class: u8) -> u8 {
    match class {
        CLASS_STRIKER | CLASS_GRAPPLER | CLASS_BULWARK | CLASS_GLIDER => class,
        _ => CLASS_STRIKER,
    }
}

/// Advance one fixed physics frame of [`DT`] seconds under `tuning`.
///
/// Every multiplier is clamped to [`TUNING_MUL_MIN`]..=[`TUNING_MUL_MAX`] (NaN
/// becomes the neutral 1.0) and `air_jumps` to `0..=TUNING_AIR_JUMPS_MAX`, so a
/// malformed tuning can never produce a non-finite or absurd step. `friction_mul`
/// is clamped to [`FRICTION_MUL_MIN`]..=[`TUNING_MUL_MAX`] (NaN becomes 1.0).
///
/// M3 additions (see the module docs):
/// - CLASS_BULWARK no longer blinks. A grounded BTN_ABILITY press starts an
///   8-frame wind-up (immobile); when the wind-up counter reaches zero the ward
///   begins: `ward_frames` is set to [`BULWARK_WARD_FRAMES`] and counts down one
///   frame per step. During the ward the player is rooted (horizontal velocity
///   held at zero, `vel.y` pinned to 0, gravity off, no jump/dash/ability).
///   `ward_frames > 0` is read by the server for the radial pulse and immunity;
///   [`bulwark_pulse_frame`] reports the frame the ward begins. The ability
///   cooldown ([`BULWARK_COOLDOWN_FRAMES`], scaled by the dash-cooldown
///   multiplier and clamped) starts at cast start.
/// - While `stagger_frames > 0` (set by the server together with a knockback
///   velocity) move input, jump, ability and dash are ignored, gravity and
///   collision apply normally and the current velocity is kept (ground friction
///   still applies, approaching zero); the counter decrements each frame.
/// - PLATFORM_JUMP_PAD platforms stay solid; landing on (or resting on) one
///   sets `vel.y = PAD_LAUNCH_VEL` and scales horizontal velocity by
///   [`PAD_HORIZONTAL_VEL_MUL`], once per contact. The pad's ascent is not
///   clipped by the variable-jump rule (that rule only caps the release of an
///   actual jump press).
/// - Step-up (amendment A2): a horizontal move blocked by a solid platform
///   whose top is at most [`STEP_HEIGHT`] above the feet is turned into a step
///   up onto it (horizontal motion is kept), only when grounded or within
///   coyote frames and only if the player box fits at the new height.
/// - TILE_FRETTED platforms stay solid; while standing on one the ground run
///   speed is multiplied by [`FRETTED_SPEED_MUL`] and BTN_ABILITY is ignored.
/// - Collision and ground queries only test platforms in the grid cells
///   overlapped by the (swept) player box; results are identical to brute force.
pub fn step_with(state: &mut ControllerState, input: &InputFrame, world: &World, tuning: &Tuning) {
    let was_grounded = state.grounded;
    let class = effective_class(state.class);

    // Defensive, deterministic clamps. With the default tuning every product
    // below is exactly the untuned constant (`x * 1.0 == x`), which is what keeps
    // `step` bit-identical to the pre-tuning controller.
    let run_speed = RUN_SPEED * clamp_mul(tuning.run_speed_mul);
    let jump_velocity = JUMP_VELOCITY * clamp_mul(tuning.jump_mul);
    let gravity = GRAVITY * clamp_mul(tuning.gravity_mul);
    let dash_speed = DASH_SPEED * clamp_mul(tuning.dash_speed_mul);
    let ability_cooldown = tuned_dash_cooldown(clamp_mul(tuning.dash_cooldown_mul));
    let bulwark_cooldown = tuned_bulwark_cooldown(tuning.dash_cooldown_mul);
    let tuning_air_jumps = tuning.air_jumps.min(TUNING_AIR_JUMPS_MAX);
    let air_jumps = if class == CLASS_GLIDER {
        (tuning_air_jumps + GLIDER_BONUS_AIR_JUMPS).min(TUNING_AIR_JUMPS_MAX + GLIDER_BONUS_AIR_JUMPS)
    } else {
        tuning_air_jumps
    };
    let friction = clamp_friction(tuning.friction_mul);

    // 1. Timers and button edges.
    state.ability_cooldown_frames = state.ability_cooldown_frames.saturating_sub(1);
    let staggered = state.stagger_frames > 0;
    state.stagger_frames = state.stagger_frames.saturating_sub(1);
    // CLASS_BULWARK: `ability_active_frames` is the wind-up counter; `ward_frames`
    // is the ward countdown. They are disjoint (the ward starts when the wind-up
    // reaches zero; see section 7).
    let warding = state.ward_frames > 0;
    let winding_up = class == CLASS_BULWARK && !warding && state.ability_active_frames > 0;
    let rooted = warding || winding_up;
    let jump_locked = rooted || staggered;
    let jump_pressed = input.buttons & BTN_JUMP != 0 && state.prev_buttons & BTN_JUMP == 0;
    let ability_pressed = input.buttons & BTN_ABILITY != 0 && state.prev_buttons & BTN_ABILITY == 0;
    if jump_pressed && !jump_locked {
        state.jump_buffer_frames = JUMP_BUFFER_FRAMES;
    }
    let (mut wx, mut wz) = wish_dir(input);
    if staggered {
        // A knockback shove: the wish direction is ignored entirely.
        wx = 0.0;
        wz = 0.0;
    }

    // Fretted-tile lock: computed from the feet as they were at the end of the
    // previous frame (same convention as `state.grounded`).
    let fretted_standing = standing_on_fretted(state, world);

    // A BTN_JUMP rising edge ends an active grapple. The normal jump path below
    // still runs, so the same press performs a jump if one is available.
    if jump_pressed && !jump_locked && class == CLASS_GRAPPLER && state.ability_active_frames > 0 {
        state.ability_active_frames = 0;
    }

    // 2. Ability start (class-dependent). Stagger, the ward/wind-up and a
    // fretted platform all swallow BTN_ABILITY (no ability starts, no cooldown).
    let ability_locked = rooted || staggered || fretted_standing;
    if ability_pressed && !ability_locked && state.ability_cooldown_frames == 0 && state.ability_active_frames == 0 {
        match class {
            CLASS_STRIKER => {
                state.ability_dir = dash_dir(wx, wz, &state.vel);
                state.ability_active_frames = DASH_FRAMES;
                state.ability_cooldown_frames = ability_cooldown;
            }
            CLASS_BULWARK => {
                // Grounded-only cast: a braced wind-up starts, the cooldown
                // counts from cast start. Airborne presses do nothing.
                if state.grounded {
                    state.ability_active_frames = BULWARK_WINDUP_FRAMES;
                    state.ability_cooldown_frames = bulwark_cooldown;
                }
            }
            CLASS_GRAPPLER => try_grapple(state, input, world, ability_cooldown),
            _ => {} // CLASS_GLIDER: BTN_ABILITY is a hold-to-glide, not a press action
        }
    }

    // Recompute the rooted/wind-up state AFTER the ability start so a cast that
    // started this frame already roots the player for the rest of the step
    // (jump, vertical and horizontal velocity, collision), instead of one frame
    // later. A cast cannot be locked out by the state it sets up (the
    // ability-lock above deliberately uses the pre-cast state); from here on the
    // cast frame is an immobile wind-up frame like any other.
    let warding = state.ward_frames > 0;
    let winding_up = class == CLASS_BULWARK && !warding && state.ability_active_frames > 0;
    let rooted = warding || winding_up;
    let jump_locked = rooted || staggered;

    // 3. Jump: consume the buffer on the first step a jump is allowed.
    if !jump_locked && state.jump_buffer_frames > 0 && (state.grounded || state.coyote_frames > 0) {
        state.vel.y = jump_velocity;
        state.grounded = false;
        state.coyote_frames = 0;
        state.jump_buffer_frames = 0;
        state.ability_active_frames = 0; // jumping cancels a dash/grapple
    } else if !jump_locked && state.jump_buffer_frames > 0 && state.air_jumps_used < air_jumps {
        // Air jump: only reachable when the ground/coyote rules said no. Coyote
        // jumps take the branch above and therefore never spend an air jump.
        state.vel.y = jump_velocity;
        state.grounded = false;
        state.coyote_frames = 0;
        state.jump_buffer_frames = 0;
        state.ability_active_frames = 0; // an air jump cancels a dash/grapple too
        state.air_jumps_used += 1;
    } else if state.jump_buffer_frames > 0 && !jump_pressed {
        // The arming step itself does not tick, so a press N steps before
        // touchdown still fires on the first step a jump is allowed.
        state.jump_buffer_frames -= 1;
    }

    // Grapple pull: decide whether a grapple in flight keeps driving velocity
    // this frame. The stored `ability_dir` is the target POINT, not a unit axis.
    // A shove (stagger) or the ward takes the velocity away from the grapple.
    let grapple_locked = rooted || staggered;
    let mut grappling = false;
    if !grapple_locked && class == CLASS_GRAPPLER && state.ability_active_frames > 0 {
        let target = state.ability_dir.clone();
        let dx = target.x - state.pos.x;
        let dy = target.y - state.pos.y;
        let dz = target.z - state.pos.z;
        let dist_sq = dx * dx + dy * dy + dz * dz;
        if dist_sq <= GRAPPLE_END_DISTANCE * GRAPPLE_END_DISTANCE {
            state.ability_active_frames = 0; // arrived; normal physics resumes
        } else {
            let scale = GRAPPLE_SPEED / dist_sq.sqrt();
            state.vel = Vec3::new(dx * scale, dy * scale, dz * scale);
            grappling = true;
        }
    }

    // A stagger interrupts the dash's velocity override; the counter still
    // ticks down in section 7 and simply expires.
    let dashing = class == CLASS_STRIKER && state.ability_active_frames > 0 && !staggered;

    // 4. Vertical velocity. Terminal fall speed is deliberately not tuned.
    if grappling {
        // Velocity was fully set by the grapple; no gravity or jump cut applies.
    } else if rooted {
        // The ward hovers in place: gravity is off.
        state.vel.y = 0.0;
    } else if dashing {
        state.vel.y = 0.0;
    } else {
        state.vel.y = (state.vel.y - gravity * DT).max(-MAX_FALL_SPEED);
        if !jump_locked
            && class == CLASS_GLIDER
            && !state.grounded
            && input.buttons & BTN_ABILITY != 0
            && state.vel.y < -GLIDE_FALL_SPEED
        {
            state.vel.y = -GLIDE_FALL_SPEED;
        }
        // Variable jump height: cap the rise only on the release edge of an
        // actual BTN_JUMP press (the frame the button went from held to not
        // held). An ascent that never involved a jump press — a jump-pad
        // launch — is not a jump to \"cut\" and keeps PAD_LAUNCH_VEL; a tapped
        // jump is cut on its release frame, exactly as under the pre-M3 rule
        // (the min() below is a no-op once vel.y <= JUMP_CUT_VELOCITY).
        if !jump_locked
            && input.buttons & BTN_JUMP == 0
            && state.prev_buttons & BTN_JUMP != 0
            && state.vel.y > 0.0
        {
            state.vel.y = state.vel.y.min(JUMP_CUT_VELOCITY);
        }
    }

    // 5. Horizontal velocity (instant, ground and air; friction scales ground response).
    if grappling {
        // Velocity was fully set by the grapple.
    } else if rooted {
        // The ward/wind-up holds the player in place horizontally.
        state.vel.x = 0.0;
        state.vel.z = 0.0;
    } else if dashing {
        state.vel.x = state.ability_dir.x * dash_speed;
        state.vel.z = state.ability_dir.z * dash_speed;
    } else if staggered {
        if state.grounded {
            // Only ground friction remains, approaching a full stop.
            state.vel.x = approach_ground(state.vel.x, 0.0, friction);
            state.vel.z = approach_ground(state.vel.z, 0.0, friction);
        }
        // Airborne: the shove velocity is kept untouched.
    } else if state.grounded {
        let max_speed = if fretted_standing {
            run_speed * FRETTED_SPEED_MUL
        } else {
            run_speed
        };
        state.vel.x = approach_ground(state.vel.x, wx * max_speed, friction);
        state.vel.z = approach_ground(state.vel.z, wz * max_speed, friction);
    } else {
        let mut target_x = wx * run_speed;
        let mut target_z = wz * run_speed;
        if class == CLASS_GLIDER && input.buttons & BTN_ABILITY != 0 && state.vel.y < 0.0 {
            target_x *= GLIDE_AIR_SPEED_MUL;
            target_z *= GLIDE_AIR_SPEED_MUL;
        }
        state.vel.x = target_x;
        state.vel.z = target_z;
    }

    // 6. Collide, one axis at a time. Jump pads never block movement.
    let landed = move_axis(state, world, Axis::Y, state.vel.y * DT);
    move_axis(state, world, Axis::X, state.vel.x * DT);
    move_axis(state, world, Axis::Z, state.vel.z * DT);

    // 7. Grounded & coyote refresh.
    state.grounded = landed || resting_on_solid(state, world);
    if state.grounded {
        state.coyote_frames = COYOTE_FRAMES;
        state.air_jumps_used = 0;
    } else if !was_grounded {
        // Only steps that began airborne consume coyote time, so the step that
        // walks off a ledge still leaves the whole window available.
        state.coyote_frames = state.coyote_frames.saturating_sub(1);
    }
    // Jump pads: launching overrides the grounded state for this frame.
    // Once per contact: after the launch the player is airborne (vel.y > 0),
    // so the launch cannot re-trigger until the next landing.
    if launch_pad_if_contact(state, world, dashing || grappling || rooted) {
        state.grounded = false;
        state.coyote_frames = 0;
    }
    // CLASS_BULWARK: the frame the wind-up counter reaches zero the ward begins
    // (this is the frame `bulwark_pulse_frame` reports). Afterwards the ward
    // counts down one frame per step.
    let windup_done =
        class == CLASS_BULWARK && state.ward_frames == 0 && state.ability_active_frames == 1;
    state.ability_active_frames = state.ability_active_frames.saturating_sub(1);
    if windup_done {
        state.ward_frames = BULWARK_WARD_FRAMES;
    } else if state.ward_frames > 0 {
        state.ward_frames = state.ward_frames.saturating_sub(1);
    }

    // 8. Remember buttons for edge detection.
    state.prev_buttons = input.buttons;
}

/// Clamp the ground-velocity response toward `target` by `friction`.
///
/// With `friction >= 1.0` the target is applied instantly (the M1 behaviour,
/// bit-identical). Below 1.0 the remaining error shrinks by that factor per
/// ground step; errors below [`GROUND_STOP_EPSILON`] snap to the target so the
/// velocity reaches exactly zero instead of approaching it forever.
fn approach_ground(current: f32, target: f32, friction: f32) -> f32 {
    if friction >= 1.0 {
        return target;
    }
    let delta = target - current;
    if delta.abs() <= GROUND_STOP_EPSILON {
        return target;
    }
    current + delta * friction
}

/// Clamp a tuning multiplier into [`TUNING_MUL_MIN`]..=[`TUNING_MUL_MAX`], mapping
/// NaN to the neutral 1.0.
///
/// Written with comparisons plus `clamp` only: `min`/`max` return the *other*
/// operand when one side is NaN, so they cannot detect NaN, while comparisons are
/// false for NaN — which is exactly what the final `else` branch keys on. No
/// transcendental function is involved, so the result is identical on every target.
fn clamp_mul(value: f32) -> f32 {
    if value >= TUNING_MUL_MIN {
        value.clamp(TUNING_MUL_MIN, TUNING_MUL_MAX)
    } else if value <= TUNING_MUL_MAX {
        TUNING_MUL_MIN
    } else {
        1.0
    }
}

/// Clamp `Tuning::friction_mul` to [`FRICTION_MUL_MIN`]..=[`TUNING_MUL_MAX`],
/// mapping NaN to the neutral 1.0 (same pattern as [`clamp_mul`]).
fn clamp_friction(value: f32) -> f32 {
    if value >= FRICTION_MUL_MIN {
        value.clamp(FRICTION_MUL_MIN, TUNING_MUL_MAX)
    } else if value <= TUNING_MUL_MAX {
        FRICTION_MUL_MIN
    } else {
        1.0
    }
}

/// Dash cooldown in frames for a multiplier already passed through [`clamp_mul`].
///
/// `(x + 0.5) as u16` is a deterministic round-half-up: `as` truncates toward zero
/// and saturates, and `x` is non-negative, so `f32::round` (whose behaviour is not
/// guaranteed to be identical on every target) is not needed. The result is then
/// clamped to [`DASH_COOLDOWN_MIN_FRAMES`]..=[`DASH_COOLDOWN_MAX_FRAMES`].
fn tuned_dash_cooldown(mul: f32) -> u16 {
    let scaled = DASH_COOLDOWN_FRAMES as f32 * mul;
    let rounded = (scaled + 0.5) as u16;
    rounded.clamp(DASH_COOLDOWN_MIN_FRAMES, DASH_COOLDOWN_MAX_FRAMES)
}

/// Unit horizontal dash direction: wish direction, else current horizontal
/// velocity, else straight back (-Z).
fn dash_dir(wx: f32, wz: f32, vel: &Vec3) -> Vec3 {
    let wish_len_sq = wx * wx + wz * wz;
    if wish_len_sq > 0.0 {
        let len = wish_len_sq.sqrt();
        return Vec3::new(wx / len, 0.0, wz / len);
    }
    let vel_len_sq = vel.x * vel.x + vel.z * vel.z;
    if vel_len_sq > 0.0 {
        let len = vel_len_sq.sqrt();
        return Vec3::new(vel.x / len, 0.0, vel.z / len);
    }
    Vec3::new(0.0, 0.0, -1.0)
}

/// Unit horizontal aim direction: wish input, else horizontal velocity, else None.
///
/// Used by the blink and grapple classes; unlike [`dash_dir`] there is no
/// fallback direction, because those abilities do nothing without an aim.
fn aim_dir(wx: f32, wz: f32, vel: &Vec3) -> Option<Vec3> {
    let wish_len_sq = wx * wx + wz * wz;
    if wish_len_sq > 0.0 {
        let len = wish_len_sq.sqrt();
        return Some(Vec3::new(wx / len, 0.0, wz / len));
    }
    let vel_len_sq = vel.x * vel.x + vel.z * vel.z;
    if vel_len_sq > 0.0 {
        let len = vel_len_sq.sqrt();
        return Some(Vec3::new(vel.x / len, 0.0, vel.z / len));
    }
    None
}

/// True when `bounds` overlap any currently solid platform. Only the platforms
/// in the grid cells overlapped by `bounds` are tested.
fn overlaps_any_solid(world: &World, bounds: &Aabb) -> bool {
    world
        .solid_candidates(bounds.min.x, bounds.min.z, bounds.max.x, bounds.max.z)
        .into_iter()
        .any(|index| match world.solid_bounds(index) {
            Some(solid) => overlaps(bounds, solid),
            None => false,
        })
}

// CLASS_BULWARK no longer blinks (M3): the blink was replaced by the braced
// wind-up and the ward. Intentionally deleted; see `bulwark_pulse_frame` and
// the CLASS_BULWARK arm in `step_with`.

/// CLASS_GRAPPLER: pick a grapple target automatically and begin the pull.
///
/// Candidates are the points on solid platforms' top faces closest (in x/z, y
/// fixed at the top) to `pos + aim * GRAPPLE_AIM_DISTANCE`. A candidate is valid
/// when its 3D distance from the feet is within [`GRAPPLE_MIN_RANGE`]..=[`GRAPPLE_RANGE`],
/// its y is at least `pos.y - GRAPPLE_MAX_BELOW`, and the horizontal direction
/// to it dots with the aim at least [`GRAPPLE_MIN_DOT`]. The valid candidate
/// closest to the anchor wins; ties go to the lowest platform id. With no aim,
/// or no valid candidate, nothing happens and no cooldown starts.
///
/// On a fire the stored `ability_dir` is the target POINT (top point +
/// [`GRAPPLE_TARGET_HEIGHT`]), not a direction axis.
fn try_grapple(state: &mut ControllerState, input: &InputFrame, world: &World, cooldown: u16) {
    let (wx, wz) = wish_dir(input);
    let Some(aim) = aim_dir(wx, wz, &state.vel) else {
        return; // no aim: no grapple, no cooldown
    };
    let anchor = Vec3::new(
        state.pos.x + aim.x * GRAPPLE_AIM_DISTANCE,
        state.pos.y,
        state.pos.z + aim.z * GRAPPLE_AIM_DISTANCE,
    );
    // (distance squared to the anchor, platform id, target point).
    let mut best: Option<(f32, u16, Vec3)> = None;
    for platform in &world.platforms {
        if world.gone.contains(&platform.id) {
            continue; // TILE_GONE platforms are not solid
        }
        let point = Vec3::new(
            anchor.x.clamp(platform.bounds.min.x, platform.bounds.max.x),
            platform.bounds.max.y,
            anchor.z.clamp(platform.bounds.min.z, platform.bounds.max.z),
        );
        let dx = point.x - state.pos.x;
        let dy = point.y - state.pos.y;
        let dz = point.z - state.pos.z;
        let dist_sq = dx * dx + dy * dy + dz * dz;
        if dist_sq < GRAPPLE_MIN_RANGE * GRAPPLE_MIN_RANGE || dist_sq > GRAPPLE_RANGE * GRAPPLE_RANGE {
            continue;
        }
        if point.y < state.pos.y - GRAPPLE_MAX_BELOW {
            continue;
        }
        let hlen_sq = dx * dx + dz * dz;
        if hlen_sq == 0.0 {
            continue; // no horizontal direction to P
        }
        let dot = (dx * aim.x + dz * aim.z) / hlen_sq.sqrt();
        if dot < GRAPPLE_MIN_DOT {
            continue;
        }
        let ax = point.x - anchor.x;
        let ay = point.y - anchor.y;
        let az = point.z - anchor.z;
        let anchor_dist_sq = ax * ax + ay * ay + az * az;
        let better = match &best {
            None => true,
            Some((best_dist, best_id, _)) => {
                anchor_dist_sq < *best_dist || (anchor_dist_sq == *best_dist && platform.id < *best_id)
            }
        };
        if better {
            best = Some((anchor_dist_sq, platform.id, point));
        }
    }
    let Some((_, _, point)) = best else {
        return; // no valid target: no grapple, no cooldown
    };
    state.ability_dir = Vec3::new(point.x, point.y + GRAPPLE_TARGET_HEIGHT, point.z);
    state.ability_active_frames = GRAPPLE_MAX_FRAMES;
    state.ability_cooldown_frames = cooldown;
}

/// True when the feet rest on a solid top (within [`GROUND_EPSILON`]) while
/// overlapping it in x/z and not moving upwards. Needed because a dash zeroes
/// `vel.y`, which makes the downward Y move (and therefore `landed`) zero.
fn resting_on_solid(state: &ControllerState, world: &World) -> bool {
    if state.vel.y > 0.0 {
        return false;
    }
    let b = player_box(&state.pos);
    for index in world.solid_candidates(b.min.x, b.min.z, b.max.x, b.max.z) {
        let Some(solid) = world.solid_bounds(index) else {
            continue;
        };
        let overlaps_xz =
            b.min.x < solid.max.x && b.max.x > solid.min.x && b.min.z < solid.max.z && b.max.z > solid.min.z;
        if overlaps_xz && (state.pos.y - solid.max.y).abs() <= GROUND_EPSILON {
            return true;
        }
    }
    false
}

#[derive(Clone, Copy)]
enum Axis {
    X,
    Y,
    Z,
}

fn get(v: &Vec3, a: Axis) -> f32 {
    match a {
        Axis::X => v.x,
        Axis::Y => v.y,
        Axis::Z => v.z,
    }
}

fn set(v: &mut Vec3, a: Axis, value: f32) {
    match a {
        Axis::X => v.x = value,
        Axis::Y => v.y = value,
        Axis::Z => v.z = value,
    }
}

/// Moves along one axis, stopping flush against the first solid hit.
/// Returns true if the move was a downward move that landed on something.
///
/// Amendment A2 (step-up): a horizontal move blocked by a solid platform whose
/// top is at most [`STEP_HEIGHT`] above the feet is turned into a step up onto
/// that platform (the player is raised so the feet rest on the top), keeping
/// horizontal motion. This happens only when the player is grounded or within
/// coyote frames and only when the player box fits at the new height.
///
/// Only platforms in the grid cells overlapped by the swept player box are
/// tested; the iteration order matches platform order, so the result is
/// identical to brute force.
fn move_axis(state: &mut ControllerState, world: &World, axis: Axis, delta: f32) -> bool {
    if delta == 0.0 {
        return false;
    }
    let mut pos = state.pos.clone();
    let target = get(&pos, axis) + delta;
    set(&mut pos, axis, target);
    // Swept box (XZ) used to pick candidate platforms from the spatial grid.
    let start_box = player_box(&state.pos);
    let end_box = player_box(&pos);
    let min_x = start_box.min.x.min(end_box.min.x);
    let max_x = start_box.max.x.max(end_box.max.x);
    let min_z = start_box.min.z.min(end_box.min.z);
    let max_z = start_box.max.z.max(end_box.max.z);
    let mut hit = false;
    for index in world.solid_candidates(min_x, min_z, max_x, max_z) {
        let Some(solid) = world.solid_bounds(index) else {
            continue;
        };
        let b = player_box(&pos);
        if !overlaps(&b, solid) {
            continue;
        }
        // Step-up attempt for horizontal moves only.
        if matches!(axis, Axis::X | Axis::Z) && (state.grounded || state.coyote_frames > 0) {
            let feet = pos.y;
            let top = solid.max.y;
            if top > feet && top - feet <= STEP_HEIGHT {
                let stepped = Vec3::new(pos.x, top, pos.z);
                if !overlaps_any_solid(world, &player_box(&stepped)) {
                    pos = stepped; // step up; horizontal motion is kept
                    continue;
                }
            }
        }
        hit = true;
        // Push back flush against the face we entered through.
        let p = get(&pos, axis);
        let resolved = match axis {
            Axis::Y if delta < 0.0 => solid.max.y,
            Axis::Y => solid.min.y - PLAYER_HEIGHT,
            _ if delta < 0.0 => get(&solid.max, axis) + PLAYER_HALF_WIDTH,
            _ => get(&solid.min, axis) - PLAYER_HALF_WIDTH,
        };
        // Only ever push backwards along the motion, never forwards.
        let clamped = if delta < 0.0 { resolved.max(p) } else { resolved.min(p) };
        set(&mut pos, axis, clamped);
    }
    if hit {
        set(&mut state.vel, axis, 0.0);
    }
    state.pos = pos;
    hit && matches!(axis, Axis::Y) && delta < 0.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::levels;
    use protocol::{EVENT_NONE, LevelData, PLATFORM_NORMAL, Platform, TILE_GONE, TILE_WARN, TileState};

    fn input(seq: u32, move_x: i8, move_z: i8, buttons: u8) -> InputFrame {
        InputFrame {
            seq,
            move_x,
            move_z,
            buttons,
        }
    }

    fn idle(seq: u32) -> InputFrame {
        input(seq, 0, 0, 0)
    }

    fn run_x(seq: u32, buttons: u8) -> InputFrame {
        input(seq, 127, 0, buttons)
    }

    fn world() -> World {
        World::from_level(&levels::bootstrap())
    }

    /// A large flat slab with no edges, for tests that must not fall off.
    pub(super) fn flat_world() -> World {
        World::from_level(&LevelData {
            level: 1,
            seed: 1,
            spawn: Vec3::ZERO,
            beacon: Vec3::new(0.0, 0.0, 30.0),
            kill_y: -200.0,
            platforms: vec![Platform {
                id: 1,
                kind: PLATFORM_NORMAL,
                bounds: Aabb {
                    min: Vec3::new(-100.0, -1.0, -100.0),
                    max: Vec3::new(100.0, 0.0, 100.0),
                },
            }],
            gifts: vec![],
            event: EVENT_NONE,
            altars: vec![],
        })
    }

    /// Two candidate grapple platforms over a void, id 3 above id 5.
    fn grapple_level() -> LevelData {
        LevelData {
            level: 1,
            seed: 7,
            spawn: Vec3::ZERO,
            beacon: Vec3::new(9.0, 5.0, 0.0),
            kill_y: -100.0,
            platforms: vec![
                Platform {
                    id: 3,
                    kind: PLATFORM_NORMAL,
                    bounds: Aabb {
                        min: Vec3::new(6.0, 4.0, -2.0),
                        max: Vec3::new(12.0, 5.0, 2.0),
                    },
                },
                Platform {
                    id: 5,
                    kind: PLATFORM_NORMAL,
                    bounds: Aabb {
                        min: Vec3::new(6.0, 1.0, -2.0),
                        max: Vec3::new(12.0, 2.0, 2.0),
                    },
                },
            ],
            gifts: vec![],
            event: EVENT_NONE,
            altars: vec![],
        }
    }

    /// Like `count_air_jumps`, but for any class.
    fn count_class_air_jumps(class: u8, tuning: &Tuning, start_y: f32, steps: u32) -> u32 {
        let w = flat_world();
        let mut s = spawn_state(Vec3::new(0.0, start_y, 0.0));
        s.class = class;
        let mut seq = 0;
        for _ in 0..3 {
            step_with(&mut s, &idle(seq), &w, tuning);
            seq += 1;
        }
        assert!(!s.grounded, "controller must still be airborne");
        let mut fired = 0;
        for i in 0..steps {
            let buttons = if i.is_multiple_of(2) { BTN_JUMP } else { 0 };
            let before = s.air_jumps_used;
            step_with(&mut s, &input(seq, 0, 0, buttons), &w, tuning);
            seq += 1;
            if s.air_jumps_used > before {
                fired += 1;
            }
        }
        fired
    }

    /// Runs +X until the controller leaves the ground. On return the controller
    /// is airborne and the next step is airborne step 1.
    fn run_off_ledge(s: &mut ControllerState, w: &World, seq: &mut u32) {
        let mut guard = 0;
        while s.grounded && guard < 2000 {
            step(s, &run_x(*seq, 0), w);
            *seq += 1;
            guard += 1;
        }
        assert!(!s.grounded, "controller never left the slab");
    }

    /// Number of steps (1-based) until a controller dropped from `start_y`
    /// first reports `grounded`.
    fn landing_step(start_y: f32) -> u32 {
        let w = world();
        let mut s = spawn_state(Vec3::new(0.0, start_y, 0.0));
        let mut n = 0;
        while !s.grounded && n < 2000 {
            step(&mut s, &idle(n), &w);
            n += 1;
        }
        assert!(s.grounded, "controller never landed");
        n
    }

    /// Counts how many air jumps fire while spamming jump edges from `start_y`.
    fn count_air_jumps(tuning: &Tuning, start_y: f32, steps: u32) -> u32 {
        let w = flat_world();
        let mut s = spawn_state(Vec3::new(0.0, start_y, 0.0));
        let mut seq = 0;
        for _ in 0..3 {
            step_with(&mut s, &idle(seq), &w, tuning);
            seq += 1;
        }
        assert!(!s.grounded, "controller must still be airborne");
        let mut fired = 0;
        for i in 0..steps {
            let buttons = if i.is_multiple_of(2) { BTN_JUMP } else { 0 };
            let before = s.air_jumps_used;
            step_with(&mut s, &input(seq, 0, 0, buttons), &w, tuning);
            seq += 1;
            if s.air_jumps_used > before {
                fired += 1;
            }
        }
        fired
    }

    #[test]
    fn falls_and_lands_on_plane() {
        let w = world();
        let mut s = spawn_state(Vec3::new(0.0, 3.0, 0.0));
        for i in 0..120 {
            step(&mut s, &idle(i), &w);
        }
        assert!(s.grounded);
        assert_eq!(s.pos.y, 0.0);
        assert_eq!(s.vel.y, 0.0);
    }

    #[test]
    fn walks_off_edge_into_void() {
        let w = world();
        let mut s = spawn_state(Vec3::ZERO);
        for i in 0..400 {
            step(&mut s, &run_x(i, 0), &w);
        }
        assert!(s.pos.y < w.kill_y);
    }

    #[test]
    fn diagonal_input_is_normalised() {
        let (x, z) = wish_dir(&input(0, 127, -128, 0));
        assert!(((x * x + z * z).sqrt() - 1.0).abs() < 1e-6);
    }

    #[test]
    fn coyote_jump_fires_on_airborne_step_seven() {
        let w = world();
        let mut s = spawn_state(Vec3::ZERO);
        let mut seq = 0;
        step(&mut s, &idle(seq), &w);
        seq += 1;
        assert!(s.grounded);
        run_off_ledge(&mut s, &w, &mut seq);
        for k in 1..=7u32 {
            let buttons = if k == 7 { BTN_JUMP } else { 0 };
            step(&mut s, &run_x(seq, buttons), &w);
            seq += 1;
        }
        assert!(s.vel.y > 0.0, "coyote jump on airborne step 7 must fire");
    }

    #[test]
    fn coyote_jump_does_not_fire_on_airborne_step_eight() {
        let w = world();
        let mut s = spawn_state(Vec3::ZERO);
        let mut seq = 0;
        step(&mut s, &idle(seq), &w);
        seq += 1;
        assert!(s.grounded);
        run_off_ledge(&mut s, &w, &mut seq);
        for k in 1..=8u32 {
            let buttons = if k == 8 { BTN_JUMP } else { 0 };
            step(&mut s, &run_x(seq, buttons), &w);
            seq += 1;
        }
        assert!(s.vel.y <= 0.0, "coyote jump on airborne step 8 must not fire");
    }

    #[test]
    fn buffered_jump_fires_within_one_step_of_landing() {
        let w = world();
        let land = landing_step(5.0);
        assert!(land > 6);
        let press_at = land - 5;
        let mut s = spawn_state(Vec3::new(0.0, 5.0, 0.0));
        let mut jumped = false;
        for i in 1..=(land + 2) {
            let buttons = if i == press_at { BTN_JUMP } else { 0 };
            step(&mut s, &input(i, 0, 0, buttons), &w);
            if i >= land && s.vel.y > 0.0 {
                jumped = true;
            }
        }
        assert!(jumped, "buffered jump must fire within one step of landing");
    }

    #[test]
    fn buffered_jump_expires_before_landing() {
        let w = world();
        let land = landing_step(5.0);
        assert!(land > 9);
        let press_at = land - 8;
        let mut s = spawn_state(Vec3::new(0.0, 5.0, 0.0));
        let mut jumped = false;
        for i in 1..=(land + 2) {
            let buttons = if i == press_at { BTN_JUMP } else { 0 };
            step(&mut s, &input(i, 0, 0, buttons), &w);
            if i >= land && s.vel.y > 0.0 {
                jumped = true;
            }
        }
        assert!(!jumped, "an 8-step-old jump press must not fire on landing");
    }

    #[test]
    fn holding_jump_does_not_rejump_after_landing() {
        let w = world();
        let mut s = spawn_state(Vec3::ZERO);
        step(&mut s, &idle(0), &w);
        assert!(s.grounded);
        step(&mut s, &input(1, 0, 0, BTN_JUMP), &w);
        assert!(s.vel.y > 0.0);
        for i in 0..200 {
            step(&mut s, &input(2 + i, 0, 0, BTN_JUMP), &w);
        }
        assert!(s.grounded);
        assert_eq!(s.vel.y, 0.0);
        // A fresh press does jump again.
        step(&mut s, &idle(300), &w);
        step(&mut s, &input(301, 0, 0, BTN_JUMP), &w);
        assert!(s.vel.y > 0.0);
    }

    #[test]
    fn jump_cut_lowers_apex() {
        let w = world();
        let apex = |hold: bool| -> f32 {
            let mut s = spawn_state(Vec3::ZERO);
            step(&mut s, &idle(0), &w);
            let mut max_y = s.pos.y;
            for i in 0..120 {
                let buttons = if i == 0 || hold { BTN_JUMP } else { 0 };
                step(&mut s, &input(1 + i, 0, 0, buttons), &w);
                if s.pos.y > max_y {
                    max_y = s.pos.y;
                }
            }
            max_y
        };
        let tapped = apex(false);
        let held = apex(true);
        assert!(tapped < held, "tapped apex {tapped} must be below held apex {held}");
    }

    #[test]
    fn dash_travels_expected_distance() {
        let w = world();
        let mut s = spawn_state(Vec3::ZERO);
        let mut seq = 0;
        step(&mut s, &idle(seq), &w);
        seq += 1;
        assert!(s.grounded);
        let start_x = s.pos.x;
        step(&mut s, &run_x(seq, BTN_ABILITY), &w);
        seq += 1;
        assert!(s.ability_active_frames > 0);
        for _ in 1..DASH_FRAMES {
            step(&mut s, &run_x(seq, 0), &w);
            seq += 1;
        }
        let travelled = s.pos.x - start_x;
        let in_range = (4.0..=4.4).contains(&travelled);
        assert!(in_range, "dash travelled {travelled}, expected 28 * 9 / 60 = 4.2");
    }

    #[test]
    fn dash_zeroes_vertical_velocity_in_air() {
        let w = world();
        let mut s = spawn_state(Vec3::new(0.0, 5.0, 0.0));
        let mut seq = 0;
        step(&mut s, &run_x(seq, BTN_ABILITY), &w);
        seq += 1;
        assert!(s.ability_active_frames > 0);
        assert_eq!(s.vel.y, 0.0);
        for _ in 1..DASH_FRAMES {
            step(&mut s, &run_x(seq, 0), &w);
            seq += 1;
            assert_eq!(s.vel.y, 0.0);
        }
    }

    #[test]
    fn dash_cooldown_blocks_and_then_allows() {
        let w = world();
        let mut s = spawn_state(Vec3::ZERO);
        let mut seq = 0;
        step(&mut s, &idle(seq), &w);
        seq += 1;
        assert!(s.grounded);

        step(&mut s, &run_x(seq, BTN_ABILITY), &w);
        seq += 1;
        assert!(s.ability_active_frames > 0);
        for _ in 1..DASH_FRAMES {
            step(&mut s, &run_x(seq, 0), &w);
            seq += 1;
        }
        assert_eq!(s.ability_active_frames, 0);
        assert!(s.ability_cooldown_frames > 0);

        // A second press while cooling down does nothing.
        let before = s.pos.x;
        step(&mut s, &run_x(seq, BTN_ABILITY), &w);
        seq += 1;
        assert_eq!(s.ability_active_frames, 0);
        assert!((s.pos.x - before - RUN_SPEED * DT).abs() < 1e-4);

        // After the cooldown it works again.
        while s.ability_cooldown_frames > 0 {
            step(&mut s, &idle(seq), &w);
            seq += 1;
        }
        step(&mut s, &run_x(seq, BTN_ABILITY), &w);
        assert!(s.ability_active_frames > 0);
    }

    #[test]
    fn dash_without_input_goes_negative_z() {
        let w = world();
        let mut s = spawn_state(Vec3::ZERO);
        let mut seq = 0;
        step(&mut s, &idle(seq), &w);
        seq += 1;
        assert_eq!(s.vel.x, 0.0);
        assert_eq!(s.vel.z, 0.0);
        step(&mut s, &input(seq, 0, 0, BTN_ABILITY), &w);
        assert_eq!(s.ability_dir, Vec3::new(0.0, 0.0, -1.0));
        assert_eq!(s.vel.x, 0.0);
        assert!(s.vel.z < 0.0);
    }

    #[test]
    fn terminal_fall_lands_on_thin_slab() {
        let level = LevelData {
            level: 1,
            seed: 1,
            spawn: Vec3::ZERO,
            beacon: Vec3::new(0.0, 0.0, 30.0),
            kill_y: -30.0,
            platforms: vec![Platform {
                id: 1,
                kind: PLATFORM_NORMAL,
                bounds: Aabb {
                    min: Vec3::new(-10.0, -1.0, -10.0),
                    max: Vec3::new(10.0, 0.0, 10.0),
                },
            }],
            gifts: vec![],
            event: EVENT_NONE,
            altars: vec![],
        };
        let w = World::from_level(&level);
        let mut s = spawn_state(Vec3::new(0.0, 40.0, 0.0));
        for i in 0..400 {
            step(&mut s, &idle(i), &w);
        }
        assert!(s.grounded);
        assert_eq!(s.pos.y, 0.0);
        assert_eq!(s.vel.y, 0.0);
    }

    #[test]
    fn wall_stops_flush_and_does_not_ground() {
        let level = LevelData {
            level: 1,
            seed: 1,
            spawn: Vec3::ZERO,
            beacon: Vec3::new(0.0, 0.0, 30.0),
            kill_y: -100.0,
            platforms: vec![Platform {
                id: 1,
                kind: PLATFORM_NORMAL,
                bounds: Aabb {
                    min: Vec3::new(5.0, -10.0, -5.0),
                    max: Vec3::new(6.0, 10.0, 5.0),
                },
            }],
            gifts: vec![],
            event: EVENT_NONE,
            altars: vec![],
        };
        let w = World::from_level(&level);
        let wall_min_x = 5.0;
        let mut s = spawn_state(Vec3::new(0.0, 5.0, 0.0));
        for i in 0..40 {
            step(&mut s, &run_x(i, 0), &w);
        }
        assert_eq!(s.pos.x, wall_min_x - PLAYER_HALF_WIDTH);
        assert!(!s.grounded);
    }

    fn scripted(seq: u32) -> InputFrame {
        let phase = seq % 120;
        let move_x = match phase {
            0..=29 => 127,
            30..=59 => -128,
            60..=89 => 64,
            _ => 0,
        };
        let move_z = if phase.is_multiple_of(17) { 127 } else { 0 };
        let mut buttons = 0u8;
        if phase.is_multiple_of(23) {
            buttons |= BTN_JUMP;
        }
        if phase.is_multiple_of(37) {
            buttons |= BTN_ABILITY;
        }
        input(seq, move_x, move_z, buttons)
    }

    #[test]
    fn step_is_deterministic() {
        let w = world();
        let run = || {
            let mut s = spawn_state(Vec3::new(0.0, 2.0, 0.0));
            for i in 0..600 {
                step(&mut s, &scripted(i), &w);
            }
            s
        };
        assert_eq!(run(), run());
    }

    #[test]
    fn step_matches_step_with_default_tuning() {
        let w = world();
        let default = Tuning::default();
        let mut plain = spawn_state(Vec3::new(0.0, 2.0, 0.0));
        let mut tuned = spawn_state(Vec3::new(0.0, 2.0, 0.0));
        for i in 0..600 {
            let frame = scripted(i);
            step(&mut plain, &frame, &w);
            step_with(&mut tuned, &frame, &w, &default);
            assert_eq!(plain, tuned, "states diverged at step {i}");
        }
    }

    #[test]
    fn nan_tuning_falls_back_to_neutral() {
        let w = world();
        let nan = Tuning {
            run_speed_mul: f32::NAN,
            jump_mul: f32::NAN,
            gravity_mul: f32::NAN,
            dash_speed_mul: f32::NAN,
            dash_cooldown_mul: f32::NAN,
            air_jumps: 0,
            friction_mul: f32::NAN,
        };
        let mut plain = spawn_state(Vec3::new(0.0, 2.0, 0.0));
        let mut tuned = spawn_state(Vec3::new(0.0, 2.0, 0.0));
        for i in 0..600 {
            let frame = scripted(i);
            step(&mut plain, &frame, &w);
            step_with(&mut tuned, &frame, &w, &nan);
        }
        assert_eq!(plain, tuned, "NaN multipliers must behave like the neutral tuning");
    }

    #[test]
    fn nan_friction_behaves_like_one() {
        let w = world();
        let nan = Tuning {
            friction_mul: f32::NAN,
            ..Tuning::default()
        };
        let mut plain = spawn_state(Vec3::new(0.0, 2.0, 0.0));
        let mut tuned = spawn_state(Vec3::new(0.0, 2.0, 0.0));
        for i in 0..600 {
            let frame = scripted(i);
            step(&mut plain, &frame, &w);
            step_with(&mut tuned, &frame, &w, &nan);
        }
        assert_eq!(plain, tuned, "NaN friction must behave like the neutral tuning");
    }

    #[test]
    fn tuning_clamps_are_defensive() {
        assert_eq!(clamp_mul(f32::NAN), 1.0);
        assert_eq!(clamp_mul(0.0), TUNING_MUL_MIN);
        assert_eq!(clamp_mul(1.0), 1.0);
        assert_eq!(clamp_mul(100.0), TUNING_MUL_MAX);
        assert_eq!(tuned_dash_cooldown(1.0), DASH_COOLDOWN_FRAMES);
        assert_eq!(tuned_dash_cooldown(0.0), DASH_COOLDOWN_MIN_FRAMES);
        assert_eq!(tuned_dash_cooldown(100.0), DASH_COOLDOWN_MAX_FRAMES);
    }

    #[test]
    fn friction_clamps_are_defensive() {
        assert_eq!(clamp_friction(f32::NAN), 1.0);
        assert_eq!(clamp_friction(0.0), FRICTION_MUL_MIN);
        assert_eq!(clamp_friction(1.0), 1.0);
        assert_eq!(clamp_friction(100.0), TUNING_MUL_MAX);
        assert_eq!(FRICTION_MUL_MIN, 0.1);
    }

    #[test]
    fn ice_friction_takes_longer_to_stop_than_normal() {
        let w = flat_world();
        let stop_steps = |friction: f32| -> u32 {
            let tuning = Tuning {
                friction_mul: friction,
                ..Tuning::default()
            };
            let mut s = spawn_state(Vec3::ZERO);
            let mut seq = 0;
            step_with(&mut s, &idle(seq), &w, &tuning);
            seq += 1;
            assert!(s.grounded);
            for _ in 0..10 {
                step_with(&mut s, &run_x(seq, 0), &w, &tuning);
                seq += 1;
            }
            let mut n = 0;
            while s.vel.x != 0.0 {
                step_with(&mut s, &idle(seq), &w, &tuning);
                seq += 1;
                n += 1;
                assert!(n < 1000, "friction {friction} never stopped");
            }
            n
        };
        let normal = stop_steps(1.0);
        let ice = stop_steps(0.25);
        assert_eq!(normal, 1, "friction 1.0 brakes instantly");
        assert!(
            ice > normal,
            "friction 0.25 ({ice} steps) must take longer to stop than 1.0 ({normal} steps)"
        );
    }

    #[test]
    fn friction_does_not_affect_air_control() {
        let w = flat_world();
        let tuning = Tuning {
            friction_mul: 0.25,
            ..Tuning::default()
        };
        let mut s = spawn_state(Vec3::new(0.0, 30.0, 0.0));
        step_with(&mut s, &run_x(0, 0), &w, &tuning);
        assert_eq!(s.vel.x, RUN_SPEED, "air control stays instant regardless of friction");
    }

    #[test]
    fn air_jumps_fire_exactly_air_jumps_times() {
        assert_eq!(count_air_jumps(&Tuning::default(), 40.0, 40), 0);
        for allowed in 1..=3u8 {
            let tuning = Tuning {
                air_jumps: allowed,
                ..Tuning::default()
            };
            assert_eq!(count_air_jumps(&tuning, 40.0, 40), u32::from(allowed));
        }
        let clamped = Tuning {
            air_jumps: 200,
            ..Tuning::default()
        };
        assert_eq!(count_air_jumps(&clamped, 40.0, 40), 3, "air_jumps clamps to 3");
    }

    #[test]
    fn glider_gets_one_extra_air_jump() {
        assert_eq!(count_class_air_jumps(CLASS_GLIDER, &Tuning::default(), 40.0, 40), 1);
        let three = Tuning {
            air_jumps: 3,
            ..Tuning::default()
        };
        assert_eq!(
            count_class_air_jumps(CLASS_GLIDER, &three, 40.0, 40),
            4,
            "glider total budget is TUNING_AIR_JUMPS_MAX + 1"
        );
        let huge = Tuning {
            air_jumps: 255,
            ..Tuning::default()
        };
        assert_eq!(
            count_class_air_jumps(CLASS_GLIDER, &huge, 40.0, 40),
            4,
            "a huge tuning air_jumps still caps the glider at 4"
        );
    }

    #[test]
    fn glider_caps_fall_speed_while_holding_ability() {
        let w = flat_world();
        // Not holding BTN_ABILITY: falls well past the glide speed.
        let mut plain = spawn_state(Vec3::new(0.0, 60.0, 0.0));
        plain.class = CLASS_GLIDER;
        for i in 0..30 {
            step_with(&mut plain, &idle(i), &w, &Tuning::default());
        }
        assert!(
            plain.vel.y < -GLIDE_FALL_SPEED,
            "un-powered glider must fall faster than the cap, got {}",
            plain.vel.y
        );
        // Holding BTN_ABILITY: fall speed is capped.
        let mut gliding = spawn_state(Vec3::new(0.0, 60.0, 0.0));
        gliding.class = CLASS_GLIDER;
        for i in 0..30 {
            step_with(&mut gliding, &input(i, 0, 0, BTN_ABILITY), &w, &Tuning::default());
        }
        assert_eq!(gliding.vel.y, -GLIDE_FALL_SPEED);
    }

    #[test]
    fn glider_air_speed_boosted_only_while_gliding() {
        let w = flat_world();
        let mut boosted = spawn_state(Vec3::new(0.0, 60.0, 0.0));
        boosted.class = CLASS_GLIDER;
        for i in 0..30 {
            step_with(&mut boosted, &input(i, 127, 0, BTN_ABILITY), &w, &Tuning::default());
        }
        assert_eq!(
            boosted.vel.x,
            RUN_SPEED * GLIDE_AIR_SPEED_MUL,
            "while gliding the horizontal air-speed limit is multiplied"
        );
        // Not holding ability: normal air speed.
        let mut normal = spawn_state(Vec3::new(0.0, 60.0, 0.0));
        normal.class = CLASS_GLIDER;
        for i in 0..30 {
            step_with(&mut normal, &run_x(i, 0), &w, &Tuning::default());
        }
        assert_eq!(normal.vel.x, RUN_SPEED);
    }

    #[test]
    fn glider_has_no_dash() {
        let w = flat_world();
        let mut s = spawn_state(Vec3::ZERO);
        s.class = CLASS_GLIDER;
        step_with(&mut s, &idle(0), &w, &Tuning::default());
        assert!(s.grounded);
        step_with(&mut s, &run_x(1, BTN_ABILITY), &w, &Tuning::default());
        assert_eq!(s.ability_active_frames, 0, "glider must not dash");
        assert_eq!(s.ability_cooldown_frames, 0, "glider's glide is not on cooldown");
    }

    #[test]
    fn striker_dash_unchanged_by_class_system() {
        let w = flat_world();
        let mut s = spawn_state(Vec3::ZERO);
        step_with(&mut s, &idle(0), &w, &Tuning::default());
        assert_eq!(s.class, CLASS_STRIKER);
        assert!(s.grounded);
        step_with(&mut s, &run_x(1, BTN_ABILITY), &w, &Tuning::default());
        // The counter is set and then decremented within the same step.
        assert_eq!(s.ability_active_frames, DASH_FRAMES - 1);
        assert_eq!(s.ability_cooldown_frames, DASH_COOLDOWN_FRAMES);
    }

    #[test]
    fn unknown_class_behaves_like_striker() {
        let w = world();
        let mut striker = spawn_state(Vec3::new(0.0, 2.0, 0.0));
        let mut unknown = spawn_state(Vec3::new(0.0, 2.0, 0.0));
        unknown.class = 200;
        for i in 0..300 {
            let frame = scripted(i);
            step(&mut striker, &frame, &w);
            step(&mut unknown, &frame, &w);
        }
        unknown.class = CLASS_STRIKER;
        assert_eq!(striker, unknown, "unknown class must behave exactly like CLASS_STRIKER");
    }

// Deleted in M3: CLASS_BULWARK no longer blinks. Replaced by the bulwark tests
// in `m3_tests`.

// Deleted in M3: CLASS_BULWARK no longer blinks. Replaced by the bulwark tests
// in `m3_tests`.

// Deleted in M3: CLASS_BULWARK no longer blinks. Replaced by the bulwark tests
// in `m3_tests`.

// Deleted in M3: CLASS_BULWARK no longer blinks. Replaced by the bulwark tests
// in `m3_tests`.

// Deleted in M3: CLASS_BULWARK no longer blinks. Replaced by the bulwark tests
// in `m3_tests`.

    #[test]
    fn grapple_picks_nearest_valid_platform_and_lands_on_it() {
        let w = World::from_level(&grapple_level());
        let mut s = spawn_state(Vec3::new(0.0, 6.0, 0.0));
        s.class = CLASS_GRAPPLER;
        step_with(&mut s, &input(0, 127, 0, BTN_ABILITY), &w, &Tuning::default());
        // Platform id 3 (top y = 5.0) is closest to the anchor at (8, 6, 0).
        // The counter is set and then decremented within the same step.
        assert_eq!(s.ability_active_frames, GRAPPLE_MAX_FRAMES - 1);
        assert_eq!(s.ability_cooldown_frames, DASH_COOLDOWN_FRAMES);
        assert_eq!(s.ability_dir.x, 8.0);
        assert!(
            (s.ability_dir.y - (5.0 + GRAPPLE_TARGET_HEIGHT)).abs() < 1e-4,
            "target is the stored point P + GRAPPLE_TARGET_HEIGHT, got {}",
            s.ability_dir.y
        );
        assert_eq!(s.ability_dir.z, 0.0);
        let target = s.ability_dir.clone();
        for i in 0..400u32 {
            step_with(&mut s, &idle(1 + i), &w, &Tuning::default());
            if s.ability_active_frames == 0 {
                break;
            }
        }
        assert_eq!(s.ability_active_frames, 0, "grapple must end");
        let dx = s.pos.x - target.x;
        let dy = s.pos.y - target.y;
        let dz = s.pos.z - target.z;
        assert!(
            (dx * dx + dy * dy + dz * dz).sqrt() <= GRAPPLE_END_DISTANCE + 0.5,
            "grapple must end near its target"
        );
        for i in 0..300u32 {
            step_with(&mut s, &idle(500 + i), &w, &Tuning::default());
        }
        assert!(s.grounded);
        assert_eq!(s.pos.y, 5.0, "must land on the chosen platform (id 3)");
    }

    #[test]
    fn grapple_ignores_gone_platforms() {
        let w = World::from_level(&grapple_level());
        let mut w = w;
        w.set_tiles(&[TileState {
            id: 3,
            state: TILE_GONE,
        }]);
        // Spawn low enough that platform id 5's top (y = 2.0) is exactly
        // GRAPPLE_MAX_BELOW below the feet (the validity bound is inclusive);
        // from y = 6.0 the only remaining candidate would be rejected.
        let mut s = spawn_state(Vec3::new(0.0, 3.0, 0.0));
        s.class = CLASS_GRAPPLER;
        step_with(&mut s, &input(0, 127, 0, BTN_ABILITY), &w, &Tuning::default());
        // The counter is set and then decremented within the same step.
        assert_eq!(s.ability_active_frames, GRAPPLE_MAX_FRAMES - 1);
        assert!(
            (s.ability_dir.y - (2.0 + GRAPPLE_TARGET_HEIGHT)).abs() < 1e-4,
            "with id 3 gone, the grapple must pick id 5, got y = {}",
            s.ability_dir.y
        );

        // With every candidate gone, no grapple fires.
        w.set_tiles(&[
            TileState {
                id: 3,
                state: TILE_GONE,
            },
            TileState {
                id: 5,
                state: TILE_GONE,
            },
        ]);
        let mut s2 = spawn_state(Vec3::new(0.0, 6.0, 0.0));
        s2.class = CLASS_GRAPPLER;
        step_with(&mut s2, &input(0, 127, 0, BTN_ABILITY), &w, &Tuning::default());
        assert_eq!(s2.ability_active_frames, 0);
        assert_eq!(s2.ability_cooldown_frames, 0);
    }

    #[test]
    fn grapple_treats_warn_tiles_as_solid() {
        let mut w = World::from_level(&grapple_level());
        w.set_tiles(&[TileState {
            id: 3,
            state: TILE_WARN,
        }]);
        let mut s = spawn_state(Vec3::new(0.0, 6.0, 0.0));
        s.class = CLASS_GRAPPLER;
        step_with(&mut s, &input(0, 127, 0, BTN_ABILITY), &w, &Tuning::default());
        // The counter is set and then decremented within the same step.
        assert_eq!(s.ability_active_frames, GRAPPLE_MAX_FRAMES - 1);
        assert!((s.ability_dir.y - (5.0 + GRAPPLE_TARGET_HEIGHT)).abs() < 1e-4);
    }

    #[test]
    fn grapple_cancelled_by_jump_edge_and_jump_fires() {
        let w = World::from_level(&grapple_level());
        let tuning = Tuning {
            air_jumps: 1,
            ..Tuning::default()
        };
        let mut s = spawn_state(Vec3::new(0.0, 6.0, 0.0));
        s.class = CLASS_GRAPPLER;
        step_with(&mut s, &input(0, 127, 0, BTN_ABILITY), &w, &tuning);
        assert!(s.ability_active_frames > 0);
        step_with(&mut s, &input(1, 0, 0, BTN_JUMP), &w, &tuning);
        assert_eq!(s.ability_active_frames, 0, "BTN_JUMP must cancel the grapple");
        assert!(s.vel.y > 0.0, "an available jump fires with the same press");
        assert_eq!(s.air_jumps_used, 1);
    }

    #[test]
    fn grapple_cancelled_by_jump_without_jump_available() {
        let w = World::from_level(&grapple_level());
        let mut s = spawn_state(Vec3::new(0.0, 6.0, 0.0));
        s.class = CLASS_GRAPPLER;
        step_with(&mut s, &input(0, 127, 0, BTN_ABILITY), &w, &Tuning::default());
        assert!(s.ability_active_frames > 0);
        step_with(&mut s, &input(1, 0, 0, BTN_JUMP), &w, &Tuning::default());
        assert_eq!(s.ability_active_frames, 0, "BTN_JUMP must cancel the grapple");
        assert_eq!(s.air_jumps_used, 0, "no jump was available, so none fired");
    }

    #[test]
    fn grapple_without_aim_does_nothing() {
        let w = World::from_level(&grapple_level());
        let mut s = spawn_state(Vec3::new(0.0, 6.0, 0.0));
        s.class = CLASS_GRAPPLER;
        step_with(&mut s, &idle(0), &w, &Tuning::default());
        assert_eq!(s.vel.x, 0.0);
        step_with(&mut s, &input(1, 0, 0, BTN_ABILITY), &w, &Tuning::default());
        assert_eq!(s.ability_active_frames, 0);
        assert_eq!(s.ability_cooldown_frames, 0);
    }

    #[test]
    fn grapple_aims_along_horizontal_velocity_when_no_input() {
        let w = World::from_level(&grapple_level());
        let mut s = spawn_state(Vec3::new(0.0, 6.0, 0.0));
        s.class = CLASS_GRAPPLER;
        step_with(&mut s, &input(0, 127, 0, 0), &w, &Tuning::default());
        assert!(s.vel.x > 0.0);
        step_with(&mut s, &input(1, 0, 0, BTN_ABILITY), &w, &Tuning::default());
        // The counter is set and then decremented within the same step.
        assert_eq!(s.ability_active_frames, GRAPPLE_MAX_FRAMES - 1);
        assert!(s.ability_dir.x > s.pos.x, "aim falls back to the velocity direction");
    }

    #[test]
    fn grapple_rejects_candidate_with_bad_dot() {
        let level = LevelData {
            level: 1,
            seed: 7,
            spawn: Vec3::ZERO,
            beacon: Vec3::new(-9.0, 4.0, 0.0),
            kill_y: -100.0,
            platforms: vec![Platform {
                id: 8,
                kind: PLATFORM_NORMAL,
                bounds: Aabb {
                    min: Vec3::new(-12.0, 3.0, -2.0),
                    max: Vec3::new(-6.0, 4.0, 2.0),
                },
            }],
            gifts: vec![],
            event: EVENT_NONE,
            altars: vec![],
        };
        let w = World::from_level(&level);
        let mut s = spawn_state(Vec3::new(0.0, 6.0, 0.0));
        s.class = CLASS_GRAPPLER;
        // Aim is +X; the only platform is behind the player (dot < 0.5).
        step_with(&mut s, &input(0, 127, 0, BTN_ABILITY), &w, &Tuning::default());
        assert_eq!(s.ability_active_frames, 0);
        assert_eq!(s.ability_cooldown_frames, 0);
    }

    #[test]
    fn grapple_rejects_candidate_within_min_range() {
        let level = LevelData {
            level: 1,
            seed: 7,
            spawn: Vec3::ZERO,
            beacon: Vec3::new(0.0, 6.0, 0.0),
            kill_y: -100.0,
            platforms: vec![Platform {
                id: 2,
                kind: PLATFORM_NORMAL,
                // Directly overhead, point P only 1 unit away (< GRAPPLE_MIN_RANGE).
                bounds: Aabb {
                    min: Vec3::new(-0.05, 5.5, -0.05),
                    max: Vec3::new(0.05, 6.0, 0.05),
                },
            }],
            gifts: vec![],
            event: EVENT_NONE,
            altars: vec![],
        };
        let w = World::from_level(&level);
        let mut s = spawn_state(Vec3::new(0.0, 6.0, 0.0));
        s.class = CLASS_GRAPPLER;
        step_with(&mut s, &input(0, 127, 0, BTN_ABILITY), &w, &Tuning::default());
        assert_eq!(s.ability_active_frames, 0);
        assert_eq!(s.ability_cooldown_frames, 0);
    }

    #[test]
    fn air_jumps_reset_on_landing() {
        let w = flat_world();
        let tuning = Tuning {
            air_jumps: 1,
            ..Tuning::default()
        };
        let mut s = spawn_state(Vec3::new(0.0, 6.0, 0.0));
        let mut seq = 0;

        // Fall, then spend the single air jump.
        step_with(&mut s, &idle(seq), &w, &tuning);
        seq += 1;
        step_with(&mut s, &input(seq, 0, 0, BTN_JUMP), &w, &tuning);
        seq += 1;
        assert_eq!(s.air_jumps_used, 1);
        assert!(s.vel.y > 0.0);

        // A second press while still airborne must not fire.
        step_with(&mut s, &idle(seq), &w, &tuning);
        seq += 1;
        step_with(&mut s, &input(seq, 0, 0, BTN_JUMP), &w, &tuning);
        seq += 1;
        assert_eq!(s.air_jumps_used, 1, "the air jump budget is spent");

        // Land: the budget comes back.
        let mut guard = 0;
        while !s.grounded && guard < 2000 {
            step_with(&mut s, &idle(seq), &w, &tuning);
            seq += 1;
            guard += 1;
        }
        assert!(s.grounded, "controller never landed");
        assert_eq!(s.air_jumps_used, 0, "landing must reset the air jump budget");

        // A ground jump does not consume an air jump; the next airborne press does.
        step_with(&mut s, &input(seq, 0, 0, BTN_JUMP), &w, &tuning);
        seq += 1;
        assert!(s.vel.y > 0.0, "ground jump after landing");
        assert_eq!(s.air_jumps_used, 0, "a ground jump must not consume an air jump");
        step_with(&mut s, &idle(seq), &w, &tuning);
        seq += 1;
        step_with(&mut s, &input(seq, 0, 0, BTN_JUMP), &w, &tuning);
        assert_eq!(s.air_jumps_used, 1, "the restored budget is usable again");
    }

    #[test]
    fn jump_mul_raises_apex() {
        let w = flat_world();
        let apex = |tuning: &Tuning| -> f32 {
            let mut s = spawn_state(Vec3::ZERO);
            step_with(&mut s, &idle(0), &w, tuning);
            assert!(s.grounded);
            let mut max_y = s.pos.y;
            for i in 0..120u32 {
                step_with(&mut s, &input(1 + i, 0, 0, BTN_JUMP), &w, tuning);
                if s.pos.y > max_y {
                    max_y = s.pos.y;
                }
            }
            max_y
        };
        let base = apex(&Tuning::default());
        let boosted = apex(&Tuning {
            jump_mul: 1.5,
            ..Tuning::default()
        });
        assert!(
            boosted > base,
            "jump_mul 1.5 apex {boosted} must exceed base apex {base}"
        );
    }

    #[test]
    fn gravity_mul_lengthens_airtime() {
        let w = flat_world();
        let airtime = |tuning: &Tuning| -> u32 {
            let mut s = spawn_state(Vec3::ZERO);
            step_with(&mut s, &idle(0), &w, tuning);
            assert!(s.grounded);
            step_with(&mut s, &input(1, 0, 0, BTN_JUMP), &w, tuning);
            assert!(s.vel.y > 0.0);
            let mut n = 0;
            // Hold jump so the jump cut cannot shorten the arc.
            while !s.grounded && n < 2000 {
                step_with(&mut s, &input(2 + n, 0, 0, BTN_JUMP), &w, tuning);
                n += 1;
            }
            assert!(s.grounded, "controller never landed");
            n
        };
        let base = airtime(&Tuning::default());
        let floaty = airtime(&Tuning {
            gravity_mul: 0.8,
            ..Tuning::default()
        });
        assert!(
            floaty > base,
            "gravity_mul 0.8 airtime {floaty} must exceed base airtime {base}"
        );
    }

    #[test]
    fn dash_cooldown_mul_halves_cooldown() {
        let w = flat_world();
        let cooldown = |tuning: &Tuning| -> u16 {
            let mut s = spawn_state(Vec3::ZERO);
            step_with(&mut s, &idle(0), &w, tuning);
            assert!(s.grounded);
            step_with(&mut s, &run_x(1, BTN_ABILITY), &w, tuning);
            assert!(s.ability_active_frames > 0);
            s.ability_cooldown_frames
        };
        let base = cooldown(&Tuning::default());
        let halved = cooldown(&Tuning {
            dash_cooldown_mul: 0.5,
            ..Tuning::default()
        });
        assert_eq!(base, DASH_COOLDOWN_FRAMES);
        // 45 * 0.5 = 22.5 rounds half-up to 23.
        assert_eq!(halved, DASH_COOLDOWN_FRAMES.div_ceil(2));
    }
}


/// CLASS_BULWARK: true on the frame the ward begins.
///
/// The wind-up runs `BULWARK_WINDUP_FRAMES` frames after a grounded cast; on
/// the frame it completes (`ward_frames` set to `BULWARK_WARD_FRAMES` at the end
/// of the step) this function returns true. The rooted ward physics apply from
/// the following frame for exactly `BULWARK_WARD_FRAMES` frames. The server
/// calls this after each step to trigger the radial pulse; the immunity applies
/// while `ward_frames > 0`.
pub fn bulwark_pulse_frame(state: &ControllerState) -> bool {
    state.ward_frames == BULWARK_WARD_FRAMES
}

/// Ability cooldown in frames for the RAW `Tuning::dash_cooldown_mul`
/// (*not* pre-clamped to [`TUNING_MUL_MIN`] like [`tuned_dash_cooldown`]).
///
/// The cooldown is scaled by the raw multiplier (NaN maps to the neutral 1.0,
/// a negative multiplier clamps to 0.0 and the upper bound is
/// [`TUNING_MUL_MAX`]); only the *result* is clamped to
/// [`DASH_COOLDOWN_MIN_FRAMES`]..=[`BULWARK_COOLDOWN_MAX_FRAMES`]. Same
/// round-half-up rule as [`tuned_dash_cooldown`]. With a multiplier of 0.0 the
/// result is therefore [`DASH_COOLDOWN_MIN_FRAMES`], not 960 * TUNING_MUL_MIN.
fn tuned_bulwark_cooldown(mul: f32) -> u16 {
    let mul = if mul >= 0.0 {
        mul.clamp(0.0, TUNING_MUL_MAX)
    } else if mul <= TUNING_MUL_MAX {
        0.0
    } else {
        1.0 // NaN
    };
    let scaled = BULWARK_COOLDOWN_FRAMES as f32 * mul;
    let rounded = (scaled + 0.5) as u16;
    rounded.clamp(DASH_COOLDOWN_MIN_FRAMES, BULWARK_COOLDOWN_MAX_FRAMES)
}

/// Index of the solid platform the player is standing on (feet within
/// [`GROUND_EPSILON`] of its top and overlapping it in x/z), or `None` when not
/// standing. Returns `None` while moving upwards.
fn standing_platform(state: &ControllerState, world: &World) -> Option<usize> {
    if state.vel.y > 0.0 {
        return None;
    }
    let b = player_box(&state.pos);
    for index in world.solid_candidates(b.min.x, b.min.z, b.max.x, b.max.z) {
        let Some(solid) = world.solid_bounds(index) else {
            continue;
        };
        let overlaps_xz =
            b.min.x < solid.max.x && b.max.x > solid.min.x && b.min.z < solid.max.z && b.max.z > solid.min.z;
        if overlaps_xz && (state.pos.y - solid.max.y).abs() <= GROUND_EPSILON {
            return Some(index);
        }
    }
    None
}

/// True while standing on a TILE_FRETTED platform (which stays solid).
fn standing_on_fretted(state: &ControllerState, world: &World) -> bool {
    match standing_platform(state, world) {
        Some(index) => world.tile_state(index) == protocol::TILE_FRETTED,
        None => false,
    }
}

/// PLATFORM_JUMP_PAD: launch once when (or while) standing on a pad.
///
/// Sets `vel.y = PAD_LAUNCH_VEL` and multiplies horizontal velocity by
/// [`PAD_HORIZONTAL_VEL_MUL`]. After the launch the player is airborne
/// (`vel.y > 0`), which makes a repeated launch on the same contact impossible;
/// a new landing is a new contact. Pads stay solid. `movement_locked` (dash /
/// grapple / ward in flight) suppresses the launch; it fires on the first
/// physics frame afterwards instead.
fn launch_pad_if_contact(state: &mut ControllerState, world: &World, movement_locked: bool) -> bool {
    if movement_locked {
        return false;
    }
    let Some(index) = standing_platform(state, world) else {
        return false;
    };
    let is_pad = world
        .platforms
        .get(index)
        .map(|p| p.kind == protocol::PLATFORM_JUMP_PAD)
        .unwrap_or(false);
    if !is_pad {
        return false;
    }
    state.vel.y = PAD_LAUNCH_VEL;
    state.vel.x *= PAD_HORIZONTAL_VEL_MUL;
    state.vel.z *= PAD_HORIZONTAL_VEL_MUL;
    true
}

#[cfg(test)]
mod m3_tests {
    use super::*;
    use super::tests::flat_world;
    use crate::tuning::{
        BULWARK_COOLDOWN_MAX_FRAMES, BULWARK_WARD_FRAMES, BULWARK_WINDUP_FRAMES, DASH_COOLDOWN_MIN_FRAMES,
        FRETTED_SPEED_MUL, PAD_LAUNCH_VEL,
    };
    use protocol::{EVENT_NONE, LevelData, PLATFORM_JUMP_PAD, PLATFORM_NORMAL, Platform, TILE_FRETTED, TileState};

    fn input(seq: u32, move_x: i8, move_z: i8, buttons: u8) -> InputFrame {
        InputFrame {
            seq,
            move_x,
            move_z,
            buttons,
        }
    }

    fn idle(seq: u32) -> InputFrame {
        input(seq, 0, 0, 0)
    }

    fn run_x(seq: u32, buttons: u8) -> InputFrame {
        input(seq, 127, 0, buttons)
    }

    fn run_x_jump(seq: u32) -> InputFrame {
        input(seq, 127, 0, BTN_JUMP)
    }

    fn staircase_world() -> World {
        let mut platforms = vec![Platform {
            id: 1,
            kind: PLATFORM_NORMAL,
            bounds: Aabb {
                min: Vec3::new(-10.0, -1.0, -2.0),
                max: Vec3::new(0.0, 0.0, 2.0),
            },
        }];
        for k in 0..5u16 {
            let top = 0.5 * (f32::from(k) + 1.0);
            platforms.push(Platform {
                id: k + 2,
                kind: PLATFORM_NORMAL,
                bounds: Aabb {
                    min: Vec3::new(f32::from(k), -1.0, -2.0),
                    max: Vec3::new(f32::from(k) + 1.0, top, 2.0),
                },
            });
        }
        // Top plateau.
        platforms.push(Platform {
            id: 8,
            kind: PLATFORM_NORMAL,
            bounds: Aabb {
                min: Vec3::new(5.0, -1.0, -2.0),
                max: Vec3::new(20.0, 2.5, 2.0),
            },
        });
        World::from_level(&LevelData {
            level: 1,
            seed: 3,
            spawn: Vec3::ZERO,
            beacon: Vec3::new(10.0, 0.0, 0.0),
            kill_y: -50.0,
            platforms,
            gifts: vec![],
            event: EVENT_NONE,
            altars: vec![],
        })
    }

    fn ledge_world(top: f32, ceiling: Option<(f32, f32)>) -> World {
        // Flat approach with a single ledge at x = 4.0. `ceiling` is
        // (min_y, max_y) for an overhead slab spanning the ledge.
        let mut platforms = vec![
            Platform {
                id: 1,
                kind: PLATFORM_NORMAL,
                bounds: Aabb {
                    min: Vec3::new(-10.0, -1.0, -2.0),
                    max: Vec3::new(4.0, 0.0, 2.0),
                },
            },
            Platform {
                id: 2,
                kind: PLATFORM_NORMAL,
                bounds: Aabb {
                    min: Vec3::new(4.0, -1.0, -2.0),
                    max: Vec3::new(8.0, top, 2.0),
                },
            },
        ];
        if let Some((ceil_min_y, ceil_max_y)) = ceiling {
            platforms.push(Platform {
                id: 3,
                kind: PLATFORM_NORMAL,
                bounds: Aabb {
                    min: Vec3::new(3.8, ceil_min_y, -2.0),
                    max: Vec3::new(8.0, ceil_max_y, 2.0),
                },
            });
        }
        World::from_level(&LevelData {
            level: 1,
            seed: 3,
            spawn: Vec3::ZERO,
            beacon: Vec3::new(0.0, 0.0, 30.0),
            kill_y: -50.0,
            platforms,
            gifts: vec![],
            event: EVENT_NONE,
            altars: vec![],
        })
    }

    fn pad_world() -> World {
        World::from_level(&LevelData {
            level: 1,
            seed: 4,
            spawn: Vec3::ZERO,
            beacon: Vec3::new(0.0, 0.0, 30.0),
            kill_y: -50.0,
            platforms: vec![Platform {
                id: 1,
                kind: PLATFORM_JUMP_PAD,
                bounds: Aabb {
                    min: Vec3::new(-2.0, -1.0, -2.0),
                    max: Vec3::new(2.0, 0.0, 2.0),
                },
            }],
            gifts: vec![],
            event: EVENT_NONE,
            altars: vec![],
        })
    }

    fn fretted_world() -> World {
        let level = LevelData {
            level: 1,
            seed: 5,
            spawn: Vec3::ZERO,
            beacon: Vec3::new(0.0, 0.0, 30.0),
            kill_y: -50.0,
            platforms: vec![Platform {
                id: 5,
                kind: PLATFORM_NORMAL,
                bounds: Aabb {
                    min: Vec3::new(-5.0, -1.0, -5.0),
                    max: Vec3::new(5.0, 0.0, 5.0),
                },
            }],
            gifts: vec![],
            event: EVENT_NONE,
            altars: vec![],
        };
        let mut w = World::from_level(&level);
        w.set_tiles(&[TileState {
            id: 5,
            state: TILE_FRETTED,
        }]);
        w
    }

    // ---------- Bulwark ----------

    #[test]
    fn bulwark_cast_requires_grounded() {
        let w = flat_world();
        let mut s = spawn_state(Vec3::new(0.0, 10.0, 0.0));
        s.class = CLASS_BULWARK;
        step_with(&mut s, &run_x(0, BTN_ABILITY), &w, &Tuning::default());
        assert_eq!(s.ability_active_frames, 0, "an airborne press must not cast");
        assert_eq!(s.ability_cooldown_frames, 0);
        assert_eq!(s.ward_frames, 0);
    }

    #[test]
    fn bulwark_grounded_cast_starts_windup_and_cooldown() {
        let w = flat_world();
        let mut s = spawn_state(Vec3::ZERO);
        s.class = CLASS_BULWARK;
        step_with(&mut s, &idle(0), &w, &Tuning::default());
        assert!(s.grounded);
        step_with(&mut s, &run_x(1, BTN_ABILITY), &w, &Tuning::default());
        assert_eq!(s.ability_active_frames, BULWARK_WINDUP_FRAMES - 1);
        assert_eq!(s.ability_cooldown_frames, BULWARK_COOLDOWN_FRAMES);
        assert_eq!(s.ward_frames, 0);
        assert!(!bulwark_pulse_frame(&s));
    }

    #[test]
    fn bulwark_cooldown_scales_and_clamps() {
        let w = flat_world();
        let cooldown = |mul: f32| -> u16 {
            let tuning = Tuning {
                dash_cooldown_mul: mul,
                ..Tuning::default()
            };
            let mut s = spawn_state(Vec3::ZERO);
            s.class = CLASS_BULWARK;
            step_with(&mut s, &idle(0), &w, &tuning);
            step_with(&mut s, &run_x(1, BTN_ABILITY), &w, &tuning);
            s.ability_cooldown_frames
        };
        assert_eq!(cooldown(1.0), BULWARK_COOLDOWN_FRAMES);
        assert_eq!(cooldown(0.5), 480);
        assert_eq!(cooldown(0.0), DASH_COOLDOWN_MIN_FRAMES);
        assert_eq!(cooldown(100.0), BULWARK_COOLDOWN_MAX_FRAMES);
    }

    #[test]
    fn bulwark_windup_is_immobile() {
        let w = flat_world();
        let mut s = spawn_state(Vec3::ZERO);
        s.class = CLASS_BULWARK;
        step_with(&mut s, &run_x(0, 0), &w, &Tuning::default());
        assert!(s.vel.x > 0.0);
        step_with(&mut s, &run_x(1, BTN_ABILITY), &w, &Tuning::default());
        for i in 0..(BULWARK_WINDUP_FRAMES - 1) {
            step_with(&mut s, &run_x(2 + u32::from(i), 0), &w, &Tuning::default());
            assert_eq!(s.vel.x, 0.0, "the wind-up holds horizontal speed at zero");
        }
    }

    #[test]
    fn bulwark_rooted_mid_run() {
        let w = flat_world();
        let mut s = spawn_state(Vec3::ZERO);
        s.class = CLASS_BULWARK;
        let mut seq = 0u32;
        for i in 0..4 {
            step_with(&mut s, &run_x(seq + i, 0), &w, &Tuning::default());
        }
        assert!(s.vel.x > 0.0);
        seq += 4;
        // Cast mid-run.
        step_with(&mut s, &run_x(seq, BTN_ABILITY), &w, &Tuning::default());
        seq += 1;
        assert_eq!(s.vel.x, 0.0, "the cast roots the player immediately");
        // Wind up, then reach the ward.
        for _ in 0..(BULWARK_WINDUP_FRAMES - 1) {
            step_with(&mut s, &run_x(seq, 0), &w, &Tuning::default());
            seq += 1;
        }
        assert_eq!(s.ward_frames, BULWARK_WARD_FRAMES);
        let frozen_x = s.pos.x;
        let frozen_y = s.pos.y;
        // Mid-ward: rooted, BTN_JUMP and BTN_ABILITY are ignored.
        for i in 0..20u32 {
            let buttons = match i {
                3 => BTN_JUMP,
                6 => BTN_ABILITY,
                _ => 0,
            };
            step_with(&mut s, &run_x(seq + i, buttons), &w, &Tuning::default());
            assert_eq!(s.vel.x, 0.0);
            assert_eq!(s.pos.y, frozen_y, "the ward hovers in place");
        }
        assert_eq!(s.pos.x, frozen_x, "the rooted player does not advance");
        assert!(s.vel.y == 0.0);
    }

    #[test]
    fn bulwark_pulse_frame_only_on_first_ward_frame() {
        let w = flat_world();
        let mut s = spawn_state(Vec3::ZERO);
        s.class = CLASS_BULWARK;
        step_with(&mut s, &idle(0), &w, &Tuning::default());
        assert!(!bulwark_pulse_frame(&s));
        step_with(&mut s, &run_x(1, BTN_ABILITY), &w, &Tuning::default());
        assert!(!bulwark_pulse_frame(&s));
        let mut seq = 2u32;
        // Wind-up is the cast frame plus (BULWARK_WINDUP_FRAMES - 1) frames;
        // the ward begins at the end of the last of those frames.
        for _ in 0..(BULWARK_WINDUP_FRAMES - 2) {
            step_with(&mut s, &run_x(seq, 0), &w, &Tuning::default());
            seq += 1;
            assert!(!bulwark_pulse_frame(&s), "the pulse must not fire during the wind-up");
        }
        // The last wind-up step ends with the ward beginning.
        step_with(&mut s, &run_x(seq, 0), &w, &Tuning::default());
        seq += 1;
        assert_eq!(s.ward_frames, BULWARK_WARD_FRAMES);
        assert!(bulwark_pulse_frame(&s), "the pulse fires on the frame the ward begins");
        step_with(&mut s, &run_x(seq, 0), &w, &Tuning::default());
        assert!(!bulwark_pulse_frame(&s));
        assert_eq!(s.ward_frames, BULWARK_WARD_FRAMES - 1);
    }

    #[test]
    fn bulwark_ward_lasts_exactly_96_frames() {
        let w = flat_world();
        let mut s = spawn_state(Vec3::ZERO);
        s.class = CLASS_BULWARK;
        let mut seq = 0u32;
        step_with(&mut s, &idle(seq), &w, &Tuning::default());
        seq += 1;
        step_with(&mut s, &run_x(seq, BTN_ABILITY), &w, &Tuning::default());
        seq += 1;
        while s.ward_frames == 0 {
            step_with(&mut s, &run_x(seq, 0), &w, &Tuning::default());
            seq += 1;
        }
        let mut rooted = 0u32;
        while s.ward_frames > 0 {
            step_with(&mut s, &run_x(seq, 0), &w, &Tuning::default());
            seq += 1;
            rooted += 1;
        }
        assert_eq!(rooted, 96, "the ward lasts exactly BULWARK_WARD_FRAMES frames");
        // The player can act again afterwards (subject to the ability cooldown).
        assert_eq!(s.ward_frames, 0);
        step_with(&mut s, &run_x(seq, 0), &w, &Tuning::default());
        assert_eq!(s.vel.x, RUN_SPEED);
    }

    // ---------- Jump pads ----------

    #[test]
    fn jump_pad_launches_on_landing() {
        let w = pad_world();
        // Spawn to the left of the pad and hold +X: the ~4 unit fall takes
        // about 30 frames, which is the time needed to reach the pad at run
        // speed. Spawning above the pad and running would leave the pad
        // (x > 2) before touchdown, so no landing occurs.
        let mut s = spawn_state(Vec3::new(-4.5, 4.0, 0.0));
        let mut seq = 0u32;
        let mut launched = false;
        for _ in 0..200 {
            step_with(&mut s, &run_x(seq, 0), &w, &Tuning::default());
            seq += 1;
            if s.vel.y == PAD_LAUNCH_VEL {
                launched = true;
                break;
            }
        }
        assert!(launched, "the pad must launch the player");
        assert_eq!(s.vel.x, RUN_SPEED * PAD_HORIZONTAL_VEL_MUL, "horizontal speed is multiplied");
        assert!(!s.grounded, "the launch leaves the ground");
    }

    #[test]
    fn jump_pad_apex_is_four_units() {
        let w = pad_world();
        // Hold BTN_JUMP through the arc (otherwise the variable-jump rule caps
        // any un-held ascent at JUMP_CUT_VELOCITY). Spawn to the left of the
        // pad and hold +X so the fall lands on it.
        let mut s = spawn_state(Vec3::new(-4.5, 4.0, 0.0));
        let mut seq = 0u32;
        while s.vel.y != PAD_LAUNCH_VEL {
            step_with(&mut s, &run_x_jump(seq), &w, &Tuning::default());
            seq += 1;
            assert!(seq < 200, "never launched");
        }
        let launch_y = s.pos.y;
        let mut max_y = s.pos.y;
        for i in 0..90u32 {
            step_with(&mut s, &run_x_jump(seq + i), &w, &Tuning::default());
            if s.pos.y > max_y {
                max_y = s.pos.y;
            }
        }
        let apex = max_y - launch_y;
        // v^2/2g = 4.0 exactly; the 60 Hz semi-implicit step lands about 0.13 lower.
        assert!((apex - 4.0).abs() <= 0.2, "apex {apex} must be ~4.0 units");
    }

    #[test]
    fn jump_pad_launches_once_per_contact() {
        let w = pad_world();
        let mut s = spawn_state(Vec3::new(0.0, 4.0, 0.0));
        let mut seq = 0u32;
        while s.vel.y != PAD_LAUNCH_VEL {
            step_with(&mut s, &idle(seq), &w, &Tuning::default());
            seq += 1;
            assert!(seq < 200, "never launched");
        }
        // One frame with BTN_JUMP held: gravity acts, the pad does not
        // re-trigger. Holding BTN_JUMP matters: an un-held ascent is capped at
        // JUMP_CUT_VELOCITY by the variable-jump rule, which would mask the
        // ballistic expectation (the apex test holds it for the same reason).
        let expected = PAD_LAUNCH_VEL - GRAVITY * DT;
        step_with(&mut s, &input(seq, 0, 0, BTN_JUMP), &w, &Tuning::default());
        assert!(
            (s.vel.y - expected).abs() < 1e-4,
            "the launch must not re-trigger while airborne, got {}",
            s.vel.y
        );
    }

    #[test]
    fn jump_pad_stays_solid() {
        let w = pad_world();
        assert_eq!(w.solids.len(), 1, "the pad is solid");
        let mut s = spawn_state(Vec3::ZERO);
        // A standing frame on the pad launches instead of sinking through it.
        s.vel.y = PAD_LAUNCH_VEL;
        let _ = s; // state sanity only
        let mut s = spawn_state(Vec3::new(0.0, 2.0, 0.0));
        for i in 0..120u32 {
            step_with(&mut s, &idle(i), &w, &Tuning::default());
        }
        assert!(s.pos.y >= -1.0, "the pad remains solid ground");
    }

    // ---------- Step-up ----------

    #[test]
    fn staircase_of_half_unit_risers_is_climbable_at_full_speed() {
        let w = staircase_world();
        let mut s = spawn_state(Vec3::new(-3.0, 0.0, 0.0));
        let mut seq = 0u32;
        for _ in 0..300 {
            step_with(&mut s, &run_x(seq, 0), &w, &Tuning::default());
            seq += 1;
            if s.pos.y == 2.5 && s.pos.x > 5.0 {
                // On the top plateau; stop before its far edge at x = 20.
                break;
            }
        }
        assert_eq!(s.pos.y, 2.5, "the staircase must be climbed without jumping");
        assert!(s.pos.x > 5.0, "must be on the top plateau, got x = {}", s.pos.x);
        assert!(s.grounded);
    }

    #[test]
    fn ledge_above_step_height_still_blocks() {
        let w = ledge_world(0.6, None);
        let mut s = spawn_state(Vec3::ZERO);
        let mut seq = 0u32;
        for _ in 0..150 {
            step_with(&mut s, &run_x(seq, 0), &w, &Tuning::default());
            seq += 1;
        }
        assert!(
            (s.pos.x - (4.0 - PLAYER_HALF_WIDTH)).abs() < 1e-4,
            "a 0.6 unit ledge must block, got x = {}",
            s.pos.x
        );
        assert_eq!(s.vel.x, 0.0);
        assert_eq!(s.pos.y, 0.0);
    }

    #[test]
    fn step_up_requires_room_to_fit() {
        // A 0.5 unit riser with a ceiling 2.3 units above the feet of someone
        // standing on the tread: the box does not fit, so no step-up.
        let w = ledge_world(0.5, Some((2.0, 4.0)));
        let mut s = spawn_state(Vec3::ZERO);
        let mut seq = 0u32;
        for _ in 0..150 {
            step_with(&mut s, &run_x(seq, 0), &w, &Tuning::default());
            seq += 1;
        }
        assert_eq!(s.pos.y, 0.0, "no step-up when the box cannot fit at the new height");
        assert!(
            (s.pos.x - (4.0 - PLAYER_HALF_WIDTH)).abs() < 1e-4,
            "the riser must block instead, got x = {}",
            s.pos.x
        );
    }

    #[test]
    fn step_up_keeps_horizontal_motion() {
        // Same riser without a ceiling: stepped onto it at speed.
        let w = ledge_world(0.5, None);
        let mut s = spawn_state(Vec3::ZERO);
        let mut seq = 0u32;
        let mut stepped = false;
        for _ in 0..120 {
            step_with(&mut s, &run_x(seq, 0), &w, &Tuning::default());
            seq += 1;
            if s.pos.y == 0.5 {
                assert!(s.vel.x > 0.0, "horizontal motion is kept through a step-up");
                stepped = true;
                break;
            }
        }
        assert!(stepped, "the 0.5 unit riser must be stepped onto");
    }

    // ---------- Stagger ----------

    #[test]
    fn stagger_shove_carries_and_ignores_backward_input() {
        let w = flat_world();
        let tuning = Tuning {
            friction_mul: 0.25, // low friction so the slide is visible
            ..Tuning::default()
        };
        let mut s = spawn_state(Vec3::ZERO);
        step_with(&mut s, &idle(0), &w, &tuning);
        assert!(s.grounded);
        s.vel = Vec3::new(18.0, 0.0, 0.0);
        s.stagger_frames = 30;
        let start_x = s.pos.x;
        for i in 0..10u32 {
            step_with(&mut s, &input(1 + i, -127, 0, 0), &w, &tuning);
        }
        // Each ground frame keeps 75% of the remaining error against a zero
        // target: slide = 18 * 0.75 * (1 - 0.75^10) / (1 - 0.75) * DT ~= 0.85.
        assert!(
            s.pos.x > start_x + 0.5,
            "the shove carries the player, got x = {}",
            s.pos.x
        );
        assert!(s.vel.x > 0.0, "backward input must not reverse a staggered shove");
    }

    #[test]
    fn stagger_ignores_jump_ability_dash_and_decrements() {
        let w = flat_world();
        let mut s = spawn_state(Vec3::ZERO);
        step(&mut s, &idle(0), &w);
        assert!(s.grounded);
        s.stagger_frames = 5;
        // Jump is swallowed.
        step(&mut s, &input(1, 0, 0, BTN_JUMP), &w);
        assert_eq!(s.vel.y, 0.0);
        assert_eq!(s.stagger_frames, 4);
        assert_eq!(s.jump_buffer_frames, 0, "no jump may be buffered during a stagger");
        // Ability (dash, for the default striker) is swallowed.
        step(&mut s, &input(2, 127, 0, BTN_ABILITY), &w);
        assert_eq!(s.ability_active_frames, 0, "no dash may start during a stagger");
        assert_eq!(s.ability_cooldown_frames, 0);
        assert_eq!(s.stagger_frames, 3);
    }

    #[test]
    fn stagger_keeps_air_velocity_and_applies_gravity() {
        let w = flat_world();
        let mut s = spawn_state(Vec3::new(0.0, 20.0, 0.0));
        s.vel = Vec3::new(12.0, 0.0, 0.0);
        s.stagger_frames = 10;
        step(&mut s, &run_x(0, 0), &w);
        assert_eq!(s.vel.x, 12.0, "air velocity is kept during a stagger");
        assert!(s.vel.y < 0.0, "gravity still applies");
    }

    // ---------- Fretted tiles ----------

    #[test]
    fn fretted_platform_slows_run_and_locks_ability() {
        let w = fretted_world();
        assert_eq!(w.tile_state(0), TILE_FRETTED);
        assert_eq!(w.solids.len(), 1, "TILE_FRETTED platforms stay solid");
        let mut s = spawn_state(Vec3::ZERO);
        step_with(&mut s, &idle(0), &w, &Tuning::default());
        assert!(s.grounded);
        step_with(&mut s, &run_x(1, 0), &w, &Tuning::default());
        assert_eq!(s.vel.x, RUN_SPEED * FRETTED_SPEED_MUL, "fretted ground caps run speed x0.6");
        // BTN_ABILITY is ignored on fretted ground (striker dash must not fire).
        step_with(&mut s, &run_x(2, BTN_ABILITY), &w, &Tuning::default());
        assert_eq!(s.ability_active_frames, 0);
        assert_eq!(s.ability_cooldown_frames, 0);
        // Jump still works.
        step_with(&mut s, &idle(3), &w, &Tuning::default());
        step_with(&mut s, &input(4, 127, 0, BTN_JUMP), &w, &Tuning::default());
        assert!(s.vel.y > 0.0, "jump is not blocked by fretted ground");
    }

    // ---------- Spatial grid equivalence ----------

    fn next_u64(state: &mut u64) -> u64 {
        let mut x = *state;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        *state = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    fn next_f32(state: &mut u64, low: f32, high: f32) -> f32 {
        let unit = ((next_u64(state) & 0xFF_FFFF) as f32) / ((1u64 << 24) as f32);
        low + (high - low) * unit
    }

    /// 500 pseudo-random platforms including one giant slab spanning many cells.
    fn grid_test_level() -> LevelData {
        let mut platforms = Vec::with_capacity(500);
        let mut rng = 0x9E37_79B9_7F4A_7C15u64;
        for i in 0..500u16 {
            let cx = next_f32(&mut rng, -200.0, 200.0);
            let cz = next_f32(&mut rng, -200.0, 200.0);
            let w = 2.0 + next_f32(&mut rng, 0.0, 10.0);
            let d = 2.0 + next_f32(&mut rng, 0.0, 10.0);
            let h = 0.5 + next_f32(&mut rng, 0.0, 2.5);
            let top = next_f32(&mut rng, -8.0, 8.0);
            platforms.push(Platform {
                id: i + 1,
                kind: PLATFORM_NORMAL,
                bounds: Aabb {
                    min: Vec3::new(cx, top - h, cz),
                    max: Vec3::new(cx + w, top, cz + d),
                },
            });
        }
        platforms[0] = Platform {
            id: 1,
            kind: PLATFORM_NORMAL,
            bounds: Aabb {
                min: Vec3::new(-300.0, -2.0, -300.0),
                max: Vec3::new(300.0, 0.0, 300.0),
            },
        };
        LevelData {
            level: 3,
            seed: 99,
            spawn: Vec3::ZERO,
            beacon: Vec3::new(0.0, 0.0, 30.0),
            kill_y: -100.0,
            platforms,
            gifts: vec![],
            event: EVENT_NONE,
            altars: vec![],
        }
    }

    #[test]
    fn grid_fast_path_matches_brute_force() {
        let mut w = World::from_level(&grid_test_level());
        w.set_tiles(&[
            TileState { id: 3, state: protocol::TILE_GONE },
            TileState {
                id: 9,
                state: TILE_FRETTED,
            },
        ]);
        let mut rng = 0x243F_6A88_85A3_08D3u64;
        for i in 0..200u32 {
            let cx = next_f32(&mut rng, -250.0, 250.0);
            let cz = next_f32(&mut rng, -250.0, 250.0);
            let half_w = 0.5 + next_f32(&mut rng, 0.0, 25.0);
            let half_d = 0.5 + next_f32(&mut rng, 0.0, 25.0);
            let y0 = next_f32(&mut rng, -12.0, 8.0);
            let y1 = y0 + 0.5 + next_f32(&mut rng, 0.0, 4.0);
            let b = Aabb {
                min: Vec3::new(cx - half_w, y0, cz - half_d),
                max: Vec3::new(cx + half_w, y1, cz + half_d),
            };
            assert_eq!(
                overlaps_any_solid(&w, &b),
                w.solids.iter().any(|s| overlaps(&b, s)),
                "grid and brute force disagree on box {b:?}"
            );
            // Resting-on-solid equivalence (half the cases pin the feet to a
            // random solid top so the check is non-trivial).
            let mut st = spawn_state(Vec3::ZERO);
            if i % 2 == 0 && !w.solids.is_empty() {
                let solid = &w.solids[(next_u64(&mut rng) as usize) % w.solids.len()];
                st.pos = Vec3::new(
                    (solid.min.x + solid.max.x) * 0.5,
                    solid.max.y,
                    (solid.min.z + solid.max.z) * 0.5,
                );
            } else {
                st.pos = Vec3::new(next_f32(&mut rng, -200.0, 200.0), next_f32(&mut rng, -12.0, 12.0), next_f32(&mut rng, -200.0, 200.0));
            }
            st.vel = Vec3::ZERO; // vel.y <= 0, as resting_on_solid requires
            let fast = resting_on_solid(&st, &w);
            let bb = player_box(&st.pos);
            let brute = w.solids.iter().any(|s| {
                let overlaps_xz =
                    bb.min.x < s.max.x && bb.max.x > s.min.x && bb.min.z < s.max.z && bb.max.z > s.min.z;
                overlaps_xz && (st.pos.y - s.max.y).abs() <= GROUND_EPSILON
            });
            assert_eq!(fast, brute, "grid and brute force disagree when resting at {st:?}");
        }
    }
}


// T-052 r3: the pad must launch with buttons unheld, so the variable-jump rule
// (which now only caps an actual jump release edge) must not clip the pad arc.
#[cfg(test)]
mod jump_pad_launch_tests {
    use super::*;
    use crate::tuning::PAD_LAUNCH_VEL;
    use protocol::{EVENT_NONE, LevelData, PLATFORM_JUMP_PAD, PLATFORM_NORMAL, Platform};

    fn platform(id: u16, kind: u8, min: [f32; 3], max: [f32; 3]) -> Platform {
        Platform {
            id,
            kind,
            bounds: Aabb {
                min: Vec3::new(min[0], min[1], min[2]),
                max: Vec3::new(max[0], max[1], max[2]),
            },
        }
    }

    /// The probe's world: a normal slab, then an adjacent 3x3 jump pad at the
    /// same top height (y = 0).
    fn probe_world() -> World {
        World::from_level(&LevelData {
            level: 1,
            seed: 1,
            spawn: Vec3::ZERO,
            beacon: Vec3::new(0.0, -50.0, 0.0),
            kill_y: -30.0,
            platforms: vec![
                platform(0, PLATFORM_NORMAL, [-10.0, -1.0, -2.0], [0.0, 0.0, 2.0]),
                platform(1, PLATFORM_JUMP_PAD, [0.0, -1.0, -1.5], [3.0, 0.0, 1.5]),
            ],
            gifts: vec![],
            event: EVENT_NONE,
            altars: vec![],
        })
    }

    fn idle(seq: u32) -> InputFrame {
        InputFrame {
            seq,
            move_x: 0,
            move_z: 0,
            buttons: 0,
        }
    }

    fn run_x(seq: u32) -> InputFrame {
        InputFrame {
            seq,
            move_x: 127,
            move_z: 0,
            buttons: 0,
        }
    }

    #[test]
    fn jump_pad_launches_apex_when_walked_onto() {
        let w = probe_world();
        let mut s = spawn_state(Vec3::new(-6.0, 0.0, 0.0));
        let mut max_y = 0.0f32;
        for i in 0..120 {
            step(&mut s, &run_x(i), &w);
            max_y = max_y.max(s.pos.y);
        }
        assert!(
            max_y >= 3.5,
            "walking onto the pad must launch the player at least 3.5 u above it, apex {max_y}"
        );
    }

    #[test]
    fn jump_pad_launches_apex_when_dropped_on() {
        let w = probe_world();
        let mut s = spawn_state(Vec3::new(1.5, 3.0, 0.0));
        let mut max_y = 0.0f32;
        let mut landed = false;
        for i in 0..120 {
            step(&mut s, &idle(i), &w);
            if s.grounded {
                landed = true;
            }
            if landed {
                max_y = max_y.max(s.pos.y);
            }
        }
        assert!(landed, "the player must land on the pad");
        assert!(
            max_y >= 3.5,
            "dropping onto the pad must launch the player at least 3.5 u above it, apex {max_y}"
        );
    }

    #[test]
    fn jump_pad_standing_still_does_not_relaunch_until_player_has_left() {
        // Drop onto the pad centre with buttons unheld (vel.x stays 0, so the
        // player falls back onto the same pad after each arc).
        let w = probe_world();
        let mut s = spawn_state(Vec3::new(1.5, 3.0, 0.0));
        let mut seq = 0u32;
        // First contact: the pad launches the player.
        while s.vel.y != PAD_LAUNCH_VEL {
            step(&mut s, &idle(seq), &w);
            seq += 1;
            assert!(seq < 300, "the pad never launched");
        }
        assert!(!s.grounded, "the launch leaves the pad");
        // The pad must not fire again while the player is in this same
        // contact. It may only fire on a later landing, i.e. after the player
        // has left (the full arc is ~60 frames).
        let mut relaunch_after = None;
        for i in 0..600u32 {
            step(&mut s, &idle(seq), &w);
            seq += 1;
            if s.vel.y == PAD_LAUNCH_VEL {
                relaunch_after = Some(i);
                break;
            }
        }
        let relaunch_after = relaunch_after.expect("the pad must re-arm on the next landing");
        assert!(
            relaunch_after >= 30,
            "the pad re-fired after {relaunch_after} frames; the player never left the pad"
        );
    }
}
