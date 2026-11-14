// EXCERPT of server/crates/sim/src/levelgen.rs (current file). Line ranges are marked.
// ---- lines 40-60: gift constants ----
const MAX_ATTEMPTS: u32 = 40;
const CANDIDATE_TRIALS: usize = 64;
const MAX_GROWTH_ITERS: usize = 4000;
const TOPUP_ATTEMPTS: usize = 8000;
const GIFT_COUNT_BASE: u32 = 46;
const GIFT_COUNT_PER_LEVEL: u32 = 10;
const GIFT_COUNT_PER_PLAYER: u32 = 6;
const GIFT_CAP: u32 = 400;
const GIFT_MIN_SPACING: f32 = 2.0;
const GIFT_BEACON_CLEAR: f32 = 2.5;
const GIFT_AREA_PER: f32 = 9.0;
const GIFT_ELEVATED_QUOTA: f32 = 0.2;
/// Path to the shared chunk catalog, relative to this source file.
const CHUNK_CATALOG_JSON: &str = include_str!("../../../../design/chunks.json");

/// Output of a successful generation pass.
#[derive(Debug, Clone)]
pub struct GeneratedLevel {
    pub data: LevelData,
    pub enemy_spawns: Vec<Vec3>,
}

// ---- lines 600-790: geometry helpers, PointGrid, platform_top_under ----
}

fn translate_aabb(b: &Aabb, offset: Vec3) -> Aabb {
    Aabb {
        min: Vec3::new(b.min.x + offset.x, b.min.y + offset.y, b.min.z + offset.z),
        max: Vec3::new(b.max.x + offset.x, b.max.y + offset.y, b.max.z + offset.z),
    }
}

fn aabb_area(b: &Aabb) -> f32 {
    (b.max.x - b.min.x) * (b.max.z - b.min.z)
}

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

/// True when the boxes share volume (touching faces do not count).
fn aabb_intersect(a: &Aabb, b: &Aabb) -> bool {
    a.min.x < b.max.x - EPS
        && b.min.x < a.max.x - EPS
        && a.min.y < b.max.y - EPS
        && b.min.y < a.max.y - EPS
        && a.min.z < b.max.z - EPS
        && b.min.z < a.max.z - EPS
}

/// True when the XZ footprints overlap with positive area.
fn xz_overlap_positive(a: &Aabb, b: &Aabb) -> bool {
    footprint_positive_overlap(a, b)
}

/// Two placed platforms are compatible iff they do not share volume and any XZ
/// overlap keeps at least 4.0 u vertical clearance (CHUNK_FORMAT stacking rule).
///
/// Test hook: exposed so integration tests can check the stacking rule directly.
pub fn platform_pair_ok(a: &Aabb, b: &Aabb) -> bool {
    if aabb_intersect(a, b) {
        return false;
    }
    if xz_overlap_positive(a, b) {
        let clearance = (a.min.y - b.max.y).max(b.min.y - a.max.y);
        // Resting contact (a dais on a floor, a stair step on its landing) is fine; only a
        // cramped gap between separate layers is rejected (CHUNK_FORMAT stacking rule).
        if clearance > EPS && clearance < STACK_CLEARANCE - EPS {
            return false;
        }
    }
    true
}

fn h_dist(ax: f32, az: f32, bx: f32, bz: f32) -> f32 {
    let dx = ax - bx;
    let dz = az - bz;
    (dx * dx + dz * dz).sqrt()
}

// ----------------------------------------------------- coarse grid ----

/// Coarse uniform 8 u XZ index over placed platforms (candidate-test acceleration).
struct CoarseGrid {
    cell: f32,
    cells: BTreeMap<(i32, i32), Vec<usize>>,
}

impl CoarseGrid {
    fn new() -> Self {
        Self {
            cell: GRID_CELL,
            cells: BTreeMap::new(),
        }
    }

    fn key(&self, x: f32, z: f32) -> (i32, i32) {
        ((x / self.cell).floor() as i32, (z / self.cell).floor() as i32)
    }

    fn insert(&mut self, idx: usize, b: &Aabb) {
        let (x0, z0) = self.key(b.min.x, b.min.z);
        let (x1, z1) = self.key(b.max.x, b.max.z);
        for cx in x0..=x1 {
            for cz in z0..=z1 {
                self.cells.entry((cx, cz)).or_default().push(idx);
            }
        }
    }

    /// Indices of placed platforms whose cells intersect `b`, sorted and deduped.
    fn candidates(&self, b: &Aabb) -> Vec<usize> {
        let (x0, z0) = self.key(b.min.x, b.min.z);
        let (x1, z1) = self.key(b.max.x, b.max.z);
        let mut out = Vec::new();
        for cx in x0..=x1 {
            for cz in z0..=z1 {
                if let Some(v) = self.cells.get(&(cx, cz)) {
                    out.extend_from_slice(v);
                }
            }
        }
        out.sort_unstable();
        out.dedup();
        out
    }
}

