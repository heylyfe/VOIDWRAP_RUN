//! Static collision world built from a [`LevelData`].

use crate::tuning::GRID_CELL;
use protocol::{
    Aabb, LevelData, PLATFORM_RAMP_NX, PLATFORM_RAMP_NZ, PLATFORM_RAMP_PX, PLATFORM_RAMP_PZ, Platform, TILE_GONE,
    TileState,
};
use std::collections::BTreeMap;

/// True when `kind` is one of the four ramp platform kinds
/// (`PLATFORM_RAMP_PX`, `PLATFORM_RAMP_NX`, `PLATFORM_RAMP_PZ`,
/// `PLATFORM_RAMP_NZ`).
///
/// A ramp's `bounds.min.y` is the surface height at its low edge and
/// `bounds.max.y` the surface height at its high edge; the kind encodes which
/// horizontal axis the slope runs along and in which direction it rises. Every
/// other kind is flat and reports `bounds.max.y` as its surface everywhere.
pub fn is_ramp(kind: u8) -> bool {
    matches!(
        kind,
        PLATFORM_RAMP_PX | PLATFORM_RAMP_NX | PLATFORM_RAMP_PZ | PLATFORM_RAMP_NZ
    )
}

/// Surface height of `platform` at `(x, z)`, ignoring its x/z bounds.
///
/// Flat platforms report `bounds.max.y` everywhere; ramps interpolate linearly
/// between `bounds.min.y` at the low edge and `bounds.max.y` at the high edge.
pub fn surface_at(platform: &Platform, x: f32, z: f32) -> f32 {
    let b = &platform.bounds;
    let fraction = match platform.kind {
        PLATFORM_RAMP_PX => ramp_fraction(x, b.min.x, b.max.x),
        PLATFORM_RAMP_NX => ramp_fraction(x, b.max.x, b.min.x),
        PLATFORM_RAMP_PZ => ramp_fraction(z, b.min.z, b.max.z),
        PLATFORM_RAMP_NZ => ramp_fraction(z, b.max.z, b.min.z),
        _ => return b.max.y,
    };
    b.min.y + (b.max.y - b.min.y) * fraction
}

/// How far `v` sits from `low_edge` towards `high_edge`, clamped to
/// `0.0..=1.0`. A degenerate (zero-width) span reports `0.0`.
fn ramp_fraction(v: f32, low_edge: f32, high_edge: f32) -> f32 {
    let span = high_edge - low_edge;
    if span == 0.0 {
        return 0.0;
    }
    ((v - low_edge) / span).clamp(0.0, 1.0)
}

/// Collision geometry the controller moves through.
///
/// `solids` always holds the currently solid platforms only: platforms whose
/// snapshot tile state is `TILE_GONE` are excluded, everything else (including
/// `TILE_WARN`, `TILE_FRETTED` and platforms absent from the tile list) is solid.
///
/// A uniform spatial grid ([`GRID_CELL`] cells, XZ plane, integer cell
/// coordinates) maps cells to the platform indices whose AABBs overlap them.
/// Cells store indices in ascending platform order; candidate queries return
/// them sorted and deduplicated, so results match brute-force iteration over
/// platforms in platform order.
///
/// Ramp platforms (see [`is_ramp`]) are solid only below their sloped surface;
/// [`World::surface_top`] reports that surface. The grid and [`World::solids`]
/// still index the full AABB of every platform.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct World {
    pub solids: Vec<Aabb>,
    /// Anything whose feet fall below this height is lost to the void.
    pub kill_y: f32,
    /// Every platform from the level, unchanged by tile states.
    pub platforms: Vec<Platform>,
    /// Ids of platforms that are currently not solid (`TILE_GONE`), sorted ascending.
    pub gone: Vec<u16>,
    grid: SpatialGrid,
    /// Per-platform solidity, parallel to [`World::platforms`].
    solid: Vec<bool>,
    /// Per-platform tile states, parallel to [`World::platforms`] (0 = normal/absent).
    tile_states: Vec<u8>,
}

