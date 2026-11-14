//! Static collision world built from a [`LevelData`].

use protocol::{Aabb, LevelData, Platform, TILE_GONE, TileState};

/// Collision geometry the controller moves through.
///
/// `solids` always holds the currently solid platforms only: platforms whose
/// snapshot tile state is `TILE_GONE` are excluded, everything else (including
/// `TILE_WARN` and platforms absent from the tile list) is solid.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct World {
    pub solids: Vec<Aabb>,
    /// Anything whose feet fall below this height is lost to the void.
    pub kill_y: f32,
    /// Every platform from the level, unchanged by tile states.
    pub platforms: Vec<Platform>,
    /// Ids of platforms that are currently not solid (`TILE_GONE`), sorted ascending.
    pub gone: Vec<u16>,
}

impl World {
    /// Build an all-solid world from level data.
    pub fn from_level(level: &LevelData) -> Self {
        World {
            solids: level.platforms.iter().map(|p| p.bounds.clone()).collect(),
            kill_y: level.kill_y,
            platforms: level.platforms.clone(),
            gone: Vec::new(),
        }
    }

    /// Apply a snapshot's tile states.
    ///
    /// Every platform whose id appears with `state == TILE_GONE` stops being
    /// solid; every other platform (including `TILE_WARN` and any id absent
    /// from `tiles`) is solid again. Pass the full list of non-normal tiles,
    /// or an empty slice to restore everything. Note: a world that was not
    /// built from a level (empty [`World::platforms`]) rebuilds `solids` to
    /// empty.
    pub fn set_tiles(&mut self, tiles: &[TileState]) {
        let mut gone: Vec<u16> = Vec::new();
        for tile in tiles {
            if tile.state == TILE_GONE && !gone.contains(&tile.id) {
                gone.push(tile.id);
            }
        }
        gone.sort_unstable();
        self.solids = self
            .platforms
            .iter()
            .filter(|p| !gone.contains(&p.id))
            .map(|p| p.bounds.clone())
            .collect();
        self.gone = gone;
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::controller::{spawn_state, step};
    use protocol::{EVENT_NONE, InputFrame, PLATFORM_NORMAL, TILE_WARN, Vec3};

    fn idle(seq: u32) -> InputFrame {
        InputFrame {
            seq,
            move_x: 0,
            move_z: 0,
            buttons: 0,
        }
    }

    fn test_level() -> LevelData {
        LevelData {
            level: 1,
            seed: 1,
            spawn: Vec3::ZERO,
            beacon: Vec3::new(6.0, 0.0, 0.0),
            kill_y: -50.0,
            platforms: vec![
                Platform {
                    id: 7,
                    kind: PLATFORM_NORMAL,
                    bounds: Aabb {
                        min: Vec3::new(-2.0, -1.0, -2.0),
                        max: Vec3::new(2.0, 0.0, 2.0),
                    },
                },
                Platform {
                    id: 9,
                    kind: PLATFORM_NORMAL,
                    bounds: Aabb {
                        min: Vec3::new(4.0, -1.0, -2.0),
                        max: Vec3::new(8.0, 0.0, 2.0),
                    },
                },
            ],
            gifts: vec![],
            event: EVENT_NONE,
            altars: vec![],
        }
    }

    #[test]
    fn warn_tile_keeps_platform_solid() {
        let mut w = World::from_level(&test_level());
        w.set_tiles(&[TileState {
            id: 7,
            state: TILE_WARN,
        }]);
        assert_eq!(w.solids.len(), 2, "TILE_WARN must remain solid");
        let mut s = spawn_state(Vec3::new(0.0, 0.0, 0.0));
        step(&mut s, &idle(0), &w);
        assert!(s.grounded);
    }

    #[test]
    fn gone_tile_removes_platform_and_standing_player_falls() {
        let mut w = World::from_level(&test_level());
        let mut s = spawn_state(Vec3::new(0.0, 0.0, 0.0));
        step(&mut s, &idle(0), &w);
        assert!(s.grounded);
        w.set_tiles(&[TileState {
            id: 7,
            state: TILE_GONE,
        }]);
        assert_eq!(w.solids.len(), 1, "TILE_GONE platform must no longer be solid");
        for i in 0..120 {
            step(&mut s, &idle(1 + i), &w);
        }
        assert!(!s.grounded, "player on a gone platform must not be grounded");
        assert!(
            s.pos.y < 0.0,
            "player on a gone platform must fall, got y = {}",
            s.pos.y
        );
    }

    #[test]
    fn restoring_tile_makes_platform_solid_again() {
        let mut w = World::from_level(&test_level());
        w.set_tiles(&[TileState {
            id: 7,
            state: TILE_GONE,
        }]);
        assert_eq!(w.solids.len(), 1);

        // A TILE_WARN state (or absence from the list) makes it solid again.
        w.set_tiles(&[TileState {
            id: 7,
            state: TILE_WARN,
        }]);
        assert_eq!(w.solids.len(), 2);
        w.set_tiles(&[]);
        assert_eq!(w.solids.len(), 2, "an empty tile list restores everything");

        let mut s = spawn_state(Vec3::new(0.0, 0.0, 0.0));
        step(&mut s, &idle(0), &w);
        assert!(s.grounded, "restored platform must be solid again");
    }
}