/// Uniform 2.0 u XZ index over accepted gift points (spacing queries).
struct PointGrid {
    cell: f32,
    cells: BTreeMap<(i32, i32), Vec<usize>>,
    points: Vec<Vec3>,
}

impl PointGrid {
    fn new() -> Self {
        Self {
            cell: GIFT_MIN_SPACING,
            cells: BTreeMap::new(),
            points: Vec::new(),
        }
    }

    fn key(&self, x: f32, z: f32) -> (i32, i32) {
        ((x / self.cell).floor() as i32, (z / self.cell).floor() as i32)
    }

    fn any_within(&self, x: f32, z: f32, dist: f32) -> bool {
        let r = (dist / self.cell).ceil().max(0.0) as i32;
        let (cx, cz) = self.key(x, z);
        for dx in -r..=r {
            for dz in -r..=r {
                if let Some(list) = self.cells.get(&(cx + dx, cz + dz)) {
                    for &i in list {
                        let p = &self.points[i];
                        let ddx = p.x - x;
                        let ddz = p.z - z;
                        if ddx * ddx + ddz * ddz < dist * dist {
                            return true;
                        }
                    }
                }
            }
        }
        false
    }

    fn insert(&mut self, p: Vec3) {
        let idx = self.points.len();
        self.points.push(p);
        let (cx, cz) = self.key(p.x, p.z);
        self.cells.entry((cx, cz)).or_default().push(idx);
    }
}

/// Finds a platform whose top is at `top_y` (within EPS) containing `(x, z)` inset by `inset`.
fn platform_top_under(platforms: &[Platform], x: f32, z: f32, top_y: f32, inset: f32) -> Option<usize> {
    for (i, p) in platforms.iter().enumerate() {
        if (p.bounds.max.y - top_y).abs() > EPS {
            continue;
        }
        if x < p.bounds.min.x + inset - EPS
            || x > p.bounds.max.x - inset + EPS
            || z < p.bounds.min.z + inset - EPS
            || z > p.bounds.max.z - inset + EPS
        {
            continue;
        }
        return Some(i);
    }
    None
}

// ------------------------------------------------------- placement ----

/// An open socket on a placed chunk, in world coordinates.
#[derive(Debug, Clone, Copy)]
pub struct OpenSocket {
    /// World-space position of the socket (on the footprint edge it faces).
    pub pos: Vec3,
    /// World-space facing after rotation.
    pub facing: Facing,
    /// Clear width of the opening.
    pub width: f32,
    /// Number of failed trial rounds used to detect dead sockets.

// ---- lines 1230-1400: gift filling (accept_candidate, fill_gifts) ----
        }
    }
    Some(stubs)
}

/// Result of gift filling.
struct GiftFill {
    gifts: Vec<Gift>,
}

/// Accepts a gift candidate iff it stands on a NORMAL platform top (edge inset >= 0.5 u),
/// is >= 2.0 u from other gifts horizontally and >= 2.5 u from the beacon.
#[allow(clippy::too_many_arguments)]
fn accept_candidate(
    platforms: &[Platform],
    grid: &mut PointGrid,
    beacon: &Vec3,
    pos: Vec3,
    elevated: bool,
    gifts: &mut Vec<Gift>,
    elevated_flags: &mut Vec<bool>,
) -> bool {
    let top = pos.y - 0.3;
    let Some(i) = platform_top_under(platforms, pos.x, pos.z, top, 0.5) else {
        return false;
    };
    if platforms[i].kind != PLATFORM_NORMAL {
        return false;
    }
    if grid.any_within(pos.x, pos.z, GIFT_MIN_SPACING - EPS) {
        return false;
    }
    if h_dist(pos.x, pos.z, beacon.x, beacon.z) < GIFT_BEACON_CLEAR - EPS {
        return false;
    }
    let y = platforms[i].bounds.max.y + 0.3;
    grid.insert(Vec3::new(pos.x, y, pos.z));
    gifts.push(Gift {
        id: gifts.len() as u16,
        pos: Vec3::new(pos.x, y, pos.z),
        kind: GIFT_NORMAL,
    });
    elevated_flags.push(elevated);
    true
}

