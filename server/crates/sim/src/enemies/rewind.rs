//! Rewind: replays the nearest player's path from 1.5 s ago.
//!
//! Tier-2 telegraph: the body trembles in place for 1.5 s with a floor mark on
//! the exact hit volume, then it starts moving. Touching it is lethal. A Striker
//! dash breaks the drag; a Grappler is immune.

use protocol::Vec3;

use crate::rules::{EnemyParams, distance, player_centre};

use super::{
    ClassRelation, ENEMY_DT, EnemyAction, EnemyClass, Hit, HitKind, Ring, StepContext, nearest_player, step_toward,
};

/// Ticks the tremble telegraph lasts (1.5 s at 20 Hz; tier 2 needs 1.0-2.0 s).
pub const REWIND_TREMBLE_TICKS: u32 = 30;
/// Ticks the replay chase lasts before the enemy fades (4.0 s).
pub const REWIND_REPLAY_TICKS: u32 = 80;
/// Ticks the harmless fade lasts (1.0 s).
pub const REWIND_FADE_TICKS: u32 = 20;
/// Player positions the brain remembers (1.5 s of history).
pub const REWIND_TRAIL_TICKS: usize = 30;
/// Replay speed, units/s.
pub const REWIND_SPEED: f32 = 7.0;
/// Fade drift speed, units/s.
pub const REWIND_FADE_SPEED: f32 = 2.0;
/// Tremble amplitude, units.
pub const REWIND_TREMBLE_AMPLITUDE: f32 = 0.25;
/// Ticks per tremble half-cycle (0.15 s).
pub const REWIND_TREMBLE_PERIOD: u32 = 3;
/// Radius around the enemy centre that kills on contact, units.
pub const REWIND_TOUCH_RADIUS: f32 = 0.9;

/// Rewind phases.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RewindPhase {
    /// Shivers in place; the tier-2 floor mark sits on the body.
    Tremble,
    /// Follows the target's recorded path 1.5 s behind; lethal on contact.
    Replay,
    /// Harmless drift back toward the last recorded point.
    Fade,
}

/// Rewind brain state.
#[derive(Debug, Clone, PartialEq)]
pub struct RewindBrain {
    /// Current phase.
    pub phase: RewindPhase,
    /// Phase countdown in ticks.
    pub timer: u32,
    /// The player whose path is being replayed.
    pub target_id: Option<u32>,
    /// Recorded target feet positions, oldest first.
    pub trail: Vec<Vec3>,
    /// Where the tier-2 floor mark is drawn.
    pub mark: Vec3,
}

impl Default for RewindBrain {
    fn default() -> Self {
        Self {
            phase: RewindPhase::Tremble,
            timer: REWIND_TREMBLE_TICKS,
            target_id: None,
            trail: Vec::new(),
            mark: Vec3::new(0.0, 0.0, 0.0),
        }
    }
}

impl RewindBrain {
    /// The tier-2 floor mark: the exact hit volume, drawn for the whole tremble.
    pub fn telegraph(&self) -> Option<Ring> {
        match self.phase {
            RewindPhase::Tremble => Some(Ring {
                centre: self.mark,
                radius: REWIND_TOUCH_RADIUS,
            }),
            RewindPhase::Replay | RewindPhase::Fade => None,
        }
    }
}

/// Registry class interaction for Rewind: Striker counters, Grappler immune.
pub fn relation(class: EnemyClass) -> ClassRelation {
    match class {
        EnemyClass::Striker => ClassRelation::Counters,
        EnemyClass::Grappler => ClassRelation::Immune,
        EnemyClass::Bulwark | EnemyClass::Glider => ClassRelation::Neutral,
    }
}

