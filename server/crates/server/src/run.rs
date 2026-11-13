//! Run state and rules: lobby drafts, generated levels, shared gift progress, golden-gift currency, lives,
//! and the enemies the team drafted. Pure game logic on top of `sim` — no I/O, fully unit-testable.
//!
//! Flow: Lobby (spawn hub + draft offer) → all players vote → generate a level → Playing (collect every normal
//! gift → golden gifts appear and the beacon opens) → beacon → Lobby for the next level. Death with no lives left
//! ends that player's run; when everyone is down the run is Over and Restart begins a new one.

use crate::content::{Content, RunMods};
use protocol::{
    CARD_CURSE, CARD_ENEMY, CARD_UPGRADE, DEATH_ENEMY, DEATH_VOID, DraftCard, EnemyView, GIFT_GOLDEN, LevelData,
    PHASE_ENEMY_DRAFT, PHASE_OVER, PHASE_PLAYING, RunInfo, Vec3,
};
use sim::levelgen;
use sim::rules::{self, BEACON_RADIUS, Enemy, GIFT_RADIUS};
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

/// Small deterministic PRNG (SplitMix32) for draft offers and level seeds. Server-only.
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
            rng: Rng::new(seed),
        };
        run.offer = Some(run.make_offer(content));
        run
    }

    fn make_offer(&mut self, content: &Content) -> Offer {
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
        if self.level_num.is_multiple_of(GREATER_EVERY) {
            let greater: Vec<_> = content.curses.iter().filter(|c| c.greater).collect();
            for i in self.rng.sample(greater.len(), 3) {
                cards.push(DraftCard {
                    kind: CARD_CURSE,
                    id: greater[i].id.clone(),
                });
            }
        } else {
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
            cards.extend(rewards);
        }
        Offer {
            enemy_picks,
            reward_picks: REWARD_PICKS,
            cards,
        }
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
        let mut roster: Vec<&str> = self.drafted_enemies.iter().map(String::as_str).collect();
        let drafted = self.drafted_enemies.len();
        if drafted > 0 {
            for k in 0..self.mods.extra_enemies as usize {
                roster.push(self.drafted_enemies[k % drafted].as_str());
            }
        }
        for (slot, (id, pos)) in roster.iter().zip(generated.enemy_spawns.iter()).enumerate() {
            if let Some(def) = content.enemy(id) {
                let params = self.mods.enemy_params(&def.params);
                let id = u32::try_from(slot).unwrap_or(u32::MAX - 1) + 1;
                self.enemies.push(rules::spawn_enemy_with(
                    id,
                    rules::ENEMY_KIND_CHASER,
                    pos.clone(),
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
        self.phase = PHASE_ENEMY_DRAFT;
        self.offer = Some(self.make_offer(content));
    }

    pub fn end(&mut self) {
        self.phase = PHASE_OVER;
        self.offer = None;
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

    pub fn step_enemies(&mut self, targets: &[Vec3]) {
        for e in &mut self.enemies {
            rules::step_enemy(e, targets);
        }
    }

    /// The death cause for a living player standing at `feet`, if something kills them this tick.
    pub fn hazard(&self, feet: &Vec3) -> Option<u8> {
        if feet.y < self.world.kill_y {
            Some(DEATH_VOID)
        } else if self.phase == PHASE_PLAYING
            && self
                .enemies
                .iter()
                .any(|e| e.active_in == 0 && rules::enemy_touches(e, feet))
        {
            // Dormant enemies (spawn grace period) are harmless: grace is a fairness window.
            Some(DEATH_ENEMY)
        } else {
            None
        }
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
            })
            .collect()
    }

    pub fn info(&self) -> RunInfo {
        RunInfo {
            phase: self.phase,
            level: self.level_num,
            lives: self.lives,
            currency: self.currency,
            tuning: self.mods.tuning(),
            fog_density_mul: self.mods.fog_density_mul,
            enemies: self.drafted_enemies.clone(),
            upgrades: self.upgrades.clone(),
            curses: self.curses.clone(),
        }
    }
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

    fn content() -> Content {
        Content::load().unwrap()
    }

    #[test]
    fn first_offer_has_enemies_and_mixed_rewards() {
        let c = content();
        let run = Run::new(&c, 7, 1);
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
        let run = Run::new(&c, 7, 1);
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
        let mut run = Run::new(&c, 7, 1);
        let picks = valid_picks(&run);
        run.resolve(&c, &[picks]);
        assert_eq!(run.phase, PHASE_PLAYING);
        assert_eq!(run.drafted_enemies.len(), 2);
        assert_eq!(run.upgrades.len() + run.curses.len(), 1);
        assert_eq!(run.level.gifts.iter().filter(|g| g.kind != GIFT_GOLDEN).count(), 5);
        assert_eq!(run.enemies.len() as u32, 2 + run.mods.extra_enemies);
        assert!(run.offer.is_none());
    }

    #[test]
    fn golden_gifts_only_after_normal_ones_then_beacon_then_lobby() {
        let c = content();
        let mut run = Run::new(&c, 11, 1);
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
        assert_eq!(run.level_num, 2);
        let offer = run.offer.clone().unwrap();
        assert_eq!(offer.enemy_picks, 0, "enemies are drafted once per run");
        assert_eq!(offer.cards.len(), 3);
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
        let mut run = Run::new(&c, 5, 1);
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
        let mut run = Run::new(&c, 13, 1);
        let picks = valid_picks(&run);
        run.resolve(&c, &[picks]);
        let e = run.enemies[0].pos.clone();
        let touching = Vec3::new(e.x, e.y - 0.9, e.z);
        assert_eq!(run.hazard(&touching), None);
        run.enemies[0].active_in = 0;
        assert_eq!(run.hazard(&touching), Some(DEATH_ENEMY));
    }
}
