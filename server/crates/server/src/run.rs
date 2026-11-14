//! Run state and rules: lobby drafts, generated levels, shared gift progress, golden-gift currency, lives,
//! the enemies the team drafted, the lobby shop, level events and altars. Pure game logic on top of `sim` —
//! no I/O, fully unit-testable.
//!
//! Flow: Lobby (spawn hub + draft offer and, from level 2 on, the shop) → all players vote → generate a level →
//! Playing (two phases: LEVEL_COLLECT until every gift is gathered, then LEVEL_COLLAPSE_PENDING — the siren — and
//! LEVEL_COLLAPSING; golden gifts exist at the collected positions, the beacon opens, the non-hub platforms fall
//! in six farthest-first batches) → beacon → Lobby for the next level. The spawn hub is exempt from collapse.
//! Death with no lives left ends that player's run; when everyone is down the run is Over and Restart begins a new one.

use crate::content::{Content, RunMods};
use protocol::{
    Aabb, CARD_CURSE, CARD_ENEMY, CARD_UPGRADE, DraftCard, EVENT_ICE, EnemyView, LEVEL_COLLAPSE_PENDING,
    LEVEL_COLLAPSING, LEVEL_COLLECT, LevelData, PHASE_ENEMY_DRAFT, PHASE_OVER, PHASE_PLAYING, Platform, RunInfo,
    TICK_HZ, TILE_GONE, TILE_WARN, TileState, Tuning, Vec3,
};
use sim::hazards::Hazards;
use sim::levelgen;
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
/// Ticks of siren/banner before the collapse proper begins (design §1.4: 3.0 s).
const COLLAPSE_PENDING_TICKS: u32 = 3 * TICK_HZ;

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

