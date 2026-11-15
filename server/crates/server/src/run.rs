//! Run state and rules: lobby drafts, generated levels, shared gift progress, golden-gift currency, lives,
//! the enemies the team drafted, the lobby shop, level events and altars. Pure game logic on top of `sim` —
//! no I/O, fully unit-testable.
//!
//! Flow: Lobby (spawn hub + draft offer and, from level 2 on, the shop) → all players vote → generate a level →
//! Playing (two phases: LEVEL_COLLECT until every gift is gathered, then LEVEL_COLLAPSE_PENDING — the siren — and
//! LEVEL_COLLAPSING; golden gifts exist at the collected positions, the beacon opens, the non-hub platforms fall
//! in six farthest-first batches) → beacon → Lobby for the next level. The spawn hub is exempt from collapse.
//! During the collapse/exit window every connected player votes on one permanent addition for the rest of the run
//! (T-078); the automatic one-enemy-per-level scaling is gone.
//! Death with no lives left ends that player's run; when everyone is down the run is Over and Restart begins a new one.
//!
//! Altars are single-use risk/reward deals: Shortcut, Gift Sight, Clear Sky and Debt. Their effects live in
//! `AltarLedger`, survive level transitions and are serialized by `Run::altar_state` for a server restart.

use crate::content::{Content, RunMods};
use protocol::{
    Aabb, CARD_CURSE, CARD_ENEMY, CARD_UPGRADE, DraftCard, EVENT_ICE, EnemyView, LEVEL_COLLAPSE_PENDING,
    LEVEL_COLLAPSING, LEVEL_COLLECT, LevelData, PHASE_ENEMY_DRAFT, PHASE_OVER, PHASE_PLAYING, Platform, RunInfo,
    TICK_HZ, TILE_GONE, TILE_WARN, TileState, Tuning, Vec3,
};
use sim::hazards::Hazards;
use sim::levelgen;
use sim::rules::enemies::chorus_candidate_allowed;
use sim::rules::{self, BEACON_RADIUS, BEHAVIOR_MIMIC, Enemy, GIFT_RADIUS};
use sim::world::World;
use std::collections::BTreeMap;

/// Currency granted per golden gift (design §1.2: one golden gift is worth 1 gold).
pub const GOLDEN_VALUE: u32 = 1;
/// Enemy cards offered at the start of a run, and how many the team keeps.
const ENEMY_OFFER: usize = 4;
const ENEMY_PICKS: u8 = 2;
/// Reward cards offered before each level (one is kept).
const REWARD_PICKS: u8 = 1;
/// Every Nth level offers greater curses only.
const GREATER_EVERY: u32 = 10;
/// Shop items offered per lobby (first, plain lobby gets none).
const SHOP_OFFER: usize = 4;
/// Cost of a contextual upgrade card offered in the shop.
const SHOP_UPGRADE_COST: u32 = 25;
/// Cost of a curse-removal (purge) item; stock equals the stacks of that curse held.
const SHOP_PURGE_COST: u32 = 30;
/// How many distinct upgrade cards are drawn into the shop per lobby.
const SHOP_UPGRADE_DRAWS: usize = 2;
/// Ticks of siren/banner before the collapse proper begins (design §1.4: 3.0 s).
const COLLAPSE_PENDING_TICKS: u32 = 3 * TICK_HZ;
/// The countdown of a vote session, in TICK_HZ ticks (20 s at 20 Hz).
pub const VOTE_COUNTDOWN_TICKS: u32 = 20 * TICK_HZ;
/// The number of options a vote session offers (the registry pool's first candidates, registry §9).
pub const VOTE_POOL_OFFER: usize = 3;
/// Salt separating the vote tie-break coin from the curse-injection coin (both derive from the run seed and level).
const VOTE_COIN_SALT: u32 = 0xA5A5_5A5A;
/// Salt separating the curse-injection draw from the tie-break coin.
const VOTE_CURSE_SALT: u32 = 0x3A6F_1D2B;

/// Currency advanced by the Debt altar.
pub const DEBT_ADVANCE: u32 = 30;
/// Levels of golden-gift income that repay a Debt altar advance.
pub const DEBT_REPAY_LEVELS: u8 = 2;
/// Temporary curses applied by the Shortcut altar.
pub const SHORTCUT_CURSES: usize = 2;
/// Extra enemy copies added to the next level by the Shortcut altar.
pub const SHORTCUT_EXTRA_ENEMIES: u32 = 2;
/// Number of altar designs; the altar id selects one with `id % ALTAR_KINDS`.
pub const ALTAR_KINDS: u16 = 4;

/// One option offered by a vote session. The wire copy is `protocol::VoteOption`; this carries the
/// server-side `temporary` flag the client does not need.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionOption {
    /// Position of this option in the offer (0-based; what the client votes with).
    pub index: u8,
    /// Stable id (registry id for enemies, content id for curses).
    pub id: String,
    /// Display name (registry name for enemies; the curse id when no curse name is available).
    pub name: String,
    /// `CARD_ENEMY` or `CARD_CURSE`.
    pub kind: u8,
    /// True when winning this option summons a one-level temporary enemy instead of a permanent (registry §9 overflow).
    pub temporary: bool,
}

/// The outcome of a closed vote session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VoteOutcome {
    /// Index of the winning option (matches `SessionOption::index`).
    pub winner: u8,
    /// Id of the winning option.
    pub id: String,
    /// Display name of the winning option.
    pub name: String,
    /// `CARD_ENEMY` or `CARD_CURSE`.
    pub kind: u8,
    /// True when the winner joins the run's permanents (enemy votes that are not overflow summons).
    pub permanent: bool,
    /// True when the winner is a one-level temporary summon (registry §9 overflow).
    pub temporary: bool,
    /// Final per-option ballot counts.
    pub counts: Vec<u32>,
}

/// A vote session: the options on offer, the ballots keyed by player id, the host and the countdown.
///
/// Ballots are keyed by player id, never by connection, so a disconnect does not destroy a cast ballot.
/// A reconnected player is a fresh player id; the old ballot still counts. All draws and tie-breaks are
/// seeded from the run seed mixed with the level — never from the clock.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VoteSession {
    /// The level the vote belongs to (the level being exited).
    pub level: u32,
    /// The options on offer (never more than `VOTE_POOL_OFFER`).
    options: Vec<SessionOption>,
    /// Ballots keyed by player id; re-voting replaces the player's earlier ballot.
    ballots: BTreeMap<u32, u8>,
    /// Ticks remaining before the session closes on its own.
    ticks_left: u32,
    /// The host's player id (the lowest connected player id when the session opened).
    host: Option<u32>,
    /// The run seed mixed with the level; drives every deterministic coin flip in this session.
    draw_seed: u32,
    /// Last whole second that was broadcast in `VoteState` (`None` before the first broadcast).
    pub last_sent_second: Option<u16>,
}

impl VoteSession {
    /// Open a session for `level` with the offered `options`.
    pub fn open(level: u32, seed: u32, options: Vec<SessionOption>, host: Option<u32>) -> Self {
        VoteSession {
            level,
            options,
            ballots: BTreeMap::new(),
            ticks_left: VOTE_COUNTDOWN_TICKS,
            host,
            draw_seed: mix_seed(seed, level),
            last_sent_second: None,
        }
    }

    /// The options on offer.
    pub fn options(&self) -> &[SessionOption] {
        &self.options
    }

    /// Whole seconds left in the countdown (rounds up; 20 right after opening).
    pub fn seconds_left(&self) -> u16 {
        u16::try_from(self.ticks_left.div_ceil(TICK_HZ)).unwrap_or(0)
    }

    /// Cast (or re-cast) a ballot. Fails when the session is closed or the option index is out of range.
    pub fn cast(&mut self, player_id: u32, option: u8) -> Result<(), &'static str> {
        if self.ticks_left == 0 {
            return Err("the vote is closed");
        }
        if usize::from(option) >= self.options.len() {
            return Err("option out of range");
        }
        self.ballots.insert(player_id, option);
        Ok(())
    }

    /// Player ids holding a ballot, ascending.
    pub fn voted_ids(&self) -> Vec<u32> {
        self.ballots.keys().copied().collect()
    }

    /// True when `player_id` has cast a ballot in this session.
    pub fn has_voted(&self, player_id: u32) -> bool {
        self.ballots.contains_key(&player_id)
    }

    /// Per-option ballot counts, in option order.
    pub fn counts(&self) -> Vec<u32> {
        let mut counts = vec![0u32; self.options.len()];
        for &option in self.ballots.values() {
            if let Some(slot) = counts.get_mut(usize::from(option)) {
                *slot = slot.saturating_add(1);
            }
        }
        counts
    }

    /// Advance the countdown one tick. Returns true on the tick the session closes.
    pub fn tick(&mut self) -> bool {
        if self.ticks_left > 0 {
            self.ticks_left -= 1;
        }
        self.ticks_left == 0
    }

    /// Resolve the ballots: majority wins; a tie goes to the host's ballot when the host voted for a tied
    /// option, otherwise to a deterministic coin flip from the run seed and the level. With no ballots at all
    /// the coin is flipped across every option. Returns `None` only when no option was offered.
    pub fn resolve(&self) -> Option<VoteOutcome> {
        if self.options.is_empty() {
            return None;
        }
        let counts = self.counts();
        let max = counts.iter().copied().max().unwrap_or(0);
        let tied: Vec<usize> = counts
            .iter()
            .enumerate()
            .filter(|&(_, &c)| c == max)
            .map(|(i, _)| i)
            .collect();
        let winner = if tied.len() == 1 {
            tied[0]
        } else {
            match self.host.and_then(|h| self.ballots.get(&h).copied()).map(usize::from) {
                Some(ballot) if tied.contains(&ballot) => ballot,
                _ => {
                    let mut rng = Rng::new(self.draw_seed.wrapping_add(VOTE_COIN_SALT));
                    tied[rng.below(tied.len())]
                }
            }
        };
        let option = &self.options[winner];
        Some(VoteOutcome {
            winner: option.index,
            id: option.id.clone(),
            name: option.name.clone(),
            kind: option.kind,
            permanent: option.kind == CARD_ENEMY && !option.temporary,
            temporary: option.temporary,
            counts,
        })
    }
}

/// One registry entry reduced to the fields the pool rules use (registry §9).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PoolEntry {
    /// Stable enemy id.
    pub id: String,
    /// Plain name for the vote UI.
    pub name: String,
    /// First level the enemy may be offered.
    pub min_level: u32,
    /// Permanent copies allowed in one run.
    pub selection_cap: u32,
    /// Tags (used by `requires` hooks).
    pub tags: Vec<String>,
    /// Class id → relation; only ever compared against `"ignores"` here.
    pub class_interactions: BTreeMap<String, String>,
    /// `requires` hooks as (tag, count) pairs.
    pub requires: Vec<(String, u32)>,
    /// Position in design/content.json (network kind index); ties in the sort are broken by this.
    pub content_order: usize,
}

/// Tag → count over the run's permanent copies (temporary summons never count; each copy counts once).
fn permanent_tag_counts<'a>(entries: &'a [PoolEntry], permanents: &[String]) -> BTreeMap<&'a str, u32> {
    let mut tag_counts: BTreeMap<&'a str, u32> = BTreeMap::new();
    for id in permanents {
        if let Some(entry) = entries.iter().find(|e| &e.id == id) {
            for tag in &entry.tags {
                *tag_counts.entry(tag.as_str()).or_default() += 1;
            }
        }
    }
    tag_counts
}

/// True when every `requires` hook of `entry` is satisfied by the run's permanents.
fn requires_satisfied(entry: &PoolEntry, tag_counts: &BTreeMap<&str, u32>) -> bool {
    entry
        .requires
        .iter()
        .all(|(tag, count)| tag_counts.get(tag.as_str()).copied().unwrap_or(0) >= *count)
}

/// True when `entry` refuses to spawn against any of the class ids present in the lobby.
fn class_excluded(entry: &PoolEntry, classes: &[&str]) -> bool {
    classes.iter().any(|class| {
        entry
            .class_interactions
            .get(*class)
            .is_some_and(|relation| relation == "ignores")
    })
}

/// The pedestal pool for `level` under the default difficulty.
///
/// Thin wrapper over [`vote_pool_with`] with [`sim::mode::DifficultyMode::Standard`], kept
/// so every existing call site and test compiles unchanged (T-085).
#[allow(dead_code)]
pub fn vote_pool(entries: &[PoolEntry], level: u32, classes: &[&str], permanents: &[String]) -> Vec<PoolEntry> {
    vote_pool_with(entries, level, classes, permanents, sim::mode::DifficultyMode::Standard)
}

/// The pedestal pool for `level`, shaped by `mode` (T-085).
///
/// Identical to [`vote_pool`] except for two mode-driven changes: every entry's `min_level`
/// is shifted by `mode.min_level_shift()` (clamped so it never drops below 1), and the
/// permanent-copy cap is raised by `mode.cap_bonus()`. The chorus gate, the `requires`
/// hooks, the class filter and the sort order are untouched.
pub fn vote_pool_with(
    entries: &[PoolEntry],
    level: u32,
    classes: &[&str],
    permanents: &[String],
    mode: sim::mode::DifficultyMode,
) -> Vec<PoolEntry> {
    let tag_counts = permanent_tag_counts(entries, permanents);
    let small = tag_counts.get("small").copied().unwrap_or(0);
    let mut pool: Vec<PoolEntry> = entries
        .iter()
        .filter(|e| (e.min_level as i32 + mode.min_level_shift()).max(1) <= level as i32)
        .filter(|e| e.id != "chorus" || chorus_candidate_allowed(small, level))
        .filter(|e| {
            let held = permanents.iter().filter(|id| id.as_str() == e.id).count();
            held < (e.selection_cap + mode.cap_bonus()) as usize
        })
        .filter(|e| requires_satisfied(e, &tag_counts))
        .filter(|e| !class_excluded(e, classes))
        .cloned()
        .collect();
    pool.sort_by(|a, b| {
        a.min_level
            .cmp(&b.min_level)
            .then(a.content_order.cmp(&b.content_order))
    });
    pool
}

/// The overflow pool for `level` under the default difficulty.
///
/// Thin wrapper over [`overflow_pool_with`] with [`sim::mode::DifficultyMode::Standard`], kept
/// so every existing call site and test compiles unchanged (T-085).
#[allow(dead_code)]
pub fn overflow_pool(entries: &[PoolEntry], level: u32, classes: &[&str], permanents: &[String]) -> Vec<PoolEntry> {
    overflow_pool_with(entries, level, classes, permanents, sim::mode::DifficultyMode::Standard)
}

/// The overflow pool for `level`, shaped by `mode` (T-085).
///
/// Same two mode-driven changes as [`vote_pool_with`] - the shifted `min_level` gate and the
/// raised permanent cap - but this pool still ignores caps when it is used for the temporary
/// overflow draft, and it still truncates to [`VOTE_POOL_OFFER`].
pub fn overflow_pool_with(
    entries: &[PoolEntry],
    level: u32,
    classes: &[&str],
    permanents: &[String],
    mode: sim::mode::DifficultyMode,
) -> Vec<PoolEntry> {
    let tag_counts = permanent_tag_counts(entries, permanents);
    let small = tag_counts.get("small").copied().unwrap_or(0);
    let mut pool: Vec<PoolEntry> = entries
        .iter()
        .filter(|e| (e.min_level as i32 + mode.min_level_shift()).max(1) <= level as i32)
        .filter(|e| e.id != "chorus" || chorus_candidate_allowed(small, level))
        .filter(|e| requires_satisfied(e, &tag_counts))
        .filter(|e| !class_excluded(e, classes))
        .cloned()
        .collect();
    pool.sort_by(|a, b| {
        b.min_level
            .cmp(&a.min_level)
            .then(a.content_order.cmp(&b.content_order))
    });
    pool.truncate(VOTE_POOL_OFFER);
    pool
}

/// Deterministic seed mix (SplitMix32-style) for vote draws and tie-breaks.
fn mix_seed(seed: u32, level: u32) -> u32 {
    let mut z = seed ^ level.wrapping_mul(0x9E37_79B9);
    z = (z ^ (z >> 16)).wrapping_mul(0x85EB_CA6B);
    z = (z ^ (z >> 13)).wrapping_mul(0xC2B2_AE35);
    z ^ (z >> 16)
}

/// True when `level` has a milestone row in the escalation table (design §5: level 4+; 25+ is the endless row).
pub fn escalation_milestone_level(level: u32) -> bool {
    const ROWS: [u32; 13] = [4, 5, 6, 7, 8, 9, 10, 12, 14, 15, 17, 18, 20];
    ROWS.contains(&level) || level >= 25
}

