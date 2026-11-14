//! Pattern metadata, tag-aware selection and composition planning (T-069).
//!
//! A *pattern* is a chunk plus the closed-vocabulary tags described in
//! `design/PATTERN_FORMAT.md`. This module owns three things:
//!
//! * [`PatternMeta`] — the parsed metadata of one pattern.
//! * [`TagBag`] — a seeded, tag-aware selection bag.
//! * [`CompositionPlan`] — the ordered pattern ids a level will use, plus
//!   [`PlanCursor`], the queue the growth walk in [`crate::levelgen`] drains.
//!
//! Everything here is integer-only and seeded: no wall clock, no float
//! transcendentals, so the module is bit-exact on every target the sim crate is
//! compiled for (native and `wasm32-unknown-unknown`).

use crate::levelgen::ChunkRole;

/// Weight every candidate starts with before any bias is applied.
const BASE_WEIGHT: u32 = 256;

/// At most this many consecutive plan entries may share a theme tag.
const MAX_THEME_RUN: usize = 3;

/// Shortest composition plan, in patterns (including the hub).
pub const MIN_PLAN_LEN: usize = 10;

/// Longest composition plan, in patterns (including the hub).
pub const MAX_PLAN_LEN: usize = 48;

// ---------------------------------------------------------------------------
// Tag vocabularies
// ---------------------------------------------------------------------------

/// Visual and atmospheric identity of a pattern. Closed vocabulary.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ThemeTag {
    /// Fire, ash, warm orange light, scorched stone.
    Ember,
    /// Starless emptiness, black sky, floating debris.
    Void,
    /// Ice, snow, cold blue light, brittle surfaces.
    Frost,
    /// Broken masonry, collapsed halls, rubble.
    Ruin,
    /// Overgrown vegetation, water, soft green light.
    Garden,
    /// Gears, pistons, hot metal, pipes.
    Machine,
    /// Verticality, thin bridges, height, wind.
    Spire,
    /// Caverns, echoes, darkness below.
    Hollow,
}

impl ThemeTag {
    /// Every allowed theme, in canonical order.
    pub const ALL: [ThemeTag; 8] = [
        ThemeTag::Ember,
        ThemeTag::Void,
        ThemeTag::Frost,
        ThemeTag::Ruin,
        ThemeTag::Garden,
        ThemeTag::Machine,
        ThemeTag::Spire,
        ThemeTag::Hollow,
    ];

    /// Number of allowed themes.
    pub const COUNT: usize = 8;

    /// The wire name used after the `theme:` prefix in `tags`.
    pub fn as_str(self) -> &'static str {
        match self {
            ThemeTag::Ember => "ember",
            ThemeTag::Void => "void",
            ThemeTag::Frost => "frost",
            ThemeTag::Ruin => "ruin",
            ThemeTag::Garden => "garden",
            ThemeTag::Machine => "machine",
            ThemeTag::Spire => "spire",
            ThemeTag::Hollow => "hollow",
        }
    }

    /// Parses a wire name. Returns `None` for anything outside the vocabulary.
    pub fn parse(name: &str) -> Option<ThemeTag> {
        ThemeTag::ALL.iter().copied().find(|t| t.as_str() == name)
    }

    /// Dense index in `0..COUNT`, used for the per-level theme counters.
    pub fn index(self) -> usize {
        match self {
            ThemeTag::Ember => 0,
            ThemeTag::Void => 1,
            ThemeTag::Frost => 2,
            ThemeTag::Ruin => 3,
            ThemeTag::Garden => 4,
            ThemeTag::Machine => 5,
            ThemeTag::Spire => 6,
            ThemeTag::Hollow => 7,
        }
    }

    /// Inverse of [`ThemeTag::index`]; wraps out-of-range indices.
    pub fn from_index(index: usize) -> ThemeTag {
        ThemeTag::ALL[index % ThemeTag::COUNT]
    }
}

/// What the player does in a pattern. Closed vocabulary.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MechanicTag {
    /// Net vertical gain.
    Climb,
    /// Net vertical loss.
    Drop,
    /// A gap crossed by a jump or a short bridge.
    Span,
    /// Relies on a jump pad or a bounce surface.
    Bounce,
    /// Low headroom, tight corridors.
    Crawl,
    /// Sustained pressure, no safe footing.
    Gauntlet,
    /// Safe footing, a place to breathe.
    Rest,
    /// Dense gift slots.
    Loot,
    /// A real choice of route; junction role only.
    Fork,
    /// Terminates a branch; dead_end role only.
    DeadEnd,
}

