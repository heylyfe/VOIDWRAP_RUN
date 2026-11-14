//! Enemy brains for the registry entries added in T-072.
//!
//! T-074 implements the first five: Rewind, Warden, Peal, Kite and Static.
//! T-075 implements the second six: Blight, Metronome, Overlook, Chorus, Coil and
//! the redesigned Lure. The two batches use separate per-tick context types
//! ([`StepContext`] for batch A, [`TickContext`] for batch B) because they were
//! written in parallel against the same frozen registry; both are live.
//!
//! Each
//! enemy lives in its own module and exposes a phase enum the client can draw, a
//! brain struct holding the phase and its memory, a `step` function that advances
//! one 20 Hz tick, and a `relation` function carrying the registry's class
//! interactions.
//!
//! Everything here is deterministic: only `+ - * /`, `sqrt`, `abs`, `min`, `max`
//! and `clamp` touch floats, and no wall clock is read. The tick index comes from
//! [`StepContext::tick`].

use protocol::{Gift, Vec3};

use crate::rules::{PlayerStatus, distance, horizontal_distance, player_centre};

pub mod blight;
pub mod chorus;
pub mod coil;
pub mod kite;
pub mod lure;
pub mod metronome;
pub mod overlook;
pub mod peal;
pub mod rewind;
pub mod r#static;
pub mod warden;

pub use kite::{KiteBrain, KitePhase};
pub use peal::{PealBrain, PealPhase};
pub use rewind::{RewindBrain, RewindPhase};
pub use r#static::{StaticBrain, StaticPhase};
pub use warden::{WardenBrain, WardenPhase};

pub use blight::{BlightBrain, BlightPhase, BlightStep, INFECTION_SPEED_MUL, infection_pauses_timer, step_blight};
pub use chorus::{ChorusBrain, ChorusPhase, ChorusStep, chorus_candidate_allowed, step_chorus};
pub use coil::{CoilBrain, CoilPhase, CoilStep, step_coil};
pub use lure::{LureBrain, LurePhase, LureStep, glimmer_intensity, lure_sees_true_silhouette, step_lure};
pub use metronome::{MetronomeBrain, MetronomePhase, MetronomeStep, step_metronome};
pub use overlook::{OverlookBrain, OverlookPhase, OverlookStep, step_overlook};

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

/// Seconds per simulation tick (60 Hz, matching `crate::tuning::DT`).
pub const SIM_DT: f32 = 1.0 / 60.0;

/// OVERTUNED: the debuff Coil inflicts that makes Peal's final ring lethal
/// (registry synergy hook `peal_lethal_when_overtuned`).
///
/// 10 seconds at 60 Hz. Batch A is adding `overtuned_ticks: u32` to the player
/// state; once it exists, the merge is one field on the player plus calls to
/// the three helpers below. If batch A has not landed yet, callers can track the
/// timer caller-side with the same helpers.
pub const OVERTUNED_TICKS: u32 = 600;

/// Apply or refresh OVERTUNED on a bare timer.
pub fn apply_overtuned(ticks: &mut u32) {
    *ticks = OVERTUNED_TICKS;
}

/// Advance an OVERTUNED timer by one tick; returns true while it is still live.
pub fn tick_overtuned(ticks: &mut u32) -> bool {
    if *ticks > 0 {
        *ticks -= 1;
    }
    *ticks > 0
}

/// True while an OVERTUNED timer is live. Peal's final ring reads this.
pub fn overtuned_active(ticks: u32) -> bool {
    ticks > 0
}

/// Per-player snapshot the brains read for one tick.
#[derive(Debug, Clone)]
pub struct PlayerSample<'a> {
    /// Stable player id.
    pub id: u32,
    /// Feet position.
    pub feet: Vec3,
    /// Class id from `design/content.json` (`striker`, `grappler`, `bulwark`, `glider`).
    pub class: &'a str,
    /// True on the tick this player launched off a jump pad.
    pub jump_launched: bool,
    /// True on the tick this player used their class ability.
    pub used_ability: bool,
    /// True on the tick this player used a support-class ability (cleanses debuffs).
    pub used_support_ability: bool,
    /// True while the glider class ability is held and the player is falling.
    pub gliding: bool,
}

