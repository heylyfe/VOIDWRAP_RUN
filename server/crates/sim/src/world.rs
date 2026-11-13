//! Static collision world built from a [`LevelData`].

use protocol::{Aabb, LevelData};

/// Collision geometry the controller moves through.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct World {
    pub solids: Vec<Aabb>,
    /// Anything whose feet fall below this height is lost to the void.
    pub kill_y: f32,
}

impl World {
    pub fn from_level(level: &LevelData) -> Self {
        World {
            solids: level.platforms.iter().map(|p| p.bounds.clone()).collect(),
            kill_y: level.kill_y,
        }
    }
}

/// Strict overlap test (touching faces do not overlap).
pub fn overlaps(a: &Aabb, b: &Aabb) -> bool {
    a.min.x < b.max.x
        && a.max.x > b.min.x
        && a.min.y < b.max.y
        && a.max.y > b.min.y
        && a.min.z < b.max.z
        && a.max.z > b.min.z
}