/// Per-tile collapse warning time for `level` (design §5 level 9+: 2.0 s, else 3.0 s).
pub(crate) fn tile_warning_seconds(level: u32) -> f32 {
    if level >= 9 { 2.0 } else { 3.0 }
}

/// Collapse duration D in seconds for `level` (design §1.4; level 17+ loses 3 s with the tier-30 floor, level 25+ loses 1 s per level beyond and the floor drops to 25).
pub(crate) fn collapse_duration_seconds(level: u32) -> f32 {
    let base = (52.0 - 2.0 * level.saturating_sub(1) as f32).max(30.0);
    let hungering = if level >= 17 { 3.0 } else { 0.0 };
    let endless = if level > 25 { (level - 25) as f32 } else { 0.0 };
    let floor = if level > 25 { 25.0 } else { 30.0 };
    (base - hungering - endless).max(floor)
}

/// Fog multiplier from the escalation table (level 12: Grey Tide +25%, level 18: Deep Night +15%; multiplies with curses).
pub(crate) fn escalation_fog_mul(level: u32) -> f32 {
    let mut fog = 1.0;
    if level >= 12 {
        fog *= 1.25;
    }
    if level >= 18 {
        fog *= 1.15;
    }
    fog
}

/// Extra enemy speed multiplier from the endless escalation row (+5% per 5 levels past 25).
pub(crate) fn escalation_speed_mul(level: u32) -> f32 {
    if level > 25 {
        1.0 + 0.05 * ((level - 25) / 5) as f32
    } else {
        1.0
    }
}

/// Extra copies of drafted enemies granted by the escalation table (design §5, cumulative rows, from level 4 on).
fn escalation_extra_copies(level: u32, drafted: &[String], most_voted: Option<&str>) -> Vec<String> {
    let mut extra: Vec<String> = Vec::new();
    if level >= 4
        && let Some(mv) = most_voted
    {
        extra.push(mv.to_string());
    }
    if level >= 6 {
        if drafted.iter().any(|d| d == "pallbearer") {
            extra.push("pallbearer".to_string());
        }
        // Crowded Wake: +1 copy of every drafted enemy every 3 levels after level 6 (cap +6).
        let per3 = ((level - 6) / 3 + 1).min(6);
        for _ in 0..per3 {
            extra.extend(drafted.iter().cloned());
        }
    }
    if level >= 10 {
        // Second Front: +1 copy per 2 drafted enemy types, floor, max +4 (round-robin over the drafted list).
        let types = drafted.len();
        let n = (types / 2).min(4);
        for k in 0..n {
            extra.push(drafted[k % types].clone());
        }
    }
    if level >= 20 {
        // Full Chorus: one extra copy of every drafted enemy.
        extra.extend(drafted.iter().cloned());
    }
    extra
}

/// Distance from a point to an AABB in 3D (0 when the point is inside).
fn point_aabb_distance(p: &Vec3, b: &Aabb) -> f32 {
    let dx = (b.min.x - p.x).max(0.0).max(p.x - b.max.x);
    let dy = (b.min.y - p.y).max(0.0).max(p.y - b.max.y);
    let dz = (b.min.z - p.z).max(0.0).max(p.z - b.max.z);
    (dx * dx + dy * dy + dz * dz).sqrt()
}

/// The spawn hub is exempt from collapse: the platform layer whose XZ footprint contains the beacon or the spawn
/// and whose top is within 2 u of it (the dais/plaza deck) never falls.
fn is_hub_platform(platform: &Platform, beacon: &Vec3, spawn: &Vec3) -> bool {
    let covers = |p: &Vec3| {
        p.x >= platform.bounds.min.x
            && p.x <= platform.bounds.max.x
            && p.z >= platform.bounds.min.z
            && p.z <= platform.bounds.max.z
    };
    let top = platform.bounds.max.y;
    (covers(beacon) && (top - beacon.y).abs() <= 2.0) || (covers(spawn) && (top - spawn.y).abs() <= 2.0)
}

/// Plan the collapse of a generated level: every non-hub platform, farthest from the beacon first
/// (ties by platform id ascending), split into 6 contiguous batches; batch i warns at `i*D/6` into phase 2.
fn build_collapse_plan(platforms: &[Platform], beacon: &Vec3, spawn: &Vec3, level: u32) -> Vec<CollapseEntry> {
    let duration = collapse_duration_seconds(level);
    let warn_ticks = (tile_warning_seconds(level) * TICK_HZ as f32).round() as u32;
    let mut planned: Vec<(f32, u16)> = platforms
        .iter()
        .filter(|p| !is_hub_platform(p, beacon, spawn))
        .map(|p| (point_aabb_distance(beacon, &p.bounds), p.id))
        .collect();
    planned.sort_by(|a, b| b.0.total_cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
    let n = planned.len();
    let mut entries = Vec::with_capacity(n);
    for (idx, (_, id)) in planned.iter().enumerate() {
        let batch = (idx as u32 * 6) / (n as u32).max(1);
        let warn_start_tick = ((batch as f32 * duration / 6.0) * TICK_HZ as f32).round() as u32;
        entries.push(CollapseEntry {
            id: *id,
            warn_start_tick,
            gone_tick: warn_start_tick + warn_ticks,
            state: CollapseTile::Normal,
        });
    }
    entries
}

/// Map every gift's id to the platform it rests on (highest top at or below the gift, within its XZ footprint).
fn gift_platform_map(platforms: &[Platform], gifts: &[protocol::Gift]) -> BTreeMap<u16, u16> {
    let mut map = BTreeMap::new();
    for gift in gifts {
        let target_y = gift.pos.y - 0.3 + 0.01;
        let mut best: Option<&Platform> = None;
        for platform in platforms {
            let top = platform.bounds.max.y;
            let covers = gift.pos.x >= platform.bounds.min.x
                && gift.pos.x <= platform.bounds.max.x
                && gift.pos.z >= platform.bounds.min.z
                && gift.pos.z <= platform.bounds.max.z;
            if !covers || top > target_y {
                continue;
            }
            let better = match best {
                Some(b) => top > b.bounds.max.y,
                None => true,
            };
            if better {
                best = Some(platform);
            }
        }
        if let Some(platform) = best {
            map.insert(gift.id, platform.id);
        }
    }
    map
}

/// Where a collapsing platform is in its warning cycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CollapseTile {
    /// Solid and quiet.
    Normal,
    /// Warning: shaking, dust; the client draws the telegraph.
    Warn,
    /// Gone: no longer solid; the golden gifts on it vanished.
    Gone,
}

/// One non-hub platform slated to fall.
#[derive(Debug, Clone)]
struct CollapseEntry {
    /// Platform id.
    id: u16,
    /// Tick after phase 2 starts when the warning begins (`i*D/6`, batch i).
    warn_start_tick: u32,
    /// Tick after phase 2 starts when the platform becomes `TILE_GONE`.
    gone_tick: u32,
    /// Current cycle state.
    state: CollapseTile,
}

/// Small deterministic PRNG (SplitMix32) for draft offers, shop offers, altar rolls, vote coins and level seeds. Server-only.
#[derive(Debug, Clone)]
pub struct Rng(u32);

impl Rng {
    pub fn new(seed: u32) -> Self {
        Rng(seed)
    }
    pub fn next_u32(&mut self) -> u32 {
        self.0 = self.0.wrapping_add(0x9e37_79b9);
        let mut z = self.0;
        z = (z ^ (z >> 16)).wrapping_mul(0x85eb_ca6b);
        z = (z ^ (z >> 13)).wrapping_mul(0xc2b2_ae35);
        z ^ (z >> 16)
    }
    /// Uniform index in 0..n (n > 0).
    pub fn below(&mut self, n: usize) -> usize {
        (self.next_u32() as usize) % n.max(1)
    }
    /// `k` distinct indices from 0..n, in random order.
    pub fn sample(&mut self, n: usize, k: usize) -> Vec<usize> {
        let mut pool: Vec<usize> = (0..n).collect();
        for i in (1..pool.len()).rev() {
            let j = self.below(i + 1);
            pool.swap(i, j);
        }
        pool.truncate(k.min(n));
        pool
    }
}

/// The four risk/reward altar designs. The wire `protocol::Altar` carries only an id and a position,
/// so the design is derived from the id: `id % ALTAR_KINDS` selects the entry in this order, which is
/// also the order of the `altars` array in `design/content.json`. The client can therefore show the
/// name, benefit and cost before the player commits without a protocol change.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AltarKind {
    /// Skip the next draft and go straight to the shop; pay with two temporary curses and two extra enemies.
    Shortcut,
    /// Highlight every gift this level; the next draft's reward cards come from the hardest pool.
    GiftSight,
    /// Clear this level's fog; Lure and Static spawn at the altar immediately.
    ClearSky,
    /// Take a currency advance; the next two levels' golden-gift income repays it.
    Debt,
}

impl AltarKind {
    /// The design selected by an altar id (`id % ALTAR_KINDS`).
    pub fn from_id(id: u16) -> Self {
        match id % ALTAR_KINDS {
            0 => AltarKind::Shortcut,
            1 => AltarKind::GiftSight,
            2 => AltarKind::ClearSky,
            _ => AltarKind::Debt,
        }
    }

    /// Stable token for the benefit, returned by `try_use_altar`.
    pub fn benefit_id(self) -> &'static str {
        match self {
            AltarKind::Shortcut => "shortcut",
            AltarKind::GiftSight => "gift_sight",
            AltarKind::ClearSky => "clear_sky",
            AltarKind::Debt => "debt",
        }
    }

    /// Stable token for the cost, returned by `try_use_altar`.
    pub fn cost_id(self) -> &'static str {
        match self {
            AltarKind::Shortcut => "two_temporary_curses",
            AltarKind::GiftSight => "hardest_pool",
            AltarKind::ClearSky => "lure_and_static",
            AltarKind::Debt => "repay_from_income",
        }
    }

    /// Display name.
    #[allow(dead_code)]
    pub fn name(self) -> &'static str {
        match self {
            AltarKind::Shortcut => "Shortcut Altar",
            AltarKind::GiftSight => "Gift Sight Altar",
            AltarKind::ClearSky => "Clear Sky Altar",
            AltarKind::Debt => "Debt Altar",
        }
    }

    /// What the player gains, in one sentence.
    #[allow(dead_code)]
    pub fn benefit_text(self) -> String {
        match self {
            AltarKind::Shortcut => "Skip the next draft and go straight to the shop.".to_string(),
            AltarKind::GiftSight => "Every gift is highlighted for this level.".to_string(),
            AltarKind::ClearSky => "This level's fog is cleared.".to_string(),
            AltarKind::Debt => format!("Gain {DEBT_ADVANCE} currency now."),
        }
    }

    /// What the player pays, in one sentence.
    #[allow(dead_code)]
    pub fn cost_text(self) -> String {
        match self {
            AltarKind::Shortcut => {
                format!("Two temporary curses and {SHORTCUT_EXTRA_ENEMIES} extra enemies next level.")
            }
            AltarKind::GiftSight => "The next draft's reward cards come from the hardest pool.".to_string(),
            AltarKind::ClearSky => "Lure and Static spawn at the altar immediately.".to_string(),
            AltarKind::Debt => {
                "The next two levels' golden-gift income repays the debt; any remainder is taken from your purse."
                    .to_string()
            }
        }
    }

    /// A client-renderable preview of the benefit and the cost.
    #[allow(dead_code)]
    pub fn preview(self) -> AltarPreview {
        AltarPreview {
            kind: self,
            name: self.name(),
            benefit: self.benefit_text(),
            cost: self.cost_text(),
        }
    }
}

/// A client-renderable description of an altar's benefit and cost.
#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(dead_code)]
pub struct AltarPreview {
    /// The design.
    pub kind: AltarKind,
    /// Display name.
    pub name: &'static str,
    /// What the player gains.
    pub benefit: String,
    /// What the player pays.
    pub cost: String,
}

/// Persistent altar effects. Every field is part of the run state: it survives level transitions and
/// is written by `Run::altar_state` / read by `Run::restore_altar_state` so a server restart keeps
/// the effects applied.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AltarLedger {
    /// Shortcut: the next level completion skips the draft and opens the shop directly.
    pub shortcut_pending: bool,
    /// Shortcut: curses waiting to be applied at the start of the next level.
    pub pending_temporary_curses: Vec<String>,
    /// Shortcut: curses currently applied for this level (cleared at level end).
    pub temporary_curses: Vec<String>,
    /// Shortcut: extra enemy copies added to the next level's roster.
    pub extra_enemies_next_level: u32,
    /// Gift sight: every gift is highlighted for this level.
    pub gift_sight: bool,
    /// Gift sight: the next draft's reward cards come from the hardest pool.
    pub hardest_pool_next: bool,
    /// Clear sky: this level's fog is cleared.
    pub fog_cleared: bool,
    /// Debt: currency still owed.
    pub debt_owed: u32,
    /// Debt: levels of income still garnished (0..=DEBT_REPAY_LEVELS).
    pub debt_levels_left: u8,
    /// Debt: true when the current level's income is being garnished.
    pub debt_garnished_this_level: bool,
    /// Next enemy id for altar-spawned enemies; kept with the altar state so a restart cannot reuse an id.
    pub next_enemy_id: u32,
}

impl AltarLedger {
    /// Serialize the ledger to the stable, line-based restart format.
    #[allow(dead_code)]
    pub fn encode(&self) -> String {
        let mut out = String::from("altar-ledger v1\n");
        out.push_str(&format!("shortcut_pending={}\n", u8::from(self.shortcut_pending)));
        out.push_str(&format!(
            "pending_temporary_curses={}\n",
            self.pending_temporary_curses.join(",")
        ));
        out.push_str(&format!("temporary_curses={}\n", self.temporary_curses.join(",")));
        out.push_str(&format!("extra_enemies_next_level={}\n", self.extra_enemies_next_level));
        out.push_str(&format!("gift_sight={}\n", u8::from(self.gift_sight)));
        out.push_str(&format!("hardest_pool_next={}\n", u8::from(self.hardest_pool_next)));
        out.push_str(&format!("fog_cleared={}\n", u8::from(self.fog_cleared)));
        out.push_str(&format!("debt_owed={}\n", self.debt_owed));
        out.push_str(&format!("debt_levels_left={}\n", self.debt_levels_left));
        out.push_str(&format!(
            "debt_garnished_this_level={}\n",
            u8::from(self.debt_garnished_this_level)
        ));
        out.push_str(&format!("next_enemy_id={}\n", self.next_enemy_id));
        out
    }

    /// Parse the restart format. Unknown keys are ignored so an older server can read a newer ledger;
    /// a missing key keeps its default. Returns `None` when the header or a known value is malformed.
    #[allow(dead_code)]
    pub fn decode(state: &str) -> Option<Self> {
        let mut lines = state.lines();
        if lines.next()? != "altar-ledger v1" {
            return None;
        }
        let mut ledger = AltarLedger::default();
        for line in lines {
            if line.is_empty() {
                continue;
            }
            let (key, value) = line.split_once('=')?;
            match key {
                "shortcut_pending" => ledger.shortcut_pending = parse_flag(value)?,
                "pending_temporary_curses" => ledger.pending_temporary_curses = parse_list(value)?,
                "temporary_curses" => ledger.temporary_curses = parse_list(value)?,
                "extra_enemies_next_level" => ledger.extra_enemies_next_level = value.parse().ok()?,
                "gift_sight" => ledger.gift_sight = parse_flag(value)?,
                "hardest_pool_next" => ledger.hardest_pool_next = parse_flag(value)?,
                "fog_cleared" => ledger.fog_cleared = parse_flag(value)?,
                "debt_owed" => ledger.debt_owed = value.parse().ok()?,
                "debt_levels_left" => ledger.debt_levels_left = value.parse().ok()?,
                "debt_garnished_this_level" => ledger.debt_garnished_this_level = parse_flag(value)?,
                "next_enemy_id" => ledger.next_enemy_id = value.parse().ok()?,
                _ => {}
            }
        }
        Some(ledger)
    }
}

/// Parse `1`/`0` into a bool.
#[allow(dead_code)]
fn parse_flag(value: &str) -> Option<bool> {
    match value {
        "1" => Some(true),
        "0" => Some(false),
        _ => None,
    }
}

/// Parse a comma-separated id list; an empty value is an empty list.
#[allow(dead_code)]
fn parse_list(value: &str) -> Option<Vec<String>> {
    if value.is_empty() {
        return Some(Vec::new());
    }
    let mut out = Vec::new();
    for part in value.split(',') {
        if part.is_empty() {
            return None;
        }
        out.push(part.to_string());
    }
    Some(out)
}

