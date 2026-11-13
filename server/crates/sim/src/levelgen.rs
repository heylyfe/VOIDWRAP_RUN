//! Procedural level generator — "roads in the void".
//!
//! Grows a tree of narrow walkways and junction pads out of the spawn
//! platform, sprinkles gifts, places the beacon on the junction/dead-end
//! with the greatest path distance from spawn, and exposes [`validate`] for
//! a full structural check. Generation is deterministic for a given
//! `(seed, level)`, uses its own PRNG (mulberry32), holds no global state
//! and runs server-side at most once per level.
//!
//! Every connection is traversable in both directions, so the walkable
//! graph is strongly connected. [`validate`] enforces a geometric hop
//! envelope for EVERY pair of platforms, so weak-only levels are rejected
//! even without connection data.

use protocol::{Aabb, GIFT_GOLDEN, GIFT_NORMAL, Gift, LevelData, Platform, Vec3};

use crate::world::overlaps;

pub const NORMAL_GIFTS: usize = 5;

const EPS: f32 = 1e-3;
// Generation is ~0.1 ms per attempt; 64 attempts keep the fallback rate well under 2%.
const MAX_ATTEMPTS: u32 = 64;
const ENEMY_SPAWNS: usize = 8;

/// Output of a successful generation pass.
#[derive(Debug, Clone)]
pub struct GeneratedLevel {
    pub data: LevelData,
    pub enemy_spawns: Vec<Vec3>,
}

// ---------------------------------------------------------------- PRNG ----

struct Rng {
    s: u32,
}

impl Rng {
    fn new(seed: u32) -> Self {
        Self { s: seed }
    }

    fn next_u32(&mut self) -> u32 {
        // mulberry32
        self.s = self.s.wrapping_add(0x6D2B_79F5);
        let mut t = self.s;
        t = (t ^ (t >> 15)).wrapping_mul(t | 1);
        t ^= t.wrapping_add((t ^ (t >> 7)).wrapping_mul(t | 61));
        t ^ (t >> 14)
    }

    fn f32(&mut self) -> f32 {
        (self.next_u32() >> 8) as f32 / 16_777_216.0
    }

    fn range_f32(&mut self, lo: f32, hi: f32) -> f32 {
        lo + (hi - lo) * self.f32()
    }

    fn range_usize(&mut self, lo: usize, hi: usize) -> usize {
        if hi <= lo {
            return lo;
        }
        lo + (self.next_u32() as usize) % (hi - lo + 1)
    }

    fn chance(&mut self, p: f32) -> bool {
        self.f32() < p
    }
}

fn splitmix32(x: u32) -> u32 {
    let mut z = x.wrapping_add(0x9E37_79B9);
    z = (z ^ (z >> 16)).wrapping_mul(0x21F0_AAAD);
    z = (z ^ (z >> 15)).wrapping_mul(0x735A_2D97);
    z ^ (z >> 15)
}

// ------------------------------------------------------------ helpers ----

fn rect_gap(a: &Aabb, b: &Aabb) -> f32 {
    let dx = (b.min.x - a.max.x).max(a.min.x - b.max.x).max(0.0);
    let dz = (b.min.z - a.max.z).max(a.min.z - b.max.z).max(0.0);
    (dx * dx + dz * dz).sqrt()
}

fn footprint_positive_overlap(a: &Aabb, b: &Aabb) -> bool {
    let ix = (a.max.x.min(b.max.x) - a.min.x.max(b.min.x)).max(0.0);
    let iz = (a.max.z.min(b.max.z) - a.min.z.max(b.min.z)).max(0.0);
    ix > EPS && iz > EPS
}

fn h_dist(ax: f32, az: f32, bx: f32, bz: f32) -> f32 {
    let dx = ax - bx;
    let dz = az - bz;
    (dx * dx + dz * dz).sqrt()
}

fn center_xz(p: &Platform) -> (f32, f32) {
    (
        (p.bounds.min.x + p.bounds.max.x) * 0.5,
        (p.bounds.min.z + p.bounds.max.z) * 0.5,
    )
}

fn golden_count(level: u32) -> usize {
    (2 + (level / 4) as usize).min(8)
}

fn walk_center(i: usize) -> f32 {
    3.0 + 6.0 * i as f32
}

struct Difficulty {
    target_pieces: usize,
    max_gap: f32,
    gap_share: f32,
    top_min: f32,
    top_max: f32,
    branch_prob: f32,
}

fn difficulty(level: u32) -> Difficulty {
    let l = level.clamp(1, 30) as f32;
    let t30 = ((l - 1.0) / 29.0).clamp(0.0, 1.0);
    Difficulty {
        target_pieces: ((12.0 + 1.2 * level.min(40) as f32).min(50.0)).floor() as usize,
        max_gap: 2.2 + 1.2 * t30,       // 2.2 (L1) -> 3.4 (L30), clamped after
        gap_share: 0.10 + 0.25 * t30,   // 0.10 (L1) -> 0.35 (L30), clamped after
        top_min: -1.0 - 5.0 * t30,      // -1   (L1) -> -6   (L30)
        top_max: 3.0 + 9.0 * t30,       //  3   (L1) -> 12   (L30)
        branch_prob: 0.18 + 0.30 * t30, // 0.18 (L1) -> 0.48 (L30)
    }
}

struct PieceMeta {
    dist: f32,
    children: usize,
    is_junction: bool,
    parent: i32,
}

struct OpenSide {
    piece: usize,
    dir: usize,
    tries: u8,
}

const DIRS: [(f32, f32); 4] = [(1.0, 0.0), (-1.0, 0.0), (0.0, 1.0), (0.0, -1.0)];