impl MechanicTag {
    /// Every allowed mechanic, in canonical order.
    pub const ALL: [MechanicTag; 10] = [
        MechanicTag::Climb,
        MechanicTag::Drop,
        MechanicTag::Span,
        MechanicTag::Bounce,
        MechanicTag::Crawl,
        MechanicTag::Gauntlet,
        MechanicTag::Rest,
        MechanicTag::Loot,
        MechanicTag::Fork,
        MechanicTag::DeadEnd,
    ];

    /// Number of allowed mechanics.
    pub const COUNT: usize = 10;

    /// The wire name used after the `mech:` prefix in `tags`.
    pub fn as_str(self) -> &'static str {
        match self {
            MechanicTag::Climb => "climb",
            MechanicTag::Drop => "drop",
            MechanicTag::Span => "span",
            MechanicTag::Bounce => "bounce",
            MechanicTag::Crawl => "crawl",
            MechanicTag::Gauntlet => "gauntlet",
            MechanicTag::Rest => "rest",
            MechanicTag::Loot => "loot",
            MechanicTag::Fork => "fork",
            MechanicTag::DeadEnd => "dead_end",
        }
    }

    /// Parses a wire name. Returns `None` for anything outside the vocabulary.
    pub fn parse(name: &str) -> Option<MechanicTag> {
        MechanicTag::ALL.iter().copied().find(|t| t.as_str() == name)
    }
}

/// Hazard a pattern reserves for a later wave. Closed vocabulary.
///
/// Patterns may declare hazards; nothing consumes them yet.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum HazardTag {
    /// A bounce surface.
    Trampoline,
    /// A triggered trap.
    Tripmine,
    /// A vision-limiting volume.
    Fog,
    /// A surface that collapses.
    Crumble,
}

impl HazardTag {
    /// Every allowed hazard, in canonical order.
    pub const ALL: [HazardTag; 4] = [
        HazardTag::Trampoline,
        HazardTag::Tripmine,
        HazardTag::Fog,
        HazardTag::Crumble,
    ];

    /// Number of allowed hazards.
    pub const COUNT: usize = 4;

    /// The wire name used after the `hazard:` prefix in `tags`.
    pub fn as_str(self) -> &'static str {
        match self {
            HazardTag::Trampoline => "trampoline",
            HazardTag::Tripmine => "tripmine",
            HazardTag::Fog => "fog",
            HazardTag::Crumble => "crumble",
        }
    }

    /// Parses a wire name. Returns `None` for anything outside the vocabulary.
    pub fn parse(name: &str) -> Option<HazardTag> {
        HazardTag::ALL.iter().copied().find(|t| t.as_str() == name)
    }
}

// ---------------------------------------------------------------------------
// PatternMeta
// ---------------------------------------------------------------------------

/// The parsed metadata of one pattern.
///
/// Built from a catalog entry's `id`, `role`, `min_level`, `difficulty` and `tags`
/// by [`PatternMeta::from_tags`]. The geometry (`platforms`, `sockets`, slots,
/// `beacon`) stays in the catalog and is not duplicated here.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PatternMeta {
    /// Catalog id, unique across the merged catalog.
    pub id: String,
    /// Structural role, shared with the chunk format.
    pub role: ChunkRole,
    /// First level this pattern may appear on.
    pub min_level: u32,
    /// Difficulty tier, `1..=3`.
    pub difficulty: u8,
    /// Theme tags, at least one after parsing.
    pub theme: Vec<ThemeTag>,
    /// Mechanic tags, possibly empty for legacy content.
    pub mechanic: Vec<MechanicTag>,
    /// Hazard tags, reserved for later waves.
    pub hazard: Vec<HazardTag>,
    /// Whether the catalog entry declared at least one explicit pattern tag.
    pub tagged: bool,
}

