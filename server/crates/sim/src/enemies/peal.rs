//! Peal: teleports next to the closest player every 6 s and rings three times.
//!
//! Tier-1 telegraph: the bell body opens for 0.6 s before each ring. The ring is
//! harmless unless the target already carries OVERTUNED, in which case it kills.
//! With `coil` permanent the final ring is lethal outright. A Bulwark is immune;
//! a Glider above the ring crosses over it.

use protocol::Vec3;

use crate::rules::{EnemyParams, distance, flat_direction};

use super::{ClassRelation, EnemyAction, EnemyClass, Hit, HitKind, Ring, StepContext, nearest_player};

/// Ticks between Peal cycles (6.0 s at 20 Hz).
pub const PEAL_CYCLE_TICKS: u32 = 120;
/// Ticks the bell body stays open before a ring (0.6 s; tier 1 needs 0.35-0.95 s).
pub const PEAL_CHIME_TICKS: u32 = 12;
/// Ticks of rest after the final ring (1.0 s).
pub const PEAL_REST_TICKS: u32 = 20;
/// Radii of the three rings, units; the third is the final one.
pub const PEAL_RING_RADII: [f32; 3] = [3.0, 4.0, 5.0];
/// Horizontal distance from the target at which Peal reappears, units.
pub const PEAL_TELEPORT_DISTANCE: f32 = 3.0;
/// A player this far above or below the ring centre is outside it, units.
pub const PEAL_RING_HEIGHT: f32 = 2.5;
/// Ticks of OVERTUNED one ring applies (6.0 s).
pub const PEAL_OVERTUNED_TICKS: u32 = 120;

/// Peal phases.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PealPhase {
    /// Waiting out the 6 s cycle.
    Idle,
    /// The bell body is open; the tier-1 chime telegraph.
    Chime,
    /// The ring resolved this tick.
    Ring,
    /// Harmless rest after the final ring.
    Rest,
}

/// Peal brain state.
#[derive(Debug, Clone, PartialEq)]
pub struct PealBrain {
    /// Current phase.
    pub phase: PealPhase,
    /// Phase countdown in ticks.
    pub timer: u32,
    /// Which of the three rings is being rung (0..=2).
    pub ring_index: usize,
    /// Centre of the ring being charged or resolved.
    pub centre: Vec3,
    /// The player Peal teleported next to.
    pub target_id: Option<u32>,
}

impl Default for PealBrain {
    fn default() -> Self {
        Self {
            phase: PealPhase::Idle,
            timer: PEAL_CYCLE_TICKS,
            ring_index: 0,
            centre: Vec3::new(0.0, 0.0, 0.0),
            target_id: None,
        }
    }
}

impl PealBrain {
    /// The tier-1 chime: the ring that will resolve, drawn while the bell body is open.
    pub fn telegraph(&self) -> Option<Ring> {
        match self.phase {
            PealPhase::Chime => Some(Ring {
                centre: self.centre,
                radius: PEAL_RING_RADII[self.ring_index],
            }),
            PealPhase::Idle | PealPhase::Ring | PealPhase::Rest => None,
        }
    }
}

/// Registry class interaction for Peal: Bulwark immune, Glider counters.
pub fn relation(class: EnemyClass) -> ClassRelation {
    match class {
        EnemyClass::Bulwark => ClassRelation::Immune,
        EnemyClass::Glider => ClassRelation::Counters,
        EnemyClass::Striker | EnemyClass::Grappler => ClassRelation::Neutral,
    }
}