/// Checks a candidate piece against every existing piece: no strict overlap,
/// the clearance rule for unrelated pieces, the geometric hop envelope
/// (validate rule B) for EVERY pair including the parent chain, and the
/// 2.4-unit headroom rule in both directions.
fn placement_ok(platforms: &[Platform], cand: &Aabb, ctop: f32, related: &[usize]) -> bool {
    for (j, e) in platforms.iter().enumerate() {
        if overlaps(cand, &e.bounds) {
            return false;
        }
        let etop = e.bounds.max.y;
        let g = rect_gap(cand, &e.bounds);
        let rel = related.contains(&j);
        if !rel {
            if g < 1.5 - EPS && (ctop - etop).abs() <= 4.0 {
                return false;
            }
            if j == 0 && g < 3.0 - EPS && (ctop - etop).abs() <= 4.0 {
                return false;
            }
        }
        // Hop envelope (validate rule B) against every piece.
        let dt = (ctop - etop).abs();
        if g <= EPS {
            if dt > 1.25 + EPS {
                return false;
            }
        } else if g <= 2.2 + EPS {
            if dt > 1.2 + EPS {
                return false;
            }
        } else if g <= 4.0 + EPS && dt > 0.5 + EPS {
            return false;
        }
        if cand.min.y >= etop - EPS && footprint_positive_overlap(cand, &e.bounds) && cand.min.y - etop < 2.4 - EPS {
            return false;
        }
        if e.bounds.min.y >= ctop - EPS
            && footprint_positive_overlap(&e.bounds, cand)
            && e.bounds.min.y - ctop < 2.4 - EPS
        {
            return false;
        }
    }
    true
}

fn spread_pick(
    platforms: &[Platform],
    metas: &[PieceMeta],
    pool: &mut Vec<usize>,
    picked: &mut Vec<(f32, f32)>,
    k: usize,
) -> Option<Vec<usize>> {
    let mut out = Vec::with_capacity(k);
    for _ in 0..k {
        let mut best: Option<(usize, f32)> = None;
        for &i in pool.iter() {
            let c = center_xz(&platforms[i]);
            let m = &metas[i];
            let mut score = m.dist * 0.05 + if m.children == 0 { 2.0 } else { 0.0 };
            if picked.is_empty() {
                score += m.dist;
            } else {
                let d = picked
                    .iter()
                    .map(|&(px, pz)| h_dist(c.0, c.1, px, pz))
                    .fold(f32::INFINITY, f32::min);
                score += d;
            }
            if best.map(|(_, b)| score > b).unwrap_or(true) {
                best = Some((i, score));
            }
        }
        let (bi, _) = best?;
        pool.retain(|&x| x != bi);
        let c = center_xz(&platforms[bi]);
        out.push(bi);
        picked.push(c);
    }
    Some(out)
}

// ------------------------------------------------------------- build ----