/// What a successful altar use did. `benefit` and `cost` are the stable tokens returned by
/// `try_use_altar`; `kind` and `altar_id` identify the design and the altar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AltarOutcome {
    /// The altar that was used.
    pub altar_id: u16,
    /// The design selected by the altar id.
    pub kind: AltarKind,
    /// Stable benefit token (`AltarKind::benefit_id`).
    pub benefit: &'static str,
    /// Stable cost token (`AltarKind::cost_id`).
    pub cost: &'static str,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Offer {
    pub enemy_picks: u8,
    pub reward_picks: u8,
    pub cards: Vec<DraftCard>,
}

/// One item on offer in the lobby shop, with the stock left for this lobby.
#[derive(Debug, Clone, PartialEq)]
pub struct ShopOffer {
    /// Id of the item in design/content.json (or a contextual id: `upgrade::<upgrade id>`,
    /// `purge::<curse id>`). `perk_<class id>` and `counter_<enemy id>` ids live in content.json.
    pub id: String,
    /// Cost in currency.
    pub cost: u32,
    /// Items left to sell in this lobby.
    pub stock: u8,
}

/// What a successful shop purchase did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShopPurchase {
    /// Bought a life: +1 life.
    Life,
    /// Rerolled the reward cards of the current draft offer.
    Reroll,
    /// Applied a mods-like item (mods, perk or counter card, or a purchased upgrade card).
    Mods,
}

/// Why a purchase was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShopError {
    /// The shop is not open (not in a lobby, or this lobby has no shop).
    NotInLobby,
    /// No such item in the current offer.
    UnknownItem,
    /// The item is sold out.
    SoldOut,
    /// Not enough (team-wide) currency.
    NotEnoughCurrency,
}

pub struct Run {
    pub phase: u8,
    /// The level being played (Playing) or about to be played (Lobby).
    pub level_num: u32,
    pub level: LevelData,
    pub world: World,
    /// Ids of every gift collected this level (normal; golden gifts reuse the same ids).
    pub gifts_taken: Vec<u16>,
    /// Normal gifts collected over the whole run.
    pub gifts_collected: u32,
    pub currency: u32,
    pub lives: u32,
    pub enemies: Vec<Enemy>,
    pub drafted_enemies: Vec<String>,
    /// Enemies won in votes (T-078); permanent for the rest of the run. Ordered by when they were won.
    pub permanents: Vec<String>,
    /// One-level temporary summons won in overflow votes; consumed by (and only by) the next level.
    pub pending_temporary: Vec<String>,
    /// The open vote session for the level being exited, if any.
    pub vote_session: Option<VoteSession>,
    /// The seed the run was created with (vote draws and tie-breaks are deterministic).
    pub seed: u32,
    /// The difficulty mode chosen in the lobby (T-085). `Standard` changes nothing.
    pub mode: sim::mode::DifficultyMode,
    pub upgrades: Vec<String>,
    pub curses: Vec<String>,
    /// Ids of shop `mods`/`perk`/`counter` items bought this run, so a curse purge can rebuild the
    /// remaining mods from scratch without silently dropping them.
    pub purchased_mods: Vec<String>,
    pub mods: RunMods,
    pub offer: Option<Offer>,
    /// Hazards view of the current level: enemies, projectiles, crumbling/fretted tiles.
    pub hazards: Hazards,
    /// Levelgen altar ids already used this level.
    pub altars_used: Vec<u16>,
    /// Persistent altar effects (shortcut, gift sight, clear sky, debt); serialized by `altar_state`.
    pub altar_ledger: AltarLedger,
    /// Ids of enemies spawned by altar use this level (for UI feedback and tests).
    pub altar_spawns: Vec<String>,
    /// Friction multiplier from the level's event (EVENT_ICE pulls it from content; else 1.0).
    pub event_friction_mul: f32,
    /// The lobby shop offer (from level 2 on); `None` outside the shop lobby and while playing.
    pub shop: Option<Vec<ShopOffer>>,
    /// Phase of the current level: LEVEL_COLLECT, LEVEL_COLLAPSE_PENDING or LEVEL_COLLAPSING.
    pub level_phase: u8,
    /// Ids of the golden gifts collected this level (same ids as the collected normal gifts).
    pub golden_taken: Vec<u16>,
    /// Ticks since phase 2 (LEVEL_COLLAPSE_PENDING) began; 0 in LEVEL_COLLECT.
    collapse_elapsed_ticks: u32,
    /// Collapse plan: one entry per non-hub platform, farthest from the beacon first.
    collapse: Vec<CollapseEntry>,
    /// Gift id -> platform id it rests on (for golden gifts vanishing with a fallen tile).
    gift_platform: BTreeMap<u16, u16>,
    /// The single most-voted drafted enemy of the last draft (escalation "Second Step", level 4+).
    most_voted_enemy: Option<String>,
    rng: Rng,
}

impl Run {
    /// A new run, waiting in the lobby for the start-of-run draft.
    pub fn new(content: &Content, seed: u32, start_level: u32) -> Self {
        let level = levelgen::hub();
        let world = World::from_level(&level);
        let mut run = Run {
            phase: PHASE_ENEMY_DRAFT,
            level_num: start_level.max(1),
            level,
            world,
            gifts_taken: Vec::new(),
            gifts_collected: 0,
            currency: 0,
            lives: 0,
            enemies: Vec::new(),
            drafted_enemies: Vec::new(),
            permanents: Vec::new(),
            pending_temporary: Vec::new(),
            vote_session: None,
            seed,
            mode: sim::mode::DifficultyMode::Standard,
            upgrades: Vec::new(),
            curses: Vec::new(),
            purchased_mods: Vec::new(),
            mods: RunMods::default(),
            offer: None,
            hazards: Hazards::new(),
            altars_used: Vec::new(),
            altar_ledger: AltarLedger::default(),
            altar_spawns: Vec::new(),
            event_friction_mul: 1.0,
            shop: None,
            level_phase: LEVEL_COLLECT,
            golden_taken: Vec::new(),
            collapse_elapsed_ticks: 0,
            collapse: Vec::new(),
            gift_platform: BTreeMap::new(),
            most_voted_enemy: None,
            rng: Rng::new(seed),
        };
        run.offer = Some(run.make_offer(content));
        run.shop = run.make_shop(content);
        run
    }

    /// Everything permanent in the run, drafted enemies first, voted permanents after; used for the
    /// pool's `selection_cap` count and the `requires` hooks. Temporary summons never appear here.
    pub fn all_permanents(&self) -> Vec<String> {
        let mut all = self.drafted_enemies.clone();
        all.extend(self.permanents.iter().cloned());
        all
    }

    /// Build the pool entries from the registry content, in content order.
    fn pool_entries(content: &Content) -> Vec<PoolEntry> {
        content
            .enemies
            .iter()
            .enumerate()
            .map(|(content_order, e)| PoolEntry {
                id: e.id.clone(),
                name: e.name.clone(),
                min_level: e.min_level.into(),
                selection_cap: e.selection_cap.into(),
                tags: e.tags.clone(),
                class_interactions: e.class_interactions.clone(),
                requires: e
                    .synergy_hooks
                    .iter()
                    .filter_map(|h| h.requires.as_ref())
                    .map(|r| (r.tag.clone(), r.count.into()))
                    .collect(),
                content_order,
            })
            .collect()
    }

    /// Build the vote options for the level being exited. Deterministic: enemy candidates come from the
    /// registry order (registry §9), a curse may replace the last enemy option on a seeded coin (eligible
    /// curses are non-greater, or greater-only on `GREATER_EVERY` levels), and an empty enemy pool overflows
    /// into temporary summons. Never pads with entries the filters exclude.
    pub fn build_vote_options(&self, content: &Content, classes: &[&str]) -> Vec<SessionOption> {
        let entries = Self::pool_entries(content);
        let permanents = self.all_permanents();
        let level = self.level_num;
        let pool = vote_pool_with(&entries, level, classes, &permanents, self.mode);
        let mut options: Vec<SessionOption> = pool
            .iter()
            .take(VOTE_POOL_OFFER)
            .map(|e| SessionOption {
                index: 0,
                id: e.id.clone(),
                name: e.name.clone(),
                kind: CARD_ENEMY,
                temporary: false,
            })
            .collect();
        if options.is_empty() {
            options = overflow_pool_with(&entries, level, classes, &permanents, self.mode)
                .into_iter()
                .map(|e| SessionOption {
                    index: 0,
                    id: e.id,
                    name: e.name,
                    kind: CARD_ENEMY,
                    temporary: true,
                })
                .collect();
        }
        // One of the three options may be a curse instead of an enemy: seeded coin, 1 in 2.
        let mut rng = Rng::new(mix_seed(self.seed, level) ^ VOTE_CURSE_SALT);
        let greater = level.is_multiple_of(GREATER_EVERY);
        let curse_pool: Vec<&crate::content::ModCard> =
            content.curses.iter().filter(|c| c.greater == greater).collect();
        if !curse_pool.is_empty() && rng.below(2) == 0 {
            let pick = curse_pool[rng.below(curse_pool.len())];
            if !options.is_empty() {
                options.pop();
            }
            options.push(SessionOption {
                index: 0,
                id: pick.id.clone(),
                // No curse name is assumed to exist in content; the wire carries the id.
                name: pick.id.clone(),
                kind: CARD_CURSE,
                temporary: false,
            });
        }
        for (i, option) in options.iter_mut().enumerate() {
            option.index = i as u8;
        }
        options
    }

    /// Open the vote session for the level being exited; `classes` are the class ids present in the lobby.
    /// Returns false when a session is already open or no option can be offered at all.
    pub fn open_vote(&mut self, content: &Content, classes: &[&str], host: Option<u32>) -> bool {
        if self.vote_session.is_some() {
            return false;
        }
        // RANDOM (T-085): no vote at all - the run picks one option itself, deterministically from
        // its own RNG, and applies it through the same path a closed vote would take.
        if self.mode.forces_random_pick() {
            let options = self.build_vote_options(content, classes);
            if options.is_empty() {
                return false;
            }
            let pick = self.rng.sample(options.len(), 1);
            let Some(&index) = pick.first() else {
                return false;
            };
            let option = &options[index];
            let outcome = VoteOutcome {
                winner: option.index,
                id: option.id.clone(),
                name: option.name.clone(),
                kind: option.kind,
                permanent: !option.temporary,
                temporary: option.temporary,
                counts: vec![0; options.len()],
            };
            self.apply_vote_outcome(content, &outcome);
            return false;
        }
        let options = self.build_vote_options(content, classes);
        if options.is_empty() {
            return false;
        }
        self.vote_session = Some(VoteSession::open(self.level_num, self.seed, options, host));
        true
    }

    /// Close the vote session (countdown elapsed, everyone voted, or the level ended) and apply the outcome.
    /// Returns `None` when no session was open.
    pub fn close_vote(&mut self, content: &Content) -> Option<VoteOutcome> {
        let session = self.vote_session.take()?;
        let outcome = session.resolve()?;
        self.apply_vote_outcome(content, &outcome);
        Some(outcome)
    }

    /// Apply a closed vote: enemies join the permanents (or the one-level temporary list on overflow),
    /// curses act like drafted curses (mods + the run's curse list).
    fn apply_vote_outcome(&mut self, content: &Content, outcome: &VoteOutcome) {
        match outcome.kind {
            CARD_ENEMY => {
                if outcome.temporary {
                    self.pending_temporary.push(outcome.id.clone());
                } else {
                    self.permanents.push(outcome.id.clone());
                }
            }
            _ => {
                if let Some(def) = content.curse(&outcome.id) {
                    let lives_before = self.mods.extra_lives;
                    self.mods.apply(&def.mods);
                    self.lives = self
                        .lives
                        .saturating_add(self.mods.extra_lives.saturating_sub(lives_before));
                    self.curses.push(outcome.id.clone());
                }
            }
        }
    }

    /// The reward cards for the current lobby (greater-curse level or mixed upgrade/curse), drawn from the run RNG.
    fn reward_cards(&mut self, content: &Content) -> Vec<DraftCard> {
        let hardest = std::mem::take(&mut self.altar_ledger.hardest_pool_next);
        if hardest || self.level_num.is_multiple_of(GREATER_EVERY) {
            let mut pool: Vec<&crate::content::ModCard> = content.curses.iter().filter(|c| c.greater).collect();
            if pool.len() < 3 {
                pool.extend(content.curses.iter().filter(|c| !c.greater));
            }
            let mut cards = Vec::with_capacity(3);
            for i in self.rng.sample(pool.len(), 3) {
                cards.push(DraftCard {
                    kind: CARD_CURSE,
                    id: pool[i].id.clone(),
                });
            }
            return cards;
        }
        // At least one upgrade and one curse; the third card comes from either pool.
        let normal_curses: Vec<_> = content.curses.iter().filter(|c| !c.greater).collect();
        let ups = self.rng.sample(content.upgrades.len(), 2);
        let curses = self.rng.sample(normal_curses.len(), 2);
        let upgrade = |i: usize| DraftCard {
            kind: CARD_UPGRADE,
            id: content.upgrades[i].id.clone(),
        };
        let curse = |i: usize| DraftCard {
            kind: CARD_CURSE,
            id: normal_curses[i].id.clone(),
        };
        let third = if self.rng.below(2) == 0 {
            upgrade(ups[1])
        } else {
            curse(curses[1])
        };
        let mut rewards = vec![upgrade(ups[0]), curse(curses[0]), third];
        let r = self.rng.below(rewards.len());
        rewards.swap(0, r);
        rewards
    }

    fn make_offer(&mut self, content: &Content) -> Offer {
        // The first round of a run is plain: no hunters, no upgrades, no curses. The lobby
        // still opens so everyone can gather and press Continue together.
        if self.level_num == 1 {
            return Offer {
                enemy_picks: 0,
                reward_picks: 0,
                cards: Vec::new(),
            };
        }
        let mut cards = Vec::new();
        let enemy_picks = if self.drafted_enemies.is_empty() {
            for i in self.rng.sample(content.enemies.len(), ENEMY_OFFER) {
                cards.push(DraftCard {
                    kind: CARD_ENEMY,
                    id: content.enemies[i].id.clone(),
                });
            }
            ENEMY_PICKS
        } else {
            0
        };
        cards.extend(self.reward_cards(content));
        Offer {
            enemy_picks,
            reward_picks: REWARD_PICKS,
            cards,
        }
    }

    /// Pick the lobby shop. Two layers, both deterministic off the run RNG:
    /// a static sample of SHOP_OFFER items from the content shop (life/reroll/mods kinds only),
    /// then contextual offerings derived from the run's state:
    /// - two distinct upgrade cards (`upgrade::<upgrade id>`),
    /// - one copy of every class perk defined in content (`perk_<class id>`),
    /// - one copy of every counter whose enemy is a permanent of this run (`counter_<enemy id>`),
    /// - one `purge::<curse id>` per distinct curse held, stock = stacks held.
    fn make_shop(&mut self, content: &Content) -> Option<Vec<ShopOffer>> {
        // CHAOS opens the shop every level, including level 1 (T-085).
        if self.level_num == 1 && !self.mode.shop_every_level() {
            return None;
        }
        let statics: Vec<usize> = content
            .shop
            .iter()
            .enumerate()
            .filter(|(_, def)| matches!(def.kind.as_str(), "life" | "reroll" | "mods"))
            .map(|(i, _)| i)
            .collect();
        let mut offer: Vec<ShopOffer> = Vec::new();
        for i in self.rng.sample(statics.len(), SHOP_OFFER.min(statics.len())) {
            let def = &content.shop[statics[i]];
            offer.push(ShopOffer {
                id: def.id.clone(),
                cost: def.cost,
                stock: def.stock,
            });
        }
        // Contextual upgrade cards.
        let draws = SHOP_UPGRADE_DRAWS.min(content.upgrades.len());
        for i in self.rng.sample(content.upgrades.len(), draws) {
            offer.push(ShopOffer {
                id: format!("upgrade::{}", content.upgrades[i].id),
                cost: SHOP_UPGRADE_COST,
                stock: 1,
            });
        }
        // Class perks from content (one copy each; the team-wide currency buys them for the run).
        for def in content.shop.iter().filter(|d| d.kind == "perk") {
            offer.push(ShopOffer {
                id: def.id.clone(),
                cost: def.cost,
                stock: 1,
            });
        }
        // Counters: only for permanents the run has actually accumulated. An enemy's counter is
        // sold as the item that helps against it; the id convention is `counter_<enemy id>`.
        let permanents = self.all_permanents();
        for def in content.shop.iter().filter(|d| d.kind == "counter") {
            let Some(enemy) = def.id.strip_prefix("counter_") else {
                continue;
            };
            if permanents.iter().any(|p| p.as_str() == enemy) {
                offer.push(ShopOffer {
                    id: def.id.clone(),
                    cost: def.cost,
                    stock: 1,
                });
            }
        }
        // Curse removals: one targeted purge per distinct curse held, stock = stacks held.
        let mut held: BTreeMap<&str, u8> = BTreeMap::new();
        for curse in &self.curses {
            *held.entry(curse.as_str()).or_default() += 1;
        }
        for (curse_id, stacks) in held {
            offer.push(ShopOffer {
                id: format!("purge::{}", curse_id),
                cost: SHOP_PURGE_COST,
                stock: stacks,
            });
        }
        if offer.is_empty() { None } else { Some(offer) }
    }

