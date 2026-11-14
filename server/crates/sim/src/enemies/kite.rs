//! Kite: locks the most isolated player and detonates a tracked strike marker.
//!
//! Tier-2 telegraph: the tether brightens and the marker tracks the target for
//! 1.4 s, then freezes for 0.4 s before the blast rings resolve. A Striker or a
//! Grappler that closes the gap cuts the line; height breaks it too.

use protocol::Vec3;

use crate::rules::{EnemyParams, distance};

use super::blight::infection_pauses_timer;

use super::{
    ClassRelation, ENEMY_DT, EnemyAction, EnemyClass, Hit, HitKind, Ring, StepContext, most_isolated_player,
    step_toward,
};

/// Ticks the tether tracks the target before it freezes (1.4 s).
pub const KITE_LOCK_TICKS: u32 = 28;
/// Ticks the frozen marker waits before detonating (0.4 s).
pub const KITE_STRIKE_TICKS: u32 = 8;
/// Ticks of recovery after a detonation (2.0 s).
pub const KITE_RECOVER_TICKS: u32 = 40;
/// Ticks of hunting between locks (1.0 s).
pub const KITE_HUNT_TICKS: u32 = 20;
/// Blast radius of the marker, units.
pub const KITE_BLAST_RADIUS: f32 = 3.5;
/// A Striker or Grappler this close to the Kite cuts the tether, units.
pub const KITE_CUT_RANGE: f32 = 3.0;
/// Height above the Kite at which the tether breaks, units.
pub const KITE_LOS_HEIGHT: f32 = 6.0;
/// Drift speed while hunting, units/s.
pub const KITE_HUNT_SPEED: f32 = 4.0;

/// Kite phases.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KitePhase {
    /// Picks the most isolated player.
    Hunt,
    /// The tether brightens and the marker tracks the target (tier-2 decal).
    Lock,
    /// The marker is frozen; the blast resolves at the end.
    Strike,
    /// Harmless drift after a detonation.
    Recover,
}

/// Kite brain state.
#[derive(Debug, Clone, PartialEq)]
pub struct KiteBrain {
    /// Current phase.
    pub phase: KitePhase,
    /// Phase countdown in ticks.
    pub timer: u32,
    /// The locked player.
    pub target_id: Option<u32>,
    /// The strike marker (blast centre).
    pub marker: Vec3,
}

impl Default for KiteBrain {
    fn default() -> Self {
        Self {
            phase: KitePhase::Hunt,
            timer: KITE_HUNT_TICKS,
            target_id: None,
            marker: Vec3::new(0.0, 0.0, 0.0),
        }
    }
}

impl KiteBrain {
    /// The tier-2 strike marker: the exact blast volume, drawn from lock to detonation.
    pub fn telegraph(&self) -> Option<Ring> {
        match self.phase {
            KitePhase::Lock | KitePhase::Strike => Some(Ring {
                centre: self.marker,
                radius: KITE_BLAST_RADIUS,
            }),
            KitePhase::Hunt | KitePhase::Recover => None,
        }
    }
}

/// Registry class interaction for Kite: Striker and Grappler both counter it.
pub fn relation(class: EnemyClass) -> ClassRelation {
    match class {
        EnemyClass::Striker | EnemyClass::Grappler => ClassRelation::Counters,
        EnemyClass::Bulwark | EnemyClass::Glider => ClassRelation::Neutral,
    }
}

/// Advance Kite by one 20 Hz tick.
pub fn step(brain: &mut KiteBrain, pos: &Vec3, params: &EnemyParams, ctx: &StepContext<'_>) -> EnemyAction {
    let mut action = EnemyAction::idle();
    // Blight synergy: while the marked player is infected, Kite's phase timer is paused.
    let paused = ctx
        .players
        .iter()
        .find(|p| Some(p.id) == brain.target_id)
        .is_some_and(|p| infection_pauses_timer(p.status.infected_ticks));

    match brain.phase {
        KitePhase::Hunt => {
            if let Some(p) = most_isolated_player(ctx.players) {
                action.motion = step_toward(pos, &p.feet, KITE_HUNT_SPEED * params.speed_mul * ENEMY_DT);
            }
            if !paused {
                brain.timer = brain.timer.saturating_sub(1);
            }
            if brain.timer == 0 {
                match most_isolated_player(ctx.players) {
                    Some(p) => {
                        brain.target_id = Some(p.id);
                        brain.marker = p.feet;
                        brain.phase = KitePhase::Lock;
                        brain.timer = KITE_LOCK_TICKS;
                    }
                    None => brain.timer = KITE_HUNT_TICKS,
                }
            }
        }
        KitePhase::Lock => {
            if let Some(p) = brain.target_id.and_then(|id| ctx.players.iter().find(|p| p.id == id)) {
                brain.marker = p.feet;
                if p.feet.y - pos.y > KITE_LOS_HEIGHT {
                    brain.phase = KitePhase::Recover;
                    brain.timer = KITE_RECOVER_TICKS;
                    return action;
                }
            }
            let cut = ctx.players.iter().any(|p| {
                matches!(p.class, EnemyClass::Striker | EnemyClass::Grappler)
                    && distance(&p.feet, pos) <= KITE_CUT_RANGE
            });
            if cut {
                brain.phase = KitePhase::Recover;
                brain.timer = KITE_RECOVER_TICKS;
                return action;
            }
            if !paused {
                brain.timer = brain.timer.saturating_sub(1);
            }
            if brain.timer == 0 {
                brain.phase = KitePhase::Strike;
                brain.timer = KITE_STRIKE_TICKS;
            }
        }
        KitePhase::Strike => {
            if !paused {
                brain.timer = brain.timer.saturating_sub(1);
            }
            if brain.timer == 0 {
                for p in ctx.players {
                    if distance(&p.feet, &brain.marker) <= KITE_BLAST_RADIUS {
                        action.hits.push(Hit {
                            player_id: p.id,
                            kind: HitKind::Lethal,
                        });
                    }
                }
                brain.phase = KitePhase::Recover;
                brain.timer = KITE_RECOVER_TICKS;
            }
        }
        KitePhase::Recover => {
            if !paused {
                brain.timer = brain.timer.saturating_sub(1);
            }
            if brain.timer == 0 {
                brain.phase = KitePhase::Hunt;
                brain.timer = KITE_HUNT_TICKS;
            }
        }
    }

    action
}