impl PatternMeta {
    /// Parses the pattern metadata out of a catalog entry's raw tag list.
    ///
    /// Recognised prefixes are `theme:`, `mech:` and `hazard:`; every other tag is
    /// a legacy free-form tag and is ignored. Unknown values after a known prefix
    /// are ignored too, so a newer catalog still loads on an older build.
    ///
    /// A pattern with no `theme:` tag is given a stable default theme derived from
    /// its id, so the theme mixer always has something to work with. Authors must
    /// still declare explicit themes; the default only exists so the 25 legacy
    /// chunks stay valid patterns before T-070/T-071 re-tag them.
    pub fn from_tags(id: &str, role: ChunkRole, min_level: u32, difficulty: u8, tags: &[String]) -> PatternMeta {
        let mut theme: Vec<ThemeTag> = Vec::new();
        let mut mechanic: Vec<MechanicTag> = Vec::new();
        let mut hazard: Vec<HazardTag> = Vec::new();
        for raw in tags {
            let tag = raw.trim();
            if let Some(rest) = tag.strip_prefix("theme:")
                && let Some(value) = ThemeTag::parse(rest)
                && !theme.contains(&value)
            {
                theme.push(value);
            } else if let Some(rest) = tag.strip_prefix("mech:")
                && let Some(value) = MechanicTag::parse(rest)
                && !mechanic.contains(&value)
            {
                mechanic.push(value);
            } else if let Some(rest) = tag.strip_prefix("hazard:")
                && let Some(value) = HazardTag::parse(rest)
                && !hazard.contains(&value)
            {
                hazard.push(value);
            }
        }
        let tagged = !theme.is_empty() || !mechanic.is_empty() || !hazard.is_empty();
        if theme.is_empty() {
            theme.push(ThemeTag::from_index(default_theme_index(id)));
        }
        PatternMeta {
            id: id.to_string(),
            role,
            min_level,
            difficulty,
            theme,
            mechanic,
            hazard,
            tagged,
        }
    }

    /// Whether this pattern declared at least one explicit `theme:`/`mech:`/`hazard:` tag.
    pub fn is_tagged(&self) -> bool {
        self.tagged
    }

    /// Whether the pattern carries `tag`.
    pub fn has_mechanic(&self, tag: MechanicTag) -> bool {
        self.mechanic.contains(&tag)
    }

    /// Whether the pattern carries `tag`.
    pub fn has_theme(&self, tag: ThemeTag) -> bool {
        self.theme.contains(&tag)
    }

    /// Whether the pattern carries `tag`.
    pub fn has_hazard(&self, tag: HazardTag) -> bool {
        self.hazard.contains(&tag)
    }
}

/// Stable default theme for a pattern that declares none: FNV-1a over the id.
fn default_theme_index(id: &str) -> usize {
    let mut hash: u32 = 0x811C_9DC5;
    for byte in id.as_bytes() {
        hash ^= u32::from(*byte);
        hash = hash.wrapping_mul(0x0100_0193);
    }
    (hash as usize) % ThemeTag::COUNT
}

// ---------------------------------------------------------------------------
// Deterministic PRNG
// ---------------------------------------------------------------------------

/// SplitMix32: integer-only, seeded, identical on every target.
struct SplitMix32 {
    state: u32,
}

impl SplitMix32 {
    fn new(seed: u32) -> SplitMix32 {
        SplitMix32 { state: seed }
    }

    fn next_u32(&mut self) -> u32 {
        self.state = self.state.wrapping_add(0x9E37_79B9);
        let mut z = self.state;
        z = (z ^ (z >> 16)).wrapping_mul(0x21F0_AAAD);
        z = (z ^ (z >> 15)).wrapping_mul(0x735A_2D97);
        z ^ (z >> 15)
    }

    /// Uniform value in `0..n`, by rejection so the distribution stays exact.
    fn below(&mut self, n: u32) -> u32 {
        if n <= 1 {
            return 0;
        }
        let zone = u32::MAX - (u32::MAX % n);
        loop {
            let value = self.next_u32();
            if value < zone {
                return value % n;
            }
        }
    }
}

/// Mixes `(seed, level, players)` into the bag seed.
fn mix_seed(seed: u32, level: u32, players: u32) -> u32 {
    let mut hash = seed ^ 0x9E37_79B9;
    hash = hash.wrapping_mul(0x85EB_CA6B) ^ level.wrapping_mul(0xC2B2_AE35);
    hash = hash.wrapping_mul(0x27D4_EB2F) ^ players.wrapping_mul(0x1656_67B1);
    hash ^ (hash >> 15)
}

// ---------------------------------------------------------------------------
// TagBag
// ---------------------------------------------------------------------------

