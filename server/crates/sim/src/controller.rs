//! Kinematic character controller.
//!
//! M0 bootstrap version: instant run speed, gravity, edge-triggered ground jump,
//! axis-separated AABB collision. Ticket T-010 replaces this with the full M1
//! controller (coyote time, jump buffering, class ability). The public signature
//! [`step`] is the contract shared by server and WASM client and must not change
//! without updating both.

use crate::tuning::*;
use crate::world::{World, overlaps};
use protocol::{Aabb, BTN_JUMP, ControllerState, InputFrame, Vec3};

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

/// Advance one fixed physics frame of [`DT`] seconds.
pub fn step(state: &mut ControllerState, input: &InputFrame, world: &World) {
    let (wx, wz) = wish_dir(input);
    state.vel.x = wx * RUN_SPEED;
    state.vel.z = wz * RUN_SPEED;

    let jump_pressed = input.buttons & BTN_JUMP != 0 && state.prev_buttons & BTN_JUMP == 0;
    if jump_pressed && state.grounded {
        state.vel.y = JUMP_VELOCITY;
        state.grounded = false;
    }
    state.vel.y = (state.vel.y - GRAVITY * DT).max(-MAX_FALL_SPEED);
    state.prev_buttons = input.buttons;

    let landed = move_axis(state, world, Axis::Y, state.vel.y * DT);
    state.grounded = landed;
    move_axis(state, world, Axis::X, state.vel.x * DT);
    move_axis(state, world, Axis::Z, state.vel.z * DT);
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
fn move_axis(state: &mut ControllerState, world: &World, axis: Axis, delta: f32) -> bool {
    if delta == 0.0 {
        return false;
    }
    let mut pos = state.pos.clone();
    let target = get(&pos, axis) + delta;
    set(&mut pos, axis, target);
    let mut hit = false;
    for solid in &world.solids {
        let b = player_box(&pos);
        if !overlaps(&b, solid) {
            continue;
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

    fn idle(seq: u32) -> InputFrame {
        InputFrame {
            seq,
            move_x: 0,
            move_z: 0,
            buttons: 0,
        }
    }

    fn world() -> World {
        World::from_level(&levels::bootstrap())
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
    fn jump_is_edge_triggered() {
        let w = world();
        let mut s = spawn_state(Vec3::ZERO);
        step(&mut s, &idle(0), &w);
        assert!(s.grounded);
        let jump = InputFrame {
            seq: 1,
            move_x: 0,
            move_z: 0,
            buttons: BTN_JUMP,
        };
        step(&mut s, &jump, &w);
        assert!(s.pos.y > 0.0 && !s.grounded);
        // Holding jump after landing must not re-jump.
        for i in 0..200 {
            step(
                &mut s,
                &InputFrame {
                    seq: 2 + i,
                    ..jump.clone()
                },
                &w,
            );
        }
        assert!(s.grounded);
    }

    #[test]
    fn walks_off_edge_into_void() {
        let w = world();
        let mut s = spawn_state(Vec3::ZERO);
        let run = |seq| InputFrame {
            seq,
            move_x: 127,
            move_z: 0,
            buttons: 0,
        };
        for i in 0..400 {
            step(&mut s, &run(i), &w);
        }
        assert!(s.pos.y < w.kill_y);
    }

    #[test]
    fn diagonal_input_is_normalised() {
        let (x, z) = wish_dir(&InputFrame {
            seq: 0,
            move_x: 127,
            move_z: -128,
            buttons: 0,
        });
        assert!(((x * x + z * z).sqrt() - 1.0).abs() < 1e-6);
    }
}