#[cfg(test)]
mod tests {
    // Tests build a brain then poke its phase/timer; that reads better than a
    // twelve-field struct literal at every call site.
    #![allow(clippy::field_reassign_with_default)]

    use super::*;
    use crate::rules::PlayerStatus;
    use crate::rules::enemies::PlayerSnapshot;

    fn params() -> EnemyParams {
        EnemyParams {
            behavior: crate::rules::BEHAVIOR_KITE,
            speed_mul: 1.0,
            size: 1.0,
            grace_ticks: 0,
        }
    }

    fn player(id: u32, x: f32, z: f32, class: EnemyClass) -> PlayerSnapshot {
        PlayerSnapshot {
            id,
            feet: Vec3::new(x, 0.0, z),
            class,
            status: PlayerStatus::default(),
            dashing: false,
        }
    }

    #[test]
    fn lock_lasts_the_tier_two_window() {
        let total = KITE_LOCK_TICKS + KITE_STRIKE_TICKS;
        assert!((20..=40).contains(&total), "tier 2 needs 1.0-2.0 s of decal");
    }

    #[test]
    fn locks_the_most_isolated_player() {
        let players = [
            player(1, 0.0, 0.0, EnemyClass::Bulwark),
            player(2, 1.0, 0.0, EnemyClass::Bulwark),
            player(3, 30.0, 0.0, EnemyClass::Bulwark),
        ];
        let mut brain = KiteBrain::default();
        brain.timer = 1;
        let pos = Vec3::new(0.0, 0.0, 0.0);
        let ctx = StepContext {
            tick: 0,
            players: &players,
            ..StepContext::default()
        };
        step(&mut brain, &pos, &params(), &ctx);
        assert_eq!(brain.phase, KitePhase::Lock);
        assert_eq!(brain.target_id, Some(3));
        assert!(brain.telegraph().is_some(), "the marker is up");
    }

    #[test]
    fn marker_tracks_then_detonates() {
        let mut brain = KiteBrain::default();
        brain.phase = KitePhase::Lock;
        brain.timer = 1;
        brain.target_id = Some(1);
        let pos = Vec3::new(0.0, 0.0, 0.0);
        let players = [player(1, 5.0, 0.0, EnemyClass::Bulwark)];

        let ctx = StepContext {
            tick: 0,
            players: &players,
            ..StepContext::default()
        };
        step(&mut brain, &pos, &params(), &ctx);
        assert_eq!(brain.marker, Vec3::new(5.0, 0.0, 0.0), "the marker tracks");
        assert_eq!(brain.phase, KitePhase::Strike);

        let mut action = EnemyAction::idle();
        for tick in 1..=KITE_STRIKE_TICKS {
            let ctx = StepContext {
                tick,
                players: &players,
                ..StepContext::default()
            };
            action = step(&mut brain, &pos, &params(), &ctx);
        }
        assert_eq!(action.hits.len(), 1);
        assert_eq!(action.hits[0].kind, HitKind::Lethal);
        assert_eq!(brain.phase, KitePhase::Recover);
    }

    #[test]
    fn striker_and_grappler_cut_the_tether() {
        for class in [EnemyClass::Striker, EnemyClass::Grappler] {
            let mut brain = KiteBrain::default();
            brain.phase = KitePhase::Lock;
            brain.timer = KITE_LOCK_TICKS;
            brain.target_id = Some(1);
            let pos = Vec3::new(0.0, 0.0, 0.0);
            let players = [player(1, 1.0, 0.0, class)];
            let ctx = StepContext {
                tick: 0,
                players: &players,
                ..StepContext::default()
            };
            let action = step(&mut brain, &pos, &params(), &ctx);
            assert_eq!(brain.phase, KitePhase::Recover, "the class cuts the line");
            assert!(action.hits.is_empty());
        }
    }

    #[test]
    fn blight_infection_pauses_the_phase_timer() {
        let mut brain = KiteBrain::default();
        brain.phase = KitePhase::Hunt;
        brain.timer = 10;
        brain.target_id = Some(1);
        let pos = Vec3::new(0.0, 0.0, 0.0);
        let mut infected = player(1, 2.0, 0.0, EnemyClass::Striker);
        infected.status.infected_ticks = 40;
        let ctx = StepContext {
            players: std::slice::from_ref(&infected),
            ..StepContext::default()
        };
        for _ in 0..5 {
            step(&mut brain, &pos, &params(), &ctx);
        }
        assert_eq!(brain.timer, 10, "the infection holds the timer still");

        let clean = player(1, 2.0, 0.0, EnemyClass::Striker);
        let ctx = StepContext {
            players: std::slice::from_ref(&clean),
            ..StepContext::default()
        };
        for _ in 0..5 {
            step(&mut brain, &pos, &params(), &ctx);
        }
        assert_eq!(brain.timer, 5, "a clean player lets the timer run");
    }
}