impl World {
    /// Build an all-solid world from level data.
    pub fn from_level(level: &LevelData) -> Self {
        let platforms = level.platforms.clone();
        let count = platforms.len();
        World {
            solids: platforms.iter().map(|p| p.bounds).collect(),
            kill_y: level.kill_y,
            gone: Vec::new(),
            grid: SpatialGrid::build(&platforms),
            solid: vec![true; count],
            tile_states: vec![0; count],
            platforms,
        }
    }

    /// Apply a snapshot's tile states.
    ///
    /// Every platform whose id appears with `state == TILE_GONE` stops being
    /// solid; every other platform (including `TILE_WARN`, `TILE_FRETTED` and
    /// any id absent from `tiles`) is solid again. When several tile states
    /// are listed for the same platform id, the first one in the list wins.
    /// Pass the full list of non-normal tiles, or an empty slice to restore
    /// everything. Note: a world that was not built from a level (empty
    /// [`World::platforms`]) rebuilds `solids` to empty.
    pub fn set_tiles(&mut self, tiles: &[TileState]) {
        let mut gone: Vec<u16> = Vec::new();
        for tile in tiles {
            if tile.state == TILE_GONE && !gone.contains(&tile.id) {
                gone.push(tile.id);
            }
        }
        gone.sort_unstable();
        self.gone = gone;

        self.tile_states.clear();
        self.solid.clear();
        for platform in &self.platforms {
            let mut state = 0u8;
            for tile in tiles {
                if tile.id == platform.id {
                    state = tile.state;
                    break;
                }
            }
            self.tile_states.push(state);
            self.solid.push(state != TILE_GONE);
        }
        self.solids = self
            .platforms
            .iter()
            .zip(&self.solid)
            .filter(|(_, solid)| **solid)
            .map(|(p, _)| p.bounds)
            .collect();
    }

    /// Tile state of the platform at `platform_index` (an index into
    /// [`World::platforms`]); absent ids and out-of-range indices report 0
    /// (normal). `TILE_FRETTED` platforms remain solid.
    pub fn tile_state(&self, platform_index: usize) -> u8 {
        self.tile_states.get(platform_index).copied().unwrap_or(0)
    }

    /// Solid platforms whose grid cells overlap the XZ box, as platform
    /// indices, sorted ascending and deduplicated.
    pub(crate) fn solid_candidates(&self, min_x: f32, min_z: f32, max_x: f32, max_z: f32) -> Vec<usize> {
        self.grid
            .candidates(min_x, min_z, max_x, max_z)
            .into_iter()
            .filter(|&index| self.solid.get(index).copied().unwrap_or(false))
            .collect()
    }

    /// Bounds of the platform at `index`, or `None` when it is not solid.
    pub(crate) fn solid_bounds(&self, index: usize) -> Option<&Aabb> {
        if self.solid.get(index).copied().unwrap_or(false) {
            self.platforms.get(index).map(|p| &p.bounds)
        } else {
            None
        }
    }

    /// Surface height of the platform at `index` at world point `(x, z)`, or
    /// `None` when the point lies outside the platform's x/z bounds (or the
    /// index is out of range).
    ///
    /// Flat platforms (every kind that is not a ramp, see [`is_ramp`]) report
    /// `bounds.max.y` everywhere. Ramps interpolate linearly between
    /// `bounds.min.y` at the low edge and `bounds.max.y` at the high edge along
    /// the axis and direction encoded in the platform kind.
    ///
    /// Solidity is not considered here; combine with [`World::solid_bounds`]
    /// when the answer must respect tile states.
    pub fn surface_top(&self, index: usize, x: f32, z: f32) -> Option<f32> {
        let platform = self.platforms.get(index)?;
        let b = &platform.bounds;
        if x < b.min.x || x > b.max.x || z < b.min.z || z > b.max.z {
            return None;
        }
        Some(surface_at(platform, x, z))
    }

