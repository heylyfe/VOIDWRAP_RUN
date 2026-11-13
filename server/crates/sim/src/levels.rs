//! Hand-authored levels.

use protocol::{Aabb, LevelData, Platform, Vec3};

fn slab(id: u16, cx: f32, top: f32, cz: f32, half_x: f32, half_z: f32) -> Platform {
    Platform {
        id,
        kind: 0,
        bounds: Aabb {
            min: Vec3::new(cx - half_x, top - 1.0, cz - half_z),
            max: Vec3::new(cx + half_x, top, cz + half_z),
        },
    }
}

/// M0 bootstrap level: one grey 40×40 plane floating in the void.
pub fn bootstrap() -> LevelData {
    LevelData {
        level: 0,
        seed: 0,
        spawn: Vec3::new(0.0, 0.0, 0.0),
        beacon: Vec3::new(0.0, 0.0, -15.0),
        kill_y: -30.0,
        platforms: vec![slab(0, 0.0, 0.0, 0.0, 20.0, 20.0)],
        gifts: vec![],
    }
}