    /// Replace the reward cards of the current offer, keeping enemy cards (and enemy picks) at the same indices.
    /// Returns false when no draft offer is open.
    pub fn reroll_rewards(&mut self, content: &Content) -> bool {
        if self.offer.is_none() {
            return false;
        }
        let rewards = self.reward_cards(content);
        if let Some(offer) = self.offer.as_mut() {
            offer.cards.retain(|c| c.kind == CARD_ENEMY);
            offer.cards.extend(rewards);
        }
        true
    }

    /// Rebuild the run's mods from scratch: base + every recorded upgrade, curse and purchased stat-mod
    /// item, plus the Shortcut altar's temporary curses. Used when a curse is purged so the removal
    /// actually unloads its effect instead of just hiding it from the list. Multiplication and addition
    /// commute here, the life pool is not re-banked, and shop purchases applied via `purchased_mods` are
    /// preserved.
    fn rebuild_mods(&mut self, content: &Content) {
        let mut fresh = RunMods::default();
        for id in &self.upgrades {
            if let Some(def) = content.upgrade(id) {
                fresh.apply(&def.mods);
            }
        }
        for id in &self.curses {
            if let Some(def) = content.curse(id) {
                fresh.apply(&def.mods);
            }
        }
        for id in &self.purchased_mods {
            if let Some(def) = content.shop_item(id) {
                fresh.apply(&def.mods);
            }
        }
        for id in &self.altar_ledger.temporary_curses {
            if let Some(def) = content.curse(id) {
                fresh.apply(&def.mods);
            }
        }
        self.mods = fresh;
    }

    /// Buy an item from the current lobby shop (team-wide currency). Nothing is charged when the purchase fails,
    /// and no stock is taken. The stock check, the currency check, the effect and the charge all happen in one
    /// `&mut self` call, so a concurrent buyer sees either the pre- or the post-purchase state, never a torn one:
    /// the loser of a race for the last copy gets `SoldOut` and is charged nothing.
    pub fn buy_shop_item(&mut self, content: &Content, item: &str) -> Result<ShopPurchase, ShopError> {
        if self.phase != PHASE_ENEMY_DRAFT {
            return Err(ShopError::NotInLobby);
        }
        // Validate against the current offer first (stock, then currency), without holding any borrows.
        let (cost, stock) = match self.shop.as_ref() {
            None => return Err(ShopError::UnknownItem),
            Some(offer) => match offer.iter().find(|entry| entry.id == item) {
                None => return Err(ShopError::UnknownItem),
                Some(entry) => (entry.cost, entry.stock),
            },
        };
        if stock == 0 {
            return Err(ShopError::SoldOut);
        }
        if self.currency < cost {
            return Err(ShopError::NotEnoughCurrency);
        }
        let purchase = if let Some(curse_id) = item.strip_prefix("purge::") {
            // The curse must actually be held; the offer can only carry purges for held curses, and a stale
            // purge (bought out in another slot) is refused without charging.
            match self.curses.iter().rposition(|c| c.as_str() == curse_id) {
                None => return Err(ShopError::UnknownItem),
                Some(pos) => {
                    self.curses.remove(pos);
                    self.rebuild_mods(content);
                    ShopPurchase::Mods
                }
            }
        } else if let Some(upgrade_id) = item.strip_prefix("upgrade::") {
            let Some(def) = content.upgrade(upgrade_id) else {
                return Err(ShopError::UnknownItem);
            };
            let lives_before = self.mods.extra_lives;
            self.mods.apply(&def.mods);
            self.lives = self
                .lives
                .saturating_add(self.mods.extra_lives.saturating_sub(lives_before));
            self.upgrades.push(upgrade_id.to_string());
            ShopPurchase::Mods
        } else {
            let Some(def) = content.shop_item(item) else {
                return Err(ShopError::UnknownItem);
            };
            // Apply the effect.
            match def.kind.as_str() {
                "life" => {
                    self.lives = self.lives.saturating_add(1);
                    ShopPurchase::Life
                }
                "reroll" => {
                    if !self.reroll_rewards(content) {
                        return Err(ShopError::UnknownItem);
                    }
                    ShopPurchase::Reroll
                }
                "mods" | "perk" | "counter" => {
                    let before = self.mods.extra_lives;
                    self.mods.apply(&def.mods);
                    self.lives = self.lives.saturating_add(self.mods.extra_lives.saturating_sub(before));
                    self.purchased_mods.push(def.id.clone());
                    ShopPurchase::Mods
                }
                _ => return Err(ShopError::UnknownItem),
            }
        };
        // Charge only after the effect went through.
        if let Some(entry) = self
            .shop
            .as_mut()
            .and_then(|offer| offer.iter_mut().find(|entry| entry.id == item))
        {
            entry.stock = entry.stock.saturating_sub(1);
        }
        self.currency = self.currency.saturating_sub(cost);
        Ok(purchase)
    }

