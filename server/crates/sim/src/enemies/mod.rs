//! Enemy brains for the registry entries added in T-072.
//!
//! T-074 implements the first five: Rewind, Warden, Peal, Kite and Static. Each
//! enemy lives in its own module and exposes a phase enum the client can draw, a
//! brain struct holding the phase and its memory, a `step` function that advances
//! one 20 Hz tick, and a `relation` function carrying the registry's class
//! interactions.
//!
//! Everything here is deterministic: only `+ - * /`, `sqrt`, `abs`, `min`, `max`
//! and `clamp` touch floats, and no wall clock is read. The tick index comes from
//! [`StepContext::tick`].

use protocol::{Gift, Vec3};

use crate::rules::{PlayerStatus, distance, player_centre};

pub mod kite;
pub mod peal;
pub mod rewind;
pub mod r#static;
pub mod warden;

pub use kite::{KiteBrain, KitePhase};
pub use peal::{PealBrain, PealPhase};
pub use rewind::{RewindBrain, RewindPhase};
pub use r#static::{StaticBrain, StaticPhase};
pub use warden::{WardenBrain, WardenPhase};

/// Seconds per enemy brain tick (20 Hz, matching the existing enemy timers).
pub const ENEMY_DT: f32 = 0.05;

/// The four shipped classes, as far as enemy class interactions care.
///
/// The server maps its own class id onto this enum; the registry pins the set to
/// exactly these four (`design/ENEMY_REGISTRY.md` §6).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnemyClass {
    /// Striker.
    Striker,
    /// Grappler.
    Grappler,
    /// Bulwark.
    Bulwark,
    /// Glider.
    Glider,
}

/// How an enemy treats a class, straight from `design/ENEMY_REGISTRY.md` §6.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClassRelation {
    /// No counter, no immunity, no exclusion.
    Neutral,
    /// The class's base ability fully answers the signature threat.
    Counters,
    /// The signature effect is never applied to that class.
    Immune,
    /// The enemy never selects that class.
    Ignores,
}

/// One player as the enemy brains see them for a single tick.
#[derive(Debug, Clone, PartialEq)]
pub struct PlayerSnapshot {
    /// Player id, echoed back in [`Hit::player_id`].
    pub id: u32,
    /// Feet position, the same point `ControllerState.pos` holds.
    pub feet: Vec3,
    /// The player's class.
    pub class: EnemyClass,
    /// Persistent debuff state (OVERTUNED and friends).
    pub status: PlayerStatus,
    /// True while the player is inside a dash.
    pub dashing: bool,
}

/// Everything a brain may read about the world for one tick.
#[derive(Debug, Clone, Copy, Default)]
pub struct StepContext<'a> {
    /// Tick index since the level started, at 20 Hz.
    pub tick: u32,
    /// Players alive this tick.
    pub players: &'a [PlayerSnapshot],
    /// Gifts still on the floor this tick.
    pub gifts: &'a [Gift],
    /// True during the collapse phase.
    pub collapse: bool,
    /// True when `coil` is a permanent in the run (`peal_lethal_when_overtuned`).
    pub coil_permanent: bool,
}

/// A circular hit volume a client can draw as a floor decal.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Ring {
    /// Centre of the ring, in world units.
    pub centre: Vec3,
    /// Radius of the ring, in world units.
    pub radius: f32,
}

/// What a brain wants applied to a player it caught.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HitKind {
    /// The hit kills.
    Lethal,
    /// The hit applies the OVERTUNED debuff.
    Overtuned,
}

/// One player caught by an enemy this tick.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Hit {
    /// The player that was caught.
    pub player_id: u32,
    /// What happens to them.
    pub kind: HitKind,
}

/// What the server should apply after one brain tick.
#[derive(Debug, Clone, PartialEq)]
pub struct EnemyAction {
    /// Movement to add to the enemy position this tick, in units.
    pub motion: Vec3,
    /// When set, the enemy is placed here instead of moving.
    pub teleport: Option<Vec3>,
    /// Players caught this tick.
    pub hits: Vec<Hit>,
}

impl Default for EnemyAction {
    fn default() -> Self {
        Self {
            motion: Vec3::new(0.0, 0.0, 0.0),
            teleport: None,
            hits: Vec::new(),
        }
    }
}

impl EnemyAction {
    /// An action that moves nothing and hits nobody.
    pub fn idle() -> Self {
        Self::default()
    }
}

/// Map a registry id (`design/content.json` → `enemies[].id`) to its `BEHAVIOR_*` constant.
///
/// Returns `None` for ids this build does not implement; callers keep their
/// existing `BEHAVIOR_PALLBEARER` fallback for those.
pub fn behavior_for_id(id: &str) -> Option<u8> {
    let behavior = match id {
        "pallbearer" => crate::rules::BEHAVIOR_PALLBEARER,
        "snapshrike" => crate::rules::BEHAVIOR_SNAPSHRIKE,
        "moteling" => crate::rules::BEHAVIOR_MOTELING,
        "rift_warden" => crate::rules::BEHAVIOR_RIFT_WARDEN,
        "gale_wight" => crate::rules::BEHAVIOR_GALE_WIGHT,
        "lantern_leech" => crate::rules::BEHAVIOR_LANTERN_LEECH,
        "turret" => crate::rules::BEHAVIOR_TURRET,
        "mimic" => crate::rules::BEHAVIOR_MIMIC,
        "corrupter" => crate::rules::BEHAVIOR_CORRUPTER,
        "rewind" => crate::rules::BEHAVIOR_REWIND,
        "warden" => crate::rules::BEHAVIOR_WARDEN,
        "peal" => crate::rules::BEHAVIOR_PEAL,
        "kite" => crate::rules::BEHAVIOR_KITE,
        "static" => crate::rules::BEHAVIOR_STATIC,
        _ => return None,
    };
    Some(behavior)
}

