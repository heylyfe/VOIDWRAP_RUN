//! WASM bridge for client-side prediction. Runs the exact same [`sim::controller::step`]
//! as the server, so prediction and authority never drift.
//!
//! Data crosses the boundary only as protocol-encoded bytes (bincode), written by the
//! generated TypeScript codec, through one shared I/O buffer:
//!
//! 1. JS calls `vw_io_buf(len)` → pointer to a `len`-byte buffer in WASM memory.
//! 2. JS writes encoded bytes there, then calls `vw_load_level(len)` / `vw_set_state(len)`
//!    / `vw_set_tuning(len)`.
//! 3. `vw_step(...)` advances the predicted controller one physics frame.
//! 4. `vw_get_state()` encodes the state into the I/O buffer and returns its length;
//!    JS reads it from the pointer returned by `vw_io_ptr()`.
//!
//! `#[unsafe(no_mangle)]` is the only unsafe-flagged construct; there are no `unsafe` blocks.
#![allow(unsafe_code)]

use protocol::{ControllerState, InputFrame, LevelData, Tuning, Vec3};
use sim::controller;
use sim::world::World;
use std::cell::RefCell;

/// Upper bound on bytes JS may ask us to hold (level data is the largest message).
const MAX_IO_BYTES: u32 = 1 << 20;

struct Bridge {
    io: Vec<u8>,
    world: World,
    state: ControllerState,
    tuning: Tuning,
}

thread_local! {
    static BRIDGE: RefCell<Bridge> = RefCell::new(Bridge {
        io: Vec::new(),
        world: World::default(),
        state: controller::spawn_state(Vec3::ZERO),
        tuning: Tuning::default(),
    });
}

/// Resize the I/O buffer to `len` bytes and return its address (null if too large).
#[unsafe(no_mangle)]
pub extern "C" fn vw_io_buf(len: u32) -> *mut u8 {
    if len > MAX_IO_BYTES {
        return std::ptr::null_mut();
    }
    BRIDGE.with_borrow_mut(|b| {
        b.io.clear();
        b.io.resize(len as usize, 0);
        b.io.as_mut_ptr()
    })
}

/// Address of the I/O buffer (valid until the next `vw_io_buf` / `vw_get_state`).
#[unsafe(no_mangle)]
pub extern "C" fn vw_io_ptr() -> *const u8 {
    BRIDGE.with_borrow(|b| b.io.as_ptr())
}

fn io_bytes(b: &Bridge, len: u32) -> Option<&[u8]> {
    b.io.get(..len as usize)
}

/// Load collision geometry from an encoded `LevelData`. Returns 0 on success, -1 on error.
#[unsafe(no_mangle)]
pub extern "C" fn vw_load_level(len: u32) -> i32 {
    BRIDGE.with_borrow_mut(
        |b| match io_bytes(b, len).and_then(|x| protocol::decode::<LevelData>(x).ok()) {
            Some(level) => {
                b.world = World::from_level(&level);
                0
            }
            None => -1,
        },
    )
}

/// Replace the predicted state with an encoded `ControllerState`. Returns 0 / -1.
#[unsafe(no_mangle)]
pub extern "C" fn vw_set_state(len: u32) -> i32 {
    BRIDGE.with_borrow_mut(
        |b| match io_bytes(b, len).and_then(|x| protocol::decode::<ControllerState>(x).ok()) {
            Some(state) => {
                b.state = state;
                0
            }
            None => -1,
        },
    )
}

/// Replace the movement tuning with an encoded `Tuning`. Returns 0 / -1.
///
/// Decoded exactly like `vw_set_state`; the stored tuning is used by every
/// following `vw_step` until it is replaced.
#[unsafe(no_mangle)]
pub extern "C" fn vw_set_tuning(len: u32) -> i32 {
    BRIDGE.with_borrow_mut(
        |b| match io_bytes(b, len).and_then(|x| protocol::decode::<Tuning>(x).ok()) {
            Some(tuning) => {
                b.tuning = tuning;
                0
            }
            None => -1,
        },
    )
}

/// Advance the predicted state one physics frame. Out-of-range inputs are clamped
/// exactly as the server would see them after i8/u8 decoding.
#[unsafe(no_mangle)]
pub extern "C" fn vw_step(move_x: i32, move_z: i32, buttons: i32) {
    let input = InputFrame {
        seq: 0,
        move_x: move_x.clamp(-128, 127) as i8,
        move_z: move_z.clamp(-128, 127) as i8,
        buttons: buttons.clamp(0, 255) as u8,
    };
    BRIDGE.with_borrow_mut(|b| {
        let Bridge {
            world, state, tuning, ..
        } = b;
        controller::step_with(state, &input, world, tuning);
    });
}

/// Encode the predicted state into the I/O buffer; returns its byte length.
#[unsafe(no_mangle)]
pub extern "C" fn vw_get_state() -> u32 {
    BRIDGE.with_borrow_mut(|b| {
        b.io = protocol::encode(&b.state).unwrap_or_default();
        b.io.len() as u32
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_io(bytes: &[u8]) -> u32 {
        vw_io_buf(bytes.len() as u32);
        BRIDGE.with_borrow_mut(|b| b.io.copy_from_slice(bytes));
        bytes.len() as u32
    }

    #[test]
    fn bridge_matches_native_controller() {
        let level = sim::levels::bootstrap();
        let len = write_io(&protocol::encode(&level).unwrap());
        assert_eq!(vw_load_level(len), 0);

        // Non-default tuning: the bridge must apply exactly what the native step does.
        let tuning = Tuning {
            run_speed_mul: 1.2,
            jump_mul: 1.1,
            gravity_mul: 1.0,
            dash_speed_mul: 1.0,
            dash_cooldown_mul: 1.0,
            air_jumps: 1,
        };
        let len = write_io(&protocol::encode(&tuning).unwrap());
        assert_eq!(vw_set_tuning(len), 0);

        let start = controller::spawn_state(Vec3::new(0.0, 2.0, 0.0));
        let len = write_io(&protocol::encode(&start).unwrap());
        assert_eq!(vw_set_state(len), 0);

        let world = World::from_level(&level);
        let mut native = start;
        for i in 0..90u8 {
            let (mx, mz, btn) = (i32::from(i % 7) * 30 - 90, 127, i32::from(i.is_multiple_of(20)));
            vw_step(mx, mz, btn);
            let input = InputFrame {
                seq: 0,
                move_x: mx as i8,
                move_z: mz as i8,
                buttons: btn as u8,
            };
            controller::step_with(&mut native, &input, &world, &tuning);
        }
        let len = vw_get_state() as usize;
        let bytes = BRIDGE.with_borrow(|b| b.io[..len].to_vec());
        assert_eq!(protocol::decode::<ControllerState>(&bytes).unwrap(), native);
    }

    #[test]
    fn rejects_garbage_and_oversize() {
        assert!(vw_io_buf(MAX_IO_BYTES + 1).is_null());
        let len = write_io(&[1, 2, 3]);
        assert_eq!(vw_set_state(len), -1);
        assert_eq!(vw_set_tuning(len), -1);
        assert_eq!(vw_load_level(len + 100), -1);
    }
}