    /// Check one player's picks against the current offer.
    pub fn validate_picks(&self, picks: &[u8]) -> Result<(), &'static str> {
        let Some(offer) = &self.offer else {
            return Err("no draft is open");
        };
        if self.phase != PHASE_ENEMY_DRAFT {
            return Err("not in the lobby");
        }
        let mut seen = [false; 256];
        let (mut enemies, mut rewards) = (0u8, 0u8);
        for &p in picks {
            let Some(card) = offer.cards.get(p as usize) else {
                return Err("pick out of range");
            };
            if std::mem::replace(&mut seen[p as usize], true) {
                return Err("duplicate pick");
            }
            if card.kind == CARD_ENEMY {
                enemies = enemies.saturating_add(1);
            } else {
                rewards = rewards.saturating_add(1);
            }
        }
        if enemies != offer.enemy_picks || rewards != offer.reward_picks {
            return Err("wrong number of picks");
        }
        Ok(())
    }

    /// Tally everyone's (validated) votes, apply the winning cards and start the generated level.
    /// Ties go to the card offered first. The number of voters is the player count used for gift generation.
    pub fn resolve(&mut self, content: &Content, votes: &[Vec<u8>]) {
        let Some(offer) = self.offer.take() else { return };
        let mut tally: BTreeMap<usize, u32> = BTreeMap::new();
        for v in votes {
            for &p in v {
                *tally.entry(p as usize).or_default() += 1;
            }
        }
        let mut ranked: Vec<usize> = (0..offer.cards.len()).collect();
        ranked.sort_by_key(|i| (std::cmp::Reverse(tally.get(i).copied().unwrap_or(0)), *i));
        let winners = |enemy: bool, n: u8| -> Vec<usize> {
            ranked
                .iter()
                .copied()
                .filter(|&i| (offer.cards[i].kind == CARD_ENEMY) == enemy)
                .take(n as usize)
                .collect()
        };
        let enemy_winners = winners(true, offer.enemy_picks);
        self.most_voted_enemy = enemy_winners.first().map(|&i| offer.cards[i].id.clone());
        for i in enemy_winners {
            self.drafted_enemies.push(offer.cards[i].id.clone());
        }
        for i in winners(false, offer.reward_picks) {
            let card = &offer.cards[i];
            let def = if card.kind == CARD_UPGRADE {
                content.upgrade(&card.id)
            } else {
                content.curse(&card.id)
            };
            if let Some(def) = def {
                let lives_before = self.mods.extra_lives;
                self.mods.apply(&def.mods);
                self.lives = self
                    .lives
                    .saturating_add(self.mods.extra_lives.saturating_sub(lives_before));
                if card.kind == CARD_UPGRADE {
                    self.upgrades.push(card.id.clone());
                } else {
                    self.curses.push(card.id.clone());
                }
            }
        }
        let players = u32::try_from(votes.len()).unwrap_or(1).max(1);
        self.start_level(content, players);
    }

    fn start_level(&mut self, content: &Content, players: u32) {
        let seed = self.rng.next_u32();
        // T-062: the Clustered Gifts curse scales the minimum gift spacing for this level.
        let generated = levelgen::generate_for_with_spacing(seed, self.level_num, players, self.mods.gift_spacing_mul);
        let spawn_slots = generated.enemy_spawns.len();
        self.level = generated.data;
        self.world = World::from_level(&self.level);
        self.gifts_taken.clear();
        self.golden_taken.clear();
        self.enemies.clear();
        self.hazards.reset();
        self.altars_used.clear();
        self.altar_spawns.clear();
        self.altar_ledger.fog_cleared = false;
        self.altar_ledger.gift_sight = false;
        self.altar_ledger.debt_garnished_this_level = self.altar_ledger.debt_levels_left > 0;
        self.apply_pending_curses(content);
        self.level_phase = LEVEL_COLLECT;
        self.collapse_elapsed_ticks = 0;
        self.collapse = build_collapse_plan(
            &self.level.platforms,
            &self.level.beacon,
            &self.level.spawn,
            self.level_num,
        );
        self.gift_platform = gift_platform_map(&self.level.platforms, &self.level.gifts);
        // The shop lives in the lobby only; starting a level closes it.
        self.shop = None;
        self.event_friction_mul = event_friction_mul(content, self.level.event);
        // Roster: drafted enemies + voted permanents + curse copies + milestone escalation copies
        // (design §5, from level 4 on). T-078: no per-level automatic scaling remains.
        let base = self.all_permanents();
        let base_len = base.len();
        let mut roster: Vec<String> = base.clone();
        if base_len > 0 {
            for k in 0..self.mods.extra_enemies as usize {
                roster.push(base[k % base_len].clone());
            }
        }
        roster.extend(escalation_extra_copies(
            self.level_num,
            &base,
            self.most_voted_enemy.as_deref(),
        ));
        // Level 1 stays plain (design A4): no enemies at all.
        if self.level_num == 1 {
            roster.clear();
        } else {
            // One-level temporary summons from the last vote's overflow (registry §9).
            roster.append(&mut self.pending_temporary);
        }
        // Shortcut: extra copies promised by the altar join the next level.
        let extra = std::mem::take(&mut self.altar_ledger.extra_enemies_next_level);
        if self.level_num != 1 && !base.is_empty() {
            for k in 0..extra as usize {
                roster.push(base[k % base.len()].clone());
            }
        }
        // The hub has 8 fixed spawn slots; extra copies cycle onto the slots.
        for (index, id) in roster.iter().enumerate() {
            let Some((content_index, def)) = content.enemy(id) else {
                continue;
            };
            let mut params = self.mods.enemy_params(&def.params);
            params.speed_mul *= escalation_speed_mul(self.level_num);
            let mut pos = if spawn_slots == 0 {
                self.level.spawn
            } else {
                generated.enemy_spawns[index % spawn_slots]
            };
            // Mimics pose as gifts: spawn them on the platform below the generated spawn
            // (pos.y = platform top + 0.5). If the spawn is not above any platform, keep the position.
            if params.behavior == BEHAVIOR_MIMIC
                && let Some(top) = platform_top_below(&self.level.platforms, pos.x, pos.z, pos.y)
            {
                pos.y = top + 0.5;
            } else if let Some(top) = platform_top_below(&self.level.platforms, pos.x, pos.z, pos.y + 0.01) {
                // Everything else starts with its body clear of the floor and its centre at least at a
                // player's chest height: hub enemy slots lie ON the floor (y = top), and a turret whose
                // centre sits on the floor surface had its line of sight and its flat bolts start inside
                // the floor, so it never fired.
                pos.y = pos.y.max(top + params.size.max(0.9) + 0.1);
            }
            let enemy_id = u32::try_from(index).unwrap_or(u32::MAX - 1) + 1;
            // kind = index into content.json enemies, so the client draws the right model.
            self.enemies.push(rules::spawn_enemy_with(
                enemy_id,
                u8::try_from(content_index).unwrap_or(u8::MAX),
                pos,
                params,
            ));
        }
        self.altar_ledger.next_enemy_id = u32::try_from(roster.len()).unwrap_or(u32::MAX - 1).saturating_add(1);
        self.phase = PHASE_PLAYING;
    }

    /// The beacon was reached: back to the lobby to draft for the next level.
    pub fn finish_level(&mut self, content: &Content) {
        let shortcut = std::mem::take(&mut self.altar_ledger.shortcut_pending);
        self.level_num = self.level_num.saturating_add(1);
        self.level = levelgen::hub();
        self.world = World::from_level(&self.level);
        self.gifts_taken.clear();
        self.golden_taken.clear();
        self.enemies.clear();
        self.hazards.reset();
        self.altars_used.clear();
        self.altar_spawns.clear();
        self.altar_ledger.fog_cleared = false;
        self.altar_ledger.gift_sight = false;
        self.clear_temporary_curses(content);
        self.tick_debt();
        self.level_phase = LEVEL_COLLECT;
        self.collapse_elapsed_ticks = 0;
        self.collapse.clear();
        self.gift_platform.clear();
        self.event_friction_mul = 1.0;
        self.phase = PHASE_ENEMY_DRAFT;
        if shortcut {
            // Shortcut: no draft, straight to the shop.
            self.offer = Some(Offer {
                enemy_picks: 0,
                reward_picks: 0,
                cards: Vec::new(),
            });
        } else {
            self.offer = Some(self.make_offer(content));
        }
        self.shop = self.make_shop(content);
    }

    pub fn end(&mut self) {
        self.phase = PHASE_OVER;
        self.offer = None;
        self.shop = None;
    }

    /// The design of the altar with `id` (`id % ALTAR_KINDS`).
    #[allow(dead_code)]
    pub fn altar_kind(id: u16) -> AltarKind {
        AltarKind::from_id(id)
    }

    /// A client-renderable preview of the altar with `id`, including its benefit and cost.
    #[allow(dead_code)]
    pub fn altar_preview(id: u16) -> AltarPreview {
        AltarKind::from_id(id).preview()
    }

    /// True while the Gift Sight altar's highlight is active for this level.
    #[allow(dead_code)]
    pub fn gift_sight(&self) -> bool {
        self.altar_ledger.gift_sight
    }

    /// Use the first unused altar within 2.0 units horizontally and 1.5 vertically of `feet`.
    /// Applies the design's benefit and cost exactly once and records the altar in `altars_used`.
    /// Returns `None` when no altar is in range, the altar was already used, or the design's
    /// benefit is already active (so a second altar of the same design cannot stack its cost).
    pub fn use_altar(&mut self, content: &Content, feet: Vec3) -> Option<AltarOutcome> {
        if self.phase != PHASE_PLAYING {
            return None;
        }
        let mut hit = None;
        for altar in &self.level.altars {
            if self.altars_used.contains(&altar.id) {
                continue;
            }
            let dx = feet.x - altar.pos.x;
            let dz = feet.z - altar.pos.z;
            let dy = (feet.y - altar.pos.y).abs();
            if dx * dx + dz * dz <= 4.0 && dy <= 1.5 {
                hit = Some((altar.id, altar.pos));
                break;
            }
        }
        let (altar_id, altar_pos) = hit?;
        let kind = AltarKind::from_id(altar_id);
        if self.altar_kind_active(kind) {
            return None;
        }
        let applied = match kind {
            AltarKind::Shortcut => self.apply_shortcut(content),
            AltarKind::GiftSight => {
                self.altar_ledger.gift_sight = true;
                self.altar_ledger.hardest_pool_next = true;
                true
            }
            AltarKind::ClearSky => {
                self.altar_ledger.fog_cleared = true;
                let offset = 1.5;
                self.spawn_altar_enemy(
                    content,
                    "lure",
                    Vec3::new(altar_pos.x + offset, altar_pos.y + 1.0, altar_pos.z),
                );
                self.spawn_altar_enemy(
                    content,
                    "static",
                    Vec3::new(altar_pos.x - offset, altar_pos.y + 1.0, altar_pos.z),
                );
                true
            }
            AltarKind::Debt => {
                self.currency = self.currency.saturating_add(DEBT_ADVANCE);
                self.altar_ledger.debt_owed = self.altar_ledger.debt_owed.saturating_add(DEBT_ADVANCE);
                self.altar_ledger.debt_levels_left = DEBT_REPAY_LEVELS;
                true
            }
        };
        if !applied {
            return None;
        }
        self.altars_used.push(altar_id);
        Some(AltarOutcome {
            altar_id,
            kind,
            benefit: kind.benefit_id(),
            cost: kind.cost_id(),
        })
    }

    /// Compatibility wrapper for the controller: returns `(benefit token, cost token)`.
    /// Prefer `use_altar` when the caller needs the altar id and design.
    pub fn try_use_altar(&mut self, content: &Content, feet: Vec3) -> Option<(String, String)> {
        self.use_altar(content, feet)
            .map(|outcome| (outcome.benefit.to_string(), outcome.cost.to_string()))
    }

    /// True when the design's benefit is already active, so a second altar of the same design
    /// would only duplicate the cost.
    fn altar_kind_active(&self, kind: AltarKind) -> bool {
        match kind {
            AltarKind::Shortcut => self.altar_ledger.shortcut_pending,
            AltarKind::GiftSight => self.altar_ledger.gift_sight,
            AltarKind::ClearSky => self.altar_ledger.fog_cleared,
            AltarKind::Debt => self.altar_ledger.debt_levels_left > 0,
        }
    }

    /// Apply the Shortcut altar: two temporary curses land next level, two extra enemies join,
    /// and the next level completion skips the draft.
    fn apply_shortcut(&mut self, content: &Content) -> bool {
        let normal: Vec<&crate::content::ModCard> = content.curses.iter().filter(|c| !c.greater).collect();
        if normal.is_empty() {
            return false;
        }
        let mut picked = Vec::with_capacity(SHORTCUT_CURSES);
        for _ in 0..SHORTCUT_CURSES {
            let i = self.rng.below(normal.len());
            picked.push(normal[i].id.clone());
        }
        self.altar_ledger.pending_temporary_curses = picked;
        self.altar_ledger.shortcut_pending = true;
        self.altar_ledger.extra_enemies_next_level = self
            .altar_ledger
            .extra_enemies_next_level
            .saturating_add(SHORTCUT_EXTRA_ENEMIES);
        true
    }

    /// Spawn one enemy by content id at `pos`, active immediately, and record the id in `altar_spawns`.
    /// Returns false when the content has no such enemy.
    fn spawn_altar_enemy(&mut self, content: &Content, id: &str, pos: Vec3) -> bool {
        let Some((content_index, def)) = content.enemy(id) else {
            return false;
        };
        let mut params = self.mods.enemy_params(&def.params);
        params.speed_mul *= escalation_speed_mul(self.level_num);
        let enemy_id = self.altar_ledger.next_enemy_id.max(1);
        self.altar_ledger.next_enemy_id = enemy_id.saturating_add(1);
        let mut enemy = rules::spawn_enemy_with(enemy_id, u8::try_from(content_index).unwrap_or(u8::MAX), pos, params);
        enemy.active_in = Default::default();
        self.enemies.push(enemy);
        self.altar_spawns.push(id.to_string());
        true
    }

    /// Apply the Shortcut altar's promised curses at the start of the next level.
    fn apply_pending_curses(&mut self, content: &Content) {
        let pending = std::mem::take(&mut self.altar_ledger.pending_temporary_curses);
        for id in pending {
            if let Some(def) = content.curse(&id) {
                let lives_before = self.mods.extra_lives;
                self.mods.apply(&def.mods);
                self.lives = self
                    .lives
                    .saturating_add(self.mods.extra_lives.saturating_sub(lives_before));
                self.altar_ledger.temporary_curses.push(id);
            }
        }
    }

    /// Drop the Shortcut altar's temporary curses and rebuild the run's mods without them.
    fn clear_temporary_curses(&mut self, content: &Content) {
        if self.altar_ledger.temporary_curses.is_empty() {
            return;
        }
        let extra_before = self.mods.extra_lives;
        self.altar_ledger.temporary_curses.clear();
        self.rebuild_mods(content);
        let extra_after = self.mods.extra_lives;
        self.lives = self.lives.saturating_sub(extra_before.saturating_sub(extra_after));
    }

    /// Advance the Debt altar's repayment window at level end.
    fn tick_debt(&mut self) {
        if !self.altar_ledger.debt_garnished_this_level {
            return;
        }
        self.altar_ledger.debt_levels_left = self.altar_ledger.debt_levels_left.saturating_sub(1);
        if self.altar_ledger.debt_levels_left == 0 && self.altar_ledger.debt_owed > 0 {
            let paid = self.currency.min(self.altar_ledger.debt_owed);
            self.currency -= paid;
            self.altar_ledger.debt_owed = 0;
        }
        self.altar_ledger.debt_garnished_this_level = false;
    }

    /// Credit golden-gift income, paying down an active Debt altar first.
    fn credit_income(&mut self, amount: u32) {
        let mut remaining = amount;
        if self.altar_ledger.debt_levels_left > 0 && self.altar_ledger.debt_owed > 0 {
            let pay = remaining.min(self.altar_ledger.debt_owed);
            self.altar_ledger.debt_owed -= pay;
            remaining -= pay;
        }
        self.currency = self.currency.saturating_add(remaining);
    }

    /// Serialize the persistent altar effects for a server restart.
    #[allow(dead_code)]
    pub fn altar_state(&self) -> String {
        self.altar_ledger.encode()
    }

    /// Restore persistent altar effects from `altar_state`. Rebuilds the run's mods so the restored
    /// temporary curses are applied exactly once. Returns false when `state` is malformed.
    #[allow(dead_code)]
    pub fn restore_altar_state(&mut self, content: &Content, state: &str) -> bool {
        let Some(ledger) = AltarLedger::decode(state) else {
            return false;
        };
        let extra_before = self.mods.extra_lives;
        self.altar_ledger = ledger;
        self.rebuild_mods(content);
        let extra_after = self.mods.extra_lives;
        if extra_after > extra_before {
            self.lives = self.lives.saturating_add(extra_after - extra_before);
        } else {
            self.lives = self.lives.saturating_sub(extra_before - extra_after);
        }
        true
    }

    fn normal_left(&self) -> usize {
        self.level
            .gifts
            .iter()
            .filter(|g| !self.gifts_taken.contains(&g.id))
            .count()
    }

    pub fn beacon_open(&self) -> bool {
        self.phase == PHASE_PLAYING && self.level_phase != LEVEL_COLLECT
    }

    /// The greater curses active on this run that carry a runtime mechanic (T-084).
    pub fn greater_mechanics(&self) -> Vec<sim::greater::GreaterCurse> {
        self.curses
            .iter()
            .filter_map(|id| sim::greater::GreaterCurse::from_id(id))
            .collect()
    }

    /// True while Hush is active: the HUD gift hint is off and the fog is thick (T-084).
    ///
    /// The fog half already runs through the curse's `fog_density_mul`; this flag is what the HUD
    /// reads to hide the gift hint, and the client half is parked (see T-084 NOTES), so only tests
    /// exercise it today.
    #[allow(dead_code)]
    pub fn hush_active(&self) -> bool {
        self.curses.iter().any(|id| id == "hush")
    }

    pub fn collect_gifts(&mut self, feet: &Vec3) -> Vec<Vec3> {
        let mut picked = Vec::new();
        if self.phase != PHASE_PLAYING {
            return picked;
        }
        let radius = GIFT_RADIUS * self.mods.gift_radius_mul;
        if self.level_phase == LEVEL_COLLECT {
            for gift in &self.level.gifts {
                if self.gifts_taken.contains(&gift.id) {
                    continue;
                }
                if rules::gift_touched_within(feet, gift, radius) {
                    self.gifts_taken.push(gift.id);
                    self.gifts_collected = self.gifts_collected.saturating_add(1);
                    if self.curses.iter().any(|c| c == "tithe") {
                        self.currency = self.currency.saturating_sub(sim::greater::TITHE_PER_GIFT);
                    }
                    picked.push(gift.pos);
                }
            }
            // The last gift (including the vacuous zero-gift case) starts phase 2.
            if self.normal_left() == 0 {
                self.enter_collapse_pending();
            }
            return picked;
        }
        // Phase 2: one golden gift per collected normal gift, at its exact position. The ids are
        // gathered first so the loop only holds an immutable borrow of `self.level.gifts`.
        let mut golden: Vec<u16> = Vec::new();
        for gift in &self.level.gifts {
            if !self.gifts_taken.contains(&gift.id) {
                continue;
            }
            if self.golden_taken.contains(&gift.id) {
                continue;
            }
            if self.platform_gone_for_gift(gift.id) {
                // The tile fell: the uncollected golden gift vanished forever.
                continue;
            }
            if rules::gift_touched_within(feet, gift, radius) {
                golden.push(gift.id);
                picked.push(gift.pos);
            }
        }
        for id in golden {
            self.golden_taken.push(id);
            self.credit_income(GOLDEN_VALUE);
        }
        picked
    }

    /// Enter LEVEL_COLLAPSE_PENDING (phase 2 starts): the siren sounds, the beacon opens.
    fn enter_collapse_pending(&mut self) {
        if self.level_phase != LEVEL_COLLECT {
            return;
        }
        self.level_phase = LEVEL_COLLAPSE_PENDING;
        self.collapse_elapsed_ticks = 0;
    }

    /// True when the gift rests on a platform that has already fallen.
    fn platform_gone_for_gift(&self, gift_id: u16) -> bool {
        let Some(platform_id) = self.gift_platform.get(&gift_id) else {
            return false;
        };
        self.collapse
            .iter()
            .find(|e| e.id == *platform_id)
            .is_some_and(|e| e.state == CollapseTile::Gone)
    }

    /// Advance the collapse timers one tick: PENDING -> COLLAPSING after 3.0 s, batch warnings, TILE_GONE flips.
    /// Batches start their warning `i*D/6` after phase 2 begins (the clock starts at PENDING).
    pub fn tick_collapse(&mut self) {
        if self.level_phase == LEVEL_COLLECT {
            return;
        }
        self.collapse_elapsed_ticks = self.collapse_elapsed_ticks.saturating_add(1);
        if self.level_phase == LEVEL_COLLAPSE_PENDING && self.collapse_elapsed_ticks >= COLLAPSE_PENDING_TICKS {
            self.level_phase = LEVEL_COLLAPSING;
        }
        let clock = self.collapse_elapsed_ticks;
        for entry in &mut self.collapse {
            if entry.state == CollapseTile::Gone {
                continue;
            }
            if clock >= entry.gone_tick {
                entry.state = CollapseTile::Gone;
            } else if clock >= entry.warn_start_tick {
                entry.state = CollapseTile::Warn;
            }
        }
    }

    /// Milliseconds until the last collapse batch has fallen (0 before phase 2 and once the collapse is done).
    pub fn collapse_ms(&self) -> u32 {
        if self.level_phase == LEVEL_COLLECT {
            return 0;
        }
        let last_gone = self.collapse.iter().map(|e| e.gone_tick).max().unwrap_or(0);
        let remaining = last_gone.saturating_sub(self.collapse_elapsed_ticks);
        remaining.saturating_mul(1000 / TICK_HZ)
    }

    /// Tiles to simulate and report: the hazards tiles overlaid by the collapse (collapse wins on the same id).
    pub fn world_tiles(&self) -> Vec<TileState> {
        let mut merged: BTreeMap<u16, TileState> = BTreeMap::new();
        for tile in self.hazards.tiles() {
            merged.insert(tile.id, tile);
        }
        for entry in &self.collapse {
            let state = match entry.state {
                CollapseTile::Normal => None,
                CollapseTile::Warn => Some(TILE_WARN),
                CollapseTile::Gone => Some(TILE_GONE),
            };
            if let Some(state) = state {
                merged.insert(entry.id, TileState { id: entry.id, state });
            }
        }
        merged.into_values().collect()
    }

    /// The shared tuning: movement mods plus the level event's friction override.
    pub fn tuning(&self) -> Tuning {
        let mut tuning = self.mods.tuning();
        tuning.friction_mul = self.event_friction_mul;
        tuning
    }

    pub fn beacon_reached(&self, feet: &Vec3) -> bool {
        self.beacon_open()
            && rules::beacon_reached_within(feet, &self.level.beacon, BEACON_RADIUS * self.mods.beacon_radius_mul)
    }

    pub fn enemy_views(&self) -> Vec<EnemyView> {
        self.hazards.views(&self.enemies)
    }

    pub fn info(&self) -> RunInfo {
        RunInfo {
            phase: self.phase,
            level: self.level_num,
            lives: self.lives,
            currency: self.currency,
            tuning: self.tuning(),
            fog_density_mul: if self.altar_ledger.fog_cleared {
                0.0
            } else {
                self.mods.fog_density_mul * escalation_fog_mul(self.level_num) * self.mode.fog_mul()
            },
            enemies: self.all_permanents(),
            upgrades: self.upgrades.clone(),
            curses: self.curses.clone(),
        }
    }
}

/// Friction multiplier for `event`: EVENT_ICE pulls it from content, anything else is 1.0.
fn event_friction_mul(content: &Content, event: u8) -> f32 {
    if event == EVENT_ICE {
        content
            .event(EVENT_ICE)
            .map(|e| e.friction_mul)
            .filter(|f| f.is_finite() && *f > 0.0)
            .unwrap_or(1.0)
    } else {
        1.0
    }
}

/// Highest platform top at or below `y` whose xz bounds contain (x, z). Tops may match `y` within a small tolerance
/// (spawns sitting exactly on a platform stay put).
fn platform_top_below(platforms: &[Platform], x: f32, z: f32, y: f32) -> Option<f32> {
    let mut best: Option<f32> = None;
    for platform in platforms {
        let top = platform.bounds.max.y;
        if x >= platform.bounds.min.x
            && x <= platform.bounds.max.x
            && z >= platform.bounds.min.z
            && z <= platform.bounds.max.z
            && top <= y + 0.01
        {
            best = Some(match best {
                Some(b) => b.max(top),
                None => top,
            });
        }
    }
    best
}

#[cfg(test)]
pub(crate) fn valid_picks(run: &Run) -> Vec<u8> {
    let Some(offer) = run.offer.as_ref() else { return vec![] };
    let mut picks = vec![];
    let (mut e, mut r) = (0, 0);
    for (i, c) in offer.cards.iter().enumerate() {
        if c.kind == CARD_ENEMY && e < offer.enemy_picks {
            picks.push(i as u8);
            e += 1;
        } else if c.kind != CARD_ENEMY && r < offer.reward_picks {
            picks.push(i as u8);
            r += 1;
        }
    }
    picks
}

#[cfg(test)]
mod tests {
    use super::*;
    use protocol::{EVENT_CRUMBLE, EVENT_HIGH_RISE, EVENT_NONE};

    fn content() -> Content {
        Content::load().unwrap()
    }

    fn playing_run(seed: u32, level: u32) -> (Content, Run) {
        let c = content();
        let mut run = Run::new(&c, seed, level);
        let picks = valid_picks(&run);
        run.resolve(&c, &[picks]);
        (c, run)
    }

    fn place_altar(run: &mut Run, id: u16) {
        run.level.altars = vec![protocol::Altar {
            id,
            pos: run.level.spawn,
        }];
        run.altars_used.clear();
    }

    #[test]
    fn first_round_is_plain_then_hunters_are_drafted() {
        let c = content();
        let mut run = Run::new(&c, 7, 1);
        let offer = run.offer.clone().unwrap();
        assert!(offer.cards.is_empty());
        assert_eq!((offer.enemy_picks, offer.reward_picks), (0, 0));
        assert!(run.shop.is_none(), "the first, plain lobby has no shop");
        assert_eq!(run.validate_picks(&[]), Ok(()));
        run.resolve(&c, &[vec![]]);
        assert_eq!(run.phase, PHASE_PLAYING);
        assert!(run.enemies.is_empty() && run.upgrades.is_empty() && run.curses.is_empty());
        run.finish_level(&c);
        let next = run.offer.clone().unwrap();
        assert_eq!(next.enemy_picks, 2);
    }

    #[test]
    fn second_level_offer_has_enemies_and_mixed_rewards() {
        let c = content();
        let run = Run::new(&c, 7, 2);
        let offer = run.offer.clone().unwrap();
        assert_eq!(offer.enemy_picks, 2);
        assert_eq!(offer.cards.iter().filter(|k| k.kind == CARD_ENEMY).count(), 4);
        let rewards: Vec<_> = offer.cards.iter().filter(|k| k.kind != CARD_ENEMY).collect();
        assert_eq!(rewards.len(), 3);
        assert!(rewards.iter().any(|k| k.kind == CARD_UPGRADE) && rewards.iter().any(|k| k.kind == CARD_CURSE));
    }

