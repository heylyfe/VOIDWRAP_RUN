//! Chorus — the gated choir (cue tier 3, min level 10, cap 1).
//!
//! Only a candidate once the run holds at least two `small` permanents at
//! level ≥ 10 (registry synergy hook `chorus_needs_small_pair`); the pool filter
//! is data-driven in the server, and [`chorus_candidate_allowed`] is the same
//! rule for the sim side.
//!
//! Ambient, tier 3: the singers are audible long before anything can hurt you
//! ([`ChorusBrain::murmur_ticks`]); there is no per-attack telegraph and no
//! unavoidable instant hit. Instruments spawn as special gifts on a cadence;
//! each one collected freezes the choir and extends its timer. When the timer
//! runs out, Chorus moves and hunts — one-way.
//!
//! Counter: Striker answers the choir (singer cleanup is out of scope for the
//! brain — no singer actors are modelled here; noted). Bulwark is immune
//! (registry); likewise the singer shove is out of scope.

use protocol::Vec3;

use super::{SIM_DT, TickContext, nearest_player_where};
use crate::rules::move_toward;

/// Base ticks until the choir hunts (45 s at 60 Hz).
pub const CHORUS_TIMER_TICKS: u32 = 2700;
/// Time granted per collected instrument (20 s at 60 Hz).
pub const CHORUS_INSTRUMENT_TIME_TICKS: u32 = 1200;
/// Ticks the choir is frozen by a collected instrument (2.0 s at 60 Hz).
pub const CHORUS_INSTRUMENT_FREEZE_TICKS: u32 = 120;
/// Ticks between instrument gift requests (8.0 s at 60 Hz).
pub const CHORUS_INSTRUMENT_INTERVAL_TICKS: u32 = 480;
/// Maximum uncollected instrument requests outstanding.
pub const CHORUS_INSTRUMENTS_MAX_OUTSTANDING: u32 = 2;
/// Speed while hunting once the timer expires (u/s).
pub const CHORUS_HUNT_SPEED: f32 = 4.5;
/// Small permanents required before Chorus may be offered.
pub const CHORUS_SMALL_PAIR_REQUIRED: u32 = 2;
/// Minimum level before Chorus may be offered (mirrors the registry entry).
pub const CHORUS_MIN_LEVEL: u32 = 10;

/// Pool gate for the `chorus_needs_small_pair` hook plus the level gate.
pub fn chorus_candidate_allowed(small_permanents: u32, level: u32) -> bool {
    small_permanents >= CHORUS_SMALL_PAIR_REQUIRED && level >= CHORUS_MIN_LEVEL
}

/// Chorus phases.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChorusPhase {
    /// Singers audible; the timer ticks down; instruments extend it.
    Hush,
    /// The timer expired; the choir moves and hunts. One-way.
    Hunt,
}

/// Chorus brain state.
#[derive(Debug, Clone, PartialEq)]
pub struct ChorusBrain {
    /// Current phase.
    pub phase: ChorusPhase,
    /// Ticks until the choir hunts; extended by collected instruments.
    pub timer: u32,
    /// Ticks the choir is frozen by a collected instrument. While frozen the
    /// timer does not advance.
    pub frozen_ticks: u32,
    /// Instrument gift requests not yet collected.
    pub instruments_outstanding: u32,
    /// Instruments collected in total.
    pub instruments_collected: u32,
    /// Ticks until the next instrument request.
    pub instrument_in: u32,
    /// Ambient murmur pulse counter; the persistent tier-3 cue. VFX only.
    pub murmur_ticks: u32,
}

impl Default for ChorusBrain {
    fn default() -> Self {
        Self {
            phase: ChorusPhase::Hush,
            timer: CHORUS_TIMER_TICKS,
            frozen_ticks: 0,
            instruments_outstanding: 0,
            instruments_collected: 0,
            instrument_in: CHORUS_INSTRUMENT_INTERVAL_TICKS,
            murmur_ticks: 0,
        }
    }
}

/// What happened this tick.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ChorusStep {
    /// Instrument gifts the server should spawn this tick.
    pub instruments_to_spawn: u32,
    /// True while a collected instrument freezes the choir.
    pub frozen: bool,
    /// True on the tick the choir starts hunting.
    pub began_hunting: bool,
    /// True while the choir is hunting.
    pub hunting: bool,
}