/// The step from `from` toward `to` of at most `max_step`, returned as a delta.
pub(crate) fn step_toward(from: &Vec3, to: &Vec3, max_step: f32) -> Vec3 {
    let dx = to.x - from.x;
    let dy = to.y - from.y;
    let dz = to.z - from.z;
    let d = (dx * dx + dy * dy + dz * dz).sqrt();
    if d <= max_step || d <= 1e-9 {
        return Vec3::new(dx, dy, dz);
    }
    let scale = max_step / d;
    Vec3::new(dx * scale, dy * scale, dz * scale)
}

/// The player nearest to `pos`, skipping one class and one id when asked.
pub(crate) fn nearest_player_excluding<'a>(
    players: &'a [PlayerSnapshot],
    pos: &Vec3,
    skip_class: Option<EnemyClass>,
    skip_id: Option<u32>,
) -> Option<&'a PlayerSnapshot> {
    let mut best: Option<&'a PlayerSnapshot> = None;
    let mut best_d = f32::MAX;
    for p in players {
        if Some(p.class) == skip_class || Some(p.id) == skip_id {
            continue;
        }
        let d = distance(pos, &player_centre(&p.feet));
        if d < best_d {
            best_d = d;
            best = Some(p);
        }
    }
    best
}

/// The player nearest to `pos`, with no class or id filter.
pub(crate) fn nearest_player<'a>(players: &'a [PlayerSnapshot], pos: &Vec3) -> Option<&'a PlayerSnapshot> {
    nearest_player_excluding(players, pos, None, None)
}

/// The player farthest from their nearest team-mate; a lone player is their own group.
pub(crate) fn most_isolated_player(players: &[PlayerSnapshot]) -> Option<&PlayerSnapshot> {
    let mut best: Option<&PlayerSnapshot> = None;
    let mut best_d = -1.0_f32;
    for p in players {
        let mut nearest = 0.0_f32;
        let mut found = false;
        for q in players {
            if q.id == p.id {
                continue;
            }
            let d = distance(&p.feet, &q.feet);
            if !found || d < nearest {
                nearest = d;
                found = true;
            }
        }
        if nearest > best_d {
            best_d = nearest;
            best = Some(p);
        }
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn behavior_for_id_covers_the_registry() {
        assert_eq!(behavior_for_id("rewind"), Some(crate::rules::BEHAVIOR_REWIND));
        assert_eq!(behavior_for_id("warden"), Some(crate::rules::BEHAVIOR_WARDEN));
        assert_eq!(behavior_for_id("peal"), Some(crate::rules::BEHAVIOR_PEAL));
        assert_eq!(behavior_for_id("kite"), Some(crate::rules::BEHAVIOR_KITE));
        assert_eq!(behavior_for_id("static"), Some(crate::rules::BEHAVIOR_STATIC));
        assert_eq!(behavior_for_id("pallbearer"), Some(crate::rules::BEHAVIOR_PALLBEARER));
        assert_eq!(behavior_for_id("blight"), None, "T-075 owns the rest");
    }

    #[test]
    fn step_toward_never_overshoots() {
        let from = Vec3::new(0.0, 0.0, 0.0);
        let to = Vec3::new(10.0, 0.0, 0.0);
        let d = step_toward(&from, &to, 1.0);
        assert!((d.x - 1.0).abs() < 1e-4);
        let d = step_toward(&from, &to, 100.0);
        assert!((d.x - 10.0).abs() < 1e-4);
    }

    #[test]
    fn most_isolated_player_picks_the_lone_one() {
        let players = [
            PlayerSnapshot {
                id: 1,
                feet: Vec3::new(0.0, 0.0, 0.0),
                class: EnemyClass::Bulwark,
                status: PlayerStatus::default(),
                dashing: false,
            },
            PlayerSnapshot {
                id: 2,
                feet: Vec3::new(1.0, 0.0, 0.0),
                class: EnemyClass::Bulwark,
                status: PlayerStatus::default(),
                dashing: false,
            },
            PlayerSnapshot {
                id: 3,
                feet: Vec3::new(40.0, 0.0, 0.0),
                class: EnemyClass::Bulwark,
                status: PlayerStatus::default(),
                dashing: false,
            },
        ];
        assert_eq!(most_isolated_player(&players).map(|p| p.id), Some(3));
        assert_eq!(most_isolated_player(&players[..1]).map(|p| p.id), Some(1));
        assert!(most_isolated_player(&[]).is_none());
    }
}