/// Tuning for [`TagBag`]. All fields are integer so the bag stays bit-exact.
#[derive(Clone, Copy, Debug)]
pub struct TagBagConfig {
    /// How many recent picks count as "recent". Default 6.
    pub history: usize,
    /// Divisor applied to the weight of a recently picked pattern. Default 32.
    pub recent_divisor: u32,
    /// Weight added per missing use of a theme, up to `theme_target`. Default 96.
    pub theme_bonus: u32,
    /// Uses of a theme after which it stops earning a bonus. Default 2.
    pub theme_target: u32,
    /// Weight removed per difficulty step above the level tier. Default 24.
    pub difficulty_penalty: u32,
}

impl Default for TagBagConfig {
    fn default() -> TagBagConfig {
        TagBagConfig {
            history: 6,
            recent_divisor: 32,
            theme_bonus: 96,
            theme_target: 2,
            difficulty_penalty: 24,
        }
    }
}

/// A seeded, tag-aware selection bag.
///
/// The bag never returns the same pattern twice in a row, strongly discourages
/// patterns seen in the last `history` picks, and biases toward themes that are
/// under-represented so far in the level. It is fully deterministic from its seed.
pub struct TagBag {
    rng: SplitMix32,
    cfg: TagBagConfig,
    tier: u32,
    themes: Vec<Vec<ThemeTag>>,
    difficulty: Vec<u8>,
    counts: [u32; ThemeTag::COUNT],
    history: Vec<usize>,
    last: Option<usize>,
    picks: u32,
}

impl TagBag {
    /// Builds a bag over `metas`, seeded by `seed` and biased against patterns
    /// whose difficulty exceeds `tier`.
    pub fn new(seed: u32, tier: u32, metas: &[PatternMeta], cfg: TagBagConfig) -> TagBag {
        TagBag {
            rng: SplitMix32::new(seed),
            cfg,
            tier,
            themes: metas.iter().map(|m| m.theme.clone()).collect(),
            difficulty: metas.iter().map(|m| m.difficulty).collect(),
            counts: [0; ThemeTag::COUNT],
            history: Vec::new(),
            last: None,
            picks: 0,
        }
    }

    /// Draws the next pattern index out of `eligible`.
    ///
    /// `eligible` holds catalog indices the caller has already filtered by role,
    /// level and budget. Returns `None` only when `eligible` is empty or holds no
    /// index this bag knows about.
    pub fn pick(&mut self, eligible: &[usize]) -> Option<usize> {
        if eligible.is_empty() {
            return None;
        }
        let mut candidates: Vec<usize> = Vec::with_capacity(eligible.len());
        for &index in eligible {
            if index >= self.themes.len() {
                continue;
            }
            if Some(index) == self.last && eligible.len() > 1 {
                continue;
            }
            candidates.push(index);
        }
        if candidates.is_empty() {
            // Only reachable when the eligible set is a single pattern.
            for &index in eligible {
                if index < self.themes.len() {
                    candidates.push(index);
                }
            }
        }
        if candidates.is_empty() {
            return None;
        }
        let mut total: u32 = 0;
        let mut weights: Vec<u32> = Vec::with_capacity(candidates.len());
        for &index in &candidates {
            let weight = self.weight(index);
            weights.push(weight);
            total = total.saturating_add(weight);
        }
        if total == 0 {
            return None;
        }
        let mut roll = self.rng.below(total);
        let mut chosen = *candidates.last()?;
        for (slot, &weight) in weights.iter().enumerate() {
            if roll < weight {
                chosen = candidates[slot];
                break;
            }
            roll -= weight;
        }
        self.record(chosen);
        Some(chosen)
    }

    /// The last `history` picks, oldest first.
    pub fn history(&self) -> &[usize] {
        &self.history
    }

    /// How many times each theme has been picked so far, indexed by
    /// [`ThemeTag::index`].
    pub fn theme_counts(&self) -> &[u32; ThemeTag::COUNT] {
        &self.counts
    }

    /// How many picks the bag has made.
    pub fn picks(&self) -> u32 {
        self.picks
    }

