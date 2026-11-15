//! VOIDWRAP RUN deterministic simulation core.
//!
//! This crate is compiled twice: natively into the authoritative server, and to
//! `wasm32-unknown-unknown` for client-side prediction. Everything here must be
//! deterministic across both targets: no wall-clock time, no randomness without an
//! explicit seed, no transcendental float functions (`sin`, `cos`, `powf`, ...)
//! inside the step path — only `+ - * / sqrt`, which IEEE-754 makes bit-exact.

pub mod controller;
pub mod greater;
pub mod hazards;
pub mod levelgen;
pub mod levels;
pub mod mode;
pub mod pattern;
pub mod rules;
pub mod tuning;
pub mod world;

pub use protocol::{Aabb, ControllerState, InputFrame, LevelData, Vec3};
