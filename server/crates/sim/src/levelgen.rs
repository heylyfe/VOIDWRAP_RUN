#![allow(clippy::needless_range_loop)] // index loops over parallel arrays read clearer here

//! Chunk-based level assembler — massive looping maps built from hand-designed chunks.
//!
//! Reads the shared chunk catalog (`design/chunks.json`, format `design/CHUNK_FORMAT.md`),
//! places a hub at the origin and grows a massive network of walkway chunks from its open
//! sockets: budgeted random walk with 90 degree rotations, per-chunk weighting, junction cap,
//! overlap and >= 4.0 u stacking-clearance rejection (via a coarse 8 u XZ grid), then
//! loop-closing with generated 3.5 u straight bridges between facing open sockets, then
//! dead-end sealing (or a railing-less 3 u stub so no walkway ends in an unexplained cliff).
//! Gifts follow amendment A3 (min(400, 46 + 10*(level-1) + 6*(players-1)), at most one per
//! 9 u^2 of eligible top), altars per the legacy rules (level >= 3, 50% roll, altar slots),
//! enemy spawns are the hub's 8 slots, and the RNG is xorshift64* seeded from
//! `seed_mix(run_seed, level)`. Generation is deterministic for a given `(seed, level, players)`.
//!
//! [`validate`] performs structural checks plus reachability over the platform graph
//! (touching with step <= 0.55 u, jumps inside the design hop envelope edge-to-edge, jump
//! pads up to +4.0 u within 5.0 u): every gift, altar and the beacon must be reachable from
//! the spawn, and the beacon must be reachable from everywhere reachable (no traps). If a
//! candidate fails validation the next attempt seed is tried (<= 40 attempts), after which a
//! guaranteed-valid fallback map (hub + straight arms) is returned.

use crate::world::{is_ramp, surface_at};
use protocol::{
    Aabb, Altar, EVENT_CRUMBLE, EVENT_HIGH_RISE, EVENT_ICE, EVENT_NONE, GIFT_GOLDEN, GIFT_NORMAL, Gift, LevelData,
    PLATFORM_CRUMBLE, PLATFORM_JUMP_PAD, PLATFORM_NORMAL, Platform, Vec3,
};
use serde::Deserialize;
use std::collections::{BTreeMap, VecDeque};
use std::sync::OnceLock;

const EPS: f32 = 1e-3;
const GRID_CELL: f32 = 8.0;
const STACK_CLEARANCE: f32 = 4.0;
const SOCKET_DY_TOL: f32 = 0.5;
const LOOP_BRIDGE_MAX_DIST: f32 = 12.0;
const LOOP_BRIDGE_PERP_TOL: f32 = 0.75;
const BRIDGE_WIDTH: f32 = 3.5;
const STUB_LENGTH: f32 = 3.0;
const STUB_WIDTH: f32 = 3.0;
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
/// Patterns authored by T-070. Merged into the catalog at load time.
const PATTERNS_A_JSON: &str = include_str!("../../../../design/patterns-a.json");
/// Patterns authored by T-071. Merged into the catalog at load time.
const PATTERNS_B_JSON: &str = include_str!("../../../../design/patterns-b.json");

/// Output of a successful generation pass.
#[derive(Debug, Clone)]
pub struct GeneratedLevel {
    pub data: LevelData,
    pub enemy_spawns: Vec<Vec3>,
    /// The T-069 composition plan this level was built from, in placement order.
    /// Empty when the catalog carries no pattern tags (the walk then picks freely)
    /// or when the level came from a fallback.
    pub plan: Vec<String>,
}

// ---------------------------------------------------------------- PRNG ----

/// xorshift64* — the assembly PRNG (design 4.3, step 1).
struct Rng {
    s: u64,
}

impl Rng {
    fn new(seed: u64) -> Self {
        Self {
            s: if seed == 0 { 0x9E37_79B9_7F4A_7C15 } else { seed },
        }
    }

    fn next_u64(&mut self) -> u64 {
        let mut x = self.s;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.s = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    fn f32(&mut self) -> f32 {
        (self.next_u64() >> 40) as f32 / 16_777_216.0
    }

    fn below(&mut self, n: usize) -> usize {
        if n == 0 {
            0
        } else {
            (self.next_u64() % n as u64) as usize
        }
    }

    fn range_f32(&mut self, lo: f32, hi: f32) -> f32 {
        lo + (hi - lo) * self.f32()
    }

    fn shuffle<T>(&mut self, v: &mut [T]) {
        for i in (1..v.len()).rev() {
            let j = self.below(i + 1);
            v.swap(i, j);
        }
    }
}

fn splitmix32(x: u32) -> u32 {
    let mut z = x.wrapping_add(0x9E37_79B9);
    z = (z ^ (z >> 16)).wrapping_mul(0x21F0_AAAD);
    z = (z ^ (z >> 15)).wrapping_mul(0x735A_2D97);
    z ^ (z >> 15)
}

/// SplitMix64 mix of a run seed and level into the 64-bit assembly seed.
fn seed_mix(seed: u32, level: u32) -> u64 {
    let mut z = ((seed as u64) << 32) | u64::from(level);
    z = z.wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// The RNG seed used for the nth regeneration attempt.
fn attempt_seed(run_seed: u32, attempt: u32) -> u32 {
    if attempt == 0 {
        run_seed
    } else {
        splitmix32(run_seed.wrapping_add(attempt.wrapping_mul(0x9E37_79B9)))
    }
}

// ------------------------------------------------------------ catalog ----

/// Role of a chunk in assembly.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChunkRole {
    /// Spawn hub; exactly one placement per level.
    Hub,
    /// Regular path/connector chunk.
    Path,
    /// Junction chunk; counts towards the `4 + level` cap.
    Junction,
    /// Dead-end pocket; used to seal open sockets.
    DeadEnd,
}

impl ChunkRole {
    fn parse(s: &str) -> Option<Self> {
        match s {
            "hub" => Some(ChunkRole::Hub),
            "path" => Some(ChunkRole::Path),
            "junction" => Some(ChunkRole::Junction),
            "dead_end" => Some(ChunkRole::DeadEnd),
            _ => None,
        }
    }
}

/// Cardinal facing of a socket. N = +Z, E = +X, S = -Z, W = -X (out of the chunk).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Facing {
    /// +Z.
    N,
    /// +X.
    E,
    /// -Z.
    S,
    /// -X.
    W,
}

impl Facing {
    fn parse(s: &str) -> Option<Self> {
        match s {
            "N" => Some(Facing::N),
            "E" => Some(Facing::E),
            "S" => Some(Facing::S),
            "W" => Some(Facing::W),
            _ => None,
        }
    }

    /// Unit XZ vector of the facing.
    fn vec(self) -> (f32, f32) {
        match self {
            Facing::N => (0.0, 1.0),
            Facing::E => (1.0, 0.0),
            Facing::S => (0.0, -1.0),
            Facing::W => (-1.0, 0.0),
        }
    }

    /// The opposite facing (required for two sockets to join).
    fn opposite(self) -> Self {
        match self {
            Facing::N => Facing::S,
            Facing::E => Facing::W,
            Facing::S => Facing::N,
            Facing::W => Facing::E,
        }
    }

    /// Rotates CCW (seen from above) by `turns` quarter-turns.
    fn turn(self, turns: u32) -> Self {
        const ORDER: [Facing; 4] = [Facing::N, Facing::E, Facing::S, Facing::W];
        let base = match self {
            Facing::N => 0usize,
            Facing::E => 1,
            Facing::S => 2,
            Facing::W => 3,
        };
        ORDER[(base + turns as usize) % 4]
    }
}

/// One platform box as authored in the catalog (kind: PLATFORM_*).
#[derive(Debug, Clone, PartialEq)]
pub struct ChunkPlatform {
    pub kind: u8,
    pub min: Vec3,
    pub max: Vec3,
}

/// A socket on a chunk edge; `pos` lies on the footprint edge it faces.
#[derive(Debug, Clone, PartialEq)]
pub struct ChunkSocket {
    pub id: String,
    pub pos: Vec3,
    pub facing: Facing,
    pub width: f32,
}

/// A gift slot in chunk-local coordinates (`pos.y = platform top + 0.3`).
#[derive(Debug, Clone, PartialEq)]
pub struct ChunkGiftSlot {
    pub pos: Vec3,
    pub elevated: bool,
}

/// One chunk definition (CHUNK_FORMAT.md).
#[derive(Debug, Clone, PartialEq)]
pub struct Chunk {
    pub id: String,
    pub name: String,
    pub role: ChunkRole,
    pub min_level: u32,
    pub difficulty: u32,
    pub tags: Vec<String>,
    pub platforms: Vec<ChunkPlatform>,
    pub sockets: Vec<ChunkSocket>,
    pub gift_slots: Vec<ChunkGiftSlot>,
    pub enemy_slots: Vec<Vec3>,
    pub altar_slots: Vec<Vec3>,
    /// Hub only: beacon dais top-centre.
    pub beacon: Option<Vec3>,
}

/// A parsed and validated chunk catalog.
#[derive(Debug, Clone, PartialEq)]
pub struct Catalog {
    pub version: u32,
    pub chunks: Vec<Chunk>,
}

#[derive(Deserialize)]
struct RawCatalog {
    version: u32,
    chunks: Vec<RawChunk>,
}

#[derive(Deserialize)]
struct RawChunk {
    id: String,
    name: String,
    role: String,
    min_level: u32,
    difficulty: u32,
    #[serde(default)]
    tags: Vec<String>,
    platforms: Vec<RawPlatform>,
    #[serde(default)]
    sockets: Vec<RawSocket>,
    #[serde(default)]
    gift_slots: Vec<RawGiftSlot>,
    #[serde(default)]
    enemy_slots: Vec<[f32; 3]>,
    #[serde(default)]
    altar_slots: Vec<[f32; 3]>,
    #[serde(default)]
    beacon: Option<[f32; 3]>,
}

#[derive(Deserialize)]
struct RawPlatform {
    kind: String,
    min: [f32; 3],
    max: [f32; 3],
}

#[derive(Deserialize)]
struct RawSocket {
    id: String,
    pos: [f32; 3],
    facing: String,
    width: f32,
}

#[derive(Deserialize)]
struct RawGiftSlot {
    pos: [f32; 3],
    #[serde(default)]
    elevated: bool,
}

fn vec3(v: [f32; 3]) -> Vec3 {
    Vec3::new(v[0], v[1], v[2])
}

/// Map a level-file platform kind name to its `PLATFORM_*` constant.
///
/// Recognised names are `NORMAL`, `CRUMBLE`, `JUMP_PAD`, `TRAMPOLINE`, `RAMP_PX`,
/// `RAMP_NX`, `RAMP_PZ` and `RAMP_NZ`. Anything else falls back to `PLATFORM_NORMAL`.
fn platform_kind_from_str(name: &str) -> Option<u8> {
    match name {
        "CRUMBLE" => Some(protocol::PLATFORM_CRUMBLE),
        "JUMP_PAD" => Some(protocol::PLATFORM_JUMP_PAD),
        "TRAMPOLINE" => Some(protocol::PLATFORM_TRAMPOLINE),
        "RAMP_PX" => Some(protocol::PLATFORM_RAMP_PX),
        "RAMP_NX" => Some(protocol::PLATFORM_RAMP_NX),
        "RAMP_PZ" => Some(protocol::PLATFORM_RAMP_PZ),
        "RAMP_NZ" => Some(protocol::PLATFORM_RAMP_NZ),
        "NORMAL" => Some(protocol::PLATFORM_NORMAL),
        _ => None,
    }
}

fn footprint(platforms: &[ChunkPlatform]) -> Aabb {
    let mut min = Vec3::new(f32::INFINITY, f32::INFINITY, f32::INFINITY);
    let mut max = Vec3::new(f32::NEG_INFINITY, f32::NEG_INFINITY, f32::NEG_INFINITY);
    for p in platforms {
        min.x = min.x.min(p.min.x);
        min.y = min.y.min(p.min.y);
        min.z = min.z.min(p.min.z);
        max.x = max.x.max(p.max.x);
        max.y = max.y.max(p.max.y);
        max.z = max.z.max(p.max.z);
    }
    Aabb { min, max }
}

/// CHUNK_FORMAT: a socket lies on the footprint edge it faces and a platform at its
/// walking height touches that edge across the full socket width.
#[allow(clippy::too_many_arguments)]
fn verify_socket(
    chunk_id: &str,
    socket_id: &str,
    platforms: &[ChunkPlatform],
    fp: &Aabb,
    pos: &Vec3,
    facing: Facing,
    width: f32,
) -> Result<(), String> {
    let (in_plane, edge) = match facing {
        Facing::N => (pos.z, fp.max.z),
        Facing::S => (pos.z, fp.min.z),
        Facing::E => (pos.x, fp.max.x),
        Facing::W => (pos.x, fp.min.x),
    };
    if (in_plane - edge).abs() > EPS {
        return Err(format!(
            "chunk '{chunk_id}' socket '{socket_id}' is not on the footprint edge it faces"
        ));
    }
    let (perp, lo, hi) = match facing {
        Facing::N | Facing::S => (pos.x, fp.min.x, fp.max.x),
        Facing::E | Facing::W => (pos.z, fp.min.z, fp.max.z),
    };
    if perp < lo - EPS || perp > hi + EPS {
        return Err(format!(
            "chunk '{chunk_id}' socket '{socket_id}' is outside the footprint"
        ));
    }
    let mut covered = false;
    for p in platforms {
        if (p.max.y - pos.y).abs() > EPS {
            continue;
        }
        let touches = match facing {
            Facing::N => (p.max.z - pos.z).abs() <= EPS,
            Facing::S => (p.min.z - pos.z).abs() <= EPS,
            Facing::E => (p.max.x - pos.x).abs() <= EPS,
            Facing::W => (p.min.x - pos.x).abs() <= EPS,
        };
        if !touches {
            continue;
        }
        let (plo, phi) = match facing {
            Facing::N | Facing::S => (p.min.x, p.max.x),
            Facing::E | Facing::W => (p.min.z, p.max.z),
        };
        if plo <= perp - width * 0.5 + EPS && phi >= perp + width * 0.5 - EPS {
            covered = true;
            break;
        }
    }
    if !covered {
        return Err(format!(
            "chunk '{chunk_id}' socket '{socket_id}' has no platform at its walking height touching the edge across its width"
        ));
    }
    Ok(())
}