    /// Integer weight of `index` under the current history and theme counts.
    fn weight(&self, index: usize) -> u32 {
        let mut weight: u32 = BASE_WEIGHT;
        for theme in &self.themes[index] {
            let used = self.counts[theme.index()];
            let missing = self.cfg.theme_target.saturating_sub(used);
            weight = weight.saturating_add(self.cfg.theme_bonus.saturating_mul(missing));
        }
        let over = u32::from(self.difficulty[index]).saturating_sub(self.tier);
        weight = weight.saturating_sub(self.cfg.difficulty_penalty.saturating_mul(over));
        if self.history.contains(&index) {
            weight /= self.cfg.recent_divisor.max(1);
        }
        weight.max(1)
    }

    /// Records a pick: history, theme counters and the last-pick guard.
    fn record(&mut self, index: usize) {
        self.last = Some(index);
        self.history.push(index);
        while self.history.len() > self.cfg.history {
            self.history.remove(0);
        }
        let counts = &mut self.counts;
        for theme in &self.themes[index] {
            let slot = &mut counts[theme.index()];
            *slot = slot.saturating_add(1);
        }
        self.picks = self.picks.saturating_add(1);
    }
}

// ---------------------------------------------------------------------------
// CompositionPlan
// ---------------------------------------------------------------------------

/// The ordered list of pattern ids a level will use.
///
/// Produced from `(seed, level, players)` before any geometry is placed. Entry 0
/// is always the hub; the remaining entries are the `path`/`junction` patterns the
/// growth walk tries, in order. `indices` are catalog indices, index-aligned with
/// `ids`, so the walk can look the geometry up without a string search.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompositionPlan {
    seed: u32,
    level: u32,
    players: u32,
    ids: Vec<String>,
    indices: Vec<usize>,
}

impl CompositionPlan {
    /// An empty plan, used when the catalog carries no pattern metadata.
    pub fn empty(seed: u32, level: u32, players: u32) -> CompositionPlan {
        CompositionPlan {
            seed,
            level,
            players,
            ids: Vec::new(),
            indices: Vec::new(),
        }
    }

    /// Builds the plan for `(seed, level, players)` with the default plan length.
    pub fn build(metas: &[PatternMeta], seed: u32, level: u32, players: u32) -> CompositionPlan {
        CompositionPlan::build_with_len(metas, seed, level, players, plan_len(level), tier_for(level))
    }

    /// Builds a plan of at most `len` entries, biased against difficulty above `tier`.
    ///
    /// The first entry is a hub with `min_level <= level`; the rest are
    /// `path`/`junction` patterns that satisfy the composition constraints in
    /// `design/PATTERN_FORMAT.md` §6. Returns an empty plan when no hub is eligible.
    pub fn build_with_len(
        metas: &[PatternMeta],
        seed: u32,
        level: u32,
        players: u32,
        len: usize,
        tier: u32,
    ) -> CompositionPlan {
        let mut plan = CompositionPlan::empty(seed, level, players);
        if metas.is_empty() || len == 0 {
            return plan;
        }
        let mut bag = TagBag::new(mix_seed(seed, level, players), tier, metas, TagBagConfig::default());
        let hubs: Vec<usize> = (0..metas.len())
            .filter(|&i| metas[i].role == ChunkRole::Hub && metas[i].min_level <= level)
            .collect();
        let Some(hub) = bag.pick(&hubs) else {
            return plan;
        };
        plan.push(hub, metas);
        let walk: Vec<usize> = (0..metas.len())
            .filter(|&i| matches!(metas[i].role, ChunkRole::Path | ChunkRole::Junction) && metas[i].min_level <= level)
            .collect();
        if walk.is_empty() {
            return plan;
        }
        while plan.len() < len {
            let previous = plan.indices.last().copied();
            let mut pool: Vec<usize> = walk
                .iter()
                .copied()
                .filter(|&i| {
                    allowed_after(previous.map(|p| &metas[p]), &metas[i])
                        && theme_run(&plan.indices, metas, i) < MAX_THEME_RUN
                })
                .collect();
            if pool.len() == 1 && Some(pool[0]) == previous {
                pool = walk.clone();
            }
            if pool.is_empty() {
                pool = walk.clone();
            }
            let Some(index) = bag.pick(&pool) else {
                break;
            };
            plan.push(index, metas);
        }
        plan
    }

    /// The seed this plan was built from.
    pub fn seed(&self) -> u32 {
        self.seed
    }

    /// The level this plan was built for.
    pub fn level(&self) -> u32 {
        self.level
    }

