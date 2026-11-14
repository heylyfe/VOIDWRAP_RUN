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

/// The filtered, deterministically ordered enemy pool (registry §9): `min_level` gate, `selection_cap`
/// against the run's permanents, `requires` hooks, class composition. Ordered by `min_level` ascending,
/// ties broken by content order. No RNG.
pub fn vote_pool(entries: &[PoolEntry], level: u32, classes: &[&str], permanents: &[String]) -> Vec<PoolEntry> {
    let tag_counts = permanent_tag_counts(entries, permanents);
    let small = tag_counts.get("small").copied().unwrap_or(0);
    let mut pool: Vec<PoolEntry> = entries
        .iter()
        .filter(|e| e.min_level <= level)
        .filter(|e| e.id != "chorus" || chorus_candidate_allowed(small, level))
        .filter(|e| {
            let held = permanents.iter().filter(|id| id.as_str() == e.id).count();
            held < e.selection_cap as usize
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

/// Registry §9 overflow: every entry is at its cap, so the vote offers TEMPORARY summons for the next
/// level. Candidates ignore `selection_cap` (temporaries never count) but still respect `min_level`, the
/// `requires` hooks and class composition; ordered by `min_level` descending, ties broken by content order.
pub fn overflow_pool(entries: &[PoolEntry], level: u32, classes: &[&str], permanents: &[String]) -> Vec<PoolEntry> {
    let tag_counts = permanent_tag_counts(entries, permanents);
    let small = tag_counts.get("small").copied().unwrap_or(0);
    let mut pool: Vec<PoolEntry> = entries
        .iter()
        .filter(|e| e.min_level <= level)
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
            upgrades: Vec::new(),
            curses: Vec::new(),
            purchased_mods: Vec::new(),
            mods: RunMods::default(),
            offer: None,
            hazards: Hazards::new(),
            altars_used: Vec::new(),
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
        let pool = vote_pool(&entries, level, classes, &permanents);
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
            options = overflow_pool(&entries, level, classes, &permanents)
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
        if self.level_num.is_multiple_of(GREATER_EVERY) {
            let greater: Vec<_> = content.curses.iter().filter(|c| c.greater).collect();
            let mut cards = Vec::with_capacity(3);
            for i in self.rng.sample(greater.len(), 3) {
                cards.push(DraftCard {
                    kind: CARD_CURSE,
                    id: greater[i].id.clone(),
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
        if self.level_num == 1 {
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
    /// item. Used when a curse is purged so the removal actually unloads its effect instead of just
    /// hiding it from the list. Multiplication and addition commute here, the life pool is not re-banked,
    /// and shop purchases applied via `purchased_mods` are preserved.
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
        self.phase = PHASE_PLAYING;
    }

    /// The beacon was reached: back to the lobby to draft for the next level.
    pub fn finish_level(&mut self, content: &Content) {
        self.level_num = self.level_num.saturating_add(1);
        self.level = levelgen::hub();
        self.world = World::from_level(&self.level);
        self.gifts_taken.clear();
        self.golden_taken.clear();
        self.enemies.clear();
        self.hazards.reset();
        self.altars_used.clear();
        self.level_phase = LEVEL_COLLECT;
        self.collapse_elapsed_ticks = 0;
        self.collapse.clear();
        self.gift_platform.clear();
        self.event_friction_mul = 1.0;
        self.phase = PHASE_ENEMY_DRAFT;
        self.offer = Some(self.make_offer(content));
        self.shop = self.make_shop(content);
    }

    pub fn end(&mut self) {
        self.phase = PHASE_OVER;
        self.offer = None;
        self.shop = None;
    }

    /// Use the first unused altar within 2.0 units horizontally and 1.5 vertically of `feet`:
    /// apply one random upgrade and one random non-greater curse (run RNG) and record both in the
    /// RunInfo lists. Returns the applied (upgrade id, curse id), or None when nothing was used.
    pub fn try_use_altar(&mut self, content: &Content, feet: &Vec3) -> Option<(String, String)> {
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
                hit = Some(altar.id);
                break;
            }
        }
        let altar_id = hit?;
        if content.upgrades.is_empty() {
            return None;
        }
        // One random upgrade...
        let up_i = self.rng.below(content.upgrades.len());
        let upgrade_id = content.upgrades[up_i].id.clone();
        let upgrade_mods = content.upgrades[up_i].mods.clone();
        // ...and one random non-greater curse.
        let normal_curses: Vec<&crate::content::ModCard> = content.curses.iter().filter(|c| !c.greater).collect();
        if normal_curses.is_empty() {
            return None;
        }
        let cur_i = self.rng.below(normal_curses.len());
        let curse_id = normal_curses[cur_i].id.clone();
        let curse_mods = normal_curses[cur_i].mods.clone();
        let lives_before = self.mods.extra_lives;
        self.mods.apply(&upgrade_mods);
        self.lives = self
            .lives
            .saturating_add(self.mods.extra_lives.saturating_sub(lives_before));
        let lives_before = self.mods.extra_lives;
        self.mods.apply(&curse_mods);
        self.lives = self
            .lives
            .saturating_add(self.mods.extra_lives.saturating_sub(lives_before));
        self.upgrades.push(upgrade_id.clone());
        self.curses.push(curse_id.clone());
        self.altars_used.push(altar_id);
        Some((upgrade_id, curse_id))
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

    /// Collect gifts the player at `feet` touches. In LEVEL_COLLECT the remaining normal gifts are gathered
    /// (the last one starts phase 2). From LEVEL_COLLAPSE_PENDING on, golden gifts sitting at the positions of
    /// the collected normal gifts are collected instead (1 gold each, same gift ids, `golden_taken` lists them).
    /// Returns the gift positions collected this call (the sim uses them for the pickup events).
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
                    picked.push(gift.pos);
                }
            }
            // The last gift (including the vacuous zero-gift case) starts phase 2.
            if self.normal_left() == 0 {
                self.enter_collapse_pending();
            }
            return picked;
        }
        // Phase 2: one golden gift per collected normal gift, at its exact position.
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
                self.golden_taken.push(gift.id);
                self.currency = self.currency.saturating_add(GOLDEN_VALUE);
                picked.push(gift.pos);
            }
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
            fog_density_mul: self.mods.fog_density_mul * escalation_fog_mul(self.level_num),
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
    fn altar_applies_upgrade_and_curse_and_is_one_shot() {
        let c = content();
        let mut run = Run::new(&c, 42, 2);
        let picks = valid_picks(&run);
        run.resolve(&c, &[picks]);
        assert_eq!(run.phase, PHASE_PLAYING);
        run.level.altars = vec![
            protocol::Altar {
                id: 9,
                pos: Vec3::new(1.0, 0.0, 1.0),
            },
            protocol::Altar {
                id: 10,
                pos: Vec3::new(0.0, 0.0, 0.0),
            },
        ];
        run.altars_used.clear();
        let upgrades_before = run.upgrades.len();
        let curses_before = run.curses.len();
        // Too far away: no use.
        let far = Vec3::new(50.0, 0.0, 50.0);
        assert_eq!(run.try_use_altar(&c, &far), None);
        // Within range (horizontal and vertical): first unused altar in level order is used.
        let near = Vec3::new(0.5, 0.5, 0.5);
        let (upgrade_id, curse_id) = run.try_use_altar(&c, &near).expect("altar should be usable");
        assert_eq!(run.altars_used, vec![9]);
        assert_eq!(run.upgrades.len(), upgrades_before + 1);
        assert_eq!(run.curses.len(), curses_before + 1);
        assert_eq!(run.upgrades.last().unwrap(), &upgrade_id);
        assert_eq!(run.curses.last().unwrap(), &curse_id);
        // Too high above the altar: no use.
        let high = Vec3::new(0.0, 1.6, 0.0);
        assert_eq!(run.try_use_altar(&c, &high), None);
        // The unused altar 10 is within range: used now.
        let low = Vec3::new(0.0, 1.4, 0.0);
        assert!(run.try_use_altar(&c, &low).is_some());
        assert_eq!(run.altars_used, vec![9, 10]);
        // Both used: no more altar uses this level.
        assert_eq!(run.try_use_altar(&c, &low), None);
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
}