/// Small deterministic PRNG (SplitMix32) for draft offers, shop offers, altar rolls and level seeds. Server-only.
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
    /// Id of the item in design/content.json.
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
    /// Applied a mods item like an upgrade (not recorded in RunInfo lists).
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
    pub upgrades: Vec<String>,
    pub curses: Vec<String>,
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
    /// The single most-voted drafted enemy of the last draft (escalation \"Second Step\", level 4+).
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
            upgrades: Vec::new(),
            curses: Vec::new(),
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

    /// Pick the lobby shop: up to SHOP_OFFER distinct items from content, stock from content, run RNG.
    fn make_shop(&mut self, content: &Content) -> Option<Vec<ShopOffer>> {
        if self.level_num == 1 || content.shop.is_empty() {
            return None;
        }
        let picks = self.rng.sample(content.shop.len(), SHOP_OFFER.min(content.shop.len()));
        Some(
            picks
                .into_iter()
                .map(|i| {
                    let def = &content.shop[i];
                    ShopOffer {
                        id: def.id.clone(),
                        cost: def.cost,
                        stock: def.stock,
                    }
                })
                .collect(),
        )
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

    /// Buy an item from the current lobby shop (team-wide currency). Nothing is charged when the purchase fails.
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
        let Some(def) = content.shop_item(item) else {
            return Err(ShopError::UnknownItem);
        };
        // Apply the effect.
        let purchase = match def.kind.as_str() {
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
            "mods" => {
                let before = self.mods.extra_lives;
                self.mods.apply(&def.mods);
                self.lives = self.lives.saturating_add(self.mods.extra_lives.saturating_sub(before));
                ShopPurchase::Mods
            }
            _ => return Err(ShopError::UnknownItem),
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
        let generated = levelgen::generate_for(seed, self.level_num, players);
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
        // Roster: drafted enemies + curse copies + escalation copies (design §5, from level 4 on).
        let mut roster: Vec<String> = self.drafted_enemies.clone();
        let drafted = self.drafted_enemies.len();
        if drafted > 0 {
            for k in 0..self.mods.extra_enemies as usize {
                roster.push(self.drafted_enemies[k % drafted].clone());
            }
        }
        roster.extend(escalation_extra_copies(
            self.level_num,
            &self.drafted_enemies,
            self.most_voted_enemy.as_deref(),
        ));
        // Level 1 stays plain (design A4): no enemies at all.
        if self.level_num == 1 {
            roster.clear();
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
            enemies: self.drafted_enemies.clone(),
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
        assert_eq!(shop.len(), 4);
        let ids: Vec<&str> = shop.iter().map(|item| item.id.as_str()).collect();
        let unique: std::collections::BTreeSet<&str> = ids.iter().copied().collect();
        assert_eq!(unique.len(), ids.len(), "offer has distinct items");
        assert!(shop.iter().all(|item| item.stock > 0 && item.cost > 0));
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
}

#[cfg(test)]
mod m3_tests {
    use super::*;
    use protocol::{EVENT_NONE, TILE_GONE, TILE_WARN};

    fn feet_above(g: &protocol::Gift) -> Vec3 {
        Vec3::new(g.pos.x, g.pos.y - 0.9, g.pos.z)
    }

    #[test]
    fn goldens_are_phase_two_only_worth_one_and_at_collected_positions() {
        let c = Content::load().unwrap();
        let mut run = Run::new(&c, 11, 2);
        run.resolve(&c, &[valid_picks(&run)]);
        assert_eq!(run.level_phase, LEVEL_COLLECT);
        assert!(run.level.gifts.iter().all(|g| g.kind == protocol::GIFT_NORMAL));
        for g in run.level.gifts.clone() {
            assert_eq!(run.collect_gifts(&feet_above(&g)).len(), 1);
            assert_eq!(run.currency, 0, "goldens do not exist during LEVEL_COLLECT");
        }
        assert_eq!(run.level_phase, LEVEL_COLLAPSE_PENDING, "last gift starts phase 2");
        let collected = run.level.gifts.len();
        for g in run.level.gifts.clone() {
            let before = run.currency;
            let picked = run.collect_gifts(&feet_above(&g));
            assert_eq!(picked.len(), 1, "golden {} at the collected position", g.id);
            assert_eq!(run.currency, before + 1, "goldens are worth 1");
        }
        assert_eq!(run.golden_taken.len(), collected);
        // Re-touching a collected position collects nothing twice.
        for g in run.level.gifts.clone() {
            assert!(run.collect_gifts(&feet_above(&g)).is_empty());
        }
    }

    #[test]
    fn golden_vanishes_once_its_platform_falls() {
        let c = Content::load().unwrap();
        let mut run = Run::new(&c, 13, 2);
        run.resolve(&c, &[valid_picks(&run)]);
        for g in run.level.gifts.clone() {
            run.collect_gifts(&feet_above(&g));
        }
        // A gift that rests on a platform which is part of the collapse plan.
        let target = run
            .level
            .gifts
            .iter()
            .find(|g| {
                run.gift_platform
                    .get(&g.id)
                    .is_some_and(|pid| run.collapse.iter().any(|e| e.id == *pid))
            })
            .cloned()
            .expect("some gift rests on a collapsible platform");
        let pid = run.gift_platform[&target.id];
        for entry in &mut run.collapse {
            if entry.id == pid {
                entry.state = CollapseTile::Gone;
            }
        }
        let picked = run.collect_gifts(&feet_above(&target));
        assert!(picked.is_empty(), "a golden on a fallen tile is gone forever");
        assert!(!run.golden_taken.contains(&target.id));
    }

    #[test]
    fn beacon_is_closed_during_collect_and_opens_from_pending() {
        let c = Content::load().unwrap();
        let mut run = Run::new(&c, 17, 2);
        run.resolve(&c, &[valid_picks(&run)]);
        assert!(!run.beacon_reached(&run.level.beacon.clone()));
        for g in run.level.gifts.clone() {
            run.collect_gifts(&feet_above(&g));
        }
        assert_eq!(run.level_phase, LEVEL_COLLAPSE_PENDING);
        assert!(run.beacon_reached(&run.level.beacon.clone()));
    }

    #[test]
    fn collapse_plan_is_farthest_first_excludes_the_hub_and_breaks_ties_by_id() {
        let mk = |id: u16, cx: f32| Platform {
            id,
            kind: protocol::PLATFORM_NORMAL,
            bounds: protocol::Aabb {
                min: Vec3::new(cx - 1.5, -0.5, -1.5),
                max: Vec3::new(cx + 1.5, 0.0, 1.5),
            },
        };
        let platforms = vec![
            mk(0, 0.0),   // spawn hub (contains beacon and spawn, top 0)
            mk(1, 8.0),   // near
            mk(2, 19.0),  // tie, lower id
            mk(3, -19.0), // tie, higher id
            mk(4, 41.0),  // far
        ];
        let beacon = Vec3::new(0.0, 0.0, 0.0);
        let spawn = Vec3::new(0.0, 0.0, 0.0);
        let plan = build_collapse_plan(&platforms, &beacon, &spawn, 1);
        let ids: Vec<u16> = plan.iter().map(|e| e.id).collect();
        assert_eq!(
            ids,
            vec![4, 2, 3, 1],
            "farthest first; ties by platform id ascending; hub exempted"
        );
        let warns: Vec<u32> = plan.iter().map(|e| e.warn_start_tick).collect();
        for w in warns.windows(2) {
            assert!(w[0] <= w[1], "batch warnings are issued in order");
        }
        assert_eq!(plan[0].warn_start_tick, 0, "the farthest batch starts warning at once");
    }

    #[test]
    fn warn_then_gone_flip_and_collapse_ms_counts_to_zero() {
        let c = Content::load().unwrap();
        let mut run = Run::new(&c, 5, 1);
        let mk = |id: u16, cx: f32| Platform {
            id,
            kind: protocol::PLATFORM_NORMAL,
            bounds: protocol::Aabb {
                min: Vec3::new(cx - 1.5, -0.5, -1.5),
                max: Vec3::new(cx + 1.5, 0.0, 1.5),
            },
        };
        run.level = LevelData {
            level: 1,
            seed: 1,
            spawn: Vec3::new(0.0, 0.0, 0.0),
            beacon: Vec3::new(0.0, 0.0, 0.0),
            kill_y: -50.0,
            platforms: vec![mk(0, 0.0), mk(1, 30.0)],
            gifts: Vec::new(),
            event: EVENT_NONE,
            altars: Vec::new(),
        };
        run.world = World::from_level(&run.level);
        run.enemies.clear();
        // Level 2 warning time (3.0 s), one collapsing platform.
        run.collapse = build_collapse_plan(&run.level.platforms, &run.level.beacon, &run.level.spawn, 2);
        run.gift_platform.clear();
        run.phase = PHASE_PLAYING;
        // A level with no gifts completes phase 1 vacuously on the first collection pass.
        run.collect_gifts(&Vec3::new(0.0, 0.0, 0.0));
        assert_eq!(run.level_phase, LEVEL_COLLAPSE_PENDING);
        run.tick_collapse();
        assert!(run.world_tiles().iter().any(|t| t.id == 1 && t.state == TILE_WARN));
        for _ in 0..60 {
            run.tick_collapse();
        }
        assert!(run.world_tiles().iter().any(|t| t.id == 1 && t.state == TILE_GONE));
        assert_eq!(run.level_phase, LEVEL_COLLAPSING, "the siren window is 3.0 s");
        assert_eq!(run.collapse_ms(), 0);
    }

    #[test]
    fn collapse_duration_matches_the_design_table() {
        assert_eq!(collapse_duration_seconds(1), 52.0);
        assert_eq!(collapse_duration_seconds(6), 42.0);
        assert_eq!(collapse_duration_seconds(11), 32.0);
        assert_eq!(
            collapse_duration_seconds(17),
            30.0,
            "Hunger is absorbed by the 30 s floor"
        );
        assert_eq!(collapse_duration_seconds(25), 30.0);
        assert_eq!(collapse_duration_seconds(30), 25.0, "level 30: floor 25");
        assert_eq!(collapse_duration_seconds(40), 25.0);
    }

    #[test]
    fn escalation_extra_copies_follow_the_table() {
        let drafted = vec!["pallbearer".to_string(), "snapshrike".to_string(), "turret".to_string()];
        // Below level 4: nothing.
        assert!(escalation_extra_copies(3, &drafted, Some("pallbearer")).is_empty());
        // Level 4 (Second Step): one copy of the most-voted enemy.
        assert_eq!(
            escalation_extra_copies(4, &drafted, Some("snapshrike")),
            vec!["snapshrike".to_string()]
        );
        // Level 6 (Crowded Wake): most-voted + pallbearer + one full roster copy.
        let six = escalation_extra_copies(6, &drafted, Some("turret"));
        let count = |s: &str| six.iter().filter(|e| e.as_str() == s).count();
        assert_eq!(count("turret"), 2, "most-voted + per-3 copy");
        assert_eq!(count("pallbearer"), 2, "level-6 pallbearer + per-3 copy");
        assert_eq!(count("snapshrike"), 1, "per-3 copy");
        // Level 21: per-3 capped at 6, plus Second Front (floor(3/2)=1, k=0 -> pallbearer) and Full Chorus.
        let many = escalation_extra_copies(21, &drafted, None);
        let count = |s: &str| many.iter().filter(|e| e.as_str() == s).count();
        assert_eq!(
            count("pallbearer"),
            9,
            "per-3 (6) + L6 pallbearer + Second Front + Full Chorus"
        );
        assert_eq!(count("snapshrike"), 7, "per-3 (6) + Full Chorus");
        // Level 40: per-3 pinned at the +6 cap.
        let endless = escalation_extra_copies(40, &drafted, None);
        let count = |s: &str| endless.iter().filter(|e| e.as_str() == s).count();
        assert_eq!(count("turret"), 7, "per-3 (6) + Full Chorus");
    }

    #[test]
    fn fog_and_milestone_escalation() {
        assert_eq!(escalation_fog_mul(4), 1.0);
        assert_eq!(escalation_fog_mul(12), 1.25);
        assert!(
            (escalation_fog_mul(18) - 1.4375).abs() < 1e-6,
            "Grey Tide and Deep Night stack"
        );
        assert!(escalation_milestone_level(4));
        assert!(escalation_milestone_level(9));
        assert!(escalation_milestone_level(17));
        assert!(escalation_milestone_level(20));
        assert!(escalation_milestone_level(25));
        assert!(escalation_milestone_level(40));
        assert!(!escalation_milestone_level(1));
        assert!(!escalation_milestone_level(11));
        assert!(!escalation_milestone_level(24));
    }

    #[test]
    fn collapse_pending_lasts_exactly_three_seconds() {
        let c = Content::load().unwrap();
        let mut run = Run::new(&c, 5, 1);
        run.phase = PHASE_PLAYING;
        run.level_phase = LEVEL_COLLAPSE_PENDING;
        run.collapse_elapsed_ticks = 0;
        for _ in 0..COLLAPSE_PENDING_TICKS {
            run.tick_collapse();
        }
        assert_eq!(run.level_phase, LEVEL_COLLAPSING);
    }
}