fn build(rng_seed: u32, out_seed: u32, level: u32) -> Option<GeneratedLevel> {
    let mut rng = Rng::new(rng_seed);
    let diff = difficulty(level);

    let mut platforms: Vec<Platform> = Vec::new();
    let mut metas: Vec<PieceMeta> = Vec::new();

    let spawn_b = Aabb {
        min: Vec3::new(-6.0, -1.0, -6.0),
        max: Vec3::new(6.0, 0.0, 6.0),
    };
    platforms.push(Platform {
        id: 0,
        kind: 0,
        bounds: spawn_b,
    });
    metas.push(PieceMeta {
        dist: 0.0,
        children: 0,
        is_junction: false,
        parent: -1,
    });

    let mut open: Vec<OpenSide> = (0..4)
        .map(|d| OpenSide {
            piece: 0,
            dir: d,
            tries: 0,
        })
        .collect();

    let mut iterations = 0usize;
    while platforms.len() < diff.target_pieces && !open.is_empty() && iterations < 900 {
        iterations += 1;
        let pick = rng.range_usize(0, open.len() - 1);
        let side = open.swap_remove(pick);
        let parent_idx = side.piece;
        let pb = platforms[parent_idx].bounds.clone();
        let ptop = pb.max.y;
        let pdist = metas[parent_idx].dist;
        let pcenter = center_xz(&platforms[parent_idx]);
        let (dx, dz) = DIRS[side.dir];
        let axis_is_x = side.dir >> 1 == 0;
        let sign: f32 = if side.dir == 0 || side.dir == 2 { 1.0 } else { -1.0 };
        let plane = match side.dir {
            0 => pb.max.x,
            1 => pb.min.x,
            2 => pb.max.z,
            _ => pb.min.z,
        };
        let reverse = side.dir ^ 1;
        let cross_parent_center = if axis_is_x {
            (pb.min.z + pb.max.z) * 0.5
        } else {
            (pb.min.x + pb.max.x) * 0.5
        };
        let cross_parent_width = if axis_is_x {
            pb.max.z - pb.min.z
        } else {
            pb.max.x - pb.min.x
        };

        // Occasional loop: close this road end onto a nearby road.
        if rng.chance(0.20) {
            let mut closed = false;
            #[allow(clippy::needless_range_loop)] // `q` is compared with parent_idx and recorded as an index
            for q in 0..platforms.len() {
                if q == parent_idx {
                    continue;
                }
                let qb = &platforms[q].bounds;
                if (qb.max.y - ptop).abs() > 1.2 {
                    continue;
                }
                if rect_gap(&pb, qb) > diff.max_gap {
                    continue;
                }
                let qc = center_xz(&platforms[q]);
                let along = (qc.0 - pcenter.0) * dx + (qc.1 - pcenter.1) * dz;
                if along <= 0.0 {
                    continue;
                }
                closed = true;
                break;
            }
            if closed {
                metas[parent_idx].children += 1;
                continue;
            }
        }

        // Connection roll: flush (same top) or a gap connection.
        // Every connection stays inside the movement envelope in BOTH
        // directions: |top difference| <= 1.2 for gaps <= 2.2 (climbable
        // drop), |top difference| <= 0.5 for wider gaps, gaps <= max_gap.
        let mut gap = 0.0f32;
        let top = if rng.chance(diff.gap_share) {
            gap = rng.range_f32(1.5, diff.max_gap);
            let bound = if gap <= 2.2 { 1.2 } else { 0.5 };
            let roll = rng.f32();
            if roll < 0.60 {
                ptop
            } else if roll < 0.80 {
                // Rise: gap <= 2.2 with rise <= 1.2, else rise <= 0.5.
                let rise = rng.range_f32(0.5, bound);
                let t = ptop + rise;
                if t > diff.top_max { ptop } else { t }
            } else {
                // Drop: at most 1.2 (must be climbable back up).
                let drop = rng.range_f32(0.4, bound);
                (ptop - drop).max(diff.top_min)
            }
        } else {
            ptop
        };

        // Shape roll: junction pad or walkway.
        let want_junction = rng.f32() < 0.30;
        let b = if want_junction {
            let sz = rng.range_f32(4.0, 7.0);
            let jr = ((cross_parent_width - sz) * 0.5).max(0.0);
            let cc = cross_parent_center + rng.range_f32(-jr, jr);
            let near = plane + sign * gap;
            let (bmin_axis, bmax_axis) = if sign > 0.0 {
                (near, near + sz)
            } else {
                (near - sz, near)
            };
            if axis_is_x {
                Aabb {
                    min: Vec3::new(bmin_axis, top - 1.0, cc - sz * 0.5),
                    max: Vec3::new(bmax_axis, top, cc + sz * 0.5),
                }
            } else {
                Aabb {
                    min: Vec3::new(cc - sz * 0.5, top - 1.0, bmin_axis),
                    max: Vec3::new(cc + sz * 0.5, top, bmax_axis),
                }
            }
        } else {
            let len = rng.range_f32(6.0, 18.0);
            let w = rng.range_f32(2.0, 3.0);
            let jr = ((cross_parent_width - w) * 0.5).max(0.0);
            let cc = cross_parent_center + rng.range_f32(-jr, jr);
            let near = plane + sign * gap;
            let (bmin_axis, bmax_axis) = if sign > 0.0 {
                (near, near + len)
            } else {
                (near - len, near)
            };
            if axis_is_x {
                Aabb {
                    min: Vec3::new(bmin_axis, top - 1.0, cc - w * 0.5),
                    max: Vec3::new(bmax_axis, top, cc + w * 0.5),
                }
            } else {
                Aabb {
                    min: Vec3::new(cc - w * 0.5, top - 1.0, bmin_axis),
                    max: Vec3::new(cc + w * 0.5, top, bmax_axis),
                }
            }
        };

        // Reject placements that overlap, crowd or hang over existing pieces.
        let mut related: Vec<usize> = Vec::new();
        let mut cur: i32 = parent_idx as i32;
        while cur >= 0 {
            related.push(cur as usize);
            cur = metas[cur as usize].parent;
        }
        if !placement_ok(&platforms, &b, top, &related) {
            if side.tries < 2 {
                open.push(OpenSide {
                    piece: parent_idx,
                    dir: side.dir,
                    tries: side.tries + 1,
                });
            }
            continue;
        }

        metas[parent_idx].children += 1;
        let pc = ((b.min.x + b.max.x) * 0.5, (b.min.z + b.max.z) * 0.5);
        let d = pdist + h_dist(pcenter.0, pcenter.1, pc.0, pc.1);
        let id = platforms.len() as u16;
        platforms.push(Platform { id, kind: 0, bounds: b });
        metas.push(PieceMeta {
            dist: d,
            children: 0,
            is_junction: want_junction,
            parent: parent_idx as i32,
        });
        let last_idx = platforms.len() - 1;

        // Emit the free sides of the new piece (1-3 for junctions).
        let mut emit: Vec<usize> = Vec::new();
        if rng.chance(0.88) {
            emit.push(side.dir);
        }
        if want_junction {
            if emit.is_empty() {
                emit.push(side.dir);
            }
            let mut extra = 0usize;
            if rng.chance((diff.branch_prob * 0.6).min(1.0)) {
                extra += 1;
            }
            if rng.chance(diff.branch_prob) {
                extra += 1;
            }
            let mut pool: Vec<usize> = (0..4usize).filter(|&d| d != reverse && !emit.contains(&d)).collect();
            while extra > 0 && !pool.is_empty() {
                let k = rng.range_usize(0, pool.len() - 1);
                emit.push(pool.swap_remove(k));
                extra -= 1;
            }
            emit.truncate(3);
        } else if rng.chance(diff.branch_prob) {
            let pool: Vec<usize> = (0..4usize).filter(|&d| d != reverse && !emit.contains(&d)).collect();
            if !pool.is_empty() {
                let k = rng.range_usize(0, pool.len() - 1);
                emit.push(pool[k]);
            }
        }
        for d in emit {
            open.push(OpenSide {
                piece: last_idx,
                dir: d,
                tries: 0,
            });
        }
    }

    if platforms.len() < 9 {
        return None;
    }

    // Beacon: junction pad or end piece with the greatest path distance.
    let mut beacon_idx: Option<usize> = None;
    for i in 1..platforms.len() {
        if (metas[i].children == 0 || metas[i].is_junction)
            && beacon_idx.map(|b| metas[i].dist > metas[b].dist).unwrap_or(true)
        {
            beacon_idx = Some(i);
        }
    }
    let beacon_idx = beacon_idx?;
    let bcenter = center_xz(&platforms[beacon_idx]);
    let beacon = Vec3::new(bcenter.0, platforms[beacon_idx].bounds.max.y, bcenter.1);

    // Gifts: prefer dead-ends / side branches, spread greedily.
    let mut pool: Vec<usize> = (1..platforms.len()).filter(|&i| i != beacon_idx).collect();
    if pool.len() < NORMAL_GIFTS + golden_count(level) {
        return None;
    }
    let mut picked_pos: Vec<(f32, f32)> = Vec::new();
    let normal_sel = spread_pick(&platforms, &metas, &mut pool, &mut picked_pos, NORMAL_GIFTS)?;
    let mut gifts: Vec<Gift> = Vec::with_capacity(NORMAL_GIFTS + golden_count(level));
    for (i, &pi) in normal_sel.iter().enumerate() {
        let c = center_xz(&platforms[pi]);
        let t = platforms[pi].bounds.max.y;
        gifts.push(Gift {
            id: i as u16,
            pos: Vec3::new(c.0, t + 1.0, c.1),
            kind: GIFT_NORMAL,
        });
    }
    let golden_sel = spread_pick(&platforms, &metas, &mut pool, &mut picked_pos, golden_count(level))?;
    for (i, &pi) in golden_sel.iter().enumerate() {
        let c = center_xz(&platforms[pi]);
        let t = platforms[pi].bounds.max.y;
        gifts.push(Gift {
            id: (NORMAL_GIFTS + i) as u16,
            pos: Vec3::new(c.0, t + 1.0, c.1),
            kind: GIFT_GOLDEN,
        });
    }

    // Enemy spawns: greedy maximin spread, >= 4 from any gift/beacon.
    let mut forbidden: Vec<(f32, f32)> = gifts.iter().map(|g| (g.pos.x, g.pos.z)).collect();
    forbidden.push((beacon.x, beacon.z));
    let cand: Vec<usize> = (0..platforms.len())
        .filter(|&i| {
            let c = center_xz(&platforms[i]);
            h_dist(c.0, c.1, 0.0, 0.0) >= 15.0 && !forbidden.iter().any(|&(fx, fz)| h_dist(c.0, c.1, fx, fz) < 4.0)
        })
        .collect();
    if cand.is_empty() {
        return None;
    }
    let mut uses: Vec<usize> = Vec::new();
    let mut enemy_spawns: Vec<Vec3> = Vec::with_capacity(ENEMY_SPAWNS);
    let mut iter = 0usize;
    while enemy_spawns.len() < ENEMY_SPAWNS && iter < 64 {
        iter += 1;
        let mut best: Option<(usize, f32)> = None;
        for &i in cand.iter() {
            let use_n = uses.iter().filter(|&&p| p == i).count() as f32;
            let c = center_xz(&platforms[i]);
            let d = if enemy_spawns.is_empty() {
                metas[i].dist + 30.0 - use_n * 60.0
            } else {
                enemy_spawns
                    .iter()
                    .map(|s| h_dist(c.0, c.1, s.x, s.z))
                    .fold(f32::INFINITY, f32::min)
                    + 30.0
                    - use_n * 60.0
            };
            if best.map(|(_, b)| d > b).unwrap_or(true) {
                best = Some((i, d));
            }
        }
        let (bi, _) = best?;
        let c = center_xz(&platforms[bi]);
        let use_n = uses.iter().filter(|&&p| p == bi).count();
        let off = match use_n {
            0 => 0.0f32,
            n => {
                let k = n.div_ceil(2) as f32 * 0.45;
                if (n & 1) == 1 { k } else { -k }
            }
        };
        // Offset tangentially so the 3D distance from spawn never drops below 15.
        let rad = (c.0 * c.0 + c.1 * c.1).sqrt().max(EPS);
        let tx = -c.1 / rad;
        let tz = c.0 / rad;
        let p = &platforms[bi];
        let top = p.bounds.max.y;
        // The clamped, offset point must still satisfy every spawn rule; try the opposite side and the centre
        // before giving up on this piece (it stays penalised through `uses` either way).
        let ok = |x: f32, z: f32| {
            !forbidden.iter().any(|&(fx, fz)| h_dist(x, z, fx, fz) < 4.0)
                && (x * x + (top + 2.0) * (top + 2.0) + z * z).sqrt() >= 15.0
        };
        let chosen = [off, -off, 0.0].into_iter().find_map(|o| {
            let px = (c.0 + tx * o).clamp(p.bounds.min.x + 0.2, p.bounds.max.x - 0.2);
            let pz = (c.1 + tz * o).clamp(p.bounds.min.z + 0.2, p.bounds.max.z - 0.2);
            ok(px, pz).then_some((px, pz))
        });
        if let Some((px, pz)) = chosen {
            enemy_spawns.push(Vec3::new(px, top + 2.0, pz));
        }
        uses.push(bi);
    }
    if enemy_spawns.len() < ENEMY_SPAWNS {
        return None;
    }

    let min_bottom = platforms.iter().map(|p| p.bounds.min.y).fold(f32::INFINITY, f32::min);
    let kill_y = min_bottom - 12.0;
    let data = LevelData {
        level,
        seed: out_seed,
        spawn: Vec3::ZERO,
        beacon,
        kill_y,
        platforms,
        gifts,
    };
    Some(GeneratedLevel { data, enemy_spawns })
}