/// Advance Chorus by one tick. `instruments_collected` is how many instrument
/// gifts were collected by any player this tick.
pub fn step_chorus(
    enemy_pos: &mut Vec3,
    brain: &mut ChorusBrain,
    ctx: &TickContext,
    instruments_collected: u32,
) -> ChorusStep {
    let mut out = ChorusStep::default();
    brain.murmur_ticks = brain.murmur_ticks.saturating_add(1);

    for _ in 0..instruments_collected {
        brain.instruments_outstanding = brain.instruments_outstanding.saturating_sub(1);
        brain.timer = brain.timer.saturating_add(CHORUS_INSTRUMENT_TIME_TICKS);
        brain.frozen_ticks = CHORUS_INSTRUMENT_FREEZE_TICKS;
        brain.instruments_collected = brain.instruments_collected.saturating_add(1);
    }

    match brain.phase {
        ChorusPhase::Hush => {
            if brain.frozen_ticks > 0 {
                brain.frozen_ticks -= 1;
                out.frozen = true;
            } else if brain.timer > 0 {
                brain.timer -= 1;
            }
            if brain.instrument_in > 0 {
                brain.instrument_in -= 1;
            } else if brain.instruments_outstanding < CHORUS_INSTRUMENTS_MAX_OUTSTANDING {
                out.instruments_to_spawn = 1;
                brain.instruments_outstanding += 1;
                brain.instrument_in = CHORUS_INSTRUMENT_INTERVAL_TICKS;
            }
            if brain.timer == 0 {
                brain.phase = ChorusPhase::Hunt;
                out.began_hunting = true;
                out.hunting = true;
            }
        }
        ChorusPhase::Hunt => {
            out.hunting = true;
            if let Some(p) = nearest_player_where(ctx.players, enemy_pos, |_| true) {
                let goal = Vec3::new(p.feet.x, enemy_pos.y, p.feet.z);
                move_toward(enemy_pos, &goal, CHORUS_HUNT_SPEED * SIM_DT);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::enemies::testutil::{arena, sample_player};
    use crate::rules::horizontal_distance;

    fn ctx<'a>(players: &'a [super::super::PlayerSample<'static>]) -> TickContext<'a> {
        let (min, max) = arena();
        TickContext {
            players,
            collapse_phase: false,
            map_min: min,
            map_max: max,
        }
    }

    #[test]
    fn chorus_requires_small_pair_and_level() {
        assert!(!chorus_candidate_allowed(1, 12));
        assert!(!chorus_candidate_allowed(2, CHORUS_MIN_LEVEL - 1));
        assert!(chorus_candidate_allowed(CHORUS_SMALL_PAIR_REQUIRED, CHORUS_MIN_LEVEL));
        assert!(chorus_candidate_allowed(3, 20));
    }

    #[test]
    fn instrument_freezes_the_choir_and_extends_its_timer() {
        let mut pos = Vec3::new(0.0, 0.0, 0.0);
        let mut brain = ChorusBrain::default();
        let t0 = brain.timer;
        let players: Vec<super::super::PlayerSample<'static>> = Vec::new();

        let out = step_chorus(&mut pos, &mut brain, &ctx(&players), 1);
        assert!(out.frozen, "the choir is frozen when an instrument is taken");
        assert_eq!(brain.timer, t0 + CHORUS_INSTRUMENT_TIME_TICKS);
        assert_eq!(brain.instruments_collected, 1);

        // While frozen the timer does not advance.
        let frozen_before = brain.frozen_ticks;
        step_chorus(&mut pos, &mut brain, &ctx(&players), 0);
        assert_eq!(brain.timer, t0 + CHORUS_INSTRUMENT_TIME_TICKS);
        assert_eq!(brain.frozen_ticks, frozen_before - 1);
    }

    #[test]
    fn choir_hunts_only_after_timer_expires_and_moves_toward_nearest() {
        let mut pos = Vec3::new(0.0, 0.0, 0.0);
        let mut brain = ChorusBrain {
            timer: 1,
            ..ChorusBrain::default()
        };
        let players = vec![sample_player(1, 10.0, 0.0, "striker")];

        let out = step_chorus(&mut pos, &mut brain, &ctx(&players), 0);
        assert!(out.began_hunting);
        assert_eq!(brain.phase, ChorusPhase::Hunt);

        let before = horizontal_distance(&pos, &Vec3::new(10.0, 0.0, 0.0));
        step_chorus(&mut pos, &mut brain, &ctx(&players), 0);
        let after = horizontal_distance(&pos, &Vec3::new(10.0, 0.0, 0.0));
        assert!(after < before, "the hunting choir closes in");
    }

    #[test]
    fn tier3_persistent_cue_advances_and_no_hunt_before_expiry() {
        let mut pos = Vec3::new(0.0, 0.0, 0.0);
        let mut brain = ChorusBrain::default();
        let players: Vec<super::super::PlayerSample<'static>> = Vec::new();
        let mut last_murmur = brain.murmur_ticks;
        for _ in 0..30 {
            let out = step_chorus(&mut pos, &mut brain, &ctx(&players), 0);
            assert!(!out.began_hunting && !out.hunting, "no unavoidable instant hit");
            assert!(brain.murmur_ticks > last_murmur, "the singers are audible");
            last_murmur = brain.murmur_ticks;
        }
        assert_eq!(pos, Vec3::new(0.0, 0.0, 0.0), "hush: no movement yet");
    }
}