/// World inputs shared by one enemy tick.
#[derive(Debug, Clone)]
pub struct TickContext<'a> {
    /// All players in the level.
    pub players: &'a [PlayerSample<'a>],
    /// True during the collapse phase (players rushing the exit).
    pub collapse_phase: bool,
    /// Minimum corner of the playable map (used for leap / re-dorm placement).
    pub map_min: Vec3,
    /// Maximum corner of the playable map.
    pub map_max: Vec3,
}

/// Nearest player to `from` (horizontal distance) that satisfies `pred`.
pub(crate) fn nearest_player_where<'a, F: Fn(&PlayerSample<'a>) -> bool>(
    players: &'a [PlayerSample<'a>],
    from: &Vec3,
    pred: F,
) -> Option<&'a PlayerSample<'a>> {
    let mut best: Option<&PlayerSample<'a>> = None;
    let mut best_d = f32::INFINITY;
    for p in players {
        if !pred(p) {
            continue;
        }
        let d = horizontal_distance(from, &p.feet);
        if d < best_d {
            best_d = d;
            best = Some(p);
        }
    }
    best
}

/// Deterministic xorshift32; `state` must not be zero (zero is remapped).
pub(crate) fn xorshift32(state: &mut u32) -> u32 {
    let mut x = *state;
    if x == 0 {
        x = 0x9E37_79B9;
    }
    x ^= x << 13;
    x ^= x >> 17;
    x ^= x << 5;
    *state = x;
    x
}

/// Next deterministic value in `[0, 1)`.
pub(crate) fn unit_f32(state: &mut u32) -> f32 {
    (xorshift32(state) % 1024) as f32 / 1024.0
}

/// Stable seed derived from a spawn position (the step API has no enemy id).
pub(crate) fn seed_from_pos(pos: &Vec3) -> u32 {
    let s = pos.x.to_bits() ^ pos.z.to_bits().rotate_left(13) ^ 0x9E37_79B9;
    if s == 0 { 0xA53A_9D1F } else { s }
}

#[cfg(test)]
pub(crate) mod testutil {
    use protocol::Vec3;

    use super::PlayerSample;

    /// Player sample with every flag clear; tests flip individual flags.
    pub(crate) fn sample_player(id: u32, x: f32, z: f32, class: &'static str) -> PlayerSample<'static> {
        PlayerSample {
            id,
            feet: Vec3::new(x, 0.0, z),
            class,
            jump_launched: false,
            used_ability: false,
            used_support_ability: false,
            gliding: false,
        }
    }

    /// Arena bounds for map-dependent enemies (Coil leap, Lure re-dorm).
    pub(crate) fn arena() -> (Vec3, Vec3) {
        (Vec3::new(-60.0, 0.0, -60.0), Vec3::new(60.0, 0.0, 60.0))
    }
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
    #[test]
    fn overtuned_helpers_roundtrip() {
        let mut ticks = 0;
        assert!(!overtuned_active(ticks));
        apply_overtuned(&mut ticks);
        assert_eq!(ticks, OVERTUNED_TICKS);
        assert!(overtuned_active(ticks));
        for _ in 0..OVERTUNED_TICKS {
            tick_overtuned(&mut ticks);
        }
        assert_eq!(ticks, 0);
        assert!(!tick_overtuned(&mut ticks));
    }

    #[test]
    fn xorshift_is_deterministic_and_nonzero() {
        let mut a = 0x1234_5678;
        let mut b = 0x1234_5678;
        assert_eq!(xorshift32(&mut a), xorshift32(&mut b));
        let mut zero = 0;
        assert_ne!(xorshift32(&mut zero), 0);
    }
}