// --------------------------------------------------------- fallback ----

fn fallback(seed: u32, level: u32) -> GeneratedLevel {
    let mut platforms: Vec<Platform> = Vec::with_capacity(FALLBACK_PLATFORM_COUNT);
    let spawn_b = Aabb {
        min: Vec3::new(-6.0, -1.0, -6.0),
        max: Vec3::new(6.0, 0.0, 6.0),
    };
    platforms.push(Platform {
        id: 0,
        kind: 0,
        bounds: spawn_b,
    });
    for i in 1..=24usize {
        let min_x = 6.0 + 6.0 * (i - 1) as f32;
        let b = Aabb {
            min: Vec3::new(min_x, -1.0, -1.25),
            max: Vec3::new(min_x + 6.0, 0.0, 1.25),
        };
        platforms.push(Platform {
            id: i as u16,
            kind: 0,
            bounds: b,
        });
    }
    let pad_min = 6.0 + 6.0 * 24.0;
    let pad = Aabb {
        min: Vec3::new(pad_min, -1.0, -2.5),
        max: Vec3::new(pad_min + 5.0, 0.0, 2.5),
    };
    platforms.push(Platform {
        id: 25,
        kind: 0,
        bounds: pad,
    });

    let beacon = Vec3::new(pad_min + 2.5, 0.0, 0.0);
    let mut gifts: Vec<Gift> = Vec::new();
    let normal_pieces = [2usize, 5, 8, 11, 14];
    for (i, &p) in normal_pieces.iter().enumerate() {
        gifts.push(Gift {
            id: i as u16,
            pos: Vec3::new(walk_center(p), 1.0, 0.0),
            kind: GIFT_NORMAL,
        });
    }
    let golden_candidates = [1usize, 3, 4, 6, 7, 9, 10, 12, 13, 15, 16, 17, 18, 19, 20, 21];
    for (i, &p) in golden_candidates.iter().take(golden_count(level)).enumerate() {
        gifts.push(Gift {
            id: (NORMAL_GIFTS + i) as u16,
            pos: Vec3::new(walk_center(p), 1.0, 0.0),
            kind: GIFT_GOLDEN,
        });
    }
    let enemy_pieces = [22usize, 23, 24, 18, 20, 13, 19, 21];
    let enemy_spawns = enemy_pieces
        .iter()
        .map(|&p| Vec3::new(walk_center(p), 2.0, 0.0))
        .collect();
    let data = LevelData {
        level,
        seed,
        spawn: Vec3::ZERO,
        beacon,
        kill_y: -13.0,
        platforms,
        gifts,
    };
    GeneratedLevel { data, enemy_spawns }
}

