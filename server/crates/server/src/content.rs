//! Draft content (enemies, upgrades, curses) loaded from design/content.json — the same file the client reads,
//! so card ids, numbers and text cannot drift. Compiled into the binary; validated at startup and in tests.

use protocol::Tuning;
use serde::Deserialize;
use sim::rules::EnemyParams;
use std::collections::BTreeMap;

const CONTENT_JSON: &str = include_str!("../../../../design/content.json");

#[derive(Debug, Clone, Deserialize)]
pub struct EnemyStats {
    pub speed: f32,
    pub aggro: f32,
    pub grace_s: f32,
    pub size: f32,
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

#[derive(Debug, Clone, Deserialize)]
pub struct Content {
    pub enemies: Vec<EnemyDef>,
    pub upgrades: Vec<ModCard>,
    pub curses: Vec<ModCard>,
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

    /// The movement tuning shared with the client predictor.
    pub fn tuning(&self) -> Tuning {
        Tuning {
            run_speed_mul: self.run_speed_mul,
            jump_mul: self.jump_mul,
            gravity_mul: self.gravity_mul,
            dash_speed_mul: self.dash_speed_mul,
            dash_cooldown_mul: self.dash_cooldown_mul,
            air_jumps: self.air_jumps as u8,
        }
    }

    /// Enemy parameters after curses.
    pub fn enemy_params(&self, stats: &EnemyStats) -> EnemyParams {
        let grace_s = (stats.grace_s * self.enemy_grace_mul).max(0.5);
        EnemyParams {
            speed: stats.speed * self.enemy_speed_mul,
            aggro: stats.aggro * self.enemy_aggro_mul,
            size: stats.size * self.enemy_size_mul,
            grace_ticks: (grace_s * protocol::TICK_HZ as f32) as u32,
        }
    }
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
        });
        assert_eq!(p.speed, 6.0);
        assert_eq!(p.grace_ticks, 40);
    }
}