/// Parses and validates a chunk catalog. Malformed input returns `Err`, never a panic.
pub fn parse_catalog(json: &str) -> Result<Catalog, String> {
    let raw: RawCatalog = serde_json::from_str(json).map_err(|e| format!("invalid catalog JSON: {e}"))?;
    if raw.version != 1 {
        return Err(format!("unsupported catalog version {}", raw.version));
    }
    let mut chunks = Vec::with_capacity(raw.chunks.len());
    let mut seen: Vec<&str> = Vec::with_capacity(raw.chunks.len());
    for rc in &raw.chunks {
        if rc.id.is_empty()
            || !rc
                .id
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
        {
            return Err(format!("chunk id '{}' is not [a-z0-9_]", rc.id));
        }
        if seen.contains(&rc.id.as_str()) {
            return Err(format!("duplicate chunk id '{}'", rc.id));
        }
        seen.push(rc.id.as_str());
        let mut role =
            ChunkRole::parse(&rc.role).ok_or_else(|| format!("chunk '{}' has unknown role '{}'", rc.id, rc.role))?;
        // A "hub" without a beacon or a full enemy ring cannot be the spawn hub; it still makes a
        // fine wide crossroads, so treat it as a junction instead of rejecting the whole catalog.
        if role == ChunkRole::Hub && (rc.beacon.is_none() || rc.enemy_slots.len() != 8) {
            role = ChunkRole::Junction;
        }
        if rc.difficulty < 1 || rc.difficulty > 3 {
            return Err(format!("chunk '{}' difficulty must be 1..=3", rc.id));
        }
        if rc.min_level < 1 {
            return Err(format!("chunk '{}' min_level must be >= 1", rc.id));
        }
        if rc.platforms.is_empty() {
            return Err(format!("chunk '{}' has no platforms", rc.id));
        }
        let mut platforms = Vec::with_capacity(rc.platforms.len());
        for p in &rc.platforms {
            let kind = platform_kind_from_str(&p.kind)
                .ok_or_else(|| format!("chunk '{}' has unknown platform kind '{}'", rc.id, p.kind))?;
            let min = vec3(p.min);
            let max = vec3(p.max);
            if !(min.x < max.x && min.y < max.y && min.z < max.z) {
                return Err(format!("chunk '{}' has an empty platform box", rc.id));
            }
            platforms.push(ChunkPlatform { kind, min, max });
        }
        let fp = footprint(&platforms);
        let mut sockets = Vec::with_capacity(rc.sockets.len());
        for s in &rc.sockets {
            let facing = Facing::parse(&s.facing)
                .ok_or_else(|| format!("chunk '{}' socket '{}' has unknown facing", rc.id, s.id))?;
            if !s.width.is_finite() || s.width < 3.0 {
                return Err(format!("chunk '{}' socket '{}' width must be >= 3.0", rc.id, s.id));
            }
            let pos = vec3(s.pos);
            verify_socket(&rc.id, &s.id, &platforms, &fp, &pos, facing, s.width)?;
            sockets.push(ChunkSocket {
                id: s.id.clone(),
                pos,
                facing,
                width: s.width,
            });
        }
        if role == ChunkRole::Hub {
            if rc.beacon.is_none() {
                return Err(format!("hub chunk '{}' has no beacon point", rc.id));
            }
            if rc.enemy_slots.len() != 8 {
                return Err(format!(
                    "hub chunk '{}' must have exactly 8 enemy slots, found {}",
                    rc.id,
                    rc.enemy_slots.len()
                ));
            }
        } else {
            if rc.beacon.is_some() {
                return Err(format!("chunk '{}' is not a hub but has a beacon point", rc.id));
            }
            if !rc.enemy_slots.is_empty() {
                return Err(format!("chunk '{}' is not a hub but has enemy slots", rc.id));
            }
        }
        chunks.push(Chunk {
            id: rc.id.clone(),
            name: rc.name.clone(),
            role,
            min_level: rc.min_level,
            difficulty: rc.difficulty,
            tags: rc.tags.clone(),
            platforms,
            sockets,
            gift_slots: rc
                .gift_slots
                .iter()
                .map(|g| ChunkGiftSlot {
                    pos: vec3(g.pos),
                    elevated: g.elevated,
                })
                .collect(),
            enemy_slots: rc.enemy_slots.iter().map(|&s| vec3(s)).collect(),
            altar_slots: rc.altar_slots.iter().map(|&s| vec3(s)).collect(),
            beacon: rc.beacon.map(vec3),
        });
    }
    if !chunks.iter().any(|c| c.role == ChunkRole::Hub) {
        return Err("catalog has no hub chunk".to_string());
    }
    Ok(Catalog { version: 1, chunks })
}

/// Parses the pattern metadata of every chunk in `cat`, in catalog order.
///
/// The returned vector is index-aligned with `cat.chunks`: `pattern_metas(cat)[i]`
/// describes `cat.chunks[i]`. That alignment is what lets a
/// [`crate::pattern::CompositionPlan`] store catalog indices instead of ids.
pub fn pattern_metas(cat: &Catalog) -> Vec<crate::pattern::PatternMeta> {
    cat.chunks
        .iter()
        .map(|c| crate::pattern::PatternMeta::from_tags(&c.id, c.role, c.min_level, c.difficulty as u8, &c.tags))
        .collect()
}

/// Error returned by [`merge_catalog_json`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CatalogError {
    /// A source was not valid JSON.
    Json(String),
    /// A source did not have the `{ "version": 1, "chunks" | "patterns": [...] }` shape.
    Shape,
    /// Two sources declared the same pattern id.
    DuplicateId(String),
}

impl std::fmt::Display for CatalogError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CatalogError::Json(message) => write!(f, "catalog source is not valid JSON: {message}"),
            CatalogError::Shape => write!(
                f,
                "catalog source must be {{ \"version\": 1, \"chunks\" | \"patterns\": [ ... ] }}"
            ),
            CatalogError::DuplicateId(id) => write!(f, "duplicate pattern id: {id}"),
        }
    }
}

impl std::error::Error for CatalogError {}

/// Merges the three catalog files into one `{ "version": 1, "chunks": [...] }`
/// document that the existing catalog parser can consume unchanged.
///
/// `sources` holds the raw text of `design/chunks.json`, `design/patterns-a.json`
/// and `design/patterns-b.json`, in that order. Both key names (`chunks` and
/// `patterns`) are accepted in any source, empty sources are skipped, and a
/// duplicate `id` across sources is an error — that is the check
/// `validate-content` reports.
pub fn merge_catalog_json(sources: &[&str]) -> Result<String, CatalogError> {
    let mut merged: Vec<serde_json::Value> = Vec::new();
    let mut seen: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    for source in sources {
        if source.trim().is_empty() {
            continue;
        }
        let root: serde_json::Value = serde_json::from_str(source).map_err(|e| CatalogError::Json(e.to_string()))?;
        let entries = root
            .get("chunks")
            .or_else(|| root.get("patterns"))
            .and_then(serde_json::Value::as_array)
            .ok_or(CatalogError::Shape)?;
        for entry in entries {
            let id = entry
                .get("id")
                .and_then(serde_json::Value::as_str)
                .ok_or(CatalogError::Shape)?
                .to_string();
            if !seen.insert(id.clone()) {
                return Err(CatalogError::DuplicateId(id));
            }
            merged.push(entry.clone());
        }
    }
    let mut root = serde_json::Map::new();
    root.insert("version".to_string(), serde_json::Value::from(1));
    root.insert("chunks".to_string(), serde_json::Value::Array(merged));
    serde_json::to_string(&serde_json::Value::Object(root)).map_err(|e| CatalogError::Json(e.to_string()))
}

/// The shared catalog, parsed once from the three catalog files merged into one
/// document. A malformed catalog degrades generation to the built-in fallback map
/// instead of panicking.
pub fn catalog() -> Result<&'static Catalog, String> {
    static CATALOG: OnceLock<Result<Catalog, String>> = OnceLock::new();
    CATALOG
        .get_or_init(|| {
            let merged = merge_catalog_json(&[CHUNK_CATALOG_JSON, PATTERNS_A_JSON, PATTERNS_B_JSON])
                .map_err(|e| e.to_string())?;
            parse_catalog_lenient(&merged)
        })
        .as_ref()
        .map_err(|e| e.clone())
}

/// Like [`parse_catalog`], but a single broken chunk does not take the whole catalog down: the
/// chunk named in the error is dropped and parsing is retried. (`tools/validate-chunks.mjs` and the
/// strict tests still reject a broken catalog; this only keeps a shipped game playable.)
pub fn parse_catalog_lenient(json: &str) -> Result<Catalog, String> {
    let mut value: serde_json::Value = serde_json::from_str(json).map_err(|e| format!("invalid catalog JSON: {e}"))?;
    for _ in 0..64 {
        let text = value.to_string();
        let err = match parse_catalog(&text) {
            Ok(catalog) => return Ok(catalog),
            Err(err) => err,
        };
        let Some(list) = value.get_mut("chunks").and_then(|c| c.as_array_mut()) else {
            return Err(err);
        };
        let culprit = list.iter().position(|c| {
            c.get("id")
                .and_then(|id| id.as_str())
                .is_some_and(|id| err.contains(&format!("'{id}'")))
        });
        match culprit {
            Some(i) => {
                list.remove(i);
            }
            None => return Err(err),
        }
    }
    Err("catalog: too many broken chunks".to_string())
}

// ------------------------------------------------------------- math ----

/// Rotates a point in XZ by `turns` quarter-turns CCW seen from above.
fn rotate_xz(x: f32, z: f32, turns: u32) -> (f32, f32) {
    let mut px = x;
    let mut pz = z;
    for _ in 0..(turns % 4) {
        let nx = pz;
        let nz = -px;
        px = nx;
        pz = nz;
    }
    (px, pz)
}

fn rotate_vec3(p: Vec3, turns: u32) -> Vec3 {
    let (x, z) = rotate_xz(p.x, p.z, turns);
    Vec3::new(x, p.y, z)
}

/// Rotates a ramp kind by `turns` quarter-turns so the kind keeps describing the
/// direction the surface actually rises in.
///
/// [`rotate_xz`] maps `(x, z) -> (z, -x)`, so one turn sends +X to -Z, -Z to -X,
/// -X to +Z and +Z to +X. A ramp whose kind is not remapped keeps its old slope
/// axis while its geometry turns, which makes the collision surface disagree with
/// the visible wedge: the player walks on a slope that is not there.
fn rotate_ramp_kind(kind: u8, turns: u32) -> u8 {
    let mut k = kind;
    for _ in 0..(turns % 4) {
        k = match k {
            protocol::PLATFORM_RAMP_PX => protocol::PLATFORM_RAMP_NZ,
            protocol::PLATFORM_RAMP_NZ => protocol::PLATFORM_RAMP_NX,
            protocol::PLATFORM_RAMP_NX => protocol::PLATFORM_RAMP_PZ,
            protocol::PLATFORM_RAMP_PZ => protocol::PLATFORM_RAMP_PX,
            other => other,
        };
    }
    k
}