    #[test]
    fn picks_are_validated() {
        let c = content();
        let run = Run::new(&c, 7, 2);
        let good = valid_picks(&run);
        assert!(run.validate_picks(&good).is_ok());
        assert!(run.validate_picks(&[]).is_err());
        assert!(run.validate_picks(&[0, 0, 4]).is_err());
        assert!(run.validate_picks(&[0, 1, 2, 3, 4, 5, 6]).is_err());
        assert!(run.validate_picks(&[200]).is_err());
    }

    #[test]
    fn resolve_applies_votes_and_generates_a_level_with_drafted_enemies() {
        let c = content();
        let mut run = Run::new(&c, 7, 2);
        let picks = valid_picks(&run);
        run.resolve(&c, &[picks]);
        assert_eq!(run.phase, PHASE_PLAYING);
        assert_eq!(run.drafted_enemies.len(), 2);
        assert_eq!(run.upgrades.len() + run.curses.len(), 1);
        assert!(run.level.gifts.iter().all(|g| g.kind == protocol::GIFT_NORMAL));
        assert!(!run.level.gifts.is_empty());
        // T-078: the automatic per-level scaling is gone; a fresh run spawns the drafted
        // enemies and nothing else (no vote has been won yet).
        assert_eq!(run.enemies.len() as u32, 2 + run.mods.extra_enemies);
        assert!(run.offer.is_none());
        assert!(run.shop.is_none(), "the shop closes while a level is being played");
    }

    #[test]
    fn golden_gifts_only_after_normal_ones_then_beacon_then_lobby() {
        let c = content();
        let mut run = Run::new(&c, 11, 2);
        let picks = valid_picks(&run);
        run.resolve(&c, &[picks]);
        let feet = |g: &protocol::Gift| Vec3::new(g.pos.x, g.pos.y - 0.9, g.pos.z);
        let gifts = run.level.gifts.clone();
        assert!(gifts.iter().all(|g| g.kind == protocol::GIFT_NORMAL));
        assert!(!gifts.is_empty());
        // Golden gifts do not exist while normal gifts remain.
        run.collect_gifts(&feet(&gifts[0]));
        assert_eq!(run.currency, 0, "golden gifts are locked while normal gifts remain");
        assert!(!run.beacon_reached(&run.level.beacon.clone()));
        // gifts[0] was gathered by the probe above; gather the remaining normal gifts.
        for g in &gifts[1..] {
            assert_eq!(run.collect_gifts(&feet(g)).len(), 1);
        }
        assert!(run.beacon_open());
        // Phase 2: golden gifts at the collected positions, same ids, 1 gold each.
        for g in &gifts {
            run.collect_gifts(&feet(g));
        }
        assert_eq!(run.currency, gifts.len() as u32 * GOLDEN_VALUE);
        assert_eq!(run.gifts_collected, gifts.len() as u32);
        assert!(run.beacon_reached(&run.level.beacon.clone()));
        run.finish_level(&c);
        assert_eq!(run.phase, PHASE_ENEMY_DRAFT);
        assert_eq!(run.level_num, 3);
        let offer = run.offer.clone().unwrap();
        assert_eq!(offer.enemy_picks, 0, "enemies are drafted once per run");
        assert_eq!(offer.cards.len(), 3);
        assert!(run.shop.is_some(), "level 2+ lobbies offer the shop");
    }

    #[test]
    fn every_tenth_level_offers_greater_curses() {
        let c = content();
        let mut run = Run::new(&c, 3, 9);
        let picks = valid_picks(&run);
        run.resolve(&c, &[picks]);
        run.finish_level(&c);
        let offer = run.offer.clone().unwrap();
        for card in &offer.cards {
            assert!(
                c.curse(&card.id).is_some_and(|k| k.greater),
                "{} is not a greater curse",
                card.id
            );
        }
    }

    #[test]
    fn votes_tally_majority_with_ties_to_first_offered() {
        let c = content();
        let mut run = Run::new(&c, 5, 2);
        let offer = run.offer.clone().unwrap();
        // Enemies: both vote 0; then 1 and 2 tie -> the first offered (1) wins. Rewards: both vote 5.
        run.resolve(&c, &[vec![0, 1, 5], vec![0, 2, 5]]);
        assert_eq!(
            run.drafted_enemies,
            vec![offer.cards[0].id.clone(), offer.cards[1].id.clone()]
        );
        let chosen = &offer.cards[5];
        assert!(run.upgrades.contains(&chosen.id) || run.curses.contains(&chosen.id));
    }

    #[test]
    fn dormant_enemies_are_harmless() {
        use sim::hazards::{HazardEffect, PlayerSense};
        let c = content();
        let mut run = Run::new(&c, 13, 2);
        let picks = valid_picks(&run);
        run.resolve(&c, &[picks]);
        // Spawn grace keeps the enemies dormant; the sim must emit no kill effect for a touching player.
        for e in &mut run.enemies {
            assert!(e.active_in > 0);
        }
        let e = run.enemies[0].pos;
        let touching = Vec3::new(e.x, e.y - 0.9, e.z);
        let senses = vec![PlayerSense {
            id: 1,
            feet: touching,
            vel: Vec3::new(0.0, 0.0, 0.0),
            warded: false,
            ..PlayerSense::default()
        }];
        let effects = run
            .hazards
            .step(&mut run.enemies, &run.world, &run.level, &senses, &[], 1.0);
        assert!(
            !effects
                .iter()
                .any(|e| matches!(e, HazardEffect::Kill { player, .. } if *player == 1)),
            "a dormant enemy deals no damage"
        );
    }

    #[test]
    fn run_info_carries_event_friction() {
        let c = content();
        let mut run = Run::new(&c, 42, 1);
        assert_eq!(run.info().tuning.friction_mul, 1.0);
        run.event_friction_mul = 0.25;
        assert_eq!(run.info().tuning.friction_mul, 0.25);
        assert_eq!(run.tuning().friction_mul, 0.25);
        assert_eq!(event_friction_mul(&c, protocol::EVENT_ICE), 0.25);
        assert_eq!(event_friction_mul(&c, EVENT_HIGH_RISE), 1.0);
        assert_eq!(event_friction_mul(&c, EVENT_CRUMBLE), 1.0);
        assert_eq!(event_friction_mul(&c, EVENT_NONE), 1.0);
    }

    #[test]
    fn shop_offer_only_from_level_two_and_is_deterministic() {
        let c = content();
        let first = Run::new(&c, 42, 1);
        assert!(first.shop.is_none(), "the first, plain lobby has no shop");
        let second = Run::new(&c, 42, 2);
        let shop = second.shop.as_ref().unwrap();
        assert!(shop.len() >= SHOP_OFFER, "base offer plus contextual items");
        let ids: Vec<&str> = shop.iter().map(|item| item.id.as_str()).collect();
        let unique: std::collections::BTreeSet<&str> = ids.iter().copied().collect();
        assert_eq!(unique.len(), ids.len(), "offer has distinct items");
        assert!(shop.iter().all(|item| item.stock > 0 && item.cost > 0));
        assert!(
            shop.iter().any(|item| item.id.starts_with("upgrade::")),
            "the shop draws contextual upgrade cards"
        );
        assert_eq!(
            shop.iter().filter(|item| item.id.starts_with("perk_")).count(),
            4,
            "one perk per class"
        );
        // Same seed, same offer.
        let again = Run::new(&c, 42, 2);
        assert_eq!(second.shop, again.shop);
    }

    #[test]
    fn buy_applies_life_and_mods_and_charges() {
        let c = content();
        let mut run = Run::new(&c, 42, 2);
        run.currency = 100;
        run.shop = Some(vec![
            ShopOffer {
                id: "kept_flame".to_string(),
                cost: 30,
                stock: 1,
            },
            ShopOffer {
                id: "worn_treads".to_string(),
                cost: 12,
                stock: 2,
            },
        ]);
        let before_lives = run.lives;
        let before_speed = run.mods.run_speed_mul;
        assert_eq!(run.buy_shop_item(&c, "kept_flame"), Ok(ShopPurchase::Life));
        assert_eq!(run.lives, before_lives + 1);
        assert_eq!(run.currency, 70);
        // Stock ran out.
        assert_eq!(run.buy_shop_item(&c, "kept_flame"), Err(ShopError::SoldOut));
        assert_eq!(run.buy_shop_item(&c, "worn_treads"), Ok(ShopPurchase::Mods));
        assert!(run.mods.run_speed_mul > before_speed);
        assert!(run.purchased_mods.iter().any(|id| id == "worn_treads"));
        assert_eq!(run.currency, 58);
        // Unknown item, and not enough currency.
        assert_eq!(run.buy_shop_item(&c, "nope"), Err(ShopError::UnknownItem));
        run.currency = 0;
        assert_eq!(run.buy_shop_item(&c, "worn_treads"), Err(ShopError::NotEnoughCurrency));
        // The shop is closed while a level is being played.
        run.phase = PHASE_PLAYING;
        assert_eq!(run.buy_shop_item(&c, "worn_treads"), Err(ShopError::NotInLobby));
    }

    #[test]
    fn reroll_replaces_rewards_and_keeps_enemy_cards() {
        let c = content();
        let mut run = Run::new(&c, 42, 2);
        let before = run.offer.clone().unwrap();
        let enemy_before: Vec<_> = before
            .cards
            .iter()
            .filter(|card| card.kind == CARD_ENEMY)
            .cloned()
            .collect();
        run.currency = 100;
        run.shop = Some(vec![ShopOffer {
            id: "new_hand".to_string(),
            cost: 15,
            stock: 2,
        }]);
        assert_eq!(run.buy_shop_item(&c, "new_hand"), Ok(ShopPurchase::Reroll));
        let after = run.offer.clone().unwrap();
        let enemy_after: Vec<_> = after
            .cards
            .iter()
            .filter(|card| card.kind == CARD_ENEMY)
            .cloned()
            .collect();
        assert_eq!(enemy_before, enemy_after, "enemy cards are kept at the same indices");
        assert_eq!(after.cards.len(), before.cards.len());
        assert_eq!(after.enemy_picks, before.enemy_picks);
        assert_eq!(after.reward_picks, before.reward_picks);
        assert!(after.cards.iter().any(|card| card.kind != CARD_ENEMY));
        assert_eq!(run.currency, 85);
        assert_eq!(run.shop.as_ref().unwrap()[0].stock, 1);
    }

    #[test]
    fn altar_ids_select_the_four_designs_in_content_order() {
        assert_eq!(AltarKind::from_id(0), AltarKind::Shortcut);
        assert_eq!(AltarKind::from_id(1), AltarKind::GiftSight);
        assert_eq!(AltarKind::from_id(2), AltarKind::ClearSky);
        assert_eq!(AltarKind::from_id(3), AltarKind::Debt);
        assert_eq!(AltarKind::from_id(4), AltarKind::Shortcut);
        let preview = AltarKind::Debt.preview();
        assert!(preview.benefit.contains(DEBT_ADVANCE.to_string().as_str()));
        assert!(!preview.cost.is_empty());
    }

    #[test]
    fn altar_ledger_round_trips_and_rejects_garbage() {
        let ledger = AltarLedger {
            shortcut_pending: true,
            pending_temporary_curses: vec!["leadened_soles".to_string()],
            temporary_curses: vec!["heavy_air".to_string(), "numb_fingers".to_string()],
            extra_enemies_next_level: 2,
            gift_sight: true,
            hardest_pool_next: true,
            fog_cleared: true,
            debt_owed: 30,
            debt_levels_left: 2,
            debt_garnished_this_level: true,
            next_enemy_id: 7,
        };
        let encoded = ledger.encode();
        assert_eq!(AltarLedger::decode(&encoded), Some(ledger));
        assert_eq!(AltarLedger::decode("not a ledger"), None);
        assert_eq!(AltarLedger::decode("altar-ledger v1\nshortcut_pending=maybe\n"), None);
    }

    #[test]
    fn shortcut_altar_skips_the_draft_and_costs_curses_and_enemies() {
        let (c, mut run) = playing_run(42, 2);
        place_altar(&mut run, 0);
        let drafted = run.drafted_enemies.len();
        let outcome = run.use_altar(&c, run.level.spawn).expect("shortcut altar");
        assert_eq!(outcome.kind, AltarKind::Shortcut);
        assert!(run.altar_ledger.shortcut_pending);
        assert_eq!(run.altar_ledger.pending_temporary_curses.len(), SHORTCUT_CURSES);
        assert_eq!(run.altar_ledger.extra_enemies_next_level, SHORTCUT_EXTRA_ENEMIES);
        // Second use of the same altar does nothing.
        let used = run.altars_used.clone();
        assert!(run.use_altar(&c, run.level.spawn).is_none());
        assert_eq!(run.altars_used, used);
        assert_eq!(run.altar_ledger.pending_temporary_curses.len(), SHORTCUT_CURSES);
        // Level completion skips the draft and opens the shop.
        run.finish_level(&c);
        let offer = run.offer.clone().expect("lobby offer");
        assert_eq!((offer.enemy_picks, offer.reward_picks), (0, 0));
        assert!(offer.cards.is_empty());
        assert!(run.shop.is_some());
        // The next level applies the curses and the extra enemies.
        run.resolve(&c, &[vec![]]);
        assert_eq!(run.altar_ledger.temporary_curses.len(), SHORTCUT_CURSES);
        assert!(run.altar_ledger.pending_temporary_curses.is_empty());
        assert_eq!(run.altar_ledger.extra_enemies_next_level, 0);
        let expected = drafted + run.mods.extra_enemies as usize + SHORTCUT_EXTRA_ENEMIES as usize;
        assert_eq!(run.enemies.len(), expected);
        // The temporary curses are gone at the end of the level.
        run.finish_level(&c);
        assert!(run.altar_ledger.temporary_curses.is_empty());
    }

    #[test]
    fn gift_sight_altar_highlights_gifts_and_hardens_the_next_offer() {
        let (c, mut run) = playing_run(42, 2);
        place_altar(&mut run, 1);
        let outcome = run.use_altar(&c, run.level.spawn).expect("gift sight altar");
        assert_eq!(outcome.kind, AltarKind::GiftSight);
        assert!(run.gift_sight());
        assert!(run.altar_ledger.hardest_pool_next);
        // Second use does nothing.
        assert!(run.use_altar(&c, run.level.spawn).is_none());
        assert!(run.gift_sight());
        // The next offer is drawn from the greater curses.
        run.finish_level(&c);
        assert!(!run.gift_sight(), "the highlight lasts one level");
        assert!(!run.altar_ledger.hardest_pool_next, "the flag is consumed by the offer");
        let offer = run.offer.clone().expect("lobby offer");
        assert!(!offer.cards.is_empty());
        for card in &offer.cards {
            assert_eq!(card.kind, CARD_CURSE);
            assert!(
                c.curse(&card.id).is_some_and(|def| def.greater),
                "{} is not a greater curse",
                card.id
            );
        }
    }

    #[test]
    fn clear_sky_altar_clears_fog_and_spawns_lure_and_static() {
        let (c, mut run) = playing_run(42, 2);
        place_altar(&mut run, 2);
        let before = run.enemies.len();
        let outcome = run.use_altar(&c, run.level.spawn).expect("clear sky altar");
        assert_eq!(outcome.kind, AltarKind::ClearSky);
        assert!(run.altar_ledger.fog_cleared);
        assert_eq!(run.info().fog_density_mul, 0.0);
        assert_eq!(run.enemies.len(), before + 2);
        assert_eq!(run.altar_spawns, vec!["lure".to_string(), "static".to_string()]);
        assert!(run.enemies[before..].iter().all(|e| e.active_in == 0));
        // Second use does nothing.
        assert!(run.use_altar(&c, run.level.spawn).is_none());
        assert_eq!(run.enemies.len(), before + 2);
        // The fog returns next level.
        run.finish_level(&c);
        assert!(!run.altar_ledger.fog_cleared);
        assert!(run.info().fog_density_mul > 0.0);
    }