    /// The player count this plan was built for.
    pub fn players(&self) -> u32 {
        self.players
    }

    /// The ordered pattern ids.
    pub fn ids(&self) -> &[String] {
        &self.ids
    }

    /// The ordered catalog indices, index-aligned with [`CompositionPlan::ids`].
    pub fn indices(&self) -> &[usize] {
        &self.indices
    }

    /// Number of entries in the plan.
    pub fn len(&self) -> usize {
        self.ids.len()
    }

    /// Whether the plan has no entries.
    pub fn is_empty(&self) -> bool {
        self.ids.is_empty()
    }

    /// The catalog index of the hub, if the plan has one.
    pub fn hub_index(&self) -> Option<usize> {
        self.indices.first().copied()
    }

    /// A cursor over this plan, starting at entry 0.
    pub fn cursor(&self) -> PlanCursor<'_> {
        PlanCursor::new(self)
    }

    fn push(&mut self, index: usize, metas: &[PatternMeta]) {
        self.ids.push(metas[index].id.clone());
        self.indices.push(index);
    }
}

/// A forward-only cursor over a [`CompositionPlan`].
///
/// The growth walk asks for the next plan entry that satisfies its own placement
/// predicate; entries that do not fit are consumed and skipped, never revisited.
pub struct PlanCursor<'a> {
    plan: &'a CompositionPlan,
    position: usize,
}

impl<'a> PlanCursor<'a> {
    /// A cursor positioned at entry 0.
    pub fn new(plan: &'a CompositionPlan) -> Self {
        PlanCursor { plan, position: 0 }
    }

    /// Advances the cursor by `count` entries without yielding them.
    pub fn skip(&mut self, count: usize) {
        self.position = (self.position + count).min(self.plan.indices.len());
    }

    /// Returns the next entry accepted by `predicate`, consuming everything before it.
    pub fn next_matching(&mut self, mut predicate: impl FnMut(usize) -> bool) -> Option<usize> {
        while self.position < self.plan.indices.len() {
            let index = self.plan.indices[self.position];
            self.position += 1;
            if predicate(index) {
                return Some(index);
            }
        }
        None
    }

    /// How many entries are left, accepted or not.
    pub fn remaining(&self) -> usize {
        self.plan.indices.len().saturating_sub(self.position)
    }

    /// The current cursor position.
    pub fn position(&self) -> usize {
        self.position
    }
}

// ---------------------------------------------------------------------------
// Composition rules
// ---------------------------------------------------------------------------

/// Whether `candidate` may directly follow `previous` under the tag-interaction
/// rules of `design/PATTERN_FORMAT.md` §6.3.
fn allowed_after(previous: Option<&PatternMeta>, candidate: &PatternMeta) -> bool {
    let Some(previous) = previous else {
        return true;
    };
    if previous.has_mechanic(MechanicTag::Rest) && candidate.has_mechanic(MechanicTag::Rest) {
        return false;
    }
    if previous.has_mechanic(MechanicTag::Gauntlet) && candidate.has_mechanic(MechanicTag::Gauntlet) {
        return false;
    }
    if candidate.has_mechanic(MechanicTag::Fork) && candidate.role != ChunkRole::Junction {
        return false;
    }
    if candidate.has_mechanic(MechanicTag::DeadEnd) && candidate.role != ChunkRole::DeadEnd {
        return false;
    }
    true
}

/// How many trailing entries of `chosen` share a theme with `candidate`.
fn theme_run(chosen: &[usize], metas: &[PatternMeta], candidate: usize) -> usize {
    let mut run = 0usize;
    for &index in chosen.iter().rev() {
        let shares = metas[index]
            .theme
            .iter()
            .any(|theme| metas[candidate].theme.contains(theme));
        if !shares {
            break;
        }
        run += 1;
    }
    run
}

/// Default plan length for `level`: `10 + level`, clamped to `[10, 48]`.
pub fn plan_len(level: u32) -> usize {
    (MIN_PLAN_LEN + level as usize).min(MAX_PLAN_LEN)
}

/// The plan's own difficulty tier estimate for `level`, `1..=3`.
///
/// [`crate::levelgen`] passes its authoritative `level_tier` when it builds the
/// real plan; this helper only serves callers that have nothing but a level.
pub fn tier_for(level: u32) -> u32 {
    (1 + level.saturating_sub(1) / 4).min(3)
}