fn rotate_aabb(b: &Aabb, turns: u32) -> Aabb {
    let corners = [
        (b.min.x, b.min.z),
        (b.max.x, b.min.z),
        (b.max.x, b.max.z),
        (b.min.x, b.max.z),
    ];
    let mut min_x = f32::INFINITY;
    let mut max_x = f32::NEG_INFINITY;
    let mut min_z = f32::INFINITY;
    let mut max_z = f32::NEG_INFINITY;
    for (cx, cz) in corners {
        let (rx, rz) = rotate_xz(cx, cz, turns);
        min_x = min_x.min(rx);
        max_x = max_x.max(rx);
        min_z = min_z.min(rz);
        max_z = max_z.max(rz);
    }
    Aabb {
        min: Vec3::new(min_x, b.min.y, min_z),
        max: Vec3::new(max_x, b.max.y, max_z),
    }
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
        // Resting contact (a dais on a floor, a ramp foot on its landing) is fine; only a
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
        if (surface_at(p, x, z) - top_y).abs() > EPS {
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
    pub failures: u8,
}

/// A positioned copy of a chunk, ready to commit (world coordinates).
#[derive(Debug, Clone)]
pub struct Placement {
    /// Chunk role (stat aggregation only).
    pub role: ChunkRole,
    /// Kind and world bounds of each platform in placement order.
    pub platforms: Vec<(u8, Aabb)>,
    /// Open sockets in world coordinates.
    pub sockets: Vec<OpenSocket>,
    /// Gift slot positions (world) and elevated flags.
    pub gift_slots: Vec<(Vec3, bool)>,
    /// Altar slot positions (world).
    pub altar_slots: Vec<Vec3>,
    /// Hub beacon (hubs only).
    pub beacon: Option<Vec3>,
    /// Enemy spawn points (world).
    pub enemy_slots: Vec<Vec3>,
    /// Sum of the platform top areas (XZ).
    pub top_area: f32,
}

/// Builds a rotated + translated placement of `chunk` with offset `offset`.
/// How far a ramp's high edge may sit from the surface it should meet, in world units.
/// A ramp is a wedge: its top face ends at `bounds.max.y` on the high edge, and the
/// platform it leads onto must be at that height or the player steps into a wall.
pub const RAMP_ANCHOR_TOLERANCE: f32 = 0.55;

/// How far past a platform's footprint the ramp's high edge may sit and still count as
/// meeting it. Small: a ramp that stops short of the deck is a gap, not a join.
pub const RAMP_ANCHOR_OVERLAP: f32 = 0.25;

/// Indices of ramps whose high edge meets nothing.
///
/// A ramp is a connector. When its high edge lands in open air the wedge rises into
/// nothing: the player walks up and falls off the end, and from the ground it reads as a
/// ramp that was placed at random. That is the "random ramps" the owner sees, and it is a
/// generation defect rather than a pattern-authoring choice, so the harness asserts it is
/// zero rather than tolerating a rate.
///
/// The check uses [`surface_at`], so a ramp that leads onto another ramp is anchored when
/// the two surfaces actually meet, not merely when their boxes touch.
pub fn orphaned_ramps(platforms: &[protocol::Platform]) -> Vec<usize> {
    let mut out = Vec::new();
    for (i, ramp) in platforms.iter().enumerate() {
        if !is_ramp(ramp.kind) {
            continue;
        }
        let b = &ramp.bounds;
        // The midpoint of the high edge: the point the ramp is trying to reach.
        let (hx, hz) = match ramp.kind {
            protocol::PLATFORM_RAMP_PX => (b.max.x, (b.min.z + b.max.z) * 0.5),
            protocol::PLATFORM_RAMP_NX => (b.min.x, (b.min.z + b.max.z) * 0.5),
            protocol::PLATFORM_RAMP_PZ => ((b.min.x + b.max.x) * 0.5, b.max.z),
            _ => ((b.min.x + b.max.x) * 0.5, b.min.z),
        };
        let anchored = platforms.iter().enumerate().any(|(j, other)| {
            if j == i {
                return false;
            }
            let ob = &other.bounds;
            let covers = hx >= ob.min.x - RAMP_ANCHOR_OVERLAP
                && hx <= ob.max.x + RAMP_ANCHOR_OVERLAP
                && hz >= ob.min.z - RAMP_ANCHOR_OVERLAP
                && hz <= ob.max.z + RAMP_ANCHOR_OVERLAP;
            covers && (surface_at(other, hx, hz) - b.max.y).abs() <= RAMP_ANCHOR_TOLERANCE
        });
        if !anchored {
            out.push(i);
        }
    }
    out
}

fn build_geometry(chunk: &Chunk, turns: u32, offset: Vec3) -> Placement {
    let platforms: Vec<(u8, Aabb)> = chunk
        .platforms
        .iter()
        .map(|p| {
            (
                rotate_ramp_kind(p.kind, turns),
                translate_aabb(&rotate_aabb(&Aabb { min: p.min, max: p.max }, turns), offset),
            )
        })
        .collect();
    let sockets: Vec<OpenSocket> = chunk
        .sockets
        .iter()
        .map(|s| {
            let rp = rotate_vec3(s.pos, turns);
            OpenSocket {
                pos: Vec3::new(rp.x + offset.x, rp.y + offset.y, rp.z + offset.z),
                facing: s.facing.turn(turns),
                width: s.width,
                failures: 0,
            }
        })
        .collect();
    let gift_slots: Vec<(Vec3, bool)> = chunk
        .gift_slots
        .iter()
        .map(|g| {
            let rp = rotate_vec3(g.pos, turns);
            (Vec3::new(rp.x + offset.x, rp.y + offset.y, rp.z + offset.z), g.elevated)
        })
        .collect();
    let altar_slots: Vec<Vec3> = chunk
        .altar_slots
        .iter()
        .map(|&a| {
            let rp = rotate_vec3(a, turns);
            Vec3::new(rp.x + offset.x, rp.y + offset.y, rp.z + offset.z)
        })
        .collect();
    let enemy_slots: Vec<Vec3> = chunk
        .enemy_slots
        .iter()
        .map(|&s| {
            let rp = rotate_vec3(s, turns);
            Vec3::new(rp.x + offset.x, rp.y + offset.y, rp.z + offset.z)
        })
        .collect();
    let beacon = chunk.beacon.map(|b| {
        let rp = rotate_vec3(b, turns);
        Vec3::new(rp.x + offset.x, rp.y + offset.y, rp.z + offset.z)
    });
    let top_area = platforms.iter().map(|(_, b)| aabb_area(b)).sum();
    Placement {
        role: chunk.role,
        platforms,
        sockets,
        gift_slots,
        altar_slots,
        beacon,
        enemy_slots,
        top_area,
    }
}

/// Aligns `chunk`'s socket `socket_idx` to `anchor` (opposite facings, |dy| <= 0.5),
/// translating the whole chunk so the two sockets coincide exactly.
///
/// Test hook: exposed so integration tests can check rotation/join math directly.
pub fn plan_join(chunk: &Chunk, socket_idx: usize, turns: u32, anchor: &OpenSocket) -> Option<Placement> {
    let s = chunk.sockets.get(socket_idx)?;
    if s.facing.turn(turns) != anchor.facing.opposite() {
        return None;
    }
    if (s.pos.y - anchor.pos.y).abs() > SOCKET_DY_TOL {
        return None;
    }
    let rp = rotate_vec3(s.pos, turns);
    let offset = Vec3::new(anchor.pos.x - rp.x, anchor.pos.y - rp.y, anchor.pos.z - rp.z);
    Some(build_geometry(chunk, turns, offset))
}

/// Index (into `plats`) of the chunk main platform: the largest XZ area, ties by file order.
fn main_platform_index(plats: &[(u8, Aabb)]) -> usize {
    let mut best = 0usize;
    let mut best_area = -1.0_f32;
    for (i, (_, b)) in plats.iter().enumerate() {
        let a = aabb_area(b);
        if a > best_area + EPS {
            best = i;
            best_area = a;
        }
    }
    best
}

/// Checks every candidate platform against placed platforms via the coarse grid.
fn placement_fits(p: &Placement, platforms: &[Platform], grid: &CoarseGrid) -> bool {
    for (_, b) in &p.platforms {
        for idx in grid.candidates(b) {
            if !platform_pair_ok(b, &platforms[idx].bounds) {
                return false;
            }
        }
    }
    true
}

/// Commits platforms; ids are assigned sequentially from `*next_id`.
fn commit_platforms(
    platforms: &mut Vec<Platform>,
    grid: &mut CoarseGrid,
    plats: Vec<(u8, Aabb)>,
    next_id: &mut u16,
) -> Option<()> {
    for (kind, bounds) in plats {
        let idx = platforms.len();
        grid.insert(idx, &bounds);
        platforms.push(Platform {
            id: *next_id,
            kind,
            bounds,
        });
        *next_id = next_id.checked_add(1)?;
    }
    Some(())
}

/// The level event for `(seed, level)`. Level 1 (and level 0 hub) are always EVENT_NONE.
/// For level >= 2, 35% of levels get an event, picked uniformly from the three.
pub fn event_for(seed: u32, level: u32) -> u8 {
    if level < 2 {
        return EVENT_NONE;
    }
    let h = splitmix32(seed ^ splitmix32(level));
    let roll = (h >> 8) as f32 / 16_777_216.0;
    if roll >= 0.35 {
        return EVENT_NONE;
    }
    match splitmix32(h) % 3 {
        0 => EVENT_HIGH_RISE,
        1 => EVENT_ICE,
        _ => EVENT_CRUMBLE,
    }
}

// ------------------------------------------------------- assembly ----

fn target_area(level: u32) -> f32 {
    (1400.0 + 260.0 * level as f32).min(6000.0)
}

fn max_chunks_for(level: u32) -> usize {
    (14 + 3 * level as usize).min(60)
}

fn max_junctions(level: u32) -> u32 {
    4 + level
}

fn level_tier(level: u32) -> u32 {
    (level / 2).clamp(1, 3)
}

/// A3 gift target: min(400, 46 + 10*(level-1) + 6*(players-1)).
pub fn gift_target(level: u32, players: u32) -> u32 {
    let p = players.max(1);
    (GIFT_COUNT_BASE + GIFT_COUNT_PER_LEVEL * level.saturating_sub(1) + GIFT_COUNT_PER_PLAYER * p.saturating_sub(1))
        .min(GIFT_CAP)
}

/// Weight for a chunk at a level: prefer lower difficulty, ×3 for climbing tags on HIGH_RISE.
fn chunk_weight(chunk: &Chunk, tier: u32, high_rise: bool) -> f32 {
    let over = chunk.difficulty as f32 - tier as f32;
    let mut w = 1.0 / (1.0 + over.max(0.0));
    if high_rise && chunk.tags.iter().any(|t| t == "climb" || t == "tower" || t == "lift") {
        w *= 3.0;
    }
    w
}

fn weighted_pick(rng: &mut Rng, weights: &[f32]) -> Option<usize> {
    let sum: f32 = weights.iter().copied().sum();
    if !sum.is_finite() || sum <= 0.0 {
        return None;
    }
    let mut roll = rng.f32() * sum;
    for (i, &w) in weights.iter().enumerate() {
        roll -= w;
        if roll <= 0.0 {
            return Some(i);
        }
    }
    Some(weights.len().saturating_sub(1))
}

fn weighted_platform_index(rng: &mut Rng, normal: &[usize], cum: &[f32]) -> usize {
    if normal.is_empty() {
        return 0;
    }
    let total = cum.last().copied().unwrap_or(1.0).max(1e-6);
    let roll = rng.f32() * total;
    let pos = cum.partition_point(|&c| c < roll);
    normal[pos.min(normal.len() - 1)]
}

/// Picks the spawn point on the hub main platform: the candidate among {centre + 4 inset
/// corners} farthest from the beacon (horizontal distance).
fn spawn_point(main: &Aabb, beacon: &Vec3) -> Vec3 {
    let cx = (main.min.x + main.max.x) * 0.5;
    let cz = (main.min.z + main.max.z) * 0.5;
    // Inset 3.5 u from the hub edges: the server spreads players up to 1.6 u (diagonal ~2.3 u) around
    // the spawn point, plus half a player width - a 1 u inset put some players over the void.
    let ix = ((main.max.x - main.min.x) * 0.5 - 3.5).max(0.0);
    let iz = ((main.max.z - main.min.z) * 0.5 - 3.5).max(0.0);
    let candidates = [
        (cx, cz),
        (cx - ix, cz - iz),
        (cx + ix, cz - iz),
        (cx + ix, cz + iz),
        (cx - ix, cz + iz),
    ];
    let mut best = Vec3::new(cx, main.max.y, cz);
    let mut best_d = -1.0_f32;
    for (x, z) in candidates {
        let dx = x - beacon.x;
        let dz = z - beacon.z;
        let d2 = dx * dx + dz * dz;
        if d2 > best_d {
            best = Vec3::new(x, main.max.y, z);
            best_d = d2;
        }
    }
    best
}

/// If `a` and `b` face each other (opposite facings) within 12 u horizontally (|dy| <= 0.5,
/// perpendicular offset <= 0.75), returns a generated straight 3.5 u bridge platform (top at
/// the higher socket). Otherwise `None`.
///
/// Test hook: exposed so integration tests can check loop-closing geometry directly.
pub fn facing_bridge(a: &OpenSocket, b: &OpenSocket) -> Option<(u8, Aabb)> {
    if a.facing != b.facing.opposite() {
        return None;
    }
    let (fa_x, fa_z) = a.facing.vec();
    let dx = b.pos.x - a.pos.x;
    let dz = b.pos.z - a.pos.z;
    let along = dx * fa_x + dz * fa_z;
    if along <= 0.0 {
        return None;
    }
    let perp = -dx * fa_z + dz * fa_x;
    if perp.abs() > LOOP_BRIDGE_PERP_TOL {
        return None;
    }
    let dist = (dx * dx + dz * dz).sqrt();
    if dist > LOOP_BRIDGE_MAX_DIST {
        return None;
    }
    if (b.pos.y - a.pos.y).abs() > SOCKET_DY_TOL {
        return None;
    }
    let top = a.pos.y.max(b.pos.y);
    let half = BRIDGE_WIDTH * 0.5;
    let bounds = if fa_x != 0.0 {
        let center = (a.pos.z + b.pos.z) * 0.5;
        Aabb {
            min: Vec3::new(a.pos.x.min(b.pos.x), top - 1.0, center - half),
            max: Vec3::new(a.pos.x.max(b.pos.x), top, center + half),
        }
    } else {
        let center = (a.pos.x + b.pos.x) * 0.5;
        Aabb {
            min: Vec3::new(center - half, top - 1.0, a.pos.z.min(b.pos.z)),
            max: Vec3::new(center + half, top, a.pos.z.max(b.pos.z)),
        }
    };
    Some((PLATFORM_NORMAL, bounds))
}

/// True when the bridge `bounds` violate no placement rule (platforms and other bridges).
fn bridge_fits(bounds: &Aabb, grid: &CoarseGrid, built: &[(u8, Aabb)], platforms: &[Platform]) -> bool {
    for idx in grid.candidates(bounds) {
        if !platform_pair_ok(bounds, &platforms[idx].bounds) {
            return false;
        }
    }
    for (_, b) in built {
        if !platform_pair_ok(bounds, b) {
            return false;
        }
    }
    true
}

/// Phase 2 (A1): close loops with generated straight bridges between facing open sockets.
/// Committed bridges get fresh platform ids; consumed sockets are removed from `open`.
fn close_loops(
    open: &mut Vec<OpenSocket>,
    platforms: &mut Vec<Platform>,
    grid: &mut CoarseGrid,
    next_id: &mut u16,
) -> Option<usize> {
    let n = open.len();
    let mut consumed = vec![false; n];
    let mut built: Vec<(u8, Aabb)> = Vec::new();
    for i in 0..n {
        if consumed[i] {
            continue;
        }
        for j in (i + 1)..n {
            if consumed[j] {
                continue;
            }
            let Some((_, bounds)) = facing_bridge(&open[i], &open[j]) else {
                continue;
            };
            if !bridge_fits(&bounds, grid, &built, platforms) {
                continue;
            }
            built.push((PLATFORM_NORMAL, bounds));
            consumed[i] = true;
            consumed[j] = true;
            break;
        }
    }
    let count = built.len();
    for (kind, bounds) in built {
        commit_platforms(platforms, grid, vec![(kind, bounds)], next_id)?;
    }
    let mut i = 0usize;
    open.retain(|_| {
        let keep = !consumed[i];
        i += 1;
        keep
    });
    Some(count)
}

/// Bounds of the railing-less 3 u cap platform for an unclosable socket.
fn stub_bounds(s: &OpenSocket) -> Aabb {
    let (fx, fz) = s.facing.vec();
    let half = STUB_WIDTH * 0.5;
    let len = STUB_LENGTH;
    let top = s.pos.y;
    if fx != 0.0 {
        let x0 = if fx > 0.0 { s.pos.x } else { s.pos.x - len };
        Aabb {
            min: Vec3::new(x0, top - 1.0, s.pos.z - half),
            max: Vec3::new(x0 + len, top, s.pos.z + half),
        }
    } else {
        let z0 = if fz > 0.0 { s.pos.z } else { s.pos.z - len };
        Aabb {
            min: Vec3::new(s.pos.x - half, top - 1.0, z0),
            max: Vec3::new(s.pos.x + half, top, z0 + len),
        }
    }
}

/// Phase 3: seal remaining open sockets with a dead-end chunk, else a 3 u stub platform.
/// Every socket is consumed; returns the number of stubs placed. Stubs must satisfy the
/// same stacking rules as any other platform — an overlapping stub fails validation and
/// wastes the whole map, so an unplaceable stub simply is not built.
fn seal_open_sockets(
    cat: &Catalog,
    level: u32,
    rng: &mut Rng,
    open: &mut Vec<OpenSocket>,
    platforms: &mut Vec<Platform>,
    grid: &mut CoarseGrid,
    next_id: &mut u16,
) -> Option<usize> {
    let mut stubs = 0usize;
    let mut work = std::mem::take(open);
    let mut guard = 0usize;
    while let Some(socket) = work.pop() {
        guard += 1;
        if guard > 512 {
            if commit_stub(&socket, platforms, grid, next_id)? {
                stubs += 1;
            }
            continue;
        }
        let mut valid: Vec<(Placement, usize)> = Vec::new();
        for chunk in &cat.chunks {
            if chunk.role != ChunkRole::DeadEnd || chunk.min_level > level {
                continue;
            }
            for (si, _) in chunk.sockets.iter().enumerate() {
                for turns in 0..4u32 {
                    let Some(p) = plan_join(chunk, si, turns, &socket) else {
                        continue;
                    };
                    if placement_fits(&p, platforms, grid) {
                        valid.push((p, si));
                    }
                }
            }
        }
        if !valid.is_empty() {
            let pick = valid.swap_remove(rng.below(valid.len()));
            let (p, used) = pick;
            for (i, s) in p.sockets.iter().enumerate() {
                if i != used {
                    work.push(*s);
                }
            }
            commit_platforms(platforms, grid, p.platforms, next_id)?;
            continue;
        }
        if commit_stub(&socket, platforms, grid, next_id)? {
            stubs += 1;
        }
    }
    Some(stubs)
}

/// Result of gift filling.
struct GiftFill {
    gifts: Vec<Gift>,
}

/// Accepts a gift candidate iff it stands on a NORMAL platform top (edge inset >= 0.5 u),
/// its body box does not overlap any wall, jump pad or ramp (any non-NORMAL platform volume),
/// it is >= the effective minimum spacing from other gifts horizontally and >= 2.5 u from
/// the beacon. Rejected candidates are not placed; the callers re-sample (the chunk-slot
/// passes skip the slot; the top-up pass samples a fresh random site), so a stuck slot never
/// yields a stuck gift.
#[allow(clippy::too_many_arguments)]
fn accept_candidate(
    platforms: &[Platform],
    grid: &mut PointGrid,
    beacon: &Vec3,
    pos: Vec3,
    elevated: bool,
    gifts: &mut Vec<Gift>,
    elevated_flags: &mut Vec<bool>,
    spacing_mul: f32,
) -> bool {
    let top = pos.y - 0.3;
    let Some(i) = platform_top_under(platforms, pos.x, pos.z, top, 0.5) else {
        return false;
    };
    if platforms[i].kind != PLATFORM_NORMAL {
        return false;
    }
    // T-062 / T-112: spawn safety — no gift may spawn stuck inside any solid volume.
    //
    // The original rule only checked non-NORMAL platforms (walls, jump pads, ramps),
    // which let a gift sit inside a NORMAL platform's side: a gift standing on a low
    // deck whose body pokes into a taller deck beside it. Measured at 1.96% of all
    // gifts (14,783 of 755,887) before this widened to every platform except the one
    // the gift actually stands on.
    if !gift_body_clear(
        platforms
            .iter()
            .enumerate()
            .filter(|(j, _)| *j != i)
            .map(|(_, p)| &p.bounds),
        pos,
    ) {
        return false;
    }
    if grid.any_within(pos.x, pos.z, gift_min_spacing(spacing_mul) - EPS) {
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
/// Minimum gift spacing honours the Clustered Gifts curse multiplier (`spacing_mul`);
/// the grid cell width stays `GIFT_MIN_SPACING`, which remains >= the reduced spacing, so the
/// `PointGrid` neighbour search stays correct under clustering.
fn fill_gifts(
    platforms: &[Platform],
    slots: &[(Vec3, bool)],
    beacon: &Vec3,
    level: u32,
    players: u32,
    rng: &mut Rng,
    spacing_mul: f32,
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
        if accept_candidate(
            platforms,
            &mut grid,
            beacon,
            pos,
            elev,
            &mut gifts,
            &mut elevated_flags,
            spacing_mul,
        ) {
            accepted_slot[si] = true;
        }
    }
    // Pass B: all remaining slots. Under the Clustered Gifts curse (spacing_mul < 1.0) the
    // remaining slots are taken nearest-first around the gifts already placed, so the level
    // grows clumps instead of an even spread; uncursed placement keeps the shuffled order.
    let clustered = spacing_mul < 1.0 - EPS;
    let mut pass_b: Vec<usize> = order.iter().copied().filter(|&si| !accepted_slot[si]).collect();
    if clustered {
        let nearest = |si: usize| -> f32 {
            let (pos, _) = slots[si];
            gifts
                .iter()
                .map(|g| h_dist(pos.x, pos.z, g.pos.x, g.pos.z))
                .fold(f32::INFINITY, f32::min)
        };
        pass_b.sort_by(|&a, &b| nearest(a).total_cmp(&nearest(b)).then(a.cmp(&b)));
    }
    for &si in &pass_b {
        if gifts.len() >= target {
            break;
        }
        if accepted_slot[si] {
            continue;
        }
        let (pos, elev) = slots[si];
        if accept_candidate(
            platforms,
            &mut grid,
            beacon,
            pos,
            elev,
            &mut gifts,
            &mut elevated_flags,
            spacing_mul,
        ) {
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
            spacing_mul,
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
fn choose_altar(
    slots: &[Vec3],
    platforms: &[Platform],
    gifts: &[Gift],
    spawn: &Vec3,
    beacon: &Vec3,
    enemy_slots: &[Vec3],
    run_seed: u32,
    level: u32,
    rng: &mut Rng,
) -> Option<(Altar, usize)> {
    if level < 3 {
        return None;
    }
    let roll = splitmix32(run_seed ^ splitmix32(level.wrapping_add(0xA17A_0000)));
    if roll & 1 != 0 {
        return None;
    }
    let mut order: Vec<usize> = (0..slots.len()).collect();
    rng.shuffle(&mut order);
    for i in order {
        let pos = slots[i];
        let si = platform_top_under(platforms, pos.x, pos.z, pos.y, 0.75)
            .or_else(|| platform_top_under(platforms, pos.x, pos.z, pos.y - 0.3, 0.75));
        let Some(si) = si else {
            continue;
        };
        if platforms[si].kind != PLATFORM_NORMAL {
            continue;
        }
        let altar_pos = Vec3::new(pos.x, platforms[si].bounds.max.y, pos.z);
        if !altar_site_clear(&altar_pos, gifts, spawn, beacon, enemy_slots) {
            continue;
        }
        return Some((Altar { id: 1, pos: altar_pos }, si));
    }
    // Top-up: random eligible NORMAL platform tops (shuffled, deterministic).
    let mut order: Vec<usize> = (0..platforms.len())
        .filter(|&i| platforms[i].kind == PLATFORM_NORMAL)
        .collect();
    rng.shuffle(&mut order);
    for i in order {
        let b = platforms[i].bounds;
        if b.max.x - b.min.x < 1.5 || b.max.z - b.min.z < 1.5 {
            continue;
        }
        let pos = Vec3::new((b.min.x + b.max.x) * 0.5, b.max.y, (b.min.z + b.max.z) * 0.5);
        if !altar_site_clear(&pos, gifts, spawn, beacon, enemy_slots) {
            continue;
        }
        return Some((Altar { id: 1, pos }, i));
    }
    None
}

/// EVENT_CRUMBLE: marks ~40% of eligible PATH platforms (never hub, never with a gift or
/// altar slot) as CRUMBLE, keeping at most 3 consecutive crumbling ids in placement order.
/// Runs before gifts are placed; gift filling only accepts NORMAL platforms, so a crumbled
/// platform can never carry a gift and the A3 density cap stays valid afterwards.
fn apply_crumble(
    platforms: &mut [Platform],
    hub_platform_count: usize,
    slot_supports: &[bool],
    run_seed: u32,
    level: u32,
) {
    let n = platforms.len();
    let mut excluded = vec![false; n];
    for i in 0..hub_platform_count.min(n) {
        excluded[i] = true;
    }
    for (i, &s) in slot_supports.iter().enumerate() {
        if i < n && s {
            excluded[i] = true;
        }
    }
    let eligible: Vec<usize> = (hub_platform_count..n)
        .filter(|&i| !excluded[i] && platforms[i].kind == PLATFORM_NORMAL)
        .collect();
    if eligible.is_empty() {
        return;
    }
    let target = ((eligible.len() as f32) * 0.4).ceil() as usize;
    let mut order = eligible;
    let mut rng = Rng::new(seed_mix(run_seed ^ 0xC0FF_EE42, level));
    rng.shuffle(&mut order);
    let mut marked = 0usize;
    for &i in &order {
        if marked >= target {
            break;
        }
        let mut left = 0usize;
        while left < i && platforms[i - 1 - left].kind == PLATFORM_CRUMBLE {
            left += 1;
        }
        let mut right = 0usize;
        while i + 1 + right < n && platforms[i + 1 + right].kind == PLATFORM_CRUMBLE {
            right += 1;
        }
        if left + right + 1 >= 4 {
            continue;
        }
        platforms[i].kind = PLATFORM_CRUMBLE;
        marked += 1;
    }
}

/// Core assembly: builds a level from `cat` with explicit budget parameters.
#[allow(clippy::too_many_arguments)]
fn assemble_level(
    cat: &Catalog,
    rng_seed: u32,
    run_seed: u32,
    level: u32,
    players: u32,
    event: u8,
    target_area_u: f32,
    max_chunks: usize,
    max_junctions_u: u32,
    spacing_mul: f32,
) -> Option<GeneratedLevel> {
    let mut rng = Rng::new(seed_mix(rng_seed, level));
    let tier = level_tier(level);
    let high_rise = event == EVENT_HIGH_RISE;

    // --- T-069 composition plan. Derived from the run seed (not the attempt seed) so every
    // regeneration attempt of one run walks the same pattern order; the geometry RNG still
    // varies per attempt. A catalog with no pattern tags yields an empty plan and the walk
    // keeps the pre-T-069 weighted picks.
    let metas = pattern_metas(cat);
    let plan = if metas.iter().any(|m| m.is_tagged()) {
        crate::pattern::CompositionPlan::build_with_len(&metas, run_seed, level, players, max_chunks, tier)
    } else {
        crate::pattern::CompositionPlan::empty(run_seed, level, players)
    };
    let mut cursor = plan.cursor();
    cursor.skip(1); // entry 0 is the hub, placed below

    // --- hub at the origin (unrotated).
    let hub_choices: Vec<usize> = cat
        .chunks
        .iter()
        .enumerate()
        .filter(|(_, c)| c.role == ChunkRole::Hub && c.min_level <= level)
        .map(|(i, _)| i)
        .collect();
    if hub_choices.is_empty() {
        return None;
    }
    let hub_ci = match plan.hub_index() {
        Some(index) if hub_choices.contains(&index) => index,
        _ => {
            let hub_weights: Vec<f32> = hub_choices
                .iter()
                .map(|&i| chunk_weight(&cat.chunks[i], tier, false))
                .collect();
            let hub_pick = weighted_pick(&mut rng, &hub_weights)?;
            hub_choices[hub_pick]
        }
    };
    let hub_chunk = &cat.chunks[hub_ci];
    let hub_geom = build_geometry(hub_chunk, 0, Vec3::new(0.0, 0.0, 0.0));
    let beacon = hub_geom.beacon?; // hubs are guaranteed a beacon by parse_catalog
    let enemy_spawns = hub_geom.enemy_slots.clone();
    let hub_top_area = hub_geom.top_area;
    let mut gift_slots = hub_geom.gift_slots;
    let mut altar_slots = hub_geom.altar_slots;
    // Platform 0 = the hub's main platform.
    let mut hub_plats = hub_geom.platforms;
    let main_idx = main_platform_index(&hub_plats);
    hub_plats.swap(0, main_idx);

    let mut platforms: Vec<Platform> = Vec::new();
    let mut grid = CoarseGrid::new();
    let mut next_id: u16 = 0;
    commit_platforms(&mut platforms, &mut grid, hub_plats, &mut next_id)?;
    let hub_platform_count = platforms.len();
    let mut area_sum = hub_top_area;
    let mut open: Vec<OpenSocket> = hub_geom.sockets;

    // --- budget-driven random walk over open sockets. The random pick may target any open
    // socket (including earlier-placed ones); when the walk stalls (>= 8 consecutive failed
    // picks) every open socket is swept in order before the map is accepted. Per A1 the walk
    // stops accepting new failures once at least 70% of the area budget is reached.
    let mut chunk_count = 1usize;
    let mut junctions_used = 0u32;
    let mut dropped: Vec<OpenSocket> = Vec::new();
    let mut iters = 0usize;
    let mut consecutive_failures = 0u32;
    while iters < MAX_GROWTH_ITERS && chunk_count < max_chunks && area_sum < target_area_u && !open.is_empty() {
        iters += 1;
        let oi = rng.below(open.len());
        let mut placed: Option<(usize, Placement, usize)> = try_place(
            cat,
            &open[oi],
            &platforms,
            &grid,
            level,
            tier,
            high_rise,
            junctions_used,
            max_junctions_u,
            &mut rng,
            Some(&mut cursor),
        )
        .map(|(p, used)| (oi, p, used));
        if placed.is_none() && consecutive_failures >= 8 {
            for k in 0..open.len() {
                if k == oi {
                    continue;
                }
                if let Some((p, used)) = try_place(
                    cat,
                    &open[k],
                    &platforms,
                    &grid,
                    level,
                    tier,
                    high_rise,
                    junctions_used,
                    max_junctions_u,
                    &mut rng,
                    Some(&mut cursor),
                ) {
                    placed = Some((k, p, used));
                    break;
                }
            }
        }
        match placed {
            Some((k, p, used)) => {
                commit_placement(
                    &mut open,
                    k,
                    p,
                    used,
                    &mut platforms,
                    &mut grid,
                    &mut next_id,
                    &mut gift_slots,
                    &mut altar_slots,
                    &mut area_sum,
                    &mut chunk_count,
                    &mut junctions_used,
                )?;
                consecutive_failures = 0;
            }
            None => {
                consecutive_failures += 1;
                open[oi].failures = open[oi].failures.saturating_add(1);
                if open[oi].failures >= 3 {
                    let s = open.swap_remove(oi);
                    dropped.push(s);
                }
                // A1: accept the map once >= 70% of the area budget is reached and the walk
                // is stalled; remaining open sockets are bridged or sealed below.
                if consecutive_failures >= STALL_LIMIT && area_sum >= BUDGET_ACCEPT * target_area_u {
                    break;
                }
            }
        }
    }

    // --- A1: close loops with generated bridges, then seal the rest.
    open.append(&mut dropped);
    let _bridges = close_loops(&mut open, &mut platforms, &mut grid, &mut next_id)?;
    let _stubs = seal_open_sockets(cat, level, &mut rng, &mut open, &mut platforms, &mut grid, &mut next_id)?;

    // --- crumble (event) BEFORE gifts: gift filling only accepts NORMAL tops, so a crumbled
    // platform can never carry a gift and the A3 density cap stays consistent with validate.
    let spawn = spawn_point(&platforms[0].bounds, &beacon);
    let mut slot_supports = vec![false; platforms.len()];
    for (pos, _) in &gift_slots {
        let top = pos.y - 0.3;
        if let Some(i) = platform_top_under(&platforms, pos.x, pos.z, top, 0.0) {
            slot_supports[i] = true;
        }
    }
    for pos in &altar_slots {
        if let Some(i) = platform_top_under(&platforms, pos.x, pos.z, pos.y, 0.0)
            .or_else(|| platform_top_under(&platforms, pos.x, pos.z, pos.y - 0.3, 0.0))
        {
            slot_supports[i] = true;
        }
    }
    if event == EVENT_CRUMBLE {
        apply_crumble(&mut platforms, hub_platform_count, &slot_supports, run_seed, level);
    }

    // --- gifts (A3), altars (legacy).
    let fill = fill_gifts(&platforms, &gift_slots, &beacon, level, players, &mut rng, spacing_mul);
    let gifts = fill.gifts;
    let altar = choose_altar(
        &altar_slots,
        &platforms,
        &gifts,
        &spawn,
        &beacon,
        &enemy_spawns,
        run_seed,
        level,
        &mut rng,
    );
    let altars = match altar {
        Some((a, _)) => vec![a],
        None => Vec::new(),
    };

    let kill_y = platforms.iter().map(|p| p.bounds.min.y).fold(f32::INFINITY, f32::min) - 15.0;
    let data = LevelData {
        level,
        seed: run_seed,
        spawn,
        beacon,
        kill_y,
        event,
        altars,
        platforms,
        gifts,
    };
    Some(GeneratedLevel {
        data,
        enemy_spawns,
        plan: plan.ids().to_vec(),
    })
}

/// Builds a level from the catalog with the design 4.3 / amendment A1 budget formulas.
fn try_build(
    cat: &Catalog,
    rng_seed: u32,
    run_seed: u32,
    level: u32,
    players: u32,
    event: u8,
    spacing_mul: f32,
) -> Option<GeneratedLevel> {
    assemble_level(
        cat,
        rng_seed,
        run_seed,
        level,
        players,
        event,
        target_area(level),
        max_chunks_for(level),
        max_junctions(level),
        spacing_mul,
    )
}

/// Fallback when the catalog yields nothing: the hub plus straight generated arms.
/// A hub whose own boxes violate the placement rules is refused (`None`) so the caller can
/// degrade to the guaranteed-valid [`builtin_fallback`]; arm segments are only committed
/// when they satisfy the placement rules.
#[allow(clippy::needless_range_loop)]
fn catalog_fallback(
    cat: &Catalog,
    run_seed: u32,
    level: u32,
    players: u32,
    spacing_mul: f32,
) -> Option<GeneratedLevel> {
    let hub = cat
        .chunks
        .iter()
        .find(|c| c.role == ChunkRole::Hub && c.min_level <= level)?;
    let geom = build_geometry(hub, 0, Vec3::new(0.0, 0.0, 0.0));
    let beacon = geom.beacon?;
    let enemy_spawns = geom.enemy_slots.clone();
    let main_idx = main_platform_index(&geom.platforms);
    let main_top = geom.platforms[main_idx].1.max.y;
    let main_bounds = geom.platforms[main_idx].1;
    let fp = footprint(
        &geom
            .platforms
            .iter()
            .map(|(kind, b)| ChunkPlatform {
                kind: *kind,
                min: b.min,
                max: b.max,
            })
            .collect::<Vec<_>>(),
    );
    // A hub whose own boxes violate the placement rules would produce an invalid fallback
    // map ("platforms N and M intersect"); refuse it so the caller can use the builtin fallback.
    for i in 0..geom.platforms.len() {
        for j in (i + 1)..geom.platforms.len() {
            if !platform_pair_ok(&geom.platforms[i].1, &geom.platforms[j].1) {
                return None;
            }
        }
    }
    let mut platforms: Vec<Platform> = Vec::new();
    let mut grid = CoarseGrid::new();
    let mut next_id: u16 = 0;
    let mut hub_plats = geom.platforms;
    hub_plats.swap(0, main_idx);
    commit_platforms(&mut platforms, &mut grid, hub_plats, &mut next_id)?;
    // Arms built explicitly (flush at the hub footprint edges, width 3). Arm segments are
    // committed only when they satisfy the placement rules.
    for i in 1..=5usize {
        let d0 = 6.0 * (i - 1) as f32;
        let d1 = 6.0 * i as f32;
        let arms = [
            Aabb {
                min: Vec3::new(fp.max.x + d0, main_top - 1.0, -1.5),
                max: Vec3::new(fp.max.x + d1, main_top, 1.5),
            },
            Aabb {
                min: Vec3::new(fp.min.x - d1, main_top - 1.0, -1.5),
                max: Vec3::new(fp.min.x - d0, main_top, 1.5),
            },
            Aabb {
                min: Vec3::new(-1.5, main_top - 1.0, fp.max.z + d0),
                max: Vec3::new(1.5, main_top, fp.max.z + d1),
            },
            Aabb {
                min: Vec3::new(-1.5, main_top - 1.0, fp.min.z - d1),
                max: Vec3::new(1.5, main_top, fp.min.z - d0),
            },
        ];
        for bounds in arms {
            let mut fits = true;
            for idx in grid.candidates(&bounds) {
                if !platform_pair_ok(&bounds, &platforms[idx].bounds) {
                    fits = false;
                    break;
                }
            }
            if fits {
                commit_platforms(&mut platforms, &mut grid, vec![(PLATFORM_NORMAL, bounds)], &mut next_id)?;
            }
        }
    }
    let spawn = spawn_point(&main_bounds, &beacon);
    let mut rng = Rng::new(seed_mix(run_seed, level));
    let fill = fill_gifts(
        &platforms,
        &geom.gift_slots,
        &beacon,
        level,
        players,
        &mut rng,
        spacing_mul,
    );
    let kill_y = platforms.iter().map(|p| p.bounds.min.y).fold(f32::INFINITY, f32::min) - 15.0;
    let data = LevelData {
        level,
        seed: run_seed,
        spawn,
        beacon,
        kill_y,
        event: EVENT_NONE,
        altars: Vec::new(),
        platforms,
        gifts: fill.gifts,
    };
    Some(GeneratedLevel {
        data,
        enemy_spawns,
        // The hub is a real catalog chunk and it is on the map, so it belongs in the
        // plan: an empty plan made every fallback level look like a malformed record to
        // the pattern audit and hid the fallback rate behind a "no plan" error.
        plan: vec![hub.id.clone()],
    })
}

/// Fallback built from scratch: 12x12 plaza + four straight 30 u arms (always valid).
fn builtin_fallback(run_seed: u32, level: u32, players: u32, spacing_mul: f32) -> GeneratedLevel {
    let mut platforms: Vec<Platform> = Vec::new();
    platforms.push(Platform {
        id: 0,
        kind: PLATFORM_NORMAL,
        bounds: Aabb {
            min: Vec3::new(-6.0, -1.0, -6.0),
            max: Vec3::new(6.0, 0.0, 6.0),
        },
    });
    let mut next_id: u16 = 1;
    for i in 1..=5usize {
        let d = i as f32;
        for bounds in [
            Aabb {
                min: Vec3::new(6.0 * d, -1.0, -1.5),
                max: Vec3::new(6.0 * d + 6.0, 0.0, 1.5),
            },
            Aabb {
                min: Vec3::new(-6.0 * d - 6.0, -1.0, -1.5),
                max: Vec3::new(-6.0 * d, 0.0, 1.5),
            },
            Aabb {
                min: Vec3::new(-1.5, -1.0, 6.0 * d),
                max: Vec3::new(1.5, 0.0, 6.0 * d + 6.0),
            },
            Aabb {
                min: Vec3::new(-1.5, -1.0, -6.0 * d - 6.0),
                max: Vec3::new(1.5, 0.0, -6.0 * d),
            },
        ] {
            platforms.push(Platform {
                id: next_id,
                kind: PLATFORM_NORMAL,
                bounds,
            });
            next_id += 1;
        }
    }
    let beacon = Vec3::new(33.0, 0.0, 0.0);
    let spawn = Vec3::new(0.0, 0.0, 0.0);
    let mut rng = Rng::new(seed_mix(run_seed, level));
    let fill = fill_gifts(&platforms, &[], &beacon, level, players, &mut rng, spacing_mul);
    let enemy_spawns = vec![
        Vec3::new(4.0, 2.0, 0.0),
        Vec3::new(2.83, 2.0, 2.83),
        Vec3::new(0.0, 2.0, 4.0),
        Vec3::new(-2.83, 2.0, 2.83),
        Vec3::new(-4.0, 2.0, 0.0),
        Vec3::new(-2.83, 2.0, -2.83),
        Vec3::new(0.0, 2.0, -4.0),
        Vec3::new(2.83, 2.0, -2.83),
    ];
    let data = LevelData {
        level,
        seed: run_seed,
        spawn,
        beacon,
        kill_y: -16.0,
        event: EVENT_NONE,
        altars: Vec::new(),
        platforms,
        gifts: fill.gifts,
    };
    GeneratedLevel {
        data,
        enemy_spawns,
        plan: Vec::new(),
    }
}

// ------------------------------------------------------- public API ----

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
            kind: PLATFORM_NORMAL,
            bounds: spawn_b,
        }],
        gifts: Vec::new(),
        event: EVENT_NONE,
        altars: Vec::new(),
    }
}

/// A fresh, completable level. Same `(seed, level)` -> identical output.
pub fn generate(seed: u32, level: u32) -> GeneratedLevel {
    generate_for(seed, level, 1)
}

/// A fresh level for a player count (gift counts scale with players, amendment A3).
/// Same `(seed, level, players)` -> identical output.
pub fn generate_for(seed: u32, level: u32, players: u32) -> GeneratedLevel {
    generate_for_with_spacing(seed, level, players, 1.0)
}

/// A fresh level whose gift placement honours the Clustered Gifts curse: `spacing_mul`
/// scales the minimum gift spacing (clamped to [0.25, 1.0]; 1.0 = scattered, the default).
/// Same `(seed, level, players, spacing_mul)` -> identical output.
pub fn generate_for_with_spacing(seed: u32, level: u32, players: u32, spacing_mul: f32) -> GeneratedLevel {
    match catalog() {
        Ok(cat) => generate_with_catalog_spacing(cat, seed, level, players, spacing_mul),
        Err(_) => builtin_fallback(seed, level, players, spacing_mul),
    }
}

// ------------------------------------------------------ validation ----

/// Design hop envelope (edge-to-edge), by landing rise; `None` = not jumpable.
/// Plain (non-pad) jumps may climb at most +2.0 u: the apex of the jump is exactly
/// +2.25 u (12^2 / (2*32)), so a +2.25 u rise can never be cleared with room to spare.
fn design_max_gap(dy: f32) -> Option<f32> {
    if dy <= -4.0 {
        Some(8.0)
    } else if dy <= -3.0 {
        Some(7.5)
    } else if dy <= -2.0 {
        Some(7.0)
    } else if dy <= -1.0 {
        Some(6.5)
    } else if dy <= 0.0 {
        Some(6.0)
    } else if dy <= 0.5 {
        Some(5.5)
    } else if dy <= 1.0 {
        Some(5.0)
    } else if dy <= 1.5 {
        Some(4.5)
    } else if dy <= 2.0 {
        Some(4.0)
    } else {
        None
    }
}

/// Surface height of `p` at the point of its footprint nearest `other`'s centre.
///
/// Used for edges with a real gap, where there is no shared boundary to sample.
fn nearest_surface(p: &Platform, other: &Platform) -> f32 {
    let cx = (other.bounds.min.x + other.bounds.max.x) * 0.5;
    let cz = (other.bounds.min.z + other.bounds.max.z) * 0.5;
    let x = cx.clamp(p.bounds.min.x, p.bounds.max.x);
    let z = cz.clamp(p.bounds.min.z, p.bounds.max.z);
    surface_at(p, x, z)
}

/// Footprint area of `b` in the XZ plane.
fn footprint_area(b: &Aabb) -> f32 {
    (b.max.x - b.min.x).max(0.0) * (b.max.z - b.min.z).max(0.0)
}

/// Is `(x, z)` inside `b` (grown by `eps`)?
fn point_in_xz(b: &Aabb, x: f32, z: f32, eps: f32) -> bool {
    x >= b.min.x - eps && x <= b.max.x + eps && z >= b.min.z - eps && z <= b.max.z + eps
}

/// Sample points along the perimeter of `b` (corners included), `per_edge` per side.
fn perimeter_samples(b: &Aabb, per_edge: usize) -> Vec<(f32, f32)> {
    let n = per_edge.max(1);
    let mut out = Vec::with_capacity(4 * (n + 1));
    for i in 0..=n {
        let t = i as f32 / n as f32;
        let x = b.min.x + (b.max.x - b.min.x) * t;
        let z = b.min.z + (b.max.z - b.min.z) * t;
        out.push((x, b.min.z));
        out.push((x, b.max.z));
        out.push((b.min.x, z));
        out.push((b.max.x, z));
    }
    out
}

/// Smallest-magnitude surface rise between two platforms that share a boundary.
///
/// A ramp's surface varies across its footprint, so a single height cannot describe
/// the edge: a ramp lying on a big platform touches that platform along its whole
/// perimeter, and the player walks on at the LOW edge. Sampling the smaller
/// footprint's perimeter and keeping the best (smallest |rise|) pair where the
/// sample lies on the other footprint gives the edge the player can actually use.
/// Flat platforms report the same rise at every sample, so their behaviour is
/// unchanged. Returns `None` when the footprints do not touch.
fn contact_rise(from: &Platform, to: &Platform) -> Option<f32> {
    let (small, other) = if footprint_area(&from.bounds) <= footprint_area(&to.bounds) {
        (from, to)
    } else {
        (to, from)
    };
    let mut best: Option<f32> = None;
    for (x, z) in perimeter_samples(&small.bounds, 8) {
        if !point_in_xz(&other.bounds, x, z, EPS) {
            continue;
        }
        let d = surface_at(other, x, z) - surface_at(small, x, z);
        if best.is_none_or(|b: f32| d.abs() < b.abs()) {
            best = Some(d);
        }
    }
    best
}

/// The rise the reachability graph uses for the directed edge `from -> to`.
///
/// Touching platforms compare their surfaces where they meet (see [`contact_rise`]),
/// which is what makes ramps walkable from the platform they rest on; platforms with
/// a real gap compare the surfaces nearest each other. Flat platforms report
/// `to.bounds.max.y - from.bounds.max.y`, exactly as before ramps existed.
pub fn edge_rise(from: &Platform, to: &Platform) -> f32 {
    if rect_gap(&from.bounds, &to.bounds) <= EPS {
        contact_rise(from, to).unwrap_or_else(|| nearest_surface(to, from) - nearest_surface(from, to))
    } else {
        nearest_surface(to, from) - nearest_surface(from, to)
    }
}

/// Directed edge: can a player land on `to` from `from`?
///
/// Touching edges (gap <= EPS) are walk/step/drop edges and always fair: a touching step (|rise| <= 0.55 u) is
/// walkable and a touching drop (rise < -0.55 u) is fine (walk off the edge, no landing-width requirement).
/// Every other edge is a JUMP edge (including a touching rise above the step limit) and must be fair for humans
/// too: a plain jump may climb at most +2.0 u (apex - 0.25; higher needs a jump pad); the landing platform's
/// smaller horizontal extent must be >= 2.0 u (pad landing targets included); and the edge-to-edge gap must be
/// <= (design max gap for its rise) - 1.0 u comfort. Jump pads launch to targets up to +3.5 u (apex - 0.5)
/// within 4.5 u, and their targets still need the >= 2.0 u extent.
fn can_jump_or_step(from: &Platform, to: &Platform) -> bool {
    let gap = rect_gap(&from.bounds, &to.bounds);
    let touching = gap <= EPS;
    let dt = edge_rise(from, to);
    if touching && dt.abs() <= 0.55 + EPS {
        return true; // touching walk/step
    }
    if touching && dt < -0.55 {
        return true; // touching drop (gap ~0, rise < -0.55): always fine
    }
    let small = (to.bounds.max.x - to.bounds.min.x).min(to.bounds.max.z - to.bounds.min.z);
    if small < 2.0 - EPS {
        return false; // unfair landing: jumping onto a ledge thinner than 2.0 u
    }
    if from.kind == PLATFORM_JUMP_PAD && dt <= 3.5 + EPS && gap <= 4.5 + EPS {
        return true; // pad launch (apex - 0.5); landing still needs the >= 2.0 u extent
    }
    let Some(limit) = design_max_gap(dt) else {
        return false;
    };
    gap <= limit - 1.0 + EPS // comfort margin: 1.0 u inside the design envelope
}

/// Full structural check plus reachability. Checks ids, platform kinds, spawn on platform
/// 0, spawn/beacon distance, volume intersection, >= 4.0 u XZ-overlap clearance, gift
/// count/spacing/support/beacon distance/density cap, beacon support, the event value,
/// altar rules, kill_y, then directed reachability (every gift/altar/beacon reachable from
/// spawn, and the beacon reachable from everywhere reachable — no traps).
pub fn validate(level: &LevelData) -> Result<(), String> {
    let platforms = &level.platforms;
    if platforms.is_empty() {
        return Err("no platforms".to_string());
    }
    let n = platforms.len();

    // Ids unique and dense 0..n.
    let mut ids: Vec<u16> = platforms.iter().map(|p| p.id).collect();
    ids.sort_unstable();
    for (i, id) in ids.iter().enumerate() {
        if *id as usize != i {
            return Err(format!("platform ids not unique/dense at {id}"));
        }
    }

    // Kinds.
    for p in platforms {
        if p.kind != PLATFORM_NORMAL
            && p.kind != PLATFORM_CRUMBLE
            && p.kind != PLATFORM_JUMP_PAD
            && p.kind != protocol::PLATFORM_TRAMPOLINE
            && p.kind != protocol::PLATFORM_TRIPMINE
            && !is_ramp(p.kind)
        {
            return Err(format!("platform {} has unknown kind {}", p.id, p.kind));
        }
    }

    // Platform 0 exists, is not crumbling, and carries the spawn.
    let Some(p0) = platforms.iter().find(|p| p.id == 0) else {
        return Err("missing spawn platform (id 0)".to_string());
    };
    if p0.kind == PLATFORM_CRUMBLE {
        return Err("platform 0 is crumbling".to_string());
    }
    if (level.spawn.y - p0.bounds.max.y).abs() > EPS {
        return Err("spawn not on platform 0 top".to_string());
    }
    if level.spawn.x < p0.bounds.min.x - EPS
        || level.spawn.x > p0.bounds.max.x + EPS
        || level.spawn.z < p0.bounds.min.z - EPS
        || level.spawn.z > p0.bounds.max.z + EPS
    {
        return Err("spawn not over platform 0".to_string());
    }
    if h_dist(level.spawn.x, level.spawn.z, level.beacon.x, level.beacon.z) < 2.5 - EPS {
        return Err("spawn within 2.5 u of the beacon".to_string());
    }

    // Pairwise: no volume intersections; XZ-overlapping pairs need >= 4.0 u clearance.
    for i in 0..n {
        for j in (i + 1)..n {
            let a = &platforms[i].bounds;
            let b = &platforms[j].bounds;
            if aabb_intersect(a, b) {
                return Err(format!(
                    "platforms {} and {} intersect",
                    platforms[i].id, platforms[j].id
                ));
            }
            if xz_overlap_positive(a, b) {
                let clearance = (a.min.y - b.max.y).max(b.min.y - a.max.y);
                if clearance > EPS && clearance < STACK_CLEARANCE - EPS {
                    return Err(format!(
                        "platforms {} and {} overlap in XZ with only {:.2} u vertical clearance",
                        platforms[i].id, platforms[j].id, clearance
                    ));
                }
            }
        }
    }

    // Gifts: ids, kinds, support on NORMAL tops at y = top + 0.3, spacing, beacon distance, density.
    if level.gifts.len() > GIFT_CAP as usize {
        return Err(format!("more than {GIFT_CAP} gifts"));
    }
    let mut gids: Vec<u16> = level.gifts.iter().map(|g| g.id).collect();
    gids.sort_unstable();
    for (i, id) in gids.iter().enumerate() {
        if *id as usize != i {
            return Err("gift ids not unique/dense".to_string());
        }
    }
    for gift in &level.gifts {
        if gift.kind != GIFT_NORMAL && gift.kind != GIFT_GOLDEN {
            return Err(format!("gift {} has unknown kind {}", gift.id, gift.kind));
        }
        let top = gift.pos.y - 0.3;
        let Some(i) = platform_top_under(platforms, gift.pos.x, gift.pos.z, top, 0.0) else {
            return Err(format!("gift {} is not on a platform top", gift.id));
        };
        if platforms[i].kind != PLATFORM_NORMAL {
            return Err(format!("gift {} is not on a NORMAL platform", gift.id));
        }
        if h_dist(gift.pos.x, gift.pos.z, level.beacon.x, level.beacon.z) < GIFT_BEACON_CLEAR - EPS {
            return Err(format!("gift {} within 2.5 u of the beacon", gift.id));
        }
    }
    for i in 0..level.gifts.len() {
        for j in (i + 1)..level.gifts.len() {
            let a = &level.gifts[i].pos;
            let b = &level.gifts[j].pos;
            let d2 = (a.x - b.x) * (a.x - b.x) + (a.z - b.z) * (a.z - b.z);
            if d2 < (GIFT_MIN_SPACING - EPS) * (GIFT_MIN_SPACING - EPS) {
                return Err(format!(
                    "gifts {} and {} are closer than 2.0 u",
                    level.gifts[i].id, level.gifts[j].id
                ));
            }
        }
    }
    let eligible_area: f32 = platforms
        .iter()
        .filter(|p| p.kind == PLATFORM_NORMAL)
        .map(|p| aabb_area(&p.bounds))
        .sum();
    if level.gifts.len() as f32 * GIFT_AREA_PER > eligible_area + 1e-3 {
        return Err("gift density exceeds one gift per 9 u^2 of eligible platform top".to_string());
    }

    // Beacon: supported on a non-jump-pad platform top.
    let mut beacon_node: Option<usize> = None;
    for (i, p) in platforms.iter().enumerate() {
        if (level.beacon.y - p.bounds.max.y).abs() <= EPS
            && level.beacon.x >= p.bounds.min.x - EPS
            && level.beacon.x <= p.bounds.max.x + EPS
            && level.beacon.z >= p.bounds.min.z - EPS
            && level.beacon.z <= p.bounds.max.z + EPS
        {
            beacon_node = Some(i);
            break;
        }
    }
    let Some(beacon_idx) = beacon_node else {
        return Err("beacon not on a platform top".to_string());
    };
    if platforms[beacon_idx].kind == PLATFORM_JUMP_PAD {
        return Err("beacon stands on a jump pad".to_string());
    }

    // Event value.
    if !matches!(level.event, EVENT_NONE | EVENT_HIGH_RISE | EVENT_ICE | EVENT_CRUMBLE) {
        return Err(format!("unknown event {}", level.event));
    }

    // Altar rules (level >= 3 only; ids dense from 1; NORMAL support; >= 0.75 inset;
    // >= 3.0 u from gifts, beacon and spawn).
    if level.level < 3 && !level.altars.is_empty() {
        return Err("altars are only available from level 3".to_string());
    }
    let mut altar_ids: Vec<u16> = level.altars.iter().map(|a| a.id).collect();
    altar_ids.sort_unstable();
    for (i, id) in altar_ids.iter().enumerate() {
        if *id as usize != i + 1 {
            return Err("altar ids not unique/dense starting from 1".to_string());
        }
    }
    for altar in &level.altars {
        let Some(si) = platform_top_under(platforms, altar.pos.x, altar.pos.z, altar.pos.y, 0.75) else {
            return Err(format!("altar {} not fully on a platform top", altar.id));
        };
        if platforms[si].kind != PLATFORM_NORMAL {
            return Err(format!("altar {} stands on a non-NORMAL platform", altar.id));
        }
        for g in &level.gifts {
            if h_dist(altar.pos.x, altar.pos.z, g.pos.x, g.pos.z) < 3.0 - EPS {
                return Err(format!("altar {} too close to gift {}", altar.id, g.id));
            }
        }
        if h_dist(altar.pos.x, altar.pos.z, level.beacon.x, level.beacon.z) < 3.0 - EPS {
            return Err(format!("altar {} too close to the beacon", altar.id));
        }
        if h_dist(altar.pos.x, altar.pos.z, level.spawn.x, level.spawn.z) < 3.0 - EPS {
            return Err(format!("altar {} too close to the spawn point", altar.id));
        }
    }

    // kill_y at least 10 below every platform bottom.
    let min_bottom = platforms.iter().map(|p| p.bounds.min.y).fold(f32::INFINITY, f32::min);
    if level.kill_y > min_bottom - 10.0 + EPS {
        return Err("kill_y not at least 10 below the lowest platform bottom".to_string());
    }

    // Reachability: directed graph; every gift/altar/beacon reachable from spawn and the
    // beacon reachable from everywhere reachable (no traps).
    let mut adj: Vec<Vec<usize>> = vec![Vec::new(); n];
    for i in 0..n {
        for j in 0..n {
            if i != j && can_jump_or_step(&platforms[i], &platforms[j]) {
                adj[i].push(j);
            }
        }
    }
    let mut reach = vec![false; n];
    reach[0] = true;
    let mut queue = VecDeque::from([0usize]);
    while let Some(a) = queue.pop_front() {
        for &b in &adj[a] {
            if !reach[b] {
                reach[b] = true;
                queue.push_back(b);
            }
        }
    }
    for gift in &level.gifts {
        let top = gift.pos.y - 0.3;
        if let Some(i) = platform_top_under(platforms, gift.pos.x, gift.pos.z, top, 0.0)
            && !reach[i]
        {
            return Err(format!("gift {} is not reachable from spawn", gift.id));
        }
    }
    for altar in &level.altars {
        if let Some(i) = platform_top_under(platforms, altar.pos.x, altar.pos.z, altar.pos.y, 0.75)
            && !reach[i]
        {
            return Err(format!("altar {} is not reachable from spawn", altar.id));
        }
    }
    if !reach[beacon_idx] {
        return Err("beacon is not reachable from spawn".to_string());
    }
    let mut radj: Vec<Vec<usize>> = vec![Vec::new(); n];
    for a in 0..n {
        for &b in &adj[a] {
            radj[b].push(a);
        }
    }
    let mut back = vec![false; n];
    back[beacon_idx] = true;
    let mut queue = VecDeque::from([beacon_idx]);
    while let Some(a) = queue.pop_front() {
        for &b in &radj[a] {
            if !back[b] {
                back[b] = true;
                queue.push_back(b);
            }
        }
    }
    for i in 0..n {
        if reach[i] && !back[i] {
            return Err(format!(
                "platform {} is a trap (no route back to the beacon)",
                platforms[i].id
            ));
        }
    }
    Ok(())
}

/// A1: accept a stalled map once it has reached at least this fraction of the area budget.
const BUDGET_ACCEPT: f32 = 0.7;
/// Consecutive failed growth picks (after the all-sockets sweep kicks in) that end the
/// growth loop when the budget is already at least [`BUDGET_ACCEPT`] filled.
const STALL_LIMIT: u32 = 48;

/// Tries to place one chunk on `anchor` (up to [`CANDIDATE_TRIALS`] picks; each pick
/// checks every socket of that chunk in all four rotations against the coarse grid).
/// Returns the placement and the index of the chunk socket that joined to `anchor`.
///
/// When `cursor` is `Some`, each pick is the next entry of the T-069 composition plan
/// that satisfies the role/level/junction predicate; entries that do not fit are
/// consumed. Once the plan is exhausted (or when `cursor` is `None`) the pick falls
/// back to today's `chunk_weight` draw, so an untagged catalog behaves exactly as
/// before.
#[allow(clippy::too_many_arguments)]
fn try_place(
    cat: &Catalog,
    anchor: &OpenSocket,
    platforms: &[Platform],
    grid: &CoarseGrid,
    level: u32,
    tier: u32,
    high_rise: bool,
    junctions_used: u32,
    max_junctions_u: u32,
    rng: &mut Rng,
    mut cursor: Option<&mut crate::pattern::PlanCursor<'_>>,
) -> Option<(Placement, usize)> {
    let eligible = |c: &Chunk| {
        c.min_level <= level
            && match c.role {
                ChunkRole::Path => true,
                ChunkRole::Junction => junctions_used < max_junctions_u,
                ChunkRole::Hub | ChunkRole::DeadEnd => false,
            }
    };
    for _ in 0..CANDIDATE_TRIALS {
        let ci = match cursor
            .as_mut()
            .and_then(|cur| cur.next_matching(|i| eligible(&cat.chunks[i])))
        {
            Some(i) => i,
            None => {
                let choices: Vec<usize> = cat
                    .chunks
                    .iter()
                    .enumerate()
                    .filter(|(_, c)| eligible(c))
                    .map(|(i, _)| i)
                    .collect();
                if choices.is_empty() {
                    return None;
                }
                let weights: Vec<f32> = choices
                    .iter()
                    .map(|&i| chunk_weight(&cat.chunks[i], tier, high_rise))
                    .collect();
                let chosen = weighted_pick(rng, &weights)?;
                choices[chosen]
            }
        };
        let chunk = &cat.chunks[ci];
        let mut valid: Vec<(Placement, usize)> = Vec::new();
        for (si, _) in chunk.sockets.iter().enumerate() {
            for turns in 0..4u32 {
                let Some(p) = plan_join(chunk, si, turns, anchor) else {
                    continue;
                };
                if placement_fits(&p, platforms, grid) {
                    valid.push((p, si));
                }
            }
        }
        if !valid.is_empty() {
            let pick = rng.below(valid.len());
            return Some(valid.swap_remove(pick));
        }
    }
    None
}

/// Commits a placement made by [`try_place`] (or the growth sweep). `at` is the index of the
/// consumed anchor in `open`. Ids are assigned by [`commit_platforms`].
#[allow(clippy::too_many_arguments)]
fn commit_placement(
    open: &mut Vec<OpenSocket>,
    at: usize,
    placement: Placement,
    used_socket: usize,
    platforms: &mut Vec<Platform>,
    grid: &mut CoarseGrid,
    next_id: &mut u16,
    gift_slots: &mut Vec<(Vec3, bool)>,
    altar_slots: &mut Vec<Vec3>,
    area_sum: &mut f32,
    chunk_count: &mut usize,
    junctions_used: &mut u32,
) -> Option<()> {
    let p = placement;
    open.swap_remove(at);
    let mut socks = p.sockets;
    if used_socket < socks.len() {
        socks.swap_remove(used_socket);
    }
    open.extend(socks);
    *area_sum += p.top_area;
    *chunk_count += 1;
    if p.role == ChunkRole::Junction {
        *junctions_used += 1;
    }
    gift_slots.extend(p.gift_slots);
    altar_slots.extend(p.altar_slots);
    commit_platforms(platforms, grid, p.platforms, next_id)
}

/// Commits the railing-less 3 u stub for `socket` iff it satisfies the placement rules.
/// Returns `true` when the stub was placed.
fn commit_stub(
    socket: &OpenSocket,
    platforms: &mut Vec<Platform>,
    grid: &mut CoarseGrid,
    next_id: &mut u16,
) -> Option<bool> {
    let bounds = stub_bounds(socket);
    for idx in grid.candidates(&bounds) {
        if !platform_pair_ok(&bounds, &platforms[idx].bounds) {
            return Some(false);
        }
    }
    commit_platforms(platforms, grid, vec![(PLATFORM_NORMAL, bounds)], next_id)?;
    Some(true)
}

/// Test hook: runs the full generate pipeline (regeneration attempts, then the
/// guaranteed-valid fallbacks) against an explicit catalog instead of the shared one.
/// Normal callers want [`generate`] / [`generate_for`].
pub fn generate_with_catalog(cat: &Catalog, seed: u32, level: u32, players: u32) -> GeneratedLevel {
    generate_with_catalog_spacing(cat, seed, level, players, 1.0)
}

/// [`generate_with_catalog`] with an explicit gift-spacing multiplier (Clustered Gifts curse).
fn generate_with_catalog_spacing(
    cat: &Catalog,
    seed: u32,
    level: u32,
    players: u32,
    spacing_mul: f32,
) -> GeneratedLevel {
    let event = event_for(seed, level);
    for attempt in 0..MAX_ATTEMPTS {
        let aseed = attempt_seed(seed, attempt);
        if let Some(cand) = try_build(cat, aseed, seed, level, players, event, spacing_mul)
            && validate(&cand.data).is_ok()
        {
            return cand;
        }
    }
    if let Some(cand) = catalog_fallback(cat, seed, level, players, spacing_mul)
        && validate(&cand.data).is_ok()
    {
        return cand;
    }
    builtin_fallback(seed, level, players, spacing_mul)
}

/// The directed platform graph used for reachability checks, exposed so integration tests
/// plan routes over exactly the same edges `validate` uses: an edge `i -> j` exists iff a
/// player can get from platform `i`'s top to platform `j`'s top — touching with rise <=
/// 0.55 u (or a touching drop, rise < -0.55 u at gap ~0), a fair design-envelope jump
/// edge-to-edge (design max gap for the rise minus a 1.0 u comfort margin, landing's
/// smaller horizontal extent >= 2.0 u), or a jump pad reaching +3.5 u within 5.0 u
/// (landing still >= 2.0 u). All edges are admitted through the same [`can_jump_or_step`]
/// test, so this is by construction the same graph `validate` checks.
pub fn platform_graph(level: &LevelData) -> Vec<Vec<usize>> {
    let platforms = &level.platforms;
    let n = platforms.len();
    let mut adj: Vec<Vec<usize>> = vec![Vec::new(); n];
    for i in 0..n {
        for j in 0..n {
            if i != j && can_jump_or_step(&platforms[i], &platforms[j]) {
                adj[i].push(j);
            }
        }
    }
    adj
}

/// What the reachability graph says about a level, for the audit.
///
/// [`validate`] already refuses a level whose gifts, altars or beacon are unreachable, so
/// `unreachable_gifts` is expected to be zero on every shipped level. The point of this
/// report is to *prove* that invariant over thousands of generated levels instead of
/// trusting the generator, and to name the platforms the graph cannot reach: the
/// decorative pillars and floating blocks that read to a player as "a gap too big to
/// jump". Those are not defects on their own — a pillar with no gift on it is scenery —
/// but a gift on one is an uncollectable gift, which softlocks the collect phase.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ReachabilityReport {
    /// Platform indices the spawn cannot reach.
    pub unreachable: Vec<usize>,
    /// Gifts whose supporting platform is unreachable.
    pub unreachable_gifts: usize,
    /// The rejected jump that came closest to being legal, as `(gap, rise, excess)`.
    ///
    /// `excess` is how far past the design envelope the gap sits, so a small excess is a
    /// near-miss worth looking at and a large one is two platforms that were never meant
    /// to connect. Only pairs within [`NEAR_MISS_RANGE`] are considered. `None` when no
    /// pair of platforms is a near-miss.
    pub near_miss: Option<(f32, f32, f32)>,
}