/// Advance Rewind by one 20 Hz tick.
///
/// Contact is tested against `pos` as it was at the start of the tick; the server
/// applies [`EnemyAction::motion`] afterwards.
pub fn step(brain: &mut RewindBrain, pos: &Vec3, params: &EnemyParams, ctx: &StepContext<'_>) -> EnemyAction {
    let mut action = EnemyAction::idle();

    let target = nearest_player(ctx.players, pos);
    brain.target_id = target.map(|p| p.id);
    if let Some(p) = target {
        brain.trail.push(p.feet);
        if brain.trail.len() > REWIND_TRAIL_TICKS {
            brain.trail.remove(0);
        }
    }

    match brain.phase {
        RewindPhase::Tremble => {
            let sign = if (ctx.tick / REWIND_TREMBLE_PERIOD).is_multiple_of(2) {
                1.0
            } else {
                -1.0
            };
            action.motion = Vec3::new(sign * REWIND_TREMBLE_AMPLITUDE, 0.0, sign * REWIND_TREMBLE_AMPLITUDE);
            brain.mark = *pos;
            brain.timer = brain.timer.saturating_sub(1);
            if brain.timer == 0 {
                brain.phase = RewindPhase::Replay;
                brain.timer = REWIND_REPLAY_TICKS;
            }
        }
        RewindPhase::Replay => {
            if let Some(goal) = brain.trail.first().copied() {
                brain.mark = goal;
                action.motion = step_toward(pos, &goal, REWIND_SPEED * params.speed_mul * ENEMY_DT);
            }
            for p in ctx.players {
                if relation(p.class) == ClassRelation::Immune {
                    continue;
                }
                if p.class == EnemyClass::Striker && p.dashing {
                    continue;
                }
                if distance(&player_centre(&p.feet), pos) <= REWIND_TOUCH_RADIUS + params.size {
                    action.hits.push(Hit {
                        player_id: p.id,
                        kind: HitKind::Lethal,
                    });
                }
            }
            brain.timer = brain.timer.saturating_sub(1);
            if brain.timer == 0 {
                brain.phase = RewindPhase::Fade;
                brain.timer = REWIND_FADE_TICKS;
            }
        }
        RewindPhase::Fade => {
            action.motion = step_toward(pos, &brain.mark, REWIND_FADE_SPEED * ENEMY_DT);
            brain.timer = brain.timer.saturating_sub(1);
            if brain.timer == 0 {
                brain.phase = RewindPhase::Tremble;
                brain.timer = REWIND_TREMBLE_TICKS;
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
            behavior: crate::rules::BEHAVIOR_REWIND,
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
    fn tremble_lasts_the_tier_two_minimum() {
        assert!(
            (20..=40).contains(&REWIND_TREMBLE_TICKS),
            "tier 2 needs 1.0-2.0 s of decal"
        );
    }

    #[test]
    fn trembles_before_it_moves() {
        let players = [player(1, 10.0, 0.0, EnemyClass::Bulwark)];
        let mut brain = RewindBrain::default();
        let mut pos = Vec3::new(0.0, 0.0, 0.0);
        for tick in 0..REWIND_TREMBLE_TICKS - 1 {
            let ctx = StepContext {
                tick,
                players: &players,
                ..StepContext::default()
            };
            let action = step(&mut brain, &pos, &params(), &ctx);
            assert_eq!(brain.phase, RewindPhase::Tremble);
            assert!(brain.telegraph().is_some(), "the floor mark is up");
            assert!(action.motion.x.abs() <= REWIND_TREMBLE_AMPLITUDE + 1e-4);
            pos.x += action.motion.x;
            pos.z += action.motion.z;
        }
        let ctx = StepContext {
            tick: REWIND_TREMBLE_TICKS,
            players: &players,
            ..StepContext::default()
        };
        step(&mut brain, &pos, &params(), &ctx);
        assert_eq!(brain.phase, RewindPhase::Replay);
        assert!(brain.telegraph().is_none(), "the mark is removed on resolve");
    }

    #[test]
    fn replays_the_delayed_trail() {
        let mut brain = RewindBrain::default();
        let mut pos = Vec3::new(0.0, 0.0, 0.0);
        let mut tick = 0_u32;
        while brain.phase == RewindPhase::Tremble {
            let players = [player(1, 20.0 + tick as f32 * 0.5, 0.0, EnemyClass::Bulwark)];
            let ctx = StepContext {
                tick,
                players: &players,
                ..StepContext::default()
            };
            let action = step(&mut brain, &pos, &params(), &ctx);
            pos.x += action.motion.x;
            pos.z += action.motion.z;
            tick += 1;
        }
        assert_eq!(brain.phase, RewindPhase::Replay);
        assert_eq!(brain.trail.len(), REWIND_TRAIL_TICKS);

        let players = [player(1, 100.0, 0.0, EnemyClass::Bulwark)];
        let ctx = StepContext {
            tick,
            players: &players,
            ..StepContext::default()
        };
        let action = step(&mut brain, &pos, &params(), &ctx);
        assert!(action.motion.x > 0.0, "replay walks the recorded path");
        assert!(brain.mark.x < 100.0, "the mark is the delayed point");
    }

    #[test]
    fn grappler_is_immune_and_striker_dash_breaks_the_drag() {
        let pos = Vec3::new(0.0, 0.0, 0.0);

        let mut brain = RewindBrain::default();
        brain.phase = RewindPhase::Replay;
        brain.timer = REWIND_REPLAY_TICKS;
        let grappler = [player(1, 0.0, 0.0, EnemyClass::Grappler)];
        let ctx = StepContext {
            tick: 0,
            players: &grappler,
            ..StepContext::default()
        };
        let action = step(&mut brain, &pos, &params(), &ctx);
        assert!(action.hits.is_empty(), "the grapple anchors");

        let striker = [player(2, 0.0, 0.0, EnemyClass::Striker)];
        let ctx = StepContext {
            tick: 0,
            players: &striker,
            ..StepContext::default()
        };
        let action = step(&mut brain, &pos, &params(), &ctx);
        assert_eq!(action.hits.len(), 1);
        assert_eq!(action.hits[0].kind, HitKind::Lethal);

        let mut dashing = player(3, 0.0, 0.0, EnemyClass::Striker);
        dashing.dashing = true;
        let striker_dash = [dashing];
        let ctx = StepContext {
            tick: 0,
            players: &striker_dash,
            ..StepContext::default()
        };
        let action = step(&mut brain, &pos, &params(), &ctx);
        assert!(action.hits.is_empty(), "a striker dash breaks the drag");
    }
}