    #[test]
    fn debt_altar_advances_currency_and_garnishes_two_levels_of_income() {
        let (c, mut run) = playing_run(42, 2);
        place_altar(&mut run, 3);
        let before = run.currency;
        let outcome = run.use_altar(&c, run.level.spawn).expect("debt altar");
        assert_eq!(outcome.kind, AltarKind::Debt);
        assert_eq!(run.currency, before + DEBT_ADVANCE);
        assert_eq!(run.altar_ledger.debt_owed, DEBT_ADVANCE);
        assert_eq!(run.altar_ledger.debt_levels_left, DEBT_REPAY_LEVELS);
        // Second use does nothing.
        assert!(run.use_altar(&c, run.level.spawn).is_none());
        assert_eq!(run.currency, before + DEBT_ADVANCE);
        // The level the debt was taken does not count against the repayment window.
        run.finish_level(&c);
        assert_eq!(run.altar_ledger.debt_levels_left, DEBT_REPAY_LEVELS);
        // First repayment level: income pays the debt first.
        run.resolve(&c, &[vec![]]);
        assert!(run.altar_ledger.debt_garnished_this_level);
        let gifts = run.level.gifts.clone();
        run.gifts_taken = gifts.iter().map(|g| g.id).collect();
        run.level_phase = LEVEL_COLLAPSE_PENDING;
        let currency_before = run.currency;
        let debt_before = run.altar_ledger.debt_owed;
        let feet = Vec3::new(gifts[0].pos.x, gifts[0].pos.y - 0.9, gifts[0].pos.z);
        let collected = run.collect_gifts(&feet).len() as u32;
        assert!(collected > 0);
        assert_eq!(run.currency, currency_before, "income goes to the debt");
        assert_eq!(run.altar_ledger.debt_owed, debt_before - collected * GOLDEN_VALUE);
        run.finish_level(&c);
        assert_eq!(run.altar_ledger.debt_levels_left, DEBT_REPAY_LEVELS - 1);
        // Second repayment level: the rest is garnished, then any remainder is taken from the purse.
        run.resolve(&c, &[vec![]]);
        assert!(run.altar_ledger.debt_garnished_this_level);
        let gifts = run.level.gifts.clone();
        run.gifts_taken = gifts.iter().map(|g| g.id).collect();
        run.level_phase = LEVEL_COLLAPSE_PENDING;
        for g in &gifts {
            let feet = Vec3::new(g.pos.x, g.pos.y - 0.9, g.pos.z);
            run.collect_gifts(&feet);
        }
        let owed = run.altar_ledger.debt_owed;
        let purse = run.currency;
        run.finish_level(&c);
        assert_eq!(run.altar_ledger.debt_levels_left, 0);
        assert_eq!(run.altar_ledger.debt_owed, 0);
        assert_eq!(run.currency, purse.saturating_sub(owed));
    }

    #[test]
    fn shortcut_effect_survives_a_restart_and_applies_after_the_transition() {
        let (c, mut run) = playing_run(42, 2);
        place_altar(&mut run, 0);
        run.use_altar(&c, run.level.spawn).expect("shortcut altar");
        let state = run.altar_state();
        let mut restarted = Run::new(&c, 42, 2);
        assert!(restarted.restore_altar_state(&c, &state));
        assert!(restarted.altar_ledger.shortcut_pending);
        restarted.finish_level(&c);
        let offer = restarted.offer.clone().expect("lobby offer");
        assert!(offer.cards.is_empty(), "the restored shortcut skips the draft");
        assert!(restarted.shop.is_some());
    }

    #[test]
    fn debt_effect_survives_a_level_transition_and_a_restart() {
        let (c, mut run) = playing_run(42, 2);
        place_altar(&mut run, 3);
        run.use_altar(&c, run.level.spawn).expect("debt altar");
        let owed = run.altar_ledger.debt_owed;
        assert_eq!(owed, DEBT_ADVANCE);
        // The debt survives the level transition.
        run.finish_level(&c);
        assert_eq!(run.altar_ledger.debt_owed, owed);
        assert_eq!(run.altar_ledger.debt_levels_left, DEBT_REPAY_LEVELS);
        // Serialize, restart into a fresh run, restore.
        let state = run.altar_state();
        let mut restarted = Run::new(&c, 42, 3);
        assert!(restarted.restore_altar_state(&c, &state));
        assert_eq!(restarted.altar_ledger.debt_owed, owed);
        assert_eq!(restarted.altar_ledger.debt_levels_left, DEBT_REPAY_LEVELS);
        // The restored debt still garnishes income.
        restarted.resolve(&c, &[vec![]]);
        assert!(restarted.altar_ledger.debt_garnished_this_level);
        let gifts = restarted.level.gifts.clone();
        restarted.gifts_taken = gifts.iter().map(|g| g.id).collect();
        restarted.level_phase = LEVEL_COLLAPSE_PENDING;
        let before = restarted.currency;
        let feet = Vec3::new(gifts[0].pos.x, gifts[0].pos.y - 0.9, gifts[0].pos.z);
        let collected = restarted.collect_gifts(&feet).len() as u32;
        assert!(collected > 0);
        assert_eq!(restarted.currency, before);
        assert_eq!(restarted.altar_ledger.debt_owed, owed - collected * GOLDEN_VALUE);
    }

    #[test]
    fn try_use_altar_returns_the_benefit_and_cost_tokens() {
        let (c, mut run) = playing_run(42, 2);
        place_altar(&mut run, 1);
        let (benefit, cost) = run.try_use_altar(&c, run.level.spawn).expect("altar");
        assert_eq!(benefit.as_str(), AltarKind::GiftSight.benefit_id());
        assert_eq!(cost.as_str(), AltarKind::GiftSight.cost_id());
        assert!(run.try_use_altar(&c, run.level.spawn).is_none());
    }

    #[test]
    fn mimic_spawn_drops_onto_platform_top() {
        let spawn_above = platform_top_below(
            &[Platform {
                id: 3,
                kind: protocol::PLATFORM_NORMAL,
                bounds: protocol::Aabb {
                    min: Vec3::new(-5.0, -1.0, -5.0),
                    max: Vec3::new(5.0, 2.0, 5.0),
                },
            }],
            0.0,
            0.0,
            10.0,
        );
        assert_eq!(spawn_above, Some(2.0));
        let none_below = platform_top_below(&[], 0.0, 0.0, 10.0);
        assert_eq!(none_below, None);
    }

    #[test]
    fn currency_is_earned_from_golden_gifts() {
        let c = content();
        let mut run = Run::new(&c, 11, 2);
        let picks = valid_picks(&run);
        run.resolve(&c, &[picks]);
        // Skip straight to phase 2 with every normal gift already gathered.
        let gift_ids: Vec<u16> = run.level.gifts.iter().map(|g| g.id).collect();
        let gifts = run.level.gifts.clone();
        run.gifts_taken = gift_ids;
        run.level_phase = LEVEL_COLLAPSE_PENDING;
        let before = run.currency;
        for g in &gifts {
            let feet = Vec3::new(g.pos.x, g.pos.y - 0.9, g.pos.z);
            run.collect_gifts(&feet);
        }
        assert_eq!(run.currency, before + gifts.len() as u32 * GOLDEN_VALUE);
        // Gifts already golden-taken cannot mint again.
        let spent = run.currency;
        for g in &gifts {
            let feet = Vec3::new(g.pos.x, g.pos.y - 0.9, g.pos.z);
            run.collect_gifts(&feet);
        }
        assert_eq!(run.currency, spent);
    }

    #[test]
    fn spending_spends_and_insufficient_funds_changes_nothing() {
        let c = content();
        let mut run = Run::new(&c, 42, 2);
        run.currency = 24;
        run.shop = Some(vec![ShopOffer {
            id: "worn_treads".to_string(),
            cost: 24,
            stock: 1,
        }]);
        let before_speed = run.mods.run_speed_mul;
        assert_eq!(run.buy_shop_item(&c, "worn_treads"), Ok(ShopPurchase::Mods));
        assert_eq!(run.currency, 0);
        assert!(run.mods.run_speed_mul > before_speed);
        // Not enough funds now: currency, stock, mods all untouched.
        let speed_after = run.mods.run_speed_mul;
        run.shop = Some(vec![ShopOffer {
            id: "worn_treads".to_string(),
            cost: 24,
            stock: 1,
        }]);
        assert_eq!(run.buy_shop_item(&c, "worn_treads"), Err(ShopError::NotEnoughCurrency));
        assert_eq!(run.currency, 0);
        assert_eq!(run.shop.as_ref().unwrap()[0].stock, 1);
        assert_eq!(run.mods.run_speed_mul, speed_after);
    }

    #[test]
    fn a_rejected_purchase_is_fully_refunded() {
        let c = content();
        let mut run = Run::new(&c, 42, 2);
        run.currency = 100;
        let before_speed = run.mods.run_speed_mul;
        let before_lives = run.lives;
        // Rejection by sold-out stock.
        run.shop = Some(vec![ShopOffer {
            id: "kept_flame".to_string(),
            cost: 30,
            stock: 0,
        }]);
        assert_eq!(run.buy_shop_item(&c, "kept_flame"), Err(ShopError::SoldOut));
        assert_eq!(run.currency, 100);
        // Rejection by insufficient funds.
        run.shop = Some(vec![ShopOffer {
            id: "kept_flame".to_string(),
            cost: 200,
            stock: 1,
        }]);
        assert_eq!(run.buy_shop_item(&c, "kept_flame"), Err(ShopError::NotEnoughCurrency));
        assert_eq!(run.currency, 100);
        assert_eq!(run.shop.as_ref().unwrap()[0].stock, 1);
        // Rejection by unknown item.
        run.shop = Some(vec![ShopOffer {
            id: "kept_flame".to_string(),
            cost: 30,
            stock: 1,
        }]);
        assert_eq!(run.buy_shop_item(&c, "not_a_thing"), Err(ShopError::UnknownItem));
        assert_eq!(run.currency, 100);
        // Rejection after validation but before the effect: a purge for a curse the run does not hold
        // charges nothing and takes no stock.
        run.shop = Some(vec![ShopOffer {
            id: "purge::leadened_soles".to_string(),
            cost: SHOP_PURGE_COST,
            stock: 1,
        }]);
        assert_eq!(
            run.buy_shop_item(&c, "purge::leadened_soles"),
            Err(ShopError::UnknownItem)
        );
        assert_eq!(run.currency, 100);
        assert_eq!(run.shop.as_ref().unwrap()[0].stock, 1);
        // Rejection by effect failure: a reroll whose draft offer is already gone charges nothing.
        run.offer = None;
        run.shop = Some(vec![ShopOffer {
            id: "new_hand".to_string(),
            cost: 15,
            stock: 1,
        }]);
        assert_eq!(run.buy_shop_item(&c, "new_hand"), Err(ShopError::UnknownItem));
        assert_eq!(run.currency, 100);
        assert_eq!(run.shop.as_ref().unwrap()[0].stock, 1);
        // Nothing leaked into mods or lives.
        assert_eq!(run.mods.run_speed_mul, before_speed);
        assert_eq!(run.lives, before_lives);
    }

    #[test]
    fn buying_an_upgrade_card_applies_it_and_records_it() {
        let c = content();
        let mut run = Run::new(&c, 42, 2);
        run.currency = SHOP_UPGRADE_COST;
        run.shop = Some(vec![ShopOffer {
            id: "upgrade::zephyr_tread".to_string(),
            cost: SHOP_UPGRADE_COST,
            stock: 1,
        }]);
        let before_speed = run.mods.run_speed_mul;
        assert_eq!(run.buy_shop_item(&c, "upgrade::zephyr_tread"), Ok(ShopPurchase::Mods));
        assert_eq!(run.currency, 0);
        assert!(run.mods.run_speed_mul > before_speed);
        assert!(run.upgrades.iter().any(|id| id == "zephyr_tread"));
        assert_eq!(run.buy_shop_item(&c, "upgrade::zephyr_tread"), Err(ShopError::SoldOut));
        // An upgrade id that is not in content is refused without charging.
        run.currency = 100;
        run.shop = Some(vec![ShopOffer {
            id: "upgrade::not_a_real_upgrade".to_string(),
            cost: SHOP_UPGRADE_COST,
            stock: 1,
        }]);
        assert_eq!(
            run.buy_shop_item(&c, "upgrade::not_a_real_upgrade"),
            Err(ShopError::UnknownItem)
        );
        assert_eq!(run.currency, 100);
    }

    #[test]
    fn buying_a_perk_applies_its_mods_and_records_the_purchase() {
        let c = content();
        let mut run = Run::new(&c, 42, 2);
        run.currency = 100;
        run.shop = Some(vec![ShopOffer {
            id: "perk_striker".to_string(),
            cost: 45,
            stock: 1,
        }]);
        let before_cd = run.mods.dash_cooldown_mul;
        assert_eq!(run.buy_shop_item(&c, "perk_striker"), Ok(ShopPurchase::Mods));
        assert!(run.mods.dash_cooldown_mul < before_cd);
        assert!(run.purchased_mods.iter().any(|id| id == "perk_striker"));
        assert_eq!(run.currency, 55);
    }

    #[test]
    fn counters_are_offered_only_for_accumulated_permanents() {
        let c = content();
        let mut run = Run::new(&c, 42, 2);
        assert!(run.all_permanents().is_empty());
        run.permanents = vec!["pallbearer".to_string(), "mimic".to_string()];
        run.shop = run.make_shop(&c);
        let shop = run.shop.as_ref().expect("level 2+ lobby has a shop");
        assert!(shop.iter().any(|item| item.id == "counter_pallbearer"));
        assert!(shop.iter().any(|item| item.id == "counter_mimic"));
        assert!(
            !shop.iter().any(|item| item.id == "counter_turret"),
            "a counter is only sold against a permanent the run has"
        );
        // Buying a counter applies its mods and records the purchase for rebuilds.
        run.currency = 100;
        let before_dash = run.mods.dash_speed_mul;
        assert_eq!(run.buy_shop_item(&c, "counter_pallbearer"), Ok(ShopPurchase::Mods));
        assert!(run.mods.dash_speed_mul > before_dash);
        assert!(run.purchased_mods.iter().any(|id| id == "counter_pallbearer"));
        assert_eq!(run.currency, 100 - 35);
    }

    #[test]
    fn a_curse_purge_drops_one_stack_and_unloads_the_effect() {
        let c = content();
        let mut run = Run::new(&c, 42, 2);
        let leadened = c.curse("leadened_soles").expect("curse exists").mods.clone();
        // Apply it twice by hand, as two draft/curse sources would.
        run.mods.apply(&leadened);
        run.mods.apply(&leadened);
        run.curses = vec!["leadened_soles".to_string(), "leadened_soles".to_string()];
        // Two stacks multiply to 0.7744, which the run-speed clamp floors at 0.8.
        assert!(
            (run.mods.run_speed_mul - 0.8).abs() < 1e-6,
            "two stacks clamp at the floor"
        );
        run.currency = 100;
        run.shop = Some(vec![ShopOffer {
            id: "purge::leadened_soles".to_string(),
            cost: SHOP_PURGE_COST,
            stock: 2,
        }]);
        assert_eq!(run.buy_shop_item(&c, "purge::leadened_soles"), Ok(ShopPurchase::Mods));
        assert_eq!(run.currency, 100 - SHOP_PURGE_COST);
        assert_eq!(run.curses, vec!["leadened_soles".to_string()]);
        assert!((run.mods.run_speed_mul - 0.88).abs() < 1e-6, "one stack remains");
        assert_eq!(run.buy_shop_item(&c, "purge::leadened_soles"), Ok(ShopPurchase::Mods));
        assert!(run.curses.is_empty());
        assert!((run.mods.run_speed_mul - 1.0).abs() < 1e-6, "no stacks remain");
        assert_eq!(run.buy_shop_item(&c, "purge::leadened_soles"), Err(ShopError::SoldOut));
    }

    #[test]
    fn a_purge_preserves_purchased_shop_mods() {
        let c = content();
        let mut run = Run::new(&c, 42, 2);
        let leadened = c.curse("leadened_soles").expect("curse exists").mods.clone();
        run.mods.apply(&leadened);
        run.curses = vec!["leadened_soles".to_string()];
        run.currency = 200;
        run.shop = Some(vec![
            ShopOffer {
                id: "worn_treads".to_string(),
                cost: 24,
                stock: 1,
            },
            ShopOffer {
                id: "purge::leadened_soles".to_string(),
                cost: SHOP_PURGE_COST,
                stock: 1,
            },
        ]);
        assert_eq!(run.buy_shop_item(&c, "worn_treads"), Ok(ShopPurchase::Mods));
        let speed_with_treads = run.mods.run_speed_mul;
        assert_eq!(run.buy_shop_item(&c, "purge::leadened_soles"), Ok(ShopPurchase::Mods));
        assert!(
            (run.mods.run_speed_mul - speed_with_treads / 0.88).abs() < 1e-5,
            "the shop-purchased mods survive the rebuild"
        );
    }