/// How far apart two platforms may be and still count as a near-miss pair, in world units.
///
/// Beyond this they are not neighbours: the widest legal jump is 8.0 u (a deep drop), so a
/// pair further apart than this was never a candidate and reporting it as "almost
/// jumpable" would be noise.
pub const NEAR_MISS_RANGE: f32 = 12.0;

/// Reachability verdict for `level`, for the audit.
///
/// Walks the same [`platform_graph`] edges [`validate`] uses, from platform 0 (the spawn
/// platform, which `validate` requires the spawn to sit on), then reports what it could
/// not reach and how close the closest rejected jump came to being legal.
pub fn reachability_report(level: &LevelData) -> ReachabilityReport {
    let platforms = &level.platforms;
    let n = platforms.len();
    if n == 0 {
        return ReachabilityReport::default();
    }

    let adj = platform_graph(level);
    let mut reach = vec![false; n];
    reach[0] = true;
    let mut queue = VecDeque::from([0usize]);
    while let Some(a) = queue.pop_front() {
        for &b in &adj[a] {
            if !reach[b] {
                reach[b] = true;
                queue.push_back(b);
            }
        }
    }

    let unreachable: Vec<usize> = (0..n).filter(|&i| !reach[i]).collect();

    let mut unreachable_gifts = 0usize;
    for gift in &level.gifts {
        let top = gift.pos.y - 0.3;
        if let Some(i) = platform_top_under(platforms, gift.pos.x, gift.pos.z, top, 0.0)
            && !reach[i]
        {
            unreachable_gifts += 1;
        }
    }

    // The near-miss: among rejected pairs that are close enough to have been candidates,
    // the one whose gap sits closest to its own design limit.
    let mut near_miss: Option<(f32, f32, f32)> = None;
    for i in 0..n {
        for j in 0..n {
            if i == j || adj[i].contains(&j) {
                continue;
            }
            let gap = rect_gap(&platforms[i].bounds, &platforms[j].bounds);
            if gap > NEAR_MISS_RANGE {
                continue;
            }
            let rise = edge_rise(&platforms[i], &platforms[j]);
            let Some(limit) = design_max_gap(rise) else {
                continue;
            };
            // `can_jump_or_step` allows `limit - 1.0`; anything past that was rejected on
            // the gap, which is the only rejection this metric is about.
            let excess = gap - (limit - 1.0);
            if excess <= 0.0 {
                continue;
            }
            if near_miss.is_none_or(|(_, _, best)| excess < best) {
                near_miss = Some((gap, rise, excess));
            }
        }
    }

    ReachabilityReport {
        unreachable,
        unreachable_gifts,
        near_miss,
    }
}

