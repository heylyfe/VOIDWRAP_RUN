//! Warden: guards the densest gift cluster and lunges at whoever takes a gift.
//!
//! Tier-2 telegraph: a ring around the guarded gift for 1.5 s before its reach
//! extends to the ring edge. Standing inside the ring when it closes is lethal.
//! A Grappler pulls the gift out of the ring; a Bulwark is immune.

use protocol::{Gift, Vec3};

use crate::rules::{EnemyParams, distance};

use super::{
    ClassRelation, ENEMY_DT, EnemyAction, EnemyClass, Hit, HitKind, Ring, StepContext, nearest_player, step_toward,
};

/// Ticks the guard ring winds up (1.5 s at 20 Hz; tier 2 needs 1.0-2.0 s).
pub const WARDEN_WIND_TICKS: u32 = 30;
/// Ticks the guard ring winds up during the collapse phase (1.0 s, the tier-2 floor).
pub const WARDEN_FRENZY_WIND_TICKS: u32 = 20;
/// Ticks the lunge lasts (0.5 s).
pub const WARDEN_LUNGE_TICKS: u32 = 10;
/// Ticks the recovery lasts (1.0 s).
pub const WARDEN_RECOVER_TICKS: u32 = 20;
/// Patrol speed, units/s.
pub const WARDEN_PATROL_SPEED: f32 = 3.5;
/// Frenzied patrol speed, units/s.
pub const WARDEN_FRENZY_SPEED: f32 = 5.5;
/// Lunge speed, units/s.
pub const WARDEN_LUNGE_SPEED: f32 = 14.0;
/// Radius used to count gifts into one cluster, units.
pub const WARDEN_CLUSTER_RADIUS: f32 = 6.0;
/// Radius of the guard ring, units.
pub const WARDEN_RING_RADIUS: f32 = 4.0;
/// A Grappler this close to the ring centre pulls the gift out, units.
pub const WARDEN_GRAPPLE_REACH: f32 = 6.0;

/// Warden phases.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WardenPhase {
    /// Patrols the densest gift cluster.
    Patrol,
    /// The guard ring winds up; the ring is the tier-2 decal.
    Wind,
    /// Committed lunge at the locked point.
    Lunge,
    /// Harmless drift back toward the cluster.
    Recover,
}

/// Warden brain state.
#[derive(Debug, Clone, PartialEq)]
pub struct WardenBrain {
    /// Current phase.
    pub phase: WardenPhase,
    /// Phase countdown in ticks.
    pub timer: u32,
    /// Position of the gift currently guarded.
    pub guarded: Option<Vec3>,
    /// Centre of the guard ring (the guarded gift's last known position).
    pub ring_centre: Vec3,
    /// The player that took the guarded gift.
    pub target_id: Option<u32>,
    /// Point the lunge commits to.
    pub locked: Vec3,
}

impl Default for WardenBrain {
    fn default() -> Self {
        Self {
            phase: WardenPhase::Patrol,
            timer: 0,
            guarded: None,
            ring_centre: Vec3::new(0.0, 0.0, 0.0),
            target_id: None,
            locked: Vec3::new(0.0, 0.0, 0.0),
        }
    }
}

impl WardenBrain {
    /// The tier-2 guard ring, drawn for the whole wind-up and removed on resolve.
    pub fn telegraph(&self) -> Option<Ring> {
        match self.phase {
            WardenPhase::Wind => Some(Ring {
                centre: self.ring_centre,
                radius: WARDEN_RING_RADIUS,
            }),
            WardenPhase::Patrol | WardenPhase::Lunge | WardenPhase::Recover => None,
        }
    }
}

/// Registry class interaction for Warden: Grappler counters, Bulwark immune.
pub fn relation(class: EnemyClass) -> ClassRelation {
    match class {
        EnemyClass::Grappler => ClassRelation::Counters,
        EnemyClass::Bulwark => ClassRelation::Immune,
        EnemyClass::Striker | EnemyClass::Glider => ClassRelation::Neutral,
    }
}

