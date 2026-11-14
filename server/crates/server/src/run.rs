//! Run state and rules: lobby drafts, generated levels, shared gift progress, golden-gift currency, lives,
//! the enemies the team drafted, the lobby shop, level events and altars. Pure game logic on top of `sim` —
//! no I/O, fully unit-testable.
//!
//! Flow: Lobby (spawn hub + draft offer and, from level 2 on, the shop) → all players vote → generate a level → Playing (collect every normal
//! gift → golden gifts appear and the beacon opens) → beacon → Lobby for the next level. Death with no lives left
//! ends that player's run; when everyone is down the run is Over and Restart begins a new one.

use crate::content::{Content, RunMods};
use protocol::{
    CARD_CURSE, CARD_ENEMY, CARD_UPGRADE, DraftCard, EVENT_ICE, EnemyView, GIFT_GOLDEN, LevelData, PHASE_ENEMY_DRAFT,
    PHASE_OVER, PHASE_PLAYING, Platform, RunInfo, Tuning, Vec3,
};
use sim::hazards::Hazards;
use sim::levelgen;
use sim::rules::{self, BEACON_RADIUS, BEHAVIOR_MIMIC, Enemy, GIFT_RADIUS, enemy_state};
use sim::world::World;
use std::collections::BTreeMap;