// ------------------------------------------------------------- tests ----

#[cfg(test)]
#[allow(dead_code)] // shared fixtures; not every helper is used by every build of the tests
mod tests {
    use super::*;
    use protocol::{PLATFORM_RAMP_NX, PLATFORM_RAMP_NZ, PLATFORM_RAMP_PX, PLATFORM_RAMP_PZ};

    /// A ramp kind must turn with its geometry, or the collision surface stops
    /// matching the visible wedge. One quarter-turn sends +X to -Z, so the cycle
    /// is PX -> NZ -> NX -> PZ -> PX.
    #[test]
    fn ramp_kinds_rotate_with_their_geometry() {
        assert_eq!(rotate_ramp_kind(PLATFORM_RAMP_PX, 0), PLATFORM_RAMP_PX);
        assert_eq!(rotate_ramp_kind(PLATFORM_RAMP_PX, 1), PLATFORM_RAMP_NZ);
        assert_eq!(rotate_ramp_kind(PLATFORM_RAMP_PX, 2), PLATFORM_RAMP_NX);
        assert_eq!(rotate_ramp_kind(PLATFORM_RAMP_PX, 3), PLATFORM_RAMP_PZ);
        assert_eq!(rotate_ramp_kind(PLATFORM_RAMP_PX, 4), PLATFORM_RAMP_PX);
        // Every kind returns to itself after four turns.
        for kind in [PLATFORM_RAMP_PX, PLATFORM_RAMP_NX, PLATFORM_RAMP_PZ, PLATFORM_RAMP_NZ] {
            assert_eq!(rotate_ramp_kind(kind, 4), kind);
            // A single turn must actually change the axis, never leave it alone.
            assert_ne!(rotate_ramp_kind(kind, 1), kind);
        }
        // Non-ramp kinds are untouched.
        assert_eq!(
            rotate_ramp_kind(protocol::PLATFORM_NORMAL, 1),
            protocol::PLATFORM_NORMAL
        );
        assert_eq!(
            rotate_ramp_kind(protocol::PLATFORM_JUMP_PAD, 3),
            protocol::PLATFORM_JUMP_PAD
        );
    }

