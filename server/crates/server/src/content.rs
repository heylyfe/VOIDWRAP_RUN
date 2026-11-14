//! Draft content (enemies, upgrades, curses, classes, shop, events, altars) loaded from design/content.json —
//! the same file the client reads, so card ids, numbers and text cannot drift. Compiled into the binary;
//! validated at startup and in tests. The M2/M3 sections are optional in the file so older content still loads.

use protocol::Tuning;
use serde::Deserialize;
use sim::rules::{
    BEHAVIOR_BLIGHT, BEHAVIOR_CHORUS, BEHAVIOR_COIL, BEHAVIOR_CORRUPTER, BEHAVIOR_GALE_WIGHT, BEHAVIOR_KITE,
    BEHAVIOR_LANTERN_LEECH, BEHAVIOR_LURE, BEHAVIOR_METRONOME, BEHAVIOR_MIMIC, BEHAVIOR_MOTELING, BEHAVIOR_OVERLOOK,
    BEHAVIOR_PALLBEARER, BEHAVIOR_PEAL, BEHAVIOR_REWIND, BEHAVIOR_RIFT_WARDEN, BEHAVIOR_SNAPSHRIKE, BEHAVIOR_STATIC,
    BEHAVIOR_TURRET, BEHAVIOR_WARDEN, EnemyParams,
};
use std::collections::BTreeMap;

const CONTENT_JSON: &str = include_str!("../../../../design/content.json");

/// Every mod key the run understands; the validator rejects anything else.
const KNOWN_MODS: [&str; 16] = [
    "run_speed_mul",
    "jump_mul",
    "gravity_mul",
    "dash_speed_mul",
    "dash_cooldown_mul",
    "gift_radius_mul",
    "gift_spacing_mul",
    "enemy_speed_mul",
    "enemy_aggro_mul",
    "enemy_grace_mul",
    "enemy_size_mul",
    "fog_density_mul",
    "beacon_radius_mul",
    "air_jumps_add",
    "extra_lives_add",
    "extra_enemies_add",
];

/// Closed enemy tag vocabulary (design/ENEMY_REGISTRY.md §5). Adding a tag is a spec change.
pub const ENEMY_TAGS: [&str; 10] = [
    "fog", "ambush", "collapse", "chase", "zone", "debuff", "summon", "small", "large", "loot",
];

/// Closed class-interaction vocabulary (§6): a class counters the enemy, is immune to it, or never attracts it.
pub const CLASS_RELATIONS: [&str; 3] = ["counters", "immune", "ignores"];

/// The playable classes, in protocol order. `design/content.json` ships exactly these four.
pub const CLASS_IDS: [&str; 4] = ["striker", "grappler", "bulwark", "glider"];

/// Closed synergy-effect vocabulary (§7). Adding one is a spec change, not a content change.
pub const SYNERGY_EFFECTS: [&str; 3] = [
    "peal_lethal_when_overtuned",
    "infection_pauses_timer",
    "chorus_needs_small_pair",
];

fn pallbearer() -> String {
    "pallbearer".to_string()
}

fn one() -> f32 {
    1.0
}

/// Registry defaults for an older content file that predates T-072: no telegraph promise (tier 3),
/// offerable from level 1, one permanent copy.
fn default_cue_tier() -> u8 {
    3
}

fn default_min_level() -> u8 {
    1
}

fn default_selection_cap() -> u8 {
    1
}

#[derive(Debug, Clone, Deserialize)]
pub struct EnemyStats {
    /// Design main speed; kept in the file for reference. The per-enemy speeds live in the sim (`sim::hazards`),
    /// so this field is not wired into `EnemyParams` (speed comes from the sim plus the `enemy_speed_mul` curse).
    #[allow(dead_code)]
    pub speed: f32,
    /// Design aggro radius; kept in the file for reference. The sim owns targeting ranges, so this is not wired in either.
    #[allow(dead_code)]
    pub aggro: f32,
    pub grace_s: f32,
    pub size: f32,
    /// "pallbearer" | "snapshrike" | ... | "corrupter"; unknown values degrade to pallbearer.
    #[serde(default = "pallbearer")]
    pub behavior: String,
}

/// A declarative pool requirement: the run must already hold `count` permanents carrying `tag`.
#[derive(Debug, Clone, Deserialize)]
pub struct SynergyRequires {
    pub tag: String,
    pub count: u8,
}