const FALLBACK_PLATFORM_COUNT: usize = 26;

// ------------------------------------------------------------ public ----

/// The lobby hub: a lone spawn platform, no gifts, beacon far below kill_y.
pub fn hub() -> LevelData {
    let spawn_b = Aabb {
        min: Vec3::new(-6.0, -1.0, -6.0),
        max: Vec3::new(6.0, 0.0, 6.0),
    };
    LevelData {
        level: 0,
        seed: 0,
        spawn: Vec3::ZERO,
        beacon: Vec3::new(0.0, -80.0, 0.0),
        kill_y: -13.0,
        platforms: vec![Platform {
            id: 0,
            kind: 0,
            bounds: spawn_b,
        }],
        gifts: Vec::new(),
    }
}

/// A fresh, completable level. Same `(seed, level)` -> identical output.
pub fn generate(seed: u32, level: u32) -> GeneratedLevel {
    for attempt in 0..MAX_ATTEMPTS {
        let attempt_seed = if attempt == 0 {
            seed
        } else {
            splitmix32(seed.wrapping_add(attempt.wrapping_mul(0x9E37_79B9)))
        };
        if let Some(candidate) = build(attempt_seed, seed, level)
            && validate(&candidate.data).is_ok()
        {
            return candidate;
        }
    }
    fallback(seed, level)
}