    /// The rotated kind must agree with the rotated geometry: the surface rises
    /// along the axis the kind names, at the corner the geometry puts it.
    #[test]
    fn rotated_ramp_surface_rises_along_the_rotated_kind() {
        // A ramp rising toward +X, 4 u run, 2 u rise.
        let aabb = Aabb {
            min: Vec3::new(0.0, 0.0, 0.0),
            max: Vec3::new(4.0, 2.0, 4.0),
        };
        for turns in 0..4u32 {
            let kind = rotate_ramp_kind(PLATFORM_RAMP_PX, turns);
            let rotated = rotate_aabb(&aabb, turns);
            let platform = protocol::Platform {
                id: 0,
                kind,
                bounds: rotated,
            };
            // Sample the two ends of the slope axis and require the high end to be
            // the one the kind claims.
            let (low, high) = match kind {
                PLATFORM_RAMP_PX => (
                    Vec3::new(rotated.min.x, 0.0, (rotated.min.z + rotated.max.z) * 0.5),
                    Vec3::new(rotated.max.x, 0.0, (rotated.min.z + rotated.max.z) * 0.5),
                ),
                PLATFORM_RAMP_NX => (
                    Vec3::new(rotated.max.x, 0.0, (rotated.min.z + rotated.max.z) * 0.5),
                    Vec3::new(rotated.min.x, 0.0, (rotated.min.z + rotated.max.z) * 0.5),
                ),
                PLATFORM_RAMP_PZ => (
                    Vec3::new((rotated.min.x + rotated.max.x) * 0.5, 0.0, rotated.min.z),
                    Vec3::new((rotated.min.x + rotated.max.x) * 0.5, 0.0, rotated.max.z),
                ),
                _ => (
                    Vec3::new((rotated.min.x + rotated.max.x) * 0.5, 0.0, rotated.max.z),
                    Vec3::new((rotated.min.x + rotated.max.x) * 0.5, 0.0, rotated.min.z),
                ),
            };
            let low_h = crate::world::surface_at(&platform, low.x, low.z);
            let high_h = crate::world::surface_at(&platform, high.x, high.z);
            assert!(
                high_h > low_h,
                "turns={turns} kind={kind}: surface must rise from {low_h} to {high_h}"
            );
        }
    }