/// The gift with the most neighbours inside [`WARDEN_CLUSTER_RADIUS`]; ties keep content order.
fn densest_gift(gifts: &[Gift]) -> Option<Vec3> {
    let mut best: Option<Vec3> = None;
    let mut best_count = 0_usize;
    for g in gifts {
        let mut count = 0_usize;
        for other in gifts {
            if distance(&g.pos, &other.pos) <= WARDEN_CLUSTER_RADIUS {
                count += 1;
            }
        }
        if count > best_count {
            best_count = count;
            best = Some(g.pos);
        }
    }
    best
}

/// Advance Warden by one 20 Hz tick.
pub fn step(brain: &mut WardenBrain, pos: &Vec3, params: &EnemyParams, ctx: &StepContext<'_>) -> EnemyAction {
    let mut action = EnemyAction::idle();
    let speed = if ctx.collapse {
        WARDEN_FRENZY_SPEED
    } else {
        WARDEN_PATROL_SPEED
    } * params.speed_mul;

    match brain.phase {
        WardenPhase::Patrol => {
            if brain.guarded.is_none() {
                brain.guarded = densest_gift(ctx.gifts);
            }
            if let Some(guarded) = brain.guarded {
                let still_there = ctx.gifts.iter().any(|g| distance(&g.pos, &guarded) <= 0.01);
                if !still_there {
                    brain.ring_centre = guarded;
                    brain.target_id = nearest_player(ctx.players, &guarded).map(|p| p.id);
                    brain.phase = WardenPhase::Wind;
                    brain.timer = if ctx.collapse {
                        WARDEN_FRENZY_WIND_TICKS
                    } else {
                        WARDEN_WIND_TICKS
                    };
                    return action;
                }
                action.motion = step_toward(pos, &guarded, speed * ENEMY_DT);
            }
        }
        WardenPhase::Wind => {
            let pulled = ctx.players.iter().any(|p| {
                p.class == EnemyClass::Grappler && distance(&p.feet, &brain.ring_centre) <= WARDEN_GRAPPLE_REACH
            });
            if pulled {
                brain.phase = WardenPhase::Patrol;
                brain.guarded = None;
                brain.timer = 0;
                return action;
            }
            brain.timer = brain.timer.saturating_sub(1);
            if brain.timer == 0 {
                for p in ctx.players {
                    if relation(p.class) == ClassRelation::Immune {
                        continue;
                    }
                    if distance(&p.feet, &brain.ring_centre) <= WARDEN_RING_RADIUS {
                        action.hits.push(Hit {
                            player_id: p.id,
                            kind: HitKind::Lethal,
                        });
                    }
                }
                brain.locked = match brain.target_id {
                    Some(id) => ctx
                        .players
                        .iter()
                        .find(|p| p.id == id)
                        .map_or(brain.ring_centre, |p| p.feet),
                    None => brain.ring_centre,
                };
                brain.phase = WardenPhase::Lunge;
                brain.timer = WARDEN_LUNGE_TICKS;
            }
        }
        WardenPhase::Lunge => {
            action.motion = step_toward(pos, &brain.locked, WARDEN_LUNGE_SPEED * params.speed_mul * ENEMY_DT);
            brain.timer = brain.timer.saturating_sub(1);
            if brain.timer == 0 {
                brain.phase = WardenPhase::Recover;
                brain.timer = WARDEN_RECOVER_TICKS;
            }
        }
        WardenPhase::Recover => {
            action.motion = step_toward(pos, &brain.ring_centre, speed * ENEMY_DT);
            brain.timer = brain.timer.saturating_sub(1);
            if brain.timer == 0 {
                brain.phase = WardenPhase::Patrol;
                brain.guarded = None;
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
            behavior: crate::rules::BEHAVIOR_WARDEN,
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

    fn gift(x: f32, z: f32) -> Gift {
        Gift {
            id: 0,
            pos: Vec3::new(x, 1.0, z),
            kind: protocol::GIFT_NORMAL,
        }
    }

    #[test]
    fn wind_lasts_the_tier_two_minimum() {
        assert!(
            (20..=40).contains(&WARDEN_WIND_TICKS),
            "tier 2 needs 1.0-2.0 s of decal"
        );
        assert!(
            (20..=40).contains(&WARDEN_FRENZY_WIND_TICKS),
            "the frenzy windup still honours tier 2"
        );
        const { assert!(WARDEN_FRENZY_WIND_TICKS < WARDEN_WIND_TICKS) };
    }

    #[test]
    fn patrols_the_densest_gift_cluster() {
        let gifts = [gift(0.0, 0.0), gift(1.0, 0.0), gift(20.0, 0.0)];
        let players = [player(1, 0.0, 0.0, EnemyClass::Bulwark)];
        let mut brain = WardenBrain::default();
        let pos = Vec3::new(0.0, 0.0, 0.0);
        let ctx = StepContext {
            tick: 0,
            players: &players,
            gifts: &gifts,
            ..StepContext::default()
        };
        step(&mut brain, &pos, &params(), &ctx);
        assert_eq!(brain.phase, WardenPhase::Patrol);
        assert_eq!(brain.guarded, Some(Vec3::new(0.0, 1.0, 0.0)));
    }

    #[test]
    fn winds_up_when_the_guarded_gift_is_taken() {
        let gifts = [gift(0.0, 0.0)];
        let players = [player(1, 0.0, 0.0, EnemyClass::Bulwark)];
        let mut brain = WardenBrain::default();
        let pos = Vec3::new(0.0, 0.0, 0.0);
        let ctx = StepContext {
            tick: 0,
            players: &players,
            gifts: &gifts,
            ..StepContext::default()
        };
        step(&mut brain, &pos, &params(), &ctx);
        assert_eq!(brain.phase, WardenPhase::Patrol);

        let empty: [Gift; 0] = [];
        let ctx = StepContext {
            tick: 1,
            players: &players,
            gifts: &empty,
            ..StepContext::default()
        };
        step(&mut brain, &pos, &params(), &ctx);
        assert_eq!(brain.phase, WardenPhase::Wind);
        assert_eq!(brain.timer, WARDEN_WIND_TICKS);
        assert!(brain.telegraph().is_some(), "the ring is up");
    }

    #[test]
    fn grappler_pulls_the_gift_out_of_the_ring() {
        let mut brain = WardenBrain::default();
        brain.phase = WardenPhase::Wind;
        brain.timer = WARDEN_WIND_TICKS;
        brain.ring_centre = Vec3::new(0.0, 1.0, 0.0);
        let pos = Vec3::new(0.0, 0.0, 0.0);

        let grappler = [player(1, 1.0, 0.0, EnemyClass::Grappler)];
        let ctx = StepContext {
            tick: 0,
            players: &grappler,
            ..StepContext::default()
        };
        let action = step(&mut brain, &pos, &params(), &ctx);
        assert_eq!(brain.phase, WardenPhase::Patrol);
        assert!(action.hits.is_empty());
        assert!(brain.telegraph().is_none());
    }

    #[test]
    fn bulwark_is_immune_to_the_ring() {
        let pos = Vec3::new(0.0, 0.0, 0.0);

        let mut brain = WardenBrain::default();
        brain.phase = WardenPhase::Wind;
        brain.timer = 1;
        brain.ring_centre = Vec3::new(0.0, 0.0, 0.0);
        let bulwark = [player(1, 0.0, 0.0, EnemyClass::Bulwark)];
        let ctx = StepContext {
            tick: 0,
            players: &bulwark,
            ..StepContext::default()
        };
        let action = step(&mut brain, &pos, &params(), &ctx);
        assert!(action.hits.is_empty(), "the ward shoves the ring off its post");
        assert_eq!(brain.phase, WardenPhase::Lunge);

        let mut brain = WardenBrain::default();
        brain.phase = WardenPhase::Wind;
        brain.timer = 1;
        brain.ring_centre = Vec3::new(0.0, 0.0, 0.0);
        let striker = [player(2, 0.0, 0.0, EnemyClass::Striker)];
        let ctx = StepContext {
            tick: 0,
            players: &striker,
            ..StepContext::default()
        };
        let action = step(&mut brain, &pos, &params(), &ctx);
        assert_eq!(action.hits.len(), 1);
        assert_eq!(action.hits[0].kind, HitKind::Lethal);
    }
}