/// Advance Peal by one 20 Hz tick.
pub fn step(brain: &mut PealBrain, pos: &Vec3, _params: &EnemyParams, ctx: &StepContext<'_>) -> EnemyAction {
    let mut action = EnemyAction::idle();

    match brain.phase {
        PealPhase::Idle => {
            brain.timer = brain.timer.saturating_sub(1);
            if brain.timer == 0 {
                let target = nearest_player(ctx.players, pos);
                brain.target_id = target.map(|p| p.id);
                let centre = match target {
                    Some(p) => {
                        let away = flat_direction(&p.feet, pos);
                        Vec3::new(
                            p.feet.x + away.x * PEAL_TELEPORT_DISTANCE,
                            p.feet.y,
                            p.feet.z + away.z * PEAL_TELEPORT_DISTANCE,
                        )
                    }
                    None => *pos,
                };
                brain.centre = centre;
                brain.ring_index = 0;
                brain.phase = PealPhase::Chime;
                brain.timer = PEAL_CHIME_TICKS;
                action.teleport = Some(centre);
            }
        }
        PealPhase::Chime => {
            brain.timer = brain.timer.saturating_sub(1);
            if brain.timer == 0 {
                let radius = PEAL_RING_RADII[brain.ring_index];
                let final_ring = brain.ring_index + 1 == PEAL_RING_RADII.len();
                for p in ctx.players {
                    if relation(p.class) == ClassRelation::Immune {
                        continue;
                    }
                    if (p.feet.y - brain.centre.y).abs() > PEAL_RING_HEIGHT {
                        continue;
                    }
                    if distance(&p.feet, &brain.centre) > radius {
                        continue;
                    }
                    let kind = if (ctx.coil_permanent && final_ring) || p.status.is_overtuned() {
                        HitKind::Lethal
                    } else {
                        HitKind::Overtuned
                    };
                    action.hits.push(Hit { player_id: p.id, kind });
                }
                brain.phase = PealPhase::Ring;
                brain.timer = 1;
            }
        }
        PealPhase::Ring => {
            brain.timer = brain.timer.saturating_sub(1);
            if brain.timer == 0 {
                if brain.ring_index + 1 < PEAL_RING_RADII.len() {
                    brain.ring_index += 1;
                    brain.phase = PealPhase::Chime;
                    brain.timer = PEAL_CHIME_TICKS;
                } else {
                    brain.phase = PealPhase::Rest;
                    brain.timer = PEAL_REST_TICKS;
                }
            }
        }
        PealPhase::Rest => {
            brain.timer = brain.timer.saturating_sub(1);
            if brain.timer == 0 {
                brain.phase = PealPhase::Idle;
                brain.timer = PEAL_CYCLE_TICKS;
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
            behavior: crate::rules::BEHAVIOR_PEAL,
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

    fn chime_brain(ring_index: usize) -> PealBrain {
        let mut brain = PealBrain::default();
        brain.phase = PealPhase::Chime;
        brain.timer = 1;
        brain.centre = Vec3::new(0.0, 0.0, 0.0);
        brain.ring_index = ring_index;
        brain
    }

    #[test]
    fn chime_lasts_the_tier_one_window() {
        assert!((7..=19).contains(&PEAL_CHIME_TICKS), "tier 1 needs 0.35-0.95 s of tell");
    }

    #[test]
    fn teleports_next_to_the_closest_player_and_chimes() {
        let players = [player(1, 10.0, 0.0, EnemyClass::Striker)];
        let mut brain = PealBrain::default();
        brain.timer = 1;
        let pos = Vec3::new(0.0, 0.0, 0.0);
        let ctx = StepContext {
            tick: 0,
            players: &players,
            ..StepContext::default()
        };
        let action = step(&mut brain, &pos, &params(), &ctx);
        assert_eq!(brain.phase, PealPhase::Chime);
        assert!(brain.telegraph().is_some(), "the chime is the tell");
        let centre = action.teleport.expect("peal teleports");
        let gap = distance(&centre, &Vec3::new(10.0, 0.0, 0.0));
        assert!((gap - PEAL_TELEPORT_DISTANCE).abs() < 1e-3);
    }

    #[test]
    fn ring_debuffs_a_clean_player() {
        let mut brain = chime_brain(0);
        let pos = Vec3::new(0.0, 0.0, 0.0);
        let players = [player(1, 1.0, 0.0, EnemyClass::Striker)];
        let ctx = StepContext {
            tick: 0,
            players: &players,
            ..StepContext::default()
        };
        let action = step(&mut brain, &pos, &params(), &ctx);
        assert_eq!(action.hits.len(), 1);
        assert_eq!(action.hits[0].kind, HitKind::Overtuned);
        assert_eq!(brain.phase, PealPhase::Ring);
    }

    #[test]
    fn ring_kills_an_overtuned_player() {
        let mut brain = chime_brain(0);
        let pos = Vec3::new(0.0, 0.0, 0.0);
        let mut p = player(1, 1.0, 0.0, EnemyClass::Striker);
        p.status.apply_overtuned(PlayerStatus::OVERTUNED_TICKS);
        let players = [p];
        let ctx = StepContext {
            tick: 0,
            players: &players,
            ..StepContext::default()
        };
        let action = step(&mut brain, &pos, &params(), &ctx);
        assert_eq!(action.hits.len(), 1);
        assert_eq!(action.hits[0].kind, HitKind::Lethal);
    }

    #[test]
    fn bulwark_is_immune_to_the_ring() {
        let mut brain = chime_brain(0);
        let pos = Vec3::new(0.0, 0.0, 0.0);
        let players = [player(1, 1.0, 0.0, EnemyClass::Bulwark)];
        let ctx = StepContext {
            tick: 0,
            players: &players,
            ..StepContext::default()
        };
        let action = step(&mut brain, &pos, &params(), &ctx);
        assert!(action.hits.is_empty(), "the ward ignores the ring");
    }

    #[test]
    fn glider_above_the_ring_is_safe() {
        let mut brain = chime_brain(0);
        let pos = Vec3::new(0.0, 0.0, 0.0);
        let mut p = player(1, 1.0, 0.0, EnemyClass::Glider);
        p.feet.y = 5.0;
        let players = [p];
        let ctx = StepContext {
            tick: 0,
            players: &players,
            ..StepContext::default()
        };
        let action = step(&mut brain, &pos, &params(), &ctx);
        assert!(action.hits.is_empty(), "the glider crosses above it");
    }

    #[test]
    fn final_ring_is_lethal_with_coil() {
        let mut brain = chime_brain(2);
        let pos = Vec3::new(0.0, 0.0, 0.0);
        let players = [player(1, 1.0, 0.0, EnemyClass::Striker)];
        let ctx = StepContext {
            tick: 0,
            players: &players,
            coil_permanent: true,
            ..StepContext::default()
        };
        let action = step(&mut brain, &pos, &params(), &ctx);
        assert_eq!(action.hits.len(), 1);
        assert_eq!(action.hits[0].kind, HitKind::Lethal);
    }
}