    /// A ramp is anchored when its high edge meets a surface the player can step onto,
    /// and orphaned when it rises into open air. The tolerance is the walkable step
    /// limit, so a 0.5 u lip counts as a join and a 2 u wall does not.
    #[test]
    fn orphaned_ramps_finds_ramps_that_rise_into_nothing() {
        let plat = |kind: u8, min: [f32; 3], max: [f32; 3]| protocol::Platform {
            id: 0,
            kind,
            bounds: Aabb {
                min: Vec3::new(min[0], min[1], min[2]),
                max: Vec3::new(max[0], max[1], max[2]),
            },
        };
        // A ramp rising toward +X from y=0 at x=0 to y=4 at x=8.
        let ramp = plat(PLATFORM_RAMP_PX, [0.0, 0.0, 0.0], [8.0, 4.0, 4.0]);

        // Deck flush with the high edge: anchored.
        let flush = plat(protocol::PLATFORM_NORMAL, [8.0, 3.5, 0.0], [14.0, 4.0, 4.0]);
        assert!(orphaned_ramps(&[ramp.clone(), flush]).is_empty());

        // Deck 0.5 u above the high edge: still a walkable step, so anchored.
        let lip = plat(protocol::PLATFORM_NORMAL, [8.0, 4.0, 0.0], [14.0, 4.5, 4.0]);
        assert!(orphaned_ramps(&[ramp.clone(), lip]).is_empty());

        // Deck 2 u above the high edge: the player walks into a wall.
        let wall = plat(protocol::PLATFORM_NORMAL, [8.0, 5.5, 0.0], [14.0, 6.0, 4.0]);
        assert_eq!(orphaned_ramps(&[ramp.clone(), wall]), vec![0]);

        // Nothing at the high edge at all.
        let far = plat(protocol::PLATFORM_NORMAL, [20.0, 3.5, 0.0], [26.0, 4.0, 4.0]);
        assert_eq!(orphaned_ramps(&[ramp.clone(), far]), vec![0]);

        // A ramp leading onto another ramp is anchored when the surfaces meet: the
        // second ramp's low edge is at y=4 at x=8 and rises to y=8 at x=16. The second
        // ramp is itself orphaned here (nothing meets its own high edge), so only the
        // first ramp's verdict is asserted.
        let up = plat(PLATFORM_RAMP_PX, [8.0, 4.0, 0.0], [16.0, 8.0, 4.0]);
        assert!(!orphaned_ramps(&[ramp, up]).contains(&0));
    }

    /// A level built from a deck plus one extra block, for the reachability report.
    ///
    /// `block_x` is the block's near edge, so the caller controls the gap; `block_top` is
    /// its top surface, so the caller controls the rise.
    fn reachability_fixture(block_top: f32, block_x: f32, gift_on_block: bool) -> LevelData {
        let plat = |id: u16, kind: u8, min: [f32; 3], max: [f32; 3]| protocol::Platform {
            id,
            kind,
            bounds: Aabb {
                min: Vec3::new(min[0], min[1], min[2]),
                max: Vec3::new(max[0], max[1], max[2]),
            },
        };
        // Platform 0 is the spawn deck (validate requires the spawn to sit on it).
        let deck = plat(0, protocol::PLATFORM_NORMAL, [0.0, 0.0, 0.0], [8.0, 0.5, 8.0]);
        // A 2x2 block whose near edge is `block_x` and whose top is `block_top`.
        let block = plat(
            1,
            protocol::PLATFORM_NORMAL,
            [block_x, 0.0, 3.0],
            [block_x + 2.0, block_top, 5.0],
        );
        let mut gifts = vec![protocol::Gift {
            id: 0,
            pos: Vec3::new(4.0, 0.8, 4.0),
            kind: GIFT_NORMAL,
        }];
        if gift_on_block {
            gifts.push(protocol::Gift {
                id: 1,
                pos: Vec3::new(block_x + 1.0, block_top + 0.3, 4.0),
                kind: GIFT_NORMAL,
            });
        }
        LevelData {
            level: 1,
            seed: 0,
            spawn: Vec3::new(4.0, 0.5, 4.0),
            beacon: Vec3::new(4.0, 0.5, 4.0),
            kill_y: -20.0,
            platforms: vec![deck, block],
            gifts,
            event: EVENT_NONE,
            altars: Vec::new(),
        }
    }

    /// A block whose top is 4.5 u above the deck is 2.5 u above the jump apex, so no jump
    /// reaches it however close it sits. The rejection is on HEIGHT, not on gap, so there
    /// is no near-miss gap to report.
    #[test]
    fn reachability_report_flags_a_block_no_jump_can_reach() {
        let level = reachability_fixture(4.5, 9.5, false);
        let report = reachability_report(&level);
        assert_eq!(report.unreachable, vec![1], "the 4.5 u block is unreachable");
        assert_eq!(report.unreachable_gifts, 0, "no gift sits on it");
        assert_eq!(
            report.near_miss, None,
            "a too-high block is rejected on height, not on gap"
        );
    }

    /// The same block lowered to a 1.5 u rise is inside the design envelope, so it is
    /// reachable and there is no near-miss left to report.
    #[test]
    fn reachability_report_clears_a_block_inside_the_envelope() {
        let level = reachability_fixture(2.0, 9.5, false);
        let report = reachability_report(&level);
        assert!(report.unreachable.is_empty(), "a 1.5 u rise is jumpable");
        assert_eq!(report.near_miss, None, "nothing was rejected");
    }

    /// A block at a jumpable height but too far away is the "gap too big to jump" case.
    /// The report must surface it as a near-miss with the measured gap and rise.
    #[test]
    fn reachability_report_measures_a_gap_too_wide_to_jump() {
        // Rise 0.5 u (design limit 5.5 u, allowed 4.5 u) with a 6.5 u gap.
        let level = reachability_fixture(1.0, 14.5, false);
        let report = reachability_report(&level);
        assert_eq!(report.unreachable, vec![1], "the far block is unreachable");
        let (gap, rise, excess) = report.near_miss.expect("a rejected jump was measured");
        assert!((gap - 6.5).abs() < 1e-3, "gap was {gap}");
        // The reported direction is the one closest to legal, which here is the drop back
        // to the deck: a drop allows 6.0 u (so 5.0 u after the comfort margin) against the
        // rise's 5.5 u (4.5 u), so the drop is the tighter rejection at 1.5 u of excess.
        assert!((rise + 0.5).abs() < 1e-3, "rise was {rise}");
        assert!((excess - 1.5).abs() < 1e-3, "excess was {excess}");
    }

    /// A gift on an unreachable block is an uncollectable gift. `validate` refuses such a
    /// level, so this is the assertion that proves the generator's promise holds.
    #[test]
    fn reachability_report_counts_gifts_on_unreachable_blocks() {
        let level = reachability_fixture(4.5, 9.5, true);
        let report = reachability_report(&level);
        assert_eq!(report.unreachable_gifts, 1);
        assert!(
            validate(&level).is_err(),
            "validate must refuse a level with an uncollectable gift"
        );
    }

