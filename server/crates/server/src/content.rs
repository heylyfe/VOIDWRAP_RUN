//! Draft content (enemies, upgrades, curses, classes, shop, events, altars) loaded from design/content.json —
//! the same file the client reads, so card ids, numbers and text cannot drift. Compiled into the binary;
//! validated at startup and in tests. The M2/M3 sections are optional in the file so older content still loads.

use protocol::Tuning;
use serde::Deserialize;
use sim::rules::{
    BEHAVIOR_CORRUPTER, BEHAVIOR_GALE_WIGHT, BEHAVIOR_LANTERN_LEECH, BEHAVIOR_MIMIC, BEHAVIOR_MOTELING,
    BEHAVIOR_PALLBEARER, BEHAVIOR_RIFT_WARDEN, BEHAVIOR_SNAPSHRIKE, BEHAVIOR_TURRET, EnemyParams,
};
use std::collections::BTreeMap;

const CONTENT_JSON: &str = include_str!("../../../../design/content.json");

/// Every mod key the run understands; the validator rejects anything else.
const KNOWN_MODS: [&str; 15] = [
    "run_speed_mul",
    "jump_mul",
    "gravity_mul",
    "dash_speed_mul",
    "dash_cooldown_mul",
    "gift_radius_mul",
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

fn pallbearer() -> String {
    "pallbearer".to_string()
}

fn one() -> f32 {
    1.0
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

#[derive(Debug, Clone, Deserialize)]
pub struct EnemyDef {
    pub id: String,
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
            let expected = ["striker", "grappler", "bulwark", "glider"];
            let valid = c.classes.len() >= expected.len()
                && c.classes.iter().zip(expected).all(|(class, want)| class.id == want);
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
    fn older_content_without_m2_or_m3_sections_still_loads() {
        // Simulates an older content.json: no classes/shop/events/altars, no behavior.
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
    }
}