/// Full structural check used by [`generate`] (retry on failure) and by tests.
/// Err describes the first problem found. Checks ids, platform 0, pairwise
/// overlap/clearance/headroom, the geometric hop envelope for every pair,
/// gifts and beacon placement, STRONG connectivity of the walkable graph
/// (0 reaches every platform and every platform reaches 0), and kill_y.
pub fn validate(level: &LevelData) -> Result<(), String> {
    let n = level.platforms.len();
    if n == 0 {
        return Err("no platforms".to_string());
    }

    // Ids unique and dense 0..n
    let mut ids: Vec<u16> = level.platforms.iter().map(|p| p.id).collect();
    ids.sort_unstable();
    for (i, id) in ids.iter().enumerate() {
        if *id as usize != i {
            return Err(format!("platform ids not unique/dense at {id}"));
        }
    }

    // Platform 0 is the 12x12 spawn platform with top at y = 0, containing spawn
    let p0 = level
        .platforms
        .iter()
        .find(|p| p.id == 0)
        .ok_or_else(|| "missing spawn platform (id 0)".to_string())?;
    if p0.bounds.max.y.abs() > EPS {
        return Err("platform 0 top not at y = 0".to_string());
    }
    let w = p0.bounds.max.x - p0.bounds.min.x;
    let d = p0.bounds.max.z - p0.bounds.min.z;
    if (w - 12.0).abs() > EPS || (d - 12.0).abs() > EPS {
        return Err("platform 0 is not 12x12".to_string());
    }
    if level.spawn.x < p0.bounds.min.x - EPS
        || level.spawn.x > p0.bounds.max.x + EPS
        || level.spawn.z < p0.bounds.min.z - EPS
        || level.spawn.z > p0.bounds.max.z + EPS
    {
        return Err("spawn not over platform 0".to_string());
    }
    if (level.spawn.y - p0.bounds.max.y).abs() > EPS {
        return Err("spawn not on platform 0 top".to_string());
    }

    // Pairwise: no strict overlap, clearance, hop envelope, headroom
    for i in 0..n {
        for j in (i + 1)..n {
            let a = &level.platforms[i];
            let b = &level.platforms[j];
            if overlaps(&a.bounds, &b.bounds) {
                return Err(format!("platforms {} and {} overlap", a.id, b.id));
            }
            let ta = a.bounds.max.y;
            let tb = b.bounds.max.y;
            let g = rect_gap(&a.bounds, &b.bounds);
            let dt = (ta - tb).abs();
            if dt <= 4.0 && g > EPS && g < 1.5 - EPS {
                return Err(format!(
                    "platforms {} and {} too close (clearance {:.2})",
                    a.id, b.id, g
                ));
            }
            // Hop envelope: any pair that looks like a jump must be a fair one;
            // pairs violating these bounds must be >= 4.0 apart.
            if g <= EPS {
                // flush (touching / overlapping footprints): stairs allow <= 1.25
                if dt > 1.25 + EPS {
                    return Err(format!(
                        "flush platforms {} and {} differ by {dt:.2} (> 1.25)",
                        a.id, b.id
                    ));
                }
            } else if g <= 2.2 + EPS {
                if dt > 1.2 + EPS {
                    return Err(format!(
                        "platforms {} and {} differ by {dt:.2} over a {g:.2} gap (> 1.20)",
                        a.id, b.id
                    ));
                }
            } else if g <= 4.0 + EPS && dt > 0.5 + EPS {
                return Err(format!(
                    "platforms {} and {} differ by {dt:.2} over a {g:.2} gap (> 0.50)",
                    a.id, b.id
                ));
            }
            if b.bounds.min.y >= ta - EPS
                && footprint_positive_overlap(&a.bounds, &b.bounds)
                && b.bounds.min.y - ta < 2.4 - EPS
            {
                return Err(format!("insufficient headroom above platform {}", a.id));
            }
            if a.bounds.min.y >= tb - EPS
                && footprint_positive_overlap(&b.bounds, &a.bounds)
                && a.bounds.min.y - tb < 2.4 - EPS
            {
                return Err(format!("insufficient headroom above platform {}", b.id));
            }
        }
    }

    // Gift counts, ids and support
    let normal = level.gifts.iter().filter(|g| g.kind == GIFT_NORMAL).count();
    if normal != NORMAL_GIFTS {
        return Err(format!("expected {NORMAL_GIFTS} normal gifts, found {normal}"));
    }
    let expected_golden = golden_count(level.level);
    if level
        .gifts
        .iter()
        .any(|g| g.kind != GIFT_NORMAL && g.kind != GIFT_GOLDEN)
    {
        return Err("unknown gift kind".to_string());
    }
    let golden = level.gifts.iter().filter(|g| g.kind == GIFT_GOLDEN).count();
    if golden != expected_golden {
        return Err(format!("expected {expected_golden} golden gifts, found {golden}"));
    }
    let mut gids: Vec<u16> = level.gifts.iter().map(|g| g.id).collect();
    gids.sort_unstable();
    for (i, id) in gids.iter().enumerate() {
        if *id as usize != i {
            return Err("gift ids not unique/dense".to_string());
        }
    }
    for gift in &level.gifts {
        let mut supported = false;
        for p in &level.platforms {
            let top = p.bounds.max.y;
            if (gift.pos.y - (top + 1.0)).abs() > EPS {
                continue;
            }
            if gift.pos.x < p.bounds.min.x + 0.8 - EPS || gift.pos.x > p.bounds.max.x - 0.8 + EPS {
                continue;
            }
            if gift.pos.z < p.bounds.min.z + 0.8 - EPS || gift.pos.z > p.bounds.max.z - 0.8 + EPS {
                continue;
            }
            supported = true;
            break;
        }
        if !supported {
            return Err(format!("gift {} not supported on a platform top", gift.id));
        }
    }

    // Beacon on a platform top
    let beacon_ok = level.platforms.iter().any(|p| {
        (level.beacon.y - p.bounds.max.y).abs() <= EPS
            && level.beacon.x >= p.bounds.min.x - EPS
            && level.beacon.x <= p.bounds.max.x + EPS
            && level.beacon.z >= p.bounds.min.z - EPS
            && level.beacon.z <= p.bounds.max.z + EPS
    });
    if !beacon_ok {
        return Err("beacon not on a platform top".to_string());
    }

    // Reachability: A -> B iff horizontal gap <= 4.0 and B's top is at most
    // 1.5 above A's top (any drop allowed). The graph must be STRONGLY
    // connected: 0 reaches every platform AND every platform reaches 0
    // (checked via BFS on the reversed graph).
    let start = level
        .platforms
        .iter()
        .position(|p| p.id == 0)
        .ok_or_else(|| "missing platform id 0".to_string())?;
    let edge = |from: usize, to: usize| -> bool {
        level.platforms[to].bounds.max.y <= level.platforms[from].bounds.max.y + 1.5 + EPS
            && rect_gap(&level.platforms[from].bounds, &level.platforms[to].bounds) <= 4.0 + EPS
    };
    let bfs = |seed: usize, reversed: bool| -> Vec<bool> {
        let mut visited = vec![false; n];
        visited[seed] = true;
        let mut stack = vec![seed];
        while let Some(a) = stack.pop() {
            #[allow(clippy::needless_range_loop)] // `b` indexes both `visited` and the edge predicate
            for b in 0..n {
                if visited[b] {
                    continue;
                }
                let ok = if reversed { edge(b, a) } else { edge(a, b) };
                if ok {
                    visited[b] = true;
                    stack.push(b);
                }
            }
        }
        visited
    };
    let fwd = bfs(start, false);
    if let Some(idx) = fwd.iter().position(|v| !v) {
        return Err(format!(
            "platform {} is unreachable from spawn",
            level.platforms[idx].id
        ));
    }
    let rev = bfs(start, true);
    if let Some(idx) = rev.iter().position(|v| !v) {
        return Err(format!(
            "no route from platform {} to spawn (graph not strongly connected)",
            level.platforms[idx].id
        ));
    }

    // kill_y at least 10 below every platform bottom
    for p in &level.platforms {
        if level.kill_y > p.bounds.min.y - 10.0 + EPS {
            return Err(format!("kill_y not at least 10 below platform {} bottom", p.id));
        }
    }
    Ok(())
}

// ------------------------------------------------------------- tests ----

#[cfg(test)]
mod tests {
    use super::*;
    use protocol::{GIFT_GOLDEN, GIFT_NORMAL};

    fn spawn_platform() -> Platform {
        Platform {
            id: 0,
            kind: 0,
            bounds: Aabb {
                min: Vec3::new(-6.0, -1.0, -6.0),
                max: Vec3::new(6.0, 0.0, 6.0),
            },
        }
    }

    fn gifts_on_spawn() -> Vec<Gift> {
        let mut gifts: Vec<Gift> = (0..NORMAL_GIFTS)
            .map(|i| Gift {
                id: i as u16,
                pos: Vec3::new(-4.0 + 2.0 * i as f32, 1.0, -3.0),
                kind: GIFT_NORMAL,
            })
            .collect();
        for i in 0..2usize {
            gifts.push(Gift {
                id: (NORMAL_GIFTS + i) as u16,
                pos: Vec3::new(-4.0 + 2.0 * i as f32, 1.0, 3.0),
                kind: GIFT_GOLDEN,
            });
        }
        gifts
    }