/// One declarative synergy hook. Exactly one of `with` (pairwise) or `requires` (self) is set.
#[derive(Debug, Clone, Deserialize)]
pub struct SynergyHook {
    /// Another enemy id; the hook fires while that enemy is a permanent in the run.
    #[serde(default)]
    pub with: Option<String>,
    /// A tag/count prerequisite on the run's own permanents.
    #[serde(default)]
    pub requires: Option<SynergyRequires>,
    /// One of `SYNERGY_EFFECTS`.
    pub effect: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct EnemyDef {
    pub id: String,
    /// 1 = under 1 s telegraph, 2 = 1-2 s, 3 = ambient. Defaults to 3 so an older file promises nothing.
    #[serde(default = "default_cue_tier")]
    pub cue_tier: u8,
    /// First level the enemy may be offered.
    #[serde(default = "default_min_level")]
    pub min_level: u8,
    /// Permanent copies allowed in one run (1 or 2).
    #[serde(default = "default_selection_cap")]
    pub selection_cap: u8,
    /// Closed vocabulary, see `ENEMY_TAGS`.
    #[serde(default)]
    pub tags: Vec<String>,
    /// class id -> "counters" | "immune" | "ignores"; absent classes are neutral.
    #[serde(default)]
    pub class_interactions: BTreeMap<String, String>,
    /// Declarative only; the engine reads these, it never executes content.
    #[serde(default)]
    pub synergy_hooks: Vec<SynergyHook>,
    pub params: EnemyStats,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ModCard {
    pub id: String,
    pub mods: BTreeMap<String, f32>,
    #[serde(default)]
    pub greater: bool,
}

/// A playable class; order in content must match the CLASS_* protocol constants.
#[derive(Debug, Clone, Deserialize)]
pub struct ClassDef {
    pub id: String,
}

/// An item sold in the lobby shop. `kind`: "life" | "reroll" | "mods".
#[derive(Debug, Clone, Deserialize)]
pub struct ShopItemDef {
    pub id: String,
    pub cost: u32,
    pub stock: u8,
    pub kind: String,
    #[serde(default)]
    pub mods: BTreeMap<String, f32>,
}

/// A level event (EVENT_*); `friction_mul` only means something for EVENT_ICE.
#[derive(Debug, Clone, Deserialize)]
pub struct EventDef {
    pub id: String,
    pub event: u8,
    #[serde(default = "one")]
    pub friction_mul: f32,
}

/// An interactable shrine; its id matches nothing on the wire (levelgen altar ids do).
#[derive(Debug, Clone, Deserialize)]
pub struct AltarDef {
    pub id: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Content {
    pub enemies: Vec<EnemyDef>,
    pub upgrades: Vec<ModCard>,
    pub curses: Vec<ModCard>,
    #[serde(default)]
    pub classes: Vec<ClassDef>,
    #[serde(default)]
    pub shop: Vec<ShopItemDef>,
    #[serde(default)]
    pub events: Vec<EventDef>,
    #[serde(default)]
    pub altars: Vec<AltarDef>,
}

impl Content {
    /// Parse the embedded content. A malformed file is a build-time mistake caught by tests and `just verify`.
    pub fn load() -> Result<Content, String> {
        let c: Content = serde_json::from_str(CONTENT_JSON).map_err(|e| format!("design/content.json: {e}"))?;
        if c.enemies.len() < 4 || c.upgrades.len() < 3 || c.curses.iter().filter(|k| !k.greater).count() < 3 {
            return Err("design/content.json: not enough cards for a draft".into());
        }
        if c.curses.iter().filter(|k| k.greater).count() < 3 {
            return Err("design/content.json: need at least 3 greater curses".into());
        }
        // Enemies: the frozen registry contract (design/ENEMY_REGISTRY.md). Ids are unique and every
        // registry field sits inside its closed vocabulary; the sim still owns the behaviour itself.
        let mut enemy_ids = std::collections::BTreeSet::new();
        for enemy in &c.enemies {
            if !enemy_ids.insert(enemy.id.as_str()) {
                return Err(format!("design/content.json: duplicate enemy {}", enemy.id));
            }
        }
        for enemy in &c.enemies {
            let id = enemy.id.as_str();
            if !(1..=3).contains(&enemy.cue_tier) {
                return Err(format!(
                    "design/content.json: enemy {id}: cue_tier {} not in 1..=3",
                    enemy.cue_tier
                ));
            }
            if enemy.min_level < 1 {
                return Err(format!("design/content.json: enemy {id}: min_level must be >= 1"));
            }
            if !(1..=2).contains(&enemy.selection_cap) {
                return Err(format!(
                    "design/content.json: enemy {id}: selection_cap {} not in 1..=2",
                    enemy.selection_cap
                ));
            }
            if enemy.tags.is_empty() {
                return Err(format!("design/content.json: enemy {id}: needs at least one tag"));
            }
            let mut tags = std::collections::BTreeSet::new();
            for tag in &enemy.tags {
                if !ENEMY_TAGS.contains(&tag.as_str()) {
                    return Err(format!("design/content.json: enemy {id}: unknown tag {tag}"));
                }
                if !tags.insert(tag.as_str()) {
                    return Err(format!("design/content.json: enemy {id}: duplicate tag {tag}"));
                }
            }
            for (class, relation) in &enemy.class_interactions {
                if !CLASS_IDS.contains(&class.as_str()) {
                    return Err(format!("design/content.json: enemy {id}: unknown class {class}"));
                }
                if !CLASS_RELATIONS.contains(&relation.as_str()) {
                    return Err(format!(
                        "design/content.json: enemy {id}: unknown class relation {relation}"
                    ));
                }
            }
            for hook in &enemy.synergy_hooks {
                if !SYNERGY_EFFECTS.contains(&hook.effect.as_str()) {
                    return Err(format!(
                        "design/content.json: enemy {id}: unknown synergy effect {}",
                        hook.effect
                    ));
                }
                match (&hook.with, &hook.requires) {
                    (Some(other), None) => {
                        if !enemy_ids.contains(other.as_str()) {
                            return Err(format!(
                                "design/content.json: enemy {id}: synergy with unknown enemy {other}"
                            ));
                        }
                    }
                    (None, Some(requires)) => {
                        if !ENEMY_TAGS.contains(&requires.tag.as_str()) {
                            return Err(format!(
                                "design/content.json: enemy {id}: unknown requirement tag {}",
                                requires.tag
                            ));
                        }
                        if requires.count < 1 {
                            return Err(format!(
                                "design/content.json: enemy {id}: requirement count must be >= 1"
                            ));
                        }
                    }
                    _ => {
                        return Err(format!(
                            "design/content.json: enemy {id}: synergy hook needs exactly one of `with` or `requires`"
                        ));
                    }
                }
            }
        }
        // Shop: unique ids, known kinds, known mod keys.
        let mut shop_ids = std::collections::BTreeSet::new();
        for item in &c.shop {
            if !shop_ids.insert(item.id.as_str()) {
                return Err(format!("design/content.json: duplicate shop item {}", item.id));
            }
            match item.kind.as_str() {
                "life" | "reroll" => {}
                "mods" => {
                    if item.mods.is_empty() {
                        return Err(format!("design/content.json: shop item {} has no mods", item.id));
                    }
                    for key in item.mods.keys() {
                        if !KNOWN_MODS.contains(&key.as_str()) {
                            return Err(format!("design/content.json: shop item {}: unknown mod {key}", item.id));
                        }
                    }
                }
                other => {
                    return Err(format!(
                        "design/content.json: shop item {}: unknown kind {other}",
                        item.id
                    ));
                }
            }
        }
        // Events: unique event numbers and a sane friction multiplier.
        let mut seen_events = std::collections::BTreeSet::new();
        for event in &c.events {
            if !seen_events.insert(event.event) {
                return Err(format!("design/content.json: duplicate event {}", event.event));
            }
            if !event.friction_mul.is_finite() || event.friction_mul <= 0.0 {
                return Err(format!(
                    "design/content.json: event {} has invalid friction_mul",
                    event.id
                ));
            }
        }
        // Classes: the first four, in order, are the CLASS_* protocol constants.
        if !c.classes.is_empty() {
            let valid = c.classes.len() >= CLASS_IDS.len()
                && c.classes.iter().zip(CLASS_IDS).all(|(class, want)| class.id == want);
            if !valid {
                return Err("design/content.json: classes must start with striker, grappler, bulwark, glider".into());
            }
        }
        // Altars: unique ids.
        let mut altar_ids = std::collections::BTreeSet::new();
        for altar in &c.altars {
            if !altar_ids.insert(altar.id.as_str()) {
                return Err(format!("design/content.json: duplicate altar {}", altar.id));
            }
        }
        Ok(c)
    }

    /// Index of an enemy in content order (sent to clients as `EnemyView.kind`) and its definition.
    pub fn enemy(&self, id: &str) -> Option<(usize, &EnemyDef)> {
        self.enemies.iter().enumerate().find(|(_, e)| e.id == id)
    }

    pub fn upgrade(&self, id: &str) -> Option<&ModCard> {
        self.upgrades.iter().find(|e| e.id == id)
    }

    pub fn curse(&self, id: &str) -> Option<&ModCard> {
        self.curses.iter().find(|e| e.id == id)
    }

    /// Shop item definition by id.
    pub fn shop_item(&self, id: &str) -> Option<&ShopItemDef> {
        self.shop.iter().find(|e| e.id == id)
    }

    /// Event definition (EVENT_*) by event number, e.g. `EVENT_ICE`.
    pub fn event(&self, event: u8) -> Option<&EventDef> {
        self.events.iter().find(|e| e.event == event)
    }
}

/// Accumulated effect of every upgrade and curse drafted this run (multipliers multiply, additions add).
#[derive(Debug, Clone, PartialEq)]
pub struct RunMods {
    pub run_speed_mul: f32,
    pub jump_mul: f32,
    pub gravity_mul: f32,
    pub dash_speed_mul: f32,
    pub dash_cooldown_mul: f32,
    pub air_jumps: u32,
    pub gift_radius_mul: f32,
    /// Clustered Gifts curse: multiplier on the sim's minimum gift spacing (1.0 = scattered default).
    /// Values below 1.0 place gifts closer together (the sim clamps to [0.25, 1.0]; this side to GENERIC_MUL_RANGE).
    pub gift_spacing_mul: f32,
    pub extra_lives: u32,
    pub enemy_speed_mul: f32,
    pub enemy_aggro_mul: f32,
    pub enemy_grace_mul: f32,
    pub enemy_size_mul: f32,
    pub extra_enemies: u32,
    pub fog_density_mul: f32,
    pub beacon_radius_mul: f32,
}

impl Default for RunMods {
    fn default() -> Self {
        RunMods {
            run_speed_mul: 1.0,
            jump_mul: 1.0,
            gravity_mul: 1.0,
            dash_speed_mul: 1.0,
            dash_cooldown_mul: 1.0,
            air_jumps: 0,
            gift_radius_mul: 1.0,
            gift_spacing_mul: 1.0,
            extra_lives: 0,
            enemy_speed_mul: 1.0,
            enemy_aggro_mul: 1.0,
            enemy_grace_mul: 1.0,
            enemy_size_mul: 1.0,
            extra_enemies: 0,
            fog_density_mul: 1.0,
            beacon_radius_mul: 1.0,
        }
    }
}

/// Hard caps on stacked effects so no combination of cards can make a level impossible or absurd.
const MOVE_MUL_RANGE: (f32, f32) = (0.8, 1.8);
const JUMP_MUL_RANGE: (f32, f32) = (0.85, 1.6);
const GRAVITY_MUL_RANGE: (f32, f32) = (0.5, 1.25);
const GENERIC_MUL_RANGE: (f32, f32) = (0.4, 2.5);
const MAX_AIR_JUMPS: u32 = 3;
const MAX_EXTRA_ENEMIES: u32 = 6;
const MAX_LIVES: u32 = 9;

impl RunMods {
    /// Fold one card's mods in. Unknown keys are ignored (the content validator rejects them before shipping).
    pub fn apply(&mut self, mods: &BTreeMap<String, f32>) {
        for (key, &v) in mods {
            let v = if v.is_finite() { v } else { continue };
            match key.as_str() {
                "run_speed_mul" => self.run_speed_mul *= v,
                "jump_mul" => self.jump_mul *= v,
                "gravity_mul" => self.gravity_mul *= v,
                "dash_speed_mul" => self.dash_speed_mul *= v,
                "dash_cooldown_mul" => self.dash_cooldown_mul *= v,
                "gift_radius_mul" => self.gift_radius_mul *= v,
                "gift_spacing_mul" => self.gift_spacing_mul *= v,
                "enemy_speed_mul" => self.enemy_speed_mul *= v,
                "enemy_aggro_mul" => self.enemy_aggro_mul *= v,
                "enemy_grace_mul" => self.enemy_grace_mul *= v,
                "enemy_size_mul" => self.enemy_size_mul *= v,
                "fog_density_mul" => self.fog_density_mul *= v,
                "beacon_radius_mul" => self.beacon_radius_mul *= v,
                "air_jumps_add" => self.air_jumps = self.air_jumps.saturating_add(v.max(0.0) as u32),
                "extra_lives_add" => self.extra_lives = self.extra_lives.saturating_add(v.max(0.0) as u32),
                "extra_enemies_add" => self.extra_enemies = self.extra_enemies.saturating_add(v.max(0.0) as u32),
                _ => {}
            }
        }
        self.clamp();
    }

    fn clamp(&mut self) {
        let c = |v: f32, (lo, hi): (f32, f32)| v.clamp(lo, hi);
        self.run_speed_mul = c(self.run_speed_mul, MOVE_MUL_RANGE);
        self.jump_mul = c(self.jump_mul, JUMP_MUL_RANGE);
        self.gravity_mul = c(self.gravity_mul, GRAVITY_MUL_RANGE);
        self.dash_speed_mul = c(self.dash_speed_mul, GENERIC_MUL_RANGE);
        self.dash_cooldown_mul = c(self.dash_cooldown_mul, GENERIC_MUL_RANGE);
        self.gift_radius_mul = c(self.gift_radius_mul, GENERIC_MUL_RANGE);
        // The curse ships 0.6; values above 1.0 (spreading) are folded away by the sim's 1.0 cap.
        self.gift_spacing_mul = c(self.gift_spacing_mul, GENERIC_MUL_RANGE);
        self.enemy_speed_mul = c(self.enemy_speed_mul, GENERIC_MUL_RANGE);
        self.enemy_aggro_mul = c(self.enemy_aggro_mul, GENERIC_MUL_RANGE);
        self.enemy_grace_mul = c(self.enemy_grace_mul, GENERIC_MUL_RANGE);
        self.enemy_size_mul = c(self.enemy_size_mul, GENERIC_MUL_RANGE);
        self.fog_density_mul = c(self.fog_density_mul, GENERIC_MUL_RANGE);
        self.beacon_radius_mul = c(self.beacon_radius_mul, GENERIC_MUL_RANGE);
        self.air_jumps = self.air_jumps.min(MAX_AIR_JUMPS);
        self.extra_enemies = self.extra_enemies.min(MAX_EXTRA_ENEMIES);
        self.extra_lives = self.extra_lives.min(MAX_LIVES);
    }

    /// The movement tuning shared with the client predictor. `friction_mul` is 1.0 here;
    /// the level event (EVENT_ICE) overrides it, see `Run::tuning`.
    pub fn tuning(&self) -> Tuning {
        Tuning {
            run_speed_mul: self.run_speed_mul,
            jump_mul: self.jump_mul,
            gravity_mul: self.gravity_mul,
            dash_speed_mul: self.dash_speed_mul,
            dash_cooldown_mul: self.dash_cooldown_mul,
            air_jumps: self.air_jumps as u8,
            friction_mul: 1.0,
        }
    }

    /// Enemy parameters after curses: size scaled, grace converted to ticks, speed multiplier from the speed curse.
    /// Per-enemy design numbers live in the sim; `speed_mul` starts at the curse multiplier (1.0 by default).
    pub fn enemy_params(&self, stats: &EnemyStats) -> EnemyParams {
        EnemyParams {
            behavior: behavior_from_str(&stats.behavior),
            speed_mul: self.enemy_speed_mul,
            size: stats.size * self.enemy_size_mul,
            grace_ticks: secs_to_ticks(stats.grace_s * self.enemy_grace_mul),
        }
    }
}

/// Map a behavior string to a BEHAVIOR_* constant; content order is the protocol's behaviour order.
/// Unknown strings degrade to pallbearer.
fn behavior_from_str(s: &str) -> u8 {
    match s {
        "pallbearer" => BEHAVIOR_PALLBEARER,
        "snapshrike" => BEHAVIOR_SNAPSHRIKE,
        "moteling" => BEHAVIOR_MOTELING,
        "rift_warden" => BEHAVIOR_RIFT_WARDEN,
        "gale_wight" => BEHAVIOR_GALE_WIGHT,
        "lantern_leech" => BEHAVIOR_LANTERN_LEECH,
        "turret" => BEHAVIOR_TURRET,
        "mimic" => BEHAVIOR_MIMIC,
        "corrupter" => BEHAVIOR_CORRUPTER,
        "rewind" => BEHAVIOR_REWIND,
        "warden" => BEHAVIOR_WARDEN,
        "peal" => BEHAVIOR_PEAL,
        "kite" => BEHAVIOR_KITE,
        "static" => BEHAVIOR_STATIC,
        "blight" => BEHAVIOR_BLIGHT,
        "metronome" => BEHAVIOR_METRONOME,
        "overlook" => BEHAVIOR_OVERLOOK,
        "chorus" => BEHAVIOR_CHORUS,
        "coil" => BEHAVIOR_COIL,
        "lure" => BEHAVIOR_LURE,
        _ => BEHAVIOR_PALLBEARER,
    }
}

/// Seconds to whole ticks at TICK_HZ, rounded, at least 1 tick (and NaN-safe).
fn secs_to_ticks(s: f32) -> u32 {
    (s * protocol::TICK_HZ as f32).round().max(1.0) as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn content_loads_and_every_card_mod_is_known() {
        let c = Content::load().unwrap();
        let known = [
            "run_speed_mul",
            "jump_mul",
            "gravity_mul",
            "dash_speed_mul",
            "dash_cooldown_mul",
            "gift_radius_mul",
            "gift_spacing_mul",
            "enemy_speed_mul",
            "enemy_aggro_mul",
            "enemy_grace_mul",
            "enemy_size_mul",
            "fog_density_mul",
            "beacon_radius_mul",
            "air_jumps_add",
            "extra_lives_add",
            "extra_enemies_add",
        ];
        for card in c.upgrades.iter().chain(c.curses.iter()) {
            for key in card.mods.keys() {
                assert!(known.contains(&key.as_str()), "{}: unknown mod {key}", card.id);
            }
        }
    }

    #[test]
    fn mods_stack_and_clamp_to_playable_limits() {
        let mut m = RunMods::default();
        let slow: BTreeMap<String, f32> = [("run_speed_mul".to_string(), 0.5)].into();
        for _ in 0..5 {
            m.apply(&slow);
        }
        assert_eq!(m.run_speed_mul, MOVE_MUL_RANGE.0);
        let jumps: BTreeMap<String, f32> = [("air_jumps_add".to_string(), 2.0)].into();
        m.apply(&jumps);
        m.apply(&jumps);
        assert_eq!(m.air_jumps, MAX_AIR_JUMPS);
        assert_eq!(m.tuning().air_jumps, 3);
        let nan: BTreeMap<String, f32> = [("jump_mul".to_string(), f32::NAN)].into();
        m.apply(&nan);
        assert_eq!(m.jump_mul, 1.0);
    }

    #[test]
    fn every_enemy_behavior_string_maps_to_its_own_brain() {
        // The registry's migration note warned that unknown behaviour strings degrade to the
        // pallbearer brain. That is a silent failure: the enemy spawns, renders and validates, it
        // just does the wrong thing. Every entry must map to a distinct BEHAVIOR_* constant.
        let c = Content::load().unwrap();
        let mut seen = std::collections::BTreeSet::new();
        for e in &c.enemies {
            let b = behavior_from_str(&e.params.behavior);
            assert_eq!(
                b,
                behavior_from_str(&e.id),
                "{}: params.behavior {:?} does not map to its own brain",
                e.id,
                e.params.behavior
            );
            assert!(
                seen.insert(b),
                "{}: behaviour {b} is claimed by more than one enemy",
                e.id
            );
        }
        assert_eq!(seen.len(), c.enemies.len());
        assert_eq!(behavior_from_str("pallbearer"), BEHAVIOR_PALLBEARER);
        assert_eq!(behavior_from_str("lure"), BEHAVIOR_LURE);
        assert_eq!(behavior_from_str("not_an_enemy"), BEHAVIOR_PALLBEARER);
    }

    #[test]
    fn enemy_params_apply_curses() {
        let mut m = RunMods::default();
        m.apply(
            &[
                ("enemy_speed_mul".to_string(), 1.5),
                ("enemy_grace_mul".to_string(), 0.5),
            ]
            .into(),
        );
        let p = m.enemy_params(&EnemyStats {
            speed: 4.0,
            aggro: 10.0,
            grace_s: 4.0,
            size: 0.5,
            behavior: "snapshrike".to_string(),
        });
        assert_eq!(p.speed_mul, 1.5);
        assert_eq!(p.size, 0.5);
        assert_eq!(p.grace_ticks, 40);
    }

    #[test]
    fn enemy_params_map_behavior_strings() {
        let m = RunMods::default();
        let stats = |behavior: &str| EnemyStats {
            speed: 0.0,
            aggro: 0.0,
            grace_s: 2.0,
            size: 0.7,
            behavior: behavior.to_string(),
        };
        let cases = [
            ("pallbearer", BEHAVIOR_PALLBEARER),
            ("snapshrike", BEHAVIOR_SNAPSHRIKE),
            ("moteling", BEHAVIOR_MOTELING),
            ("rift_warden", BEHAVIOR_RIFT_WARDEN),
            ("gale_wight", BEHAVIOR_GALE_WIGHT),
            ("lantern_leech", BEHAVIOR_LANTERN_LEECH),
            ("turret", BEHAVIOR_TURRET),
            ("mimic", BEHAVIOR_MIMIC),
            ("corrupter", BEHAVIOR_CORRUPTER),
            ("rewind", BEHAVIOR_REWIND),
            ("warden", BEHAVIOR_WARDEN),
            ("peal", BEHAVIOR_PEAL),
            ("kite", BEHAVIOR_KITE),
            ("static", BEHAVIOR_STATIC),
            ("blight", BEHAVIOR_BLIGHT),
            ("metronome", BEHAVIOR_METRONOME),
            ("overlook", BEHAVIOR_OVERLOOK),
            ("chorus", BEHAVIOR_CHORUS),
            ("coil", BEHAVIOR_COIL),
            ("lure", BEHAVIOR_LURE),
        ];
        for (name, want) in cases {
            assert_eq!(m.enemy_params(&stats(name)).behavior, want, "{name}");
        }
        // Unknown behavior degrades to pallbearer.
        assert_eq!(m.enemy_params(&stats("wraith")).behavior, BEHAVIOR_PALLBEARER);
        // Ticks are rounded and at least 1 (even for zero/negative seconds).
        let tiny = EnemyStats {
            speed: 1.0,
            aggro: 1.0,
            grace_s: 0.01,
            size: 1.0,
            behavior: "pallbearer".to_string(),
        };
        let p = m.enemy_params(&tiny);
        assert_eq!(p.grace_ticks, 1);
        let negative = EnemyStats {
            speed: 1.0,
            aggro: 1.0,
            grace_s: -5.0,
            size: 1.0,
            behavior: "pallbearer".to_string(),
        };
        assert_eq!(m.enemy_params(&negative).grace_ticks, 1);
    }

    #[test]
    fn new_sections_parse_and_validate() {
        let c = Content::load().unwrap();
        let class_ids: Vec<&str> = c.classes.iter().map(|k| k.id.as_str()).collect();
        assert_eq!(class_ids, ["striker", "grappler", "bulwark", "glider"]);
        assert!(c.shop.len() >= 4, "shop needs at least 4 items for an offer");
        let kept_flame = c.shop_item("kept_flame").unwrap();
        assert_eq!(
            (kept_flame.cost, kept_flame.stock, kept_flame.kind.as_str()),
            (60, 1, "life")
        );
        let ice = c.event(2).unwrap();
        assert_eq!(ice.id, "slick_ice");
        assert_eq!(ice.friction_mul, 0.25);
        assert!(c.events.iter().any(|e| e.event == 1 && e.id == "high_rise"));
        assert!(c.events.iter().any(|e| e.event == 3 && e.id == "crumbling"));
        assert!(!c.altars.is_empty());
        for item in &c.shop {
            for key in item.mods.keys() {
                assert!(KNOWN_MODS.contains(&key.as_str()), "{}: unknown mod {key}", item.id);
            }
        }
    }

    #[test]
    fn shipped_registry_has_twenty_valid_entries() {
        let c = Content::load().unwrap();
        let ids: Vec<&str> = c.enemies.iter().map(|e| e.id.as_str()).collect();
        let expected = [
            "pallbearer",
            "snapshrike",
            "moteling",
            "rift_warden",
            "gale_wight",
            "lantern_leech",
            "turret",
            "mimic",
            "corrupter",
            "rewind",
            "warden",
            "peal",
            "kite",
            "static",
            "blight",
            "metronome",
            "overlook",
            "chorus",
            "coil",
            "lure",
        ];
        assert_eq!(ids, expected, "content order is the network kind index");
        for e in &c.enemies {
            assert!(!e.id.trim().is_empty(), "enemy without an id");
            assert!((1..=3).contains(&e.cue_tier), "{}: cue_tier {}", e.id, e.cue_tier);
            assert!(e.min_level >= 1, "{}: min_level {}", e.id, e.min_level);
            assert!(
                (1..=2).contains(&e.selection_cap),
                "{}: selection_cap {}",
                e.id,
                e.selection_cap
            );
            assert!(!e.tags.is_empty(), "{}: no tags", e.id);
            for tag in &e.tags {
                assert!(ENEMY_TAGS.contains(&tag.as_str()), "{}: tag {tag}", e.id);
            }
            for (class, relation) in &e.class_interactions {
                assert!(CLASS_IDS.contains(&class.as_str()), "{}: class {class}", e.id);
                assert!(
                    CLASS_RELATIONS.contains(&relation.as_str()),
                    "{}: relation {relation}",
                    e.id
                );
            }
            for hook in &e.synergy_hooks {
                assert!(
                    SYNERGY_EFFECTS.contains(&hook.effect.as_str()),
                    "{}: effect {}",
                    e.id,
                    hook.effect
                );
                if let Some(other) = &hook.with {
                    assert!(ids.contains(&other.as_str()), "{}: with {other}", e.id);
                }
            }
        }
        // The three hooks the registry pins.
        let hooks = |id: &str| c.enemy(id).map(|(_, e)| e.synergy_hooks.clone()).unwrap_or_default();
        assert!(
            hooks("peal")
                .iter()
                .any(|h| h.with.as_deref() == Some("coil") && h.effect == "peal_lethal_when_overtuned")
        );
        assert!(
            hooks("blight")
                .iter()
                .any(|h| h.with.as_deref() == Some("rewind") && h.effect == "infection_pauses_timer")
        );
        assert!(
            hooks("blight")
                .iter()
                .any(|h| h.with.as_deref() == Some("kite") && h.effect == "infection_pauses_timer")
        );
        assert!(hooks("chorus").iter().any(|h| {
            h.effect == "chorus_needs_small_pair"
                && h.requires.as_ref().is_some_and(|r| r.tag == "small" && r.count == 2)
        }));
    }

    #[test]
    fn older_content_without_m2_or_m3_sections_still_loads() {
        // Simulates an older content.json: no classes/shop/events/altars, no behavior, no registry fields.
        let older = r#"{
            "enemies": [
                {"id": "a", "params": {"speed": 1.0, "aggro": 1.0, "grace_s": 1.0, "size": 1.0}},
                {"id": "b", "params": {"speed": 1.0, "aggro": 1.0, "grace_s": 1.0, "size": 1.0}},
                {"id": "c", "params": {"speed": 1.0, "aggro": 1.0, "grace_s": 1.0, "size": 1.0}},
                {"id": "d", "params": {"speed": 1.0, "aggro": 1.0, "grace_s": 1.0, "size": 1.0}}
            ],
            "upgrades": [
                {"id": "u1", "mods": {"run_speed_mul": 1.1}},
                {"id": "u2", "mods": {"jump_mul": 1.1}},
                {"id": "u3", "mods": {"dash_speed_mul": 1.1}}
            ],
            "curses": [
                {"id": "c1", "mods": {"jump_mul": 0.9}},
                {"id": "c2", "mods": {"jump_mul": 0.9}},
                {"id": "c3", "mods": {"jump_mul": 0.9}},
                {"id": "g1", "greater": true, "mods": {"gravity_mul": 1.2}},
                {"id": "g2", "greater": true, "mods": {"gravity_mul": 1.2}},
                {"id": "g3", "greater": true, "mods": {"gravity_mul": 1.2}}
            ]
        }"#;
        let c: Content = serde_json::from_str(older).unwrap();
        assert!(c.shop.is_empty() && c.events.is_empty() && c.classes.is_empty() && c.altars.is_empty());
        assert_eq!(c.enemies[0].params.behavior, "pallbearer");
        assert_eq!(c.event(2).map(|e| e.id.as_str()), None);
        // Registry fields fall back to the permissive defaults.
        assert_eq!(c.enemies[0].cue_tier, 3);
        assert_eq!(c.enemies[0].min_level, 1);
        assert_eq!(c.enemies[0].selection_cap, 1);
        assert!(c.enemies[0].tags.is_empty());
        assert!(c.enemies[0].class_interactions.is_empty());
        assert!(c.enemies[0].synergy_hooks.is_empty());
    }
}