    fn hub_json_variant() -> String {
        r#"{"version":1,"chunks":[
          {"id":"hub","name":"Test Hub","role":"hub","min_level":1,"difficulty":1,"tags":[],
           "platforms":[{"kind":"NORMAL","min":[-8,-1,-7],"max":[8,0,7]}],
           "sockets":[{"id":"n","pos":[0,0,7],"facing":"N","width":4},
                      {"id":"e","pos":[8,0,0],"facing":"E","width":4},
                      {"id":"s","pos":[0,0,-7],"facing":"S","width":4},
                      {"id":"w","pos":[-8,0,0],"facing":"W","width":4}],
           "gift_slots":[{"pos":[4,0.3,-4],"elevated":false}],
           "enemy_slots":[[5,2,0],[3.5,2,3.5],[0,2,5],[-3.5,2,3.5],[-5,2,0],[-3.5,2,-3.5],[0,2,-5],[3.5,2,-3.5]],
           "altar_slots":[],
           "beacon":[0,0,0]},
          {"id":"straight","name":"Straight","role":"path","min_level":1,"difficulty":1,"tags":[],
           "platforms":[{"kind":"NORMAL","min":[-4,-1,-1.5],"max":[4,0,1.5]}],
           "sockets":[{"id":"w","pos":[-4,0,0],"facing":"W","width":3},
                      {"id":"e","pos":[4,0,0],"facing":"E","width":3}],
           "gift_slots":[{"pos":[-2,0.3,0],"elevated":false},{"pos":[2,0.3,0],"elevated":false}],
           "enemy_slots":[],"altar_slots":[]},
          {"id":"ramp","name":"Ramp","role":"path","min_level":1,"difficulty":1,"tags":["ramp","climb"],
           "platforms":[{"kind":"NORMAL","min":[-4,-1,-1.5],"max":[0,0,1.5]},
                        {"kind":"RAMP_PX","min":[0,0,-1.5],"max":[4,1.0,1.5]}],
           "sockets":[{"id":"w","pos":[-4,0,0],"facing":"W","width":3},
                      {"id":"e","pos":[4,1.0,0],"facing":"E","width":3}],
           "gift_slots":[{"pos":[-2,0.3,0],"elevated":false}],
           "enemy_slots":[],"altar_slots":[]},
          {"id":"junction","name":"Junction","role":"junction","min_level":1,"difficulty":2,"tags":[],
           "platforms":[{"kind":"NORMAL","min":[-4,-1,-4],"max":[4,0,4]}],
           "sockets":[{"id":"w","pos":[-4,0,0],"facing":"W","width":4},
                      {"id":"n","pos":[0,0,4],"facing":"N","width":4},
                      {"id":"e","pos":[4,0,0],"facing":"E","width":4}],
           "gift_slots":[{"pos":[0,0.3,2],"elevated":false}],
           "enemy_slots":[],"altar_slots":[]},
          {"id":"dead_end","name":"Dead End","role":"dead_end","min_level":1,"difficulty":1,"tags":[],
           "platforms":[{"kind":"NORMAL","min":[-2.5,-1,-2.5],"max":[2.5,0,2.5]}],
           "sockets":[{"id":"w","pos":[-2.5,0,0],"facing":"W","width":3}],
           "gift_slots":[{"pos":[1.5,0.3,1.5],"elevated":false},{"pos":[-1.5,0.3,1.5],"elevated":false},
                         {"pos":[1.5,0.3,-1.5],"elevated":false},{"pos":[-1.5,0.3,-1.5],"elevated":false}],
           "enemy_slots":[],"altar_slots":[]}
         ]}"#
        .to_string()
    }

    fn test_catalog() -> Catalog {
        parse_catalog(&hub_json_variant()).expect("inline catalog must parse")
    }

    fn sparse_catalog() -> Catalog {
        let json = r#"{"version":1,"chunks":[
          {"id":"hub","name":"Test Hub","role":"hub","min_level":1,"difficulty":1,"tags":[],
           "platforms":[{"kind":"NORMAL","min":[-8,-1,-7],"max":[8,0,7]}],
           "sockets":[{"id":"n","pos":[0,0,7],"facing":"N","width":4},
                      {"id":"e","pos":[8,0,0],"facing":"E","width":4},
                      {"id":"s","pos":[0,0,-7],"facing":"S","width":4},
                      {"id":"w","pos":[-8,0,0],"facing":"W","width":4}],
           "gift_slots":[],
           "enemy_slots":[[5,2,0],[3.5,2,3.5],[0,2,5],[-3.5,2,3.5],[-5,2,0],[-3.5,2,-3.5],[0,2,-5],[3.5,2,-3.5]],
           "altar_slots":[],
           "beacon":[0,0,0]},
          {"id":"straight","name":"Straight","role":"path","min_level":1,"difficulty":1,"tags":[],
           "platforms":[{"kind":"NORMAL","min":[-4,-1,-1.5],"max":[4,0,1.5]}],
           "sockets":[{"id":"w","pos":[-4,0,0],"facing":"W","width":3},
                      {"id":"e","pos":[4,0,0],"facing":"E","width":3}],
           "gift_slots":[],
           "enemy_slots":[],"altar_slots":[]}
         ]}"#;
        parse_catalog(json).expect("sparse catalog must parse")
    }

    fn ok_level() -> LevelData {
        let p0 = Platform {
            id: 0,
            kind: PLATFORM_NORMAL,
            bounds: Aabb {
                min: Vec3::new(-6.0, -1.0, -6.0),
                max: Vec3::new(6.0, 0.0, 6.0),
            },
        };
        let p1 = Platform {
            id: 1,
            kind: PLATFORM_NORMAL,
            bounds: Aabb {
                min: Vec3::new(6.0, -1.0, -1.5),
                max: Vec3::new(12.0, 0.0, 1.5),
            },
        };
        let gifts = (0..3u16)
            .map(|i| Gift {
                id: i,
                pos: Vec3::new(-4.0 + 4.0 * i as f32, 0.3, -3.0),
                kind: GIFT_NORMAL,
            })
            .collect();
        LevelData {
            level: 1,
            seed: 1,
            spawn: Vec3::ZERO,
            beacon: Vec3::new(9.0, 0.0, 0.0),
            kill_y: -16.0,
            event: EVENT_NONE,
            platforms: vec![p0, p1],
            gifts,
            altars: Vec::new(),
        }
    }

    // ---------------- parse_catalog ----------------

    #[test]
    fn parse_rejects_bad_version() {
        assert!(parse_catalog(r#"{"version":2,"chunks":[]}"#).is_err());
    }

    #[test]
    fn parse_rejects_duplicate_ids() {
        let json = r#"{"version":1,"chunks":[
          {"id":"hub","name":"A","role":"hub","min_level":1,"difficulty":1,"platforms":[{"kind":"NORMAL","min":[0,-1,0],"max":[4,0,4]}],
           "sockets":[],"gift_slots":[],"enemy_slots":[[0,1,0],[1,1,0],[2,1,0],[3,1,0],[0,1,2],[1,1,2],[2,1,2],[3,1,2]],"altar_slots":[],"beacon":[1,0,1]},
          {"id":"hub","name":"B","role":"hub","min_level":1,"difficulty":1,"platforms":[{"kind":"NORMAL","min":[0,-1,0],"max":[4,0,4]}],
           "sockets":[],"gift_slots":[],"enemy_slots":[[0,1,0],[1,1,0],[2,1,0],[3,1,0],[0,1,2],[1,1,2],[2,1,2],[3,1,2]],"altar_slots":[],"beacon":[1,0,1]}
        ]}"#;
        let err = parse_catalog(json).unwrap_err();
        assert!(err.contains("duplicate"));
    }

    #[test]
    fn parse_rejects_bad_platform_kind_and_empty_box() {
        let bad_kind = r#"{"version":1,"chunks":[
          {"id":"hub","name":"A","role":"hub","min_level":1,"difficulty":1,"platforms":[{"kind":"GLASS","min":[0,-1,0],"max":[4,0,4]}],
           "sockets":[],"gift_slots":[],"enemy_slots":[[0,1,0],[1,1,0],[2,1,0],[3,1,0],[0,1,2],[1,1,2],[2,1,2],[3,1,2]],"altar_slots":[],"beacon":[1,0,1]}
        ]}"#;
        assert!(parse_catalog(bad_kind).is_err());
        let empty_box = r#"{"version":1,"chunks":[
          {"id":"hub","name":"A","role":"hub","min_level":1,"difficulty":1,"platforms":[{"kind":"NORMAL","min":[4,-1,0],"max":[0,0,4]}],
           "sockets":[],"gift_slots":[],"enemy_slots":[[0,1,0],[1,1,0],[2,1,0],[3,1,0],[0,1,2],[1,1,2],[2,1,2],[3,1,2]],"altar_slots":[],"beacon":[1,0,1]}
        ]}"#;
        assert!(parse_catalog(empty_box).is_err());
    }

    /// A ramp resting on a big platform touches it along its whole perimeter, so the
    /// edge has to be judged where the surfaces meet (the ramp's LOW edge), not at the
    /// box tops. Judging by box tops made every ramp-on-platform edge a +4 u jump the
    /// controller cannot perform, which disconnected whole levels.
    #[test]
    fn ramp_resting_on_a_platform_is_walkable_from_it() {
        let base = Platform {
            id: 0,
            kind: PLATFORM_NORMAL,
            bounds: Aabb {
                min: Vec3::new(0.0, -0.5, 0.0),
                max: Vec3::new(20.0, 0.0, 12.0),
            },
        };
        let ramp = Platform {
            id: 1,
            kind: PLATFORM_RAMP_PX,
            bounds: Aabb {
                min: Vec3::new(0.0, 0.0, 4.5),
                max: Vec3::new(8.0, 4.0, 7.5),
            },
        };
        assert_eq!(edge_rise(&base, &ramp), 0.0, "walk on at the ramp's low edge");
        assert_eq!(edge_rise(&ramp, &base), 0.0, "walk off at the ramp's low edge");
        assert!(can_jump_or_step(&base, &ramp));
        assert!(can_jump_or_step(&ramp, &base));

        // The ramp's high end meets a slab whose top is 0.5 u above it: a step, not a jump.
        let slab = Platform {
            id: 2,
            kind: PLATFORM_NORMAL,
            bounds: Aabb {
                min: Vec3::new(8.0, 4.0, 0.0),
                max: Vec3::new(14.0, 4.5, 12.0),
            },
        };
        assert!((edge_rise(&ramp, &slab) - 0.5).abs() < 1e-4);
        assert!(can_jump_or_step(&ramp, &slab));

        // A platform 4.5 u away and 2.5 u above the ramp's surface there is out of reach.
        let far_high = Platform {
            id: 3,
            kind: PLATFORM_NORMAL,
            bounds: Aabb {
                min: Vec3::new(0.0, 4.0, 12.0),
                max: Vec3::new(8.0, 4.5, 14.0),
            },
        };
        assert!(!can_jump_or_step(&ramp, &far_high));
    }

    #[test]
    fn parse_rejects_socket_off_edge_and_narrow_socket() {
        let off = r#"{"version":1,"chunks":[
          {"id":"hub","name":"A","role":"hub","min_level":1,"difficulty":1,"platforms":[{"kind":"NORMAL","min":[-4,-1,-4],"max":[4,0,4]}],
           "sockets":[{"id":"n","pos":[0,0,9],"facing":"N","width":4}],"gift_slots":[],
           "enemy_slots":[[0,1,0],[1,1,0],[2,1,0],[3,1,0],[0,1,2],[1,1,2],[2,1,2],[3,1,2]],"altar_slots":[],"beacon":[0,0,0]}
        ]}"#;
        let err = parse_catalog(off).unwrap_err();
        assert!(err.contains("edge"), "expected edge error, got {err}");
        let narrow = r#"{"version":1,"chunks":[
          {"id":"hub","name":"A","role":"hub","min_level":1,"difficulty":1,"platforms":[{"kind":"NORMAL","min":[-4,-1,-4],"max":[4,0,4]}],
           "sockets":[{"id":"n","pos":[0,0,4],"facing":"N","width":2.9}],"gift_slots":[],
           "enemy_slots":[[0,1,0],[1,1,0],[2,1,0],[3,1,0],[0,1,2],[1,1,2],[2,1,2],[3,1,2]],"altar_slots":[],"beacon":[0,0,0]}
        ]}"#;
        assert!(parse_catalog(narrow).is_err());
    }

    #[test]
    fn parse_rejects_hub_without_beacon_or_enemy_slots() {
        let no_beacon = r#"{"version":1,"chunks":[
          {"id":"hub","name":"A","role":"hub","min_level":1,"difficulty":1,"platforms":[{"kind":"NORMAL","min":[-4,-1,-4],"max":[4,0,4]}],
           "sockets":[],"gift_slots":[],"enemy_slots":[[0,1,0],[1,1,0],[2,1,0],[3,1,0],[0,1,2],[1,1,2],[2,1,2],[3,1,2]],"altar_slots":[]}
        ]}"#;
        assert!(parse_catalog(no_beacon).is_err());
        let few_slots = r#"{"version":1,"chunks":[
          {"id":"hub","name":"A","role":"hub","min_level":1,"difficulty":1,"platforms":[{"kind":"NORMAL","min":[-4,-1,-4],"max":[4,0,4]}],
           "sockets":[],"gift_slots":[],"enemy_slots":[[0,1,0],[1,1,0]],"altar_slots":[],"beacon":[0,0,0]}
        ]}"#;
        assert!(parse_catalog(few_slots).is_err());
    }

    #[test]
    fn parse_rejects_malformed_json_and_unknown_facing() {
        assert!(parse_catalog("not json").is_err());
        let bad_facing = r#"{"version":1,"chunks":[
          {"id":"hub","name":"A","role":"hub","min_level":1,"difficulty":1,"platforms":[{"kind":"NORMAL","min":[-4,-1,-4],"max":[4,0,4]}],
           "sockets":[{"id":"n","pos":[0,0,4],"facing":"NW","width":4}],"gift_slots":[],
           "enemy_slots":[[0,1,0],[1,1,0],[2,1,0],[3,1,0],[0,1,2],[1,1,2],[2,1,2],[3,1,2]],"altar_slots":[],"beacon":[0,0,0]}
        ]}"#;
        assert!(parse_catalog(bad_facing).is_err());
    }

    // ---------------- geometry / rotation ----------------

    #[test]
    fn rotate_xz_quarter_turns() {
        assert_eq!(rotate_xz(1.0, 0.0, 1), (0.0, -1.0)); // E -> S
        assert_eq!(rotate_xz(0.0, 1.0, 1), (1.0, 0.0)); //  N -> E
        assert_eq!(rotate_xz(1.0, 0.0, 2), (-1.0, 0.0)); // E -> W
        assert_eq!(rotate_xz(1.0, 0.0, 4), (1.0, 0.0)); //  identity
        assert_eq!(Facing::N.turn(1), Facing::E);
        assert_eq!(Facing::E.turn(1), Facing::S);
        assert_eq!(Facing::W.turn(2), Facing::E);
        assert_eq!(Facing::S.opposite(), Facing::N);
    }

    #[test]
    fn rotate_aabb_four_turns_is_identity() {
        let b = Aabb {
            min: Vec3::new(2.0, -1.0, 3.0),
            max: Vec3::new(5.0, 0.0, 8.0),
        };
        for k in 0..4u32 {
            let r = rotate_aabb(&b, k * 4);
            assert_eq!(r.min, b.min);
            assert_eq!(r.max, b.max);
        }
        let r = rotate_aabb(&b, 1);
        assert_eq!(r.min.x, b.min.z);
        assert_eq!(r.max.x, b.max.z);
        assert_eq!(r.min.z, -b.max.x);
        assert_eq!(r.max.z, -b.min.x);
    }
}

/// Half-extent (XZ) of the gift body box used by the spawn-safety rule (T-062).
const GIFT_BODY_HALF: f32 = 0.35;

/// Height of the gift body box above its base (base = pos.y - 0.3).
const GIFT_BODY_HEIGHT: f32 = 0.9;

/// Lower clamp for the gift-spacing multiplier (0.25 => a 0.5 u minimum gift spacing at the
/// 2.0 u base). The curse can only make gifts denser, never spread.
const GIFT_SPACING_MUL_MIN: f32 = 0.25;

/// The volume a spawned gift occupies: a small box rising from its support surface
/// (base = pos.y - 0.3). Used by the gift spawn-safety rule.
fn gift_body_aabb(pos: Vec3) -> Aabb {
    let base = pos.y - 0.3;
    Aabb {
        min: Vec3::new(pos.x - GIFT_BODY_HALF, base + EPS, pos.z - GIFT_BODY_HALF),
        max: Vec3::new(pos.x + GIFT_BODY_HALF, base + GIFT_BODY_HEIGHT, pos.z + GIFT_BODY_HALF),
    }
}

/// True when no blocking volume overlaps the gift body box at `pos`.
/// Kept free of `Platform` (pure AABBs in, bool out) so the spawn-safety rule stays
/// trivially unit-testable. Blockers are the non-NORMAL platforms (walls, jump pads, ramps).
fn gift_body_clear<'a>(blockers: impl Iterator<Item = &'a Aabb>, pos: Vec3) -> bool {
    let body = gift_body_aabb(pos);
    !blockers.into_iter().any(|b| aabb_intersect(&body, b))
}

/// Minimum gift spacing after the Clustered Gifts curse: the base constant scaled by
/// `spacing_mul`, always within [`GIFT_MIN_SPACING` * 0.25, `GIFT_MIN_SPACING`] = [0.5 u, 2.0 u].
fn gift_min_spacing(spacing_mul: f32) -> f32 {
    let mul = if spacing_mul.is_finite() && spacing_mul > 0.0 {
        spacing_mul.clamp(GIFT_SPACING_MUL_MIN, 1.0)
    } else {
        1.0
    };
    GIFT_MIN_SPACING * mul
}

/// Unit tests for the T-062 gift spawn-safety rule (pure geometry, no `Platform` needed).
#[cfg(test)]
mod gift_spawn_safety_tests {
    use super::*;

    fn aabb(min: (f32, f32, f32), max: (f32, f32, f32)) -> Aabb {
        Aabb {
            min: Vec3::new(min.0, min.1, min.2),
            max: Vec3::new(max.0, max.1, max.2),
        }
    }

    #[test]
    fn gift_body_clear_rejects_wall_overlap() {
        // Wall shares XZ with the gift and spans its body height.
        let wall = aabb((0.0, 0.0, 0.0), (1.0, 3.0, 1.0));
        let pos = Vec3::new(0.5, 0.3, 0.5); // base at y = 0
        assert!(!gift_body_clear([&wall].into_iter(), pos));
    }

    #[test]
    fn gift_body_clear_rejects_jump_pad_overlap() {
        let pad = aabb((-0.5, 0.0, -0.5), (0.5, 0.4, 0.5));
        let pos = Vec3::new(0.0, 0.3, 0.0);
        assert!(!gift_body_clear([&pad].into_iter(), pos));
    }

    #[test]
    fn gift_body_clear_accepts_unobstructed_site() {
        let wall = aabb((5.0, 0.0, 5.0), (6.0, 3.0, 6.0));
        let pos = Vec3::new(0.0, 0.3, 0.0);
        assert!(gift_body_clear([&wall].into_iter(), pos));
        assert!(gift_body_clear(std::iter::empty(), pos));
    }

    #[test]
    fn gift_body_clear_allows_touching_face() {
        // Wall min.x exactly at the body face x = 0.35: touching is not blocking (aabb_intersect semantics).
        let wall = aabb((0.35, 0.0, -5.0), (1.0, 0.9, 5.0));
        let pos = Vec3::new(0.0, 0.3, 0.0);
        assert!(gift_body_clear([&wall].into_iter(), pos));
    }
}