    fn hop_edge(a: &Platform, b: &Platform) -> bool {
        b.bounds.max.y <= a.bounds.max.y + 1.5 + EPS && rect_gap(&a.bounds, &b.bounds) <= 4.0 + EPS
    }

    fn strongly_connected(level: &LevelData) -> bool {
        let n = level.platforms.len();
        let start = level.platforms.iter().position(|p| p.id == 0).unwrap_or(0);
        let mut fwd = vec![false; n];
        fwd[start] = true;
        let mut stack = vec![start];
        while let Some(a) = stack.pop() {
            #[allow(clippy::needless_range_loop)] // `b` indexes `fwd` and the platform list together
            for b in 0..n {
                if !fwd[b] && hop_edge(&level.platforms[a], &level.platforms[b]) {
                    fwd[b] = true;
                    stack.push(b);
                }
            }
        }
        let mut rev = vec![false; n];
        rev[start] = true;
        let mut rstack = vec![start];
        while let Some(a) = rstack.pop() {
            #[allow(clippy::needless_range_loop)] // `b` indexes `rev` and the platform list together
            for b in 0..n {
                if !rev[b] && hop_edge(&level.platforms[b], &level.platforms[a]) {
                    rev[b] = true;
                    rstack.push(b);
                }
            }
        }
        fwd.iter().all(|&v| v) && rev.iter().all(|&v| v)
    }

    fn assert_enemy_rules(g: &GeneratedLevel) {
        assert_eq!(g.enemy_spawns.len(), 8, "expected 8 enemy spawns");
        for s in &g.enemy_spawns {
            let supported = g.data.platforms.iter().any(|p| {
                (s.y - (p.bounds.max.y + 2.0)).abs() < 1e-3
                    && s.x >= p.bounds.min.x - 1e-3
                    && s.x <= p.bounds.max.x + 1e-3
                    && s.z >= p.bounds.min.z - 1e-3
                    && s.z <= p.bounds.max.z + 1e-3
            });
            assert!(
                supported,
                "enemy spawn not 2.0 above a platform top / inside its extent"
            );
            let d3 = ((s.x - g.data.spawn.x).powi(2) + (s.y - g.data.spawn.y).powi(2) + (s.z - g.data.spawn.z).powi(2))
                .sqrt();
            assert!(d3 >= 15.0 - 1e-3, "enemy spawn only {d3} from spawn");
            for gift in &g.data.gifts {
                let dh = ((s.x - gift.pos.x).powi(2) + (s.z - gift.pos.z).powi(2)).sqrt();
                assert!(dh >= 4.0 - 1e-3, "enemy spawn {dh} horizontally from a gift");
            }
            let db = ((s.x - g.data.beacon.x).powi(2) + (s.z - g.data.beacon.z).powi(2)).sqrt();
            assert!(db >= 4.0 - 1e-3, "enemy spawn {db} horizontally from the beacon");
        }
    }

    #[test]
    fn validate_wide_seed_sweep() {
        let mut fallback_hits = 0usize;
        let mut total = 0usize;
        for level in [1u32, 5, 10, 20, 40] {
            for seed in 0..300u32 {
                let g = generate(seed, level);
                if let Err(e) = validate(&g.data) {
                    panic!("validate failed for seed {seed} level {level}: {e}");
                }
                assert!(
                    strongly_connected(&g.data),
                    "level not strongly connected for seed {seed} level {level}"
                );
                if g.data == fallback(seed, level).data {
                    fallback_hits += 1;
                }
                total += 1;
            }
        }
        assert!(
            fallback_hits * 50 <= total,
            "fallback rate too high: {fallback_hits}/{total} (> 2%)"
        );
    }

    #[test]
    fn determinism_and_variety() {
        let a = generate(42, 7).data;
        let b = generate(42, 7).data;
        assert_eq!(a, b);
        let c = generate(43, 7).data;
        assert_ne!(a.platforms, c.platforms);
    }

    #[test]
    fn difficulty_grows_with_level() {
        let d1 = difficulty(1);
        let d25 = difficulty(25);
        let d40 = difficulty(40);
        assert!(d1.target_pieces < d40.target_pieces);
        assert!(d1.max_gap < d25.max_gap);
        assert!(d25.max_gap <= d40.max_gap + 1e-6);
        assert!(d1.gap_share < d25.gap_share);
        assert!(d1.top_min > d40.top_min);
        assert!(d1.top_max < d40.top_max);
        assert!((d1.max_gap - 2.2).abs() < 1e-6);
        assert!((d40.max_gap - 3.4).abs() < 1e-6);

        let mean_len = |level: u32| -> f32 {
            let mut sum = 0usize;
            for s in [3u32, 17, 29, 51, 101] {
                sum += generate(s, level).data.platforms.len();
            }
            sum as f32 / 5.0
        };
        assert!(mean_len(40) > mean_len(1));
    }

    #[test]
    fn hub_is_minimal() {
        let h = hub();
        assert_eq!(h.platforms.len(), 1);
        assert_eq!(h.platforms[0].id, 0);
        assert_eq!((h.platforms[0].bounds.max.x - h.platforms[0].bounds.min.x) as i32, 12);
        assert!(h.gifts.is_empty());
        assert!(h.beacon.y < h.kill_y, "hub beacon must be unreachable (below kill_y)");
        assert!(h.kill_y <= -11.0);
    }