    /// [`World::surface_top`] with `x`/`z` clamped into the platform's x/z
    /// bounds, so a point just outside the footprint still reports the height
    /// of the nearest edge. `None` only when `index` is out of range.
    ///
    /// The controller uses this because a player box can overlap a platform
    /// while the player's centre sits just outside it.
    pub(crate) fn surface_top_clamped(&self, index: usize, x: f32, z: f32) -> Option<f32> {
        let platform = self.platforms.get(index)?;
        let b = &platform.bounds;
        Some(surface_at(
            platform,
            x.clamp(b.min.x, b.max.x),
            z.clamp(b.min.z, b.max.z),
        ))
    }

    /// True when the platform at `index` is a ramp (see [`is_ramp`]).
    pub(crate) fn is_ramp_at(&self, index: usize) -> bool {
        self.platforms.get(index).is_some_and(|p| is_ramp(p.kind))
    }
}

/// Uniform XZ grid over platform indices. Platforms are inserted in ascending
/// index order, so each cell's index list is already sorted.
#[derive(Debug, Clone, Default, PartialEq)]
struct SpatialGrid {
    cells: BTreeMap<(i32, i32), Vec<usize>>,
}

impl SpatialGrid {
    /// Insert every platform into the cells its AABB overlaps.
    fn build(platforms: &[Platform]) -> Self {
        let mut cells: BTreeMap<(i32, i32), Vec<usize>> = BTreeMap::new();
        for (index, platform) in platforms.iter().enumerate() {
            let (x0, z0) = cell_of(platform.bounds.min.x, platform.bounds.min.z);
            let (x1, z1) = cell_of(platform.bounds.max.x, platform.bounds.max.z);
            for cx in x0..=x1 {
                for cz in z0..=z1 {
                    cells.entry((cx, cz)).or_default().push(index);
                }
            }
        }
        SpatialGrid { cells }
    }

    /// Sorted, deduplicated platform indices of every cell the XZ box touches.
    fn candidates(&self, min_x: f32, min_z: f32, max_x: f32, max_z: f32) -> Vec<usize> {
        let (x0, z0) = cell_of(min_x, min_z);
        let (x1, z1) = cell_of(max_x, max_z);
        let mut out = Vec::new();
        for cx in x0..=x1 {
            for cz in z0..=z1 {
                if let Some(ids) = self.cells.get(&(cx, cz)) {
                    out.extend_from_slice(ids);
                }
            }
        }
        out.sort_unstable();
        out.dedup();
        out
    }
}