/// A3: fill up to `min(gift_target(level, players), eligible_area / 9)` gifts from chunk
/// slots (>= 20% elevated preferred first) then top up on random eligible platform tops.
fn fill_gifts(
    platforms: &[Platform],
    slots: &[(Vec3, bool)],
    beacon: &Vec3,
    level: u32,
    players: u32,
    rng: &mut Rng,
) -> GiftFill {
    let mut gifts = Vec::new();
    let mut elevated_flags: Vec<bool> = Vec::new();
    let mut grid = PointGrid::new();
    let eligible_area: f32 = platforms
        .iter()
        .filter(|p| p.kind == PLATFORM_NORMAL)
        .map(|p| aabb_area(&p.bounds))
        .sum();
    let cap = ((eligible_area + 1e-3) / GIFT_AREA_PER).floor().max(0.0) as u32;
    let target = gift_target(level, players).min(cap) as usize;
    let quota = ((target as f32) * GIFT_ELEVATED_QUOTA).ceil() as usize;
    let mut accepted_slot = vec![false; slots.len()];
    let mut order: Vec<usize> = (0..slots.len()).collect();
    rng.shuffle(&mut order);
    // Pass A: elevated slots first so the quota is met when slots exist.
    for &si in &order {
        let elevated_taken = elevated_flags.iter().filter(|&&e| e).count();
        if elevated_taken >= quota || gifts.len() >= target {
            break;
        }
        if !slots[si].1 || accepted_slot[si] {
            continue;
        }
        let (pos, elev) = slots[si];
        if accept_candidate(platforms, &mut grid, beacon, pos, elev, &mut gifts, &mut elevated_flags) {
            accepted_slot[si] = true;
        }
    }
    // Pass B: all remaining slots.
    for &si in &order {
        if gifts.len() >= target {
            break;
        }
        if accepted_slot[si] {
            continue;
        }
        let (pos, elev) = slots[si];
        if accept_candidate(platforms, &mut grid, beacon, pos, elev, &mut gifts, &mut elevated_flags) {
            accepted_slot[si] = true;
        }
    }
    // Pass C: top up on random eligible platform tops (area-weighted).
    let normal_idx: Vec<usize> = platforms
        .iter()
        .enumerate()
        .filter(|(_, p)| p.kind == PLATFORM_NORMAL)
        .map(|(i, _)| i)
        .collect();
    let mut cum: Vec<f32> = Vec::with_capacity(normal_idx.len());
    let mut acc = 0.0_f32;
    for &i in &normal_idx {
        acc += aabb_area(&platforms[i].bounds);
        cum.push(acc);
    }
    let mut attempts = 0usize;
    while gifts.len() < target && attempts < TOPUP_ATTEMPTS {
        attempts += 1;
        if normal_idx.is_empty() {
            break;
        }
        let idx = weighted_platform_index(rng, &normal_idx, &cum);
        let p = &platforms[idx];
        let min_x = p.bounds.min.x + 0.5;
        let max_x = p.bounds.max.x - 0.5;
        let min_z = p.bounds.min.z + 0.5;
        let max_z = p.bounds.max.z - 0.5;
        if min_x > max_x - EPS || min_z > max_z - EPS {
            continue;
        }
        let pos = Vec3::new(
            rng.range_f32(min_x, max_x),
            p.bounds.max.y + 0.3,
            rng.range_f32(min_z, max_z),
        );
        accept_candidate(
            platforms,
            &mut grid,
            beacon,
            pos,
            false,
            &mut gifts,
            &mut elevated_flags,
        );
    }
    GiftFill { gifts }
}

/// True when `pos` is >= 3.0 u (horizontal) from every gift, the beacon, the spawn and the enemy slots.
fn altar_site_clear(pos: &Vec3, gifts: &[Gift], spawn: &Vec3, beacon: &Vec3, enemy_slots: &[Vec3]) -> bool {
    if h_dist(pos.x, pos.z, spawn.x, spawn.z) < 3.0 - EPS {
        return false;
    }
    if h_dist(pos.x, pos.z, beacon.x, beacon.z) < 3.0 - EPS {
        return false;
    }
    for g in gifts {
        if h_dist(pos.x, pos.z, g.pos.x, g.pos.z) < 3.0 - EPS {
            return false;
        }
    }
    for s in enemy_slots {
        if h_dist(pos.x, pos.z, s.x, s.z) < 3.0 - EPS {
            return false;
        }
    }
    true
}

/// Legacy altar rules: level >= 3, deterministic 50% roll, placed on an altar slot of a
/// NORMAL platform top, >= 3.0 u from spawn/beacon/gifts/enemy slots. At most one altar.
/// Catalog slots are tried first; both common slot conventions (pos.y = platform top and
/// pos.y = platform top + 0.3) are accepted. When no slot yields a site, the altar is placed
/// on the centre of a random eligible NORMAL platform (shuffled, deterministic) so catalogues
/// with few or no altar slots still get the legacy ~50% frequency.
#[allow(clippy::too_many_arguments)]

// ---- lines 1910-1930: generate / generate_for ----
    }
}

/// A fresh, completable level. Same `(seed, level)` -> identical output.
pub fn generate(seed: u32, level: u32) -> GeneratedLevel {
    generate_for(seed, level, 1)
}

/// A fresh level for a player count (gift counts scale with players, amendment A3).
pub fn generate_for(seed: u32, level: u32, players: u32) -> GeneratedLevel {
    match catalog() {
        Ok(cat) => generate_with_catalog(cat, seed, level, players),
        Err(_) => builtin_fallback(seed, level, players),
    }
}

// ------------------------------------------------------ validation ----

/// Design hop envelope (edge-to-edge), by landing rise; `None` = not jumpable.
/// Plain (non-pad) jumps may climb at most +2.0 u: the apex of the jump is exactly
/// +2.25 u (12^2 / (2*32)), so a +2.25 u rise can never be cleared with room to spare.