    #[test]
    fn golden_gift_counts_and_enemy_rules() {
        for level in [1u32, 5, 20, 40] {
            let g = generate(123, level);
            let golden = g.data.gifts.iter().filter(|k| k.kind == GIFT_GOLDEN).count();
            assert_eq!(golden, (2 + level / 4).min(8) as usize);
            assert_eq!(
                g.data.gifts.iter().filter(|k| k.kind == GIFT_NORMAL).count(),
                NORMAL_GIFTS
            );
            assert_eq!(g.data.gifts.len(), NORMAL_GIFTS + golden);
            assert_enemy_rules(&g);
        }
    }

    #[test]
    fn gift_ids_unique_and_dense() {
        let g = generate(321, 12);
        let mut ids: Vec<u16> = g.data.gifts.iter().map(|k| k.id).collect();
        ids.sort_unstable();
        for (i, id) in ids.iter().enumerate() {
            assert_eq!(*id as usize, i);
        }
    }

    #[test]
    fn validate_accepts_simple_valid_level() {
        let w = Platform {
            id: 1,
            kind: 0,
            bounds: Aabb {
                min: Vec3::new(6.0, -1.0, -1.5),
                max: Vec3::new(12.0, 0.0, 1.5),
            },
        };
        let data = LevelData {
            level: 1,
            seed: 1,
            spawn: Vec3::ZERO,
            beacon: Vec3::new(9.0, 0.0, 0.0),
            kill_y: -13.0,
            platforms: vec![spawn_platform(), w],
            gifts: gifts_on_spawn(),
        };
        assert_eq!(validate(&data), Ok(()));
    }

    #[test]
    fn validate_rejects_unreachable_gift() {
        let far = Platform {
            id: 1,
            kind: 0,
            bounds: Aabb {
                min: Vec3::new(30.0, -1.0, -1.5),
                max: Vec3::new(36.0, 0.0, 1.5),
            },
        };
        let mut gifts = gifts_on_spawn(); // 5 normal + 2 golden (level 1 quota so far)
        gifts.push(Gift {
            id: 7,
            pos: Vec3::new(33.0, 1.0, 0.0),
            kind: GIFT_GOLDEN,
        }); // golden = 3, expected for level 5
        let data = LevelData {
            level: 5,
            seed: 1,
            spawn: Vec3::ZERO,
            beacon: Vec3::new(2.0, 0.0, 2.0),
            kill_y: -13.0,
            platforms: vec![spawn_platform(), far],
            gifts,
        };
        assert!(validate(&data).is_err());
    }

    #[test]
    fn validate_rejects_overlapping_platforms() {
        let p = Platform {
            id: 1,
            kind: 0,
            bounds: Aabb {
                min: Vec3::new(4.0, -1.0, -1.5),
                max: Vec3::new(10.0, 0.0, 1.5),
            },
        };
        let data = LevelData {
            level: 1,
            seed: 1,
            spawn: Vec3::ZERO,
            beacon: Vec3::ZERO,
            kill_y: -13.0,
            platforms: vec![spawn_platform(), p],
            gifts: Vec::new(),
        };
        assert!(validate(&data).is_err());
    }

    #[test]
    fn validate_rejects_missing_spawn_platform() {
        let p = Platform {
            id: 0,
            kind: 0,
            bounds: Aabb {
                min: Vec3::new(0.0, -1.0, 0.0),
                max: Vec3::new(4.0, 0.0, 4.0),
            },
        };
        let data = LevelData {
            level: 1,
            seed: 1,
            spawn: Vec3::ZERO,
            beacon: Vec3::new(2.0, 0.0, 2.0),
            kill_y: -13.0,
            platforms: vec![p],
            gifts: Vec::new(),
        };
        assert!(validate(&data).is_err());
    }

    #[test]
    fn validate_rejects_orphan_platform() {
        let far = Platform {
            id: 1,
            kind: 0,
            bounds: Aabb {
                min: Vec3::new(30.0, -1.0, -1.5),
                max: Vec3::new(36.0, 0.0, 1.5),
            },
        };
        let data = LevelData {
            level: 1,
            seed: 1,
            spawn: Vec3::ZERO,
            beacon: Vec3::new(2.0, 0.0, 2.0),
            kill_y: -13.0,
            platforms: vec![spawn_platform(), far],
            gifts: gifts_on_spawn(),
        };
        assert!(validate(&data).is_err());
    }

    #[test]
    fn validate_rejects_one_way_drop() {
        // Spawn edge at x = 6; neighbour at gap 2.0 with top at -2.0:
        // hoppable down, but no route back up (drop > 1.2).
        let drop_down = Platform {
            id: 1,
            kind: 0,
            bounds: Aabb {
                min: Vec3::new(8.0, -3.0, -1.5),
                max: Vec3::new(14.0, -2.0, 1.5),
            },
        };
        let data = LevelData {
            level: 1,
            seed: 1,
            spawn: Vec3::ZERO,
            beacon: Vec3::new(2.0, 0.0, 2.0),
            kill_y: -13.0,
            platforms: vec![spawn_platform(), drop_down],
            gifts: gifts_on_spawn(),
        };
        assert!(validate(&data).is_err());
    }

    #[test]
    fn validate_rejects_wide_gap_with_rise() {
        // Gap 3.0 with a 1.0 rise: the hop envelope allows at most 0.5 there.
        let raised = Platform {
            id: 1,
            kind: 0,
            bounds: Aabb {
                min: Vec3::new(9.0, 0.0, -1.5),
                max: Vec3::new(15.0, 1.0, 1.5),
            },
        };
        let data = LevelData {
            level: 1,
            seed: 1,
            spawn: Vec3::ZERO,
            beacon: Vec3::new(2.0, 0.0, 2.0),
            kill_y: -13.0,
            platforms: vec![spawn_platform(), raised],
            gifts: gifts_on_spawn(),
        };
        assert!(validate(&data).is_err());
    }

    #[test]
    fn fallback_passes_validate() {
        for level in [1u32, 12, 40] {
            let g = fallback(999, level);
            assert_eq!(validate(&g.data), Ok(()));
            assert_eq!(g.enemy_spawns.len(), 8);
        }
    }
}