    #[test]
    fn purges_are_offered_for_held_curses_only() {
        let c = content();
        let mut run = Run::new(&c, 42, 2);
        run.curses = vec![
            "heavy_air".to_string(),
            "heavy_air".to_string(),
            "numb_fingers".to_string(),
        ];
        run.shop = run.make_shop(&c);
        let shop = run.shop.as_ref().expect("level 2+ lobby has a shop");
        let heavy = shop
            .iter()
            .find(|item| item.id == "purge::heavy_air")
            .expect("purge offered for the held curse");
        assert_eq!(heavy.stock, 2, "stock equals the stacks held");
        assert!(shop.iter().any(|item| item.id == "purge::numb_fingers"));
        assert!(
            !shop.iter().any(|item| item.id == "purge::leadened_soles"),
            "unheld curses are not offered a purge"
        );
    }

    #[test]
    fn two_clients_racing_for_the_last_copy_exactly_one_wins() {
        let c = content();
        let mut run = Run::new(&c, 42, 2);
        run.currency = 100;
        let lives_before = run.lives;
        run.shop = Some(vec![ShopOffer {
            id: "kept_flame".to_string(),
            cost: 60,
            stock: 1,
        }]);
        let shared = std::sync::Mutex::new(run);
        let results: Vec<Result<ShopPurchase, ShopError>> = std::thread::scope(|scope| {
            let mut handles = Vec::new();
            for _ in 0..8 {
                let shared = &shared;
                let c = &c;
                handles.push(scope.spawn(move || {
                    let mut guard = shared.lock().unwrap();
                    guard.buy_shop_item(c, "kept_flame")
                }));
            }
            handles.into_iter().map(|h| h.join().unwrap()).collect()
        });
        let wins = results.iter().filter(|r| matches!(r, Ok(ShopPurchase::Life))).count();
        let sold_out = results.iter().filter(|r| matches!(r, Err(ShopError::SoldOut))).count();
        assert_eq!(wins, 1, "exactly one client wins the last copy");
        assert_eq!(sold_out, results.len() - 1, "everyone else is told it sold out");
        let run = shared.into_inner().unwrap();
        assert_eq!(run.currency, 40, "the currency is charged exactly once");
        assert_eq!(run.shop.as_ref().unwrap()[0].stock, 0);
        assert_eq!(run.lives, lives_before + 1, "the life is granted exactly once");
    }
}

#[cfg(test)]
mod vote_tests {
    use super::*;

    fn content() -> Content {
        Content::load().unwrap()
    }

    fn entry(id: &str, min_level: u32, cap: u32, tags: &[&str], order: usize) -> PoolEntry {
        PoolEntry {
            id: id.to_string(),
            name: id.to_string(),
            min_level,
            selection_cap: cap,
            tags: tags.iter().map(|t| t.to_string()).collect(),
            class_interactions: BTreeMap::new(),
            requires: Vec::new(),
            content_order: order,
        }
    }

    fn opt(index: u8, id: &str, kind: u8) -> SessionOption {
        SessionOption {
            index,
            id: id.to_string(),
            name: id.to_string(),
            kind,
            temporary: false,
        }
    }

    fn session(level: u32, seed: u32, host: Option<u32>) -> VoteSession {
        VoteSession::open(
            level,
            seed,
            vec![
                opt(0, "a", CARD_ENEMY),
                opt(1, "b", CARD_ENEMY),
                opt(2, "c", CARD_ENEMY),
            ],
            host,
        )
    }

    #[test]
    fn majority_winner_takes_the_session() {
        let mut s = session(3, 42, Some(1));
        for (id, option) in [(1, 1u8), (2, 1), (3, 1), (4, 0)] {
            s.cast(id, option).unwrap();
        }
        let outcome = s.resolve().unwrap();
        assert_eq!(outcome.winner, 1);
        assert_eq!(outcome.id, "b");
        assert!(outcome.permanent);
        assert!(!outcome.temporary);
        assert_eq!(outcome.counts, vec![1, 3, 0]);
    }

    #[test]
    fn a_tie_goes_to_the_hosts_vote() {
        // Host voted for option 0; the rest split 1-1: the host's option wins.
        let mut s = session(3, 42, Some(7));
        s.cast(7, 0).unwrap();
        s.cast(8, 0).unwrap();
        s.cast(9, 1).unwrap();
        assert_eq!(s.resolve().unwrap().winner, 0);
        // Host voted for option 1 instead; the tie flips to option 1.
        let mut s = session(3, 42, Some(7));
        s.cast(7, 1).unwrap();
        s.cast(8, 1).unwrap();
        s.cast(9, 0).unwrap();
        assert_eq!(s.resolve().unwrap().winner, 1);
    }

    #[test]
    fn a_tie_without_a_host_vote_is_a_deterministic_coin() {
        // Host present in the session but silent; the tie is broken by the seed, not the clock.
        let winner_of = || {
            let mut s = session(3, 42, Some(7));
            s.cast(8, 0).unwrap();
            s.cast(9, 1).unwrap();
            s.resolve().unwrap().winner
        };
        let first = winner_of();
        assert!(first == 0 || first == 1, "the coin lands on a tied option");
        assert_eq!(first, winner_of(), "same seed and level, same coin");
        // A three-way tie: same determinism, winner in the tied set.
        let winner_of_three = || {
            let mut s = session(9, 1234, Some(7));
            s.cast(8, 0).unwrap();
            s.cast(9, 1).unwrap();
            s.cast(10, 2).unwrap();
            s.resolve().unwrap().winner
        };
        let w = winner_of_three();
        assert!(w <= 2, "the coin lands on a tied option");
        assert_eq!(w, winner_of_three());
    }

    #[test]
    fn the_pool_respects_selection_cap() {
        let entries = vec![entry("unique", 1, 1, &[], 0), entry("pair", 1, 2, &[], 1)];
        let pool = vote_pool(&entries, 1, &[], &["unique".to_string(), "pair".to_string()]);
        assert_eq!(
            pool.iter().map(|e| e.id.as_str()).collect::<Vec<_>>(),
            vec!["pair"],
            "a cap-1 entry with a permanent is out; the pair with one copy stays"
        );
        let pool = vote_pool(
            &entries,
            1,
            &[],
            &["unique".to_string(), "pair".to_string(), "pair".to_string()],
        );
        assert!(pool.is_empty(), "both entries are at their cap");
    }

    #[test]
    fn chorus_needs_two_small_permanents_and_level_ten() {
        let entries = vec![
            entry("chorus", 10, 1, &["ambient"], 0),
            entry("small_a", 1, 2, &["small"], 1),
            entry("small_b", 1, 2, &["small"], 2),
        ];
        let ids = |level: u32, permanents: &[String]| {
            vote_pool(&entries, level, &[], permanents)
                .into_iter()
                .map(|e| e.id)
                .collect::<Vec<_>>()
        };
        let pair = vec!["small_a".to_string(), "small_b".to_string()];
        assert!(!ids(10, &[]).contains(&"chorus".to_string()), "no small permanents");
        assert!(
            !ids(9, &pair).contains(&"chorus".to_string()),
            "level is below the gate"
        );
        assert!(ids(10, &pair).contains(&"chorus".to_string()), "both gates met");
    }

    #[test]
    fn the_pool_scales_by_level() {
        let entries = vec![
            entry("early", 1, 2, &[], 0),
            entry("mid", 3, 1, &[], 1),
            entry("late", 6, 1, &[], 2),
        ];
        let ids = |level: u32| {
            vote_pool(&entries, level, &[], &[])
                .into_iter()
                .map(|e| e.id)
                .collect::<Vec<_>>()
        };
        assert_eq!(ids(1), vec!["early"]);
        assert_eq!(ids(2), vec!["early"]);
        assert_eq!(ids(3), vec!["early", "mid"]);
        assert_eq!(ids(6), vec!["early", "mid", "late"]);
        // Deterministic order: min_level ascending, ties by content order (no RNG).
        let tie = vec![entry("b", 1, 1, &[], 5), entry("a", 1, 1, &[], 0)];
        let ids: Vec<String> = vote_pool(&tie, 1, &[], &[]).into_iter().map(|e| e.id).collect();
        assert_eq!(ids, vec!["a", "b"]);
    }

    #[test]
    fn pool_filters_requires_hooks_and_class_composition() {
        let mut needs_pair = entry("needs_pair", 1, 1, &[], 0);
        needs_pair.requires = vec![("small".to_string(), 2)];
        let small_a = entry("small_a", 1, 2, &["small"], 1);
        let small_b = entry("small_b", 1, 2, &["small"], 2);
        let entries = vec![needs_pair, small_a.clone(), small_b.clone()];
        // Not satisfied: no small permanents.
        let pool = vote_pool(&entries, 1, &[], &[]);
        assert!(!pool.iter().any(|e| e.id == "needs_pair"));
        // Satisfied: two small permanents.
        let permanents = vec!["small_a".to_string(), "small_b".to_string()];
        let pool = vote_pool(&entries, 1, &[], &permanents);
        assert!(pool.iter().any(|e| e.id == "needs_pair"));
        // Class composition: an entry that refuses the lobby's class is never offered.
        let mut refuses = entry("refuses", 1, 1, &[], 3);
        refuses
            .class_interactions
            .insert("striker".to_string(), "ignores".to_string());
        let entries = vec![refuses];
        let pool = vote_pool(&entries, 1, &["striker"], &[]);
        assert!(pool.is_empty());
        // ...but a different class in the lobby leaves it in the pool.
        let pool = vote_pool(&entries, 1, &["bulwark"], &[]);
        assert_eq!(pool.len(), 1);
    }

    #[test]
    fn a_vote_win_is_permanent_across_levels() {
        let c = content();
        let mut run = Run::new(&c, 7, 2);
        // Cast a winning vote for a permanent mid-collapse.
        run.vote_session = Some(VoteSession::open(
            2,
            run.seed,
            vec![SessionOption {
                index: 0,
                id: "moteling".to_string(),
                name: "Stillness Shrieker".to_string(),
                kind: CARD_ENEMY,
                temporary: false,
            }],
            Some(1),
        ));
        if let Some(session) = run.vote_session.as_mut() {
            session.cast(1, 0).unwrap();
        }
        let outcome = run.close_vote(&c).unwrap();
        assert!(outcome.permanent);
        assert_eq!(run.permanents, vec!["moteling".to_string()]);
        // The current level is unaffected; permanents spawn from the next level on.
        let picks = valid_picks(&run);
        run.resolve(&c, &[picks]);
        let drafted = run.drafted_enemies.len();
        assert_eq!(run.enemies.len(), drafted + 1, "the permanent is on the field");
        assert_eq!(
            run.info().enemies.iter().filter(|id| id.as_str() == "moteling").count(),
            1
        );
        // And on every level after that.
        run.finish_level(&c);
        let picks = valid_picks(&run);
        run.resolve(&c, &[picks]);
        assert_eq!(run.enemies.len(), drafted + 1, "the permanent spawns again next level");
        // Run::new(&c, 7, 2) starts on level 2; finish_level is the only thing that advances it.
        assert_eq!(run.level_num, 3);
    }

    #[test]
    fn an_exhausted_pool_overflows_into_a_temporary_for_one_level_only() {
        let c = content();
        let mut run = Run::new(&c, 7, 2);
        // Drive every registry entry to its cap with the run's permanents.
        let mut permanents = Vec::new();
        for e in &c.enemies {
            for _ in 0..u32::from(e.selection_cap) {
                permanents.push(e.id.clone());
            }
        }
        let total_permanents = permanents.len();
        run.permanents = permanents;
        assert!(run.open_vote(&c, &["striker"], Some(1)), "the session still opens");
        let options: Vec<SessionOption> = run
            .vote_session
            .as_ref()
            .map(|s| s.options().to_vec())
            .unwrap_or_default();
        assert!(!options.is_empty());
        assert!(
            options.iter().any(|o| o.kind == CARD_ENEMY),
            "the overflow still offers at least one enemy"
        );
        assert!(
            options.iter().filter(|o| o.kind == CARD_ENEMY).all(|o| o.temporary),
            "every enemy option is a temporary summon when the pool is exhausted"
        );
        if let Some(session) = run.vote_session.as_mut() {
            session.cast(1, 0).unwrap();
        }
        let outcome = run.close_vote(&c).unwrap();
        assert!(!outcome.permanent);
        assert!(outcome.temporary);
        assert_eq!(run.permanents.len(), total_permanents, "no new permanent was added");
        assert_eq!(run.pending_temporary.len(), 1);
        // The temporary spawns on the next level and only that level. `resolve` also drafts, so the
        // drafted count is read after each resolve rather than before it.
        let picks = valid_picks(&run);
        run.resolve(&c, &[picks]);
        let drafted = run.drafted_enemies.len();
        assert_eq!(run.enemies.len(), drafted + total_permanents + 1);
        assert!(run.pending_temporary.is_empty(), "the temporary is consumed");
        run.finish_level(&c);
        let picks = valid_picks(&run);
        run.resolve(&c, &[picks]);
        let drafted = run.drafted_enemies.len();
        assert_eq!(
            run.enemies.len(),
            drafted + total_permanents,
            "the temporary does not return"
        );
    }

    #[test]
    fn the_countdown_closes_the_session() {
        let mut s = session(1, 99, None);
        for i in 0..VOTE_COUNTDOWN_TICKS - 1 {
            assert!(!s.tick(), "the session closed too early at tick {i}");
        }
        assert_eq!(s.seconds_left(), 1);
        assert!(s.tick(), "the session closes when the countdown elapses");
        assert_eq!(s.seconds_left(), 0);
        assert!(s.cast(1, 0).is_err(), "no ballots after the countdown closed");
    }

    #[test]
    fn ballots_are_keyed_by_player_id_not_connection() {
        let mut s = session(1, 99, Some(1));
        s.cast(7, 0).unwrap();
        // Re-voting replaces the ballot.
        s.cast(7, 2).unwrap();
        assert_eq!(s.counts(), vec![0, 0, 1]);
        assert!(s.has_voted(7));
        assert_eq!(s.voted_ids(), vec![7]);
    }
    // --- T-085: difficulty modes -------------------------------------------------------------

    #[test]
    fn extreme_offers_an_enemy_a_level_earlier_and_allows_an_extra_copy() {
        use sim::mode::DifficultyMode;
        let entries = vec![entry("late", 5, 1, &[], 0)];
        // STANDARD: min_level 5 is not offered at level 4.
        assert!(vote_pool_with(&entries, 4, &[], &[], DifficultyMode::Standard).is_empty());
        // EXTREME shifts min_level by -1, so it is offered at level 4.
        assert_eq!(
            vote_pool_with(&entries, 4, &[], &[], DifficultyMode::Extreme).len(),
            1,
            "EXTREME offers a min_level-5 enemy at level 4"
        );
        // EXTREME raises the cap by one: a cap-1 entry with one permanent stays in the pool.
        let held = vec!["late".to_string()];
        assert!(vote_pool_with(&entries, 5, &[], &held, DifficultyMode::Standard).is_empty());
        assert_eq!(
            vote_pool_with(&entries, 5, &[], &held, DifficultyMode::Extreme).len(),
            1,
            "EXTREME allows a second copy"
        );
    }

    #[test]
    fn chaos_opens_the_shop_at_level_one() {
        use sim::mode::DifficultyMode;
        let content = Content::load().unwrap();
        let mut run = Run::new(&content, 42, 1);
        assert!(run.make_shop(&content).is_none(), "STANDARD has no level-1 shop");
        run.mode = DifficultyMode::Chaos;
        assert!(run.make_shop(&content).is_some(), "CHAOS opens the shop at level 1");
    }

    #[test]
    fn random_never_opens_a_vote_and_still_adds_a_permanent() {
        use sim::mode::DifficultyMode;
        let content = Content::load().unwrap();
        let mut run = Run::new(&content, 42, 1);
        run.mode = DifficultyMode::Random;
        assert!(!run.open_vote(&content, &[], None), "RANDOM never opens a vote session");
        assert!(run.vote_session.is_none(), "no session is left behind");
        assert!(
            !run.permanents.is_empty() || !run.curses.is_empty(),
            "the server pick still lands as a permanent or a curse"
        );
        // Deterministic: the same seed picks the same thing.
        let mut again = Run::new(&content, 42, 1);
        again.mode = DifficultyMode::Random;
        again.open_vote(&content, &[], None);
        assert_eq!(run.permanents, again.permanents, "same seed, same pick");
        assert_eq!(run.curses, again.curses, "same seed, same pick");
    }

    #[test]
    fn the_mode_survives_a_level_transition() {
        use sim::mode::DifficultyMode;
        let content = Content::load().unwrap();
        let mut run = Run::new(&content, 42, 1);
        run.mode = DifficultyMode::Extreme;
        run.start_level(&content, 1);
        assert_eq!(run.mode, DifficultyMode::Extreme, "the mode survives start_level");
        run.start_level(&content, 1);
        assert_eq!(run.mode, DifficultyMode::Extreme, "and again on the next level");
    }
}