/// Grid cell containing the point `x`/`z` (floor of position / cell size).
fn cell_of(x: f32, z: f32) -> (i32, i32) {
    ((x / GRID_CELL).floor() as i32, (z / GRID_CELL).floor() as i32)
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
    use protocol::{
        EVENT_NONE, InputFrame, PLATFORM_CRUMBLE, PLATFORM_JUMP_PAD, PLATFORM_NORMAL, TILE_FRETTED, TILE_WARN, Vec3,
    };

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

    /// Flat slab at y = 0 followed by a `RAMP_PZ` rising from y = 0 at z = 0 to
    /// y = 6 at z = 15.
    fn ramp_level() -> LevelData {
        LevelData {
            level: 2,
            seed: 2,
            spawn: Vec3::ZERO,
            beacon: Vec3::new(0.0, 6.0, 15.0),
            kill_y: -50.0,
            platforms: vec![
                Platform {
                    id: 1,
                    kind: PLATFORM_NORMAL,
                    bounds: Aabb {
                        min: Vec3::new(-2.0, -1.0, -6.0),
                        max: Vec3::new(2.0, 0.0, 0.0),
                    },
                },
                Platform {
                    id: 2,
                    kind: PLATFORM_RAMP_PZ,
                    bounds: Aabb {
                        min: Vec3::new(-2.0, 0.0, 0.0),
                        max: Vec3::new(2.0, 6.0, 15.0),
                    },
                },
            ],
            gifts: vec![],
            event: EVENT_NONE,
            altars: vec![],
        }
    }

    /// A level holding exactly one ramp platform with bounds
    /// `(0, 1, 0)..(10, 5, 10)`.
    fn ramp_only_level(kind: u8) -> LevelData {
        LevelData {
            level: 1,
            seed: 1,
            spawn: Vec3::ZERO,
            beacon: Vec3::ZERO,
            kill_y: -50.0,
            platforms: vec![Platform {
                id: 1,
                kind,
                bounds: Aabb {
                    min: Vec3::new(0.0, 1.0, 0.0),
                    max: Vec3::new(10.0, 5.0, 10.0),
                },
            }],
            gifts: vec![],
            event: EVENT_NONE,
            altars: vec![],
        }
    }

    fn assert_close(actual: Option<f32>, expected: f32) {
        match actual {
            Some(value) => assert!((value - expected).abs() <= 1e-5, "expected {expected}, got {value}"),
            None => panic!("expected {expected}, got None"),
        }
    }

    fn next_u64(state: &mut u64) -> u64 {
        // xorshift64*; deterministic, used only to build test geometry.
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

    /// 500 platforms with deterministic pseudo-random placement, including one
    /// giant slab spanning many cells.
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

    /// Brute-force reference: solid platforms whose cell ranges intersect the
    /// query's cell range, in platform index order.
    fn brute_force_cells(world: &World, min_x: f32, min_z: f32, max_x: f32, max_z: f32) -> Vec<usize> {
        let cell = |v: f32| (v / GRID_CELL).floor() as i32;
        let (x0, z0, x1, z1) = (cell(min_x), cell(min_z), cell(max_x), cell(max_z));
        let mut out = Vec::new();
        for (i, platform) in world.platforms.iter().enumerate() {
            if !world.solid[i] {
                continue;
            }
            let (px0, pz0) = cell_of(platform.bounds.min.x, platform.bounds.min.z);
            let (px1, pz1) = cell_of(platform.bounds.max.x, platform.bounds.max.z);
            if px1 < x0 || px0 > x1 || pz1 < z0 || pz0 > z1 {
                continue;
            }
            out.push(i);
        }
        out
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

    #[test]
    fn grid_candidates_match_brute_force() {
        let level = grid_test_level();
        let mut w = World::from_level(&level);
        w.set_tiles(&[
            TileState {
                id: 3,
                state: TILE_GONE,
            },
            TileState {
                id: 9,
                state: TILE_FRETTED,
            },
        ]);
        let mut rng = 0x243F_6A88_85A3_08D3u64;
        for _ in 0..300 {
            let cx = next_f32(&mut rng, -250.0, 250.0);
            let cz = next_f32(&mut rng, -250.0, 250.0);
            let half_w = 0.5 + next_f32(&mut rng, 0.0, 25.0);
            let half_d = 0.5 + next_f32(&mut rng, 0.0, 25.0);
            let (x0, z0, x1, z1) = (cx - half_w, cz - half_d, cx + half_w, cz + half_d);
            assert_eq!(
                w.solid_candidates(x0, z0, x1, z1),
                brute_force_cells(&w, x0, z0, x1, z1),
                "grid and brute force disagree on box ({x0}, {z0})..({x1}, {z1})"
            );
        }
    }

    #[test]
    fn tile_state_query_reports_all_cases() {
        let level = test_level();
        let mut w = World::from_level(&level);
        assert_eq!(w.tile_state(0), 0, "absent from the list means normal");
        assert_eq!(w.tile_state(1), 0);
        assert_eq!(w.tile_state(999), 0, "out-of-range index reports normal");

        w.set_tiles(&[TileState {
            id: 7,
            state: TILE_FRETTED,
        }]);
        assert_eq!(w.tile_state(0), TILE_FRETTED);
        assert_eq!(w.solids.len(), 2, "TILE_FRETTED platforms stay solid");

        w.set_tiles(&[TileState {
            id: 7,
            state: TILE_GONE,
        }]);
        assert_eq!(w.tile_state(0), TILE_GONE);

        w.set_tiles(&[TileState {
            id: 7,
            state: TILE_WARN,
        }]);
        assert_eq!(w.tile_state(0), TILE_WARN);

        w.set_tiles(&[]);
        assert_eq!(w.tile_state(0), 0, "empty tile list restores normal");
    }

    #[test]
    fn is_ramp_covers_exactly_the_four_ramp_kinds() {
        assert!(is_ramp(PLATFORM_RAMP_PX));
        assert!(is_ramp(PLATFORM_RAMP_NX));
        assert!(is_ramp(PLATFORM_RAMP_PZ));
        assert!(is_ramp(PLATFORM_RAMP_NZ));
        assert!(!is_ramp(PLATFORM_NORMAL));
        assert!(!is_ramp(PLATFORM_CRUMBLE));
        assert!(!is_ramp(PLATFORM_JUMP_PAD));
        assert!(!is_ramp(7), "unknown kinds are not ramps");
    }

    #[test]
    fn surface_top_reports_edges_and_rejects_outside_points() {
        let w = World::from_level(&ramp_level());
        // Flat platform: `max.y` everywhere inside the footprint.
        assert_close(w.surface_top(0, 0.0, -3.0), 0.0);
        assert_close(w.surface_top(0, -2.0, -6.0), 0.0);
        assert_close(w.surface_top(0, 2.0, 0.0), 0.0);
        // RAMP_PZ: low at min.z, high at max.z.
        assert_close(w.surface_top(1, 0.0, 0.0), 0.0);
        assert_close(w.surface_top(1, 0.0, 15.0), 6.0);
        assert_close(w.surface_top(1, 0.0, 7.5), 3.0);
        assert_close(w.surface_top(1, 0.0, 3.0), 1.2);
        // Outside the x/z bounds.
        assert_eq!(w.surface_top(1, 0.0, -0.001), None);
        assert_eq!(w.surface_top(1, 0.0, 15.001), None);
        assert_eq!(w.surface_top(1, -2.001, 7.5), None);
        assert_eq!(w.surface_top(1, 2.001, 7.5), None);
        assert_eq!(w.surface_top(99, 0.0, 0.0), None, "out-of-range index");
    }

    #[test]
    fn surface_top_follows_each_ramp_direction() {
        let px = World::from_level(&ramp_only_level(PLATFORM_RAMP_PX));
        assert_close(px.surface_top(0, 0.0, 5.0), 1.0);
        assert_close(px.surface_top(0, 10.0, 5.0), 5.0);
        assert_close(px.surface_top(0, 5.0, 5.0), 3.0);

        let nx = World::from_level(&ramp_only_level(PLATFORM_RAMP_NX));
        assert_close(nx.surface_top(0, 0.0, 5.0), 5.0);
        assert_close(nx.surface_top(0, 10.0, 5.0), 1.0);
        assert_close(nx.surface_top(0, 5.0, 5.0), 3.0);

        let pz = World::from_level(&ramp_only_level(PLATFORM_RAMP_PZ));
        assert_close(pz.surface_top(0, 5.0, 0.0), 1.0);
        assert_close(pz.surface_top(0, 5.0, 10.0), 5.0);
        assert_close(pz.surface_top(0, 5.0, 5.0), 3.0);

        let nz = World::from_level(&ramp_only_level(PLATFORM_RAMP_NZ));
        assert_close(nz.surface_top(0, 5.0, 0.0), 5.0);
        assert_close(nz.surface_top(0, 5.0, 10.0), 1.0);
        assert_close(nz.surface_top(0, 5.0, 5.0), 3.0);
    }
}