/// Currency granted per golden gift.
pub const GOLDEN_VALUE: u32 = 5;
/// Enemy cards offered at the start of a run, and how many the team keeps.
const ENEMY_OFFER: usize = 4;
const ENEMY_PICKS: u8 = 2;
/// Reward cards offered before each level (one is kept).
const REWARD_PICKS: u8 = 1;
/// Every Nth level offers greater curses only.
const GREATER_EVERY: u32 = 10;
/// Shop items offered per lobby (first, plain lobby gets none).
const SHOP_OFFER: usize = 4;

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
    /// Projectiles, crumbling tiles and turret/mimic/corrupter behaviour for the current level.
    pub hazards: Hazards,
    /// Levelgen altar ids already used this level.
    pub altars_used: Vec<u16>,
    /// Friction multiplier from the level's event (EVENT_ICE pulls it from content; else 1.0).
    pub event_friction_mul: f32,
    /// The lobby shop offer (from level 2 on); `None` outside the shop lobby and while playing.
    pub shop: Option<Vec<ShopOffer>>,
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
    /// Ties go to the card offered first.
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
        for i in winners(true, offer.enemy_picks) {
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
        self.start_level(content);
    }

    fn start_level(&mut self, content: &Content) {
        let seed = self.rng.next_u32();
        let generated = levelgen::generate(seed, self.level_num);
        self.level = generated.data;
        self.world = World::from_level(&self.level);
        self.gifts_taken.clear();
        self.enemies.clear();
        self.hazards.reset();
        self.altars_used.clear();
        // The shop lives in the lobby only; starting a level closes it.
        self.shop = None;
        self.event_friction_mul = event_friction_mul(content, self.level.event);
        let mut roster: Vec<&str> = self.drafted_enemies.iter().map(String::as_str).collect();
        let drafted = self.drafted_enemies.len();
        if drafted > 0 {
            for k in 0..self.mods.extra_enemies as usize {
                roster.push(self.drafted_enemies[k % drafted].as_str());
            }
        }
        for (slot, (id, pos)) in roster.iter().zip(generated.enemy_spawns.iter()).enumerate() {
            if let Some((index, def)) = content.enemy(id) {
                let params = self.mods.enemy_params(&def.params);
                let mut pos = pos.clone();
                // Mimics pose as gifts: spawn them on the platform below the generated spawn
                // (pos.y = platform top + 0.5). If the spawn is not above any platform, keep the position.
                if params.behavior == BEHAVIOR_MIMIC
                    && let Some(top) = platform_top_below(&self.level.platforms, pos.x, pos.z, pos.y)
                {
                    pos.y = top + 0.5;
                }
                let id = u32::try_from(slot).unwrap_or(u32::MAX - 1) + 1;
                // kind = index into content.json enemies, so the client draws the right model.
                self.enemies.push(rules::spawn_enemy_with(
                    id,
                    u8::try_from(index).unwrap_or(u8::MAX),
                    pos,
                    params,
                ));
            }
        }
        self.phase = PHASE_PLAYING;
    }

    /// The beacon was reached: back to the lobby to draft for the next level.
    pub fn finish_level(&mut self, content: &Content) {
        self.level_num = self.level_num.saturating_add(1);
        self.level = levelgen::hub();
        self.world = World::from_level(&self.level);
        self.gifts_taken.clear();
        self.enemies.clear();
        self.hazards.reset();
        self.altars_used.clear();
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
            .filter(|g| g.kind != GIFT_GOLDEN && !self.gifts_taken.contains(&g.id))
            .count()
    }

    pub fn beacon_open(&self) -> bool {
        self.phase == PHASE_PLAYING && self.normal_left() == 0
    }

    /// Collect gifts the player at `feet` touches. Golden gifts only count once every normal gift is taken.
    pub fn collect_gifts(&mut self, feet: &Vec3) {
        if self.phase != PHASE_PLAYING {
            return;
        }
        let radius = GIFT_RADIUS * self.mods.gift_radius_mul;
        let golden_open = self.normal_left() == 0;
        for gift in &self.level.gifts {
            let golden = gift.kind == GIFT_GOLDEN;
            if self.gifts_taken.contains(&gift.id) || (golden && !golden_open) {
                continue;
            }
            if rules::gift_touched_within(feet, gift, radius) {
                self.gifts_taken.push(gift.id);
                if golden {
                    self.currency = self.currency.saturating_add(GOLDEN_VALUE);
                } else {
                    self.gifts_collected = self.gifts_collected.saturating_add(1);
                }
            }
        }
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
        self.enemies
            .iter()
            .map(|e| EnemyView {
                id: e.id,
                kind: e.kind,
                pos: e.pos.clone(),
                radius: e.params.size,
                // DORMANT / HUNTING / DISGUISED (mimic still posing as a gift).
                state: enemy_state(e),
                // M3 (T-053): telegraph anchor / progress / heavy flag.
                target: Vec3::ZERO,
                prog: 0,
                flags: 0,
            })
            .collect()
    }

    pub fn info(&self) -> RunInfo {
        RunInfo {
            phase: self.phase,
            level: self.level_num,
            lives: self.lives,
            currency: self.currency,
            tuning: self.tuning(),
            fog_density_mul: self.mods.fog_density_mul,
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
    use sim::rules::ENEMY_KIND_CHASER;

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
        assert_eq!(run.level.gifts.iter().filter(|g| g.kind != GIFT_GOLDEN).count(), 5);
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
        let golden: Vec<_> = gifts.iter().filter(|g| g.kind == GIFT_GOLDEN).cloned().collect();
        assert!(!golden.is_empty());
        run.collect_gifts(&feet(&golden[0]));
        assert_eq!(run.currency, 0, "golden gifts are locked while normal gifts remain");
        assert!(!run.beacon_reached(&run.level.beacon.clone()));
        for g in gifts.iter().filter(|g| g.kind != GIFT_GOLDEN) {
            run.collect_gifts(&feet(g));
        }
        assert!(run.beacon_open());
        run.collect_gifts(&feet(&golden[0]));
        assert_eq!(run.currency, GOLDEN_VALUE);
        assert_eq!(run.gifts_collected, 5);
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
        let c = content();
        let mut run = Run::new(&c, 13, 2);
        let picks = valid_picks(&run);
        run.resolve(&c, &[picks]);
        let e = run.enemies[0].pos.clone();
        let touching = Vec3::new(e.x, e.y - 0.9, e.z);
        assert_eq!(run.hazards.hit(&run.enemies, &touching), None);
        run.enemies[0].active_in = 0;
        assert_eq!(run.hazards.hit(&run.enemies, &touching), Some(protocol::DEATH_ENEMY));
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
        let _ = ENEMY_KIND_CHASER; // silence unused import in this module
    }
}
