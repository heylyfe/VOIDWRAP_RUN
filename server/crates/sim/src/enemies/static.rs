//! Static: fog-bound dasher that vanishes after every strike.
//!
//! Tier-3 telegraph: no per-attack tell. The fog patch around the body is the
//! persistent cue and it is also the hit volume, so a player can walk out of it
//! at any time. A Glider is never selected; a Bulwark is immune to the strike.

use protocol::Vec3;

use crate::rules::{EnemyParams, distance, flat_direction};

use super::{
    ClassRelation, ENEMY_DT, EnemyAction, EnemyClass, Hit, HitKind, Ring, StepContext, nearest_player_excluding,
    step_toward,
};

/// Ticks the fog zone grows before the dash (2.0 s of visible cue).
pub const STATIC_TRACK_TICKS: u32 = 40;
/// Ticks the dash lasts (0.4 s).
pub const STATIC_DASH_TICKS: u32 = 8;
/// Ticks the enemy stays invisible after a strike (1.0 s).
pub const STATIC_VANISH_TICKS: u32 = 20;
/// Fog radius at the start of a track, units.
pub const STATIC_FOG_MIN: f32 = 2.0;
/// Fog radius at the end of a track, units.
pub const STATIC_FOG_MAX: f32 = 5.0;
/// Dash speed, units/s.
pub const STATIC_DASH_SPEED: f32 = 18.0;
/// Track drift speed, units/s.
pub const STATIC_TRACK_SPEED: f32 = 3.0;
/// Distance from the next target at which Static reappears, units.
pub const STATIC_TELEPORT_DISTANCE: f32 = 4.0;

/// Static phases.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StaticPhase {
    /// Fog thickens around it while it walks the target down (the tier-3 cue).
    Track,
    /// Committed dash at the locked point.
    Dash,
    /// The strike resolves; only players still inside the fog are hit.
    Strike,
    /// Invisible and harmless; teleports to the next target at the end.
    Vanish,
}

/// Static brain state.
#[derive(Debug, Clone, PartialEq)]
pub struct StaticBrain {
    /// Current phase.
    pub phase: StaticPhase,
    /// Phase countdown in ticks.
    pub timer: u32,
    /// The player being tracked.
    pub target_id: Option<u32>,
    /// Centre of the fog patch.
    pub fog_centre: Vec3,
    /// Radius of the fog patch this tick.
    pub fog_radius: f32,
    /// Point the dash commits to.
    pub locked: Vec3,
    /// True while the body is invisible to clients.
    pub hidden: bool,
}

impl Default for StaticBrain {
    fn default() -> Self {
        Self {
            phase: StaticPhase::Track,
            timer: STATIC_TRACK_TICKS,
            target_id: None,
            fog_centre: Vec3::new(0.0, 0.0, 0.0),
            fog_radius: STATIC_FOG_MIN,
            locked: Vec3::new(0.0, 0.0, 0.0),
            hidden: false,
        }
    }
}

impl StaticBrain {
    /// The tier-3 ambient cue: the fog patch around the body, gone while it is away.
    pub fn telegraph(&self) -> Option<Ring> {
        if self.hidden {
            None
        } else {
            Some(Ring {
                centre: self.fog_centre,
                radius: self.fog_radius,
            })
        }
    }
}

/// Registry class interaction for Static: Glider ignores, Bulwark immune.
pub fn relation(class: EnemyClass) -> ClassRelation {
    match class {
        EnemyClass::Glider => ClassRelation::Ignores,
        EnemyClass::Bulwark => ClassRelation::Immune,
        EnemyClass::Striker | EnemyClass::Grappler => ClassRelation::Neutral,
    }
}

/// Advance Static by one 20 Hz tick.
pub fn step(brain: &mut StaticBrain, pos: &Vec3, params: &EnemyParams, ctx: &StepContext<'_>) -> EnemyAction {
    let mut action = EnemyAction::idle();

    match brain.phase {
        StaticPhase::Track => {
            brain.hidden = false;
            let target = nearest_player_excluding(ctx.players, pos, Some(EnemyClass::Glider), None);
            brain.target_id = target.map(|p| p.id);
            if let Some(p) = target {
                action.motion = step_toward(pos, &p.feet, STATIC_TRACK_SPEED * params.speed_mul * ENEMY_DT);
            }
            brain.fog_centre = *pos;
            let span = (STATIC_TRACK_TICKS - 1) as f32;
            let elapsed = (STATIC_TRACK_TICKS - brain.timer) as f32;
            brain.fog_radius = STATIC_FOG_MIN + (STATIC_FOG_MAX - STATIC_FOG_MIN) * (elapsed / span);
            brain.timer = brain.timer.saturating_sub(1);
            if brain.timer == 0 {
                brain.locked = target.map_or(*pos, |p| p.feet);
                brain.phase = StaticPhase::Dash;
                brain.timer = STATIC_DASH_TICKS;
            }
        }
        StaticPhase::Dash => {
            action.motion = step_toward(pos, &brain.locked, STATIC_DASH_SPEED * params.speed_mul * ENEMY_DT);
            brain.fog_centre = *pos;
            brain.fog_radius = STATIC_FOG_MAX;
            brain.timer = brain.timer.saturating_sub(1);
            if brain.timer == 0 {
                brain.phase = StaticPhase::Strike;
                brain.timer = 1;
            }
        }
        StaticPhase::Strike => {
            for p in ctx.players {
                if relation(p.class) == ClassRelation::Immune {
                    continue;
                }
                if distance(&p.feet, &brain.fog_centre) <= brain.fog_radius {
                    action.hits.push(Hit {
                        player_id: p.id,
                        kind: HitKind::Lethal,
                    });
                }
            }
            brain.phase = StaticPhase::Vanish;
            brain.timer = STATIC_VANISH_TICKS;
            brain.hidden = true;
        }
        StaticPhase::Vanish => {
            brain.hidden = true;
            brain.timer = brain.timer.saturating_sub(1);
            if brain.timer == 0 {
                let next = nearest_player_excluding(ctx.players, pos, Some(EnemyClass::Glider), brain.target_id)
                    .or_else(|| nearest_player_excluding(ctx.players, pos, Some(EnemyClass::Glider), None));
                let centre = match next {
                    Some(p) => {
                        let away = flat_direction(&p.feet, pos);
                        Vec3::new(
                            p.feet.x + away.x * STATIC_TELEPORT_DISTANCE,
                            p.feet.y,
                            p.feet.z + away.z * STATIC_TELEPORT_DISTANCE,
                        )
                    }
                    None => *pos,
                };
                brain.fog_centre = centre;
                brain.fog_radius = STATIC_FOG_MIN;
                brain.target_id = next.map(|p| p.id);
                brain.phase = StaticPhase::Track;
                brain.timer = STATIC_TRACK_TICKS;
                brain.hidden = false;
                action.teleport = Some(centre);
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
            behavior: crate::rules::BEHAVIOR_STATIC,
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
    fn fog_cue_is_up_for_the_whole_track() {
        const { assert!(STATIC_TRACK_TICKS >= 20, "tier 3 wants a cue the player can read") };
        let players = [player(1, 3.0, 0.0, EnemyClass::Striker)];
        let mut brain = StaticBrain::default();
        let pos = Vec3::new(0.0, 0.0, 0.0);
        for tick in 0..STATIC_TRACK_TICKS {
            let ctx = StepContext {
                tick,
                players: &players,
                ..StepContext::default()
            };
            step(&mut brain, &pos, &params(), &ctx);
            assert!(brain.telegraph().is_some(), "the fog cue never blinks out");
        }
    }

    #[test]
    fn tracks_then_dashes_and_strikes() {
        let players = [player(1, 3.0, 0.0, EnemyClass::Striker)];
        let mut brain = StaticBrain::default();
        let pos = Vec3::new(0.0, 0.0, 0.0);
        let mut tick = 0_u32;

        for _ in 0..STATIC_TRACK_TICKS {
            let ctx = StepContext {
                tick,
                players: &players,
                ..StepContext::default()
            };
            step(&mut brain, &pos, &params(), &ctx);
            tick += 1;
        }
        assert_eq!(brain.phase, StaticPhase::Dash);
        assert!(brain.fog_radius >= STATIC_FOG_MAX - 1e-3);

        for _ in 0..STATIC_DASH_TICKS {
            let ctx = StepContext {
                tick,
                players: &players,
                ..StepContext::default()
            };
            step(&mut brain, &pos, &params(), &ctx);
            tick += 1;
        }
        assert_eq!(brain.phase, StaticPhase::Strike);

        let ctx = StepContext {
            tick,
            players: &players,
            ..StepContext::default()
        };
        let action = step(&mut brain, &pos, &params(), &ctx);
        assert_eq!(action.hits.len(), 1);
        assert_eq!(action.hits[0].kind, HitKind::Lethal);
        assert_eq!(brain.phase, StaticPhase::Vanish);
        assert!(brain.telegraph().is_none(), "it is invisible while it is away");
    }

    #[test]
    fn glider_is_never_targeted() {
        let players = [
            player(1, 1.0, 0.0, EnemyClass::Glider),
            player(2, 8.0, 0.0, EnemyClass::Striker),
        ];
        let mut brain = StaticBrain::default();
        let pos = Vec3::new(0.0, 0.0, 0.0);
        let ctx = StepContext {
            tick: 0,
            players: &players,
            ..StepContext::default()
        };
        step(&mut brain, &pos, &params(), &ctx);
        assert_eq!(brain.target_id, Some(2), "the glider is silent");
    }

    #[test]
    fn bulwark_is_immune_to_the_strike() {
        let players = [player(1, 3.0, 0.0, EnemyClass::Bulwark)];
        let mut brain = StaticBrain::default();
        brain.phase = StaticPhase::Strike;
        brain.fog_centre = Vec3::new(0.0, 0.0, 0.0);
        brain.fog_radius = STATIC_FOG_MAX;
        let pos = Vec3::new(0.0, 0.0, 0.0);
        let ctx = StepContext {
            tick: 0,
            players: &players,
            ..StepContext::default()
        };
        let action = step(&mut brain, &pos, &params(), &ctx);
        assert!(action.hits.is_empty(), "the ward grounds it");
    }

    #[test]
    fn vanishes_and_switches_to_the_next_target() {
        let players = [
            player(1, 2.0, 0.0, EnemyClass::Striker),
            player(2, 9.0, 0.0, EnemyClass::Striker),
        ];
        let mut brain = StaticBrain::default();
        brain.phase = StaticPhase::Vanish;
        brain.timer = 1;
        brain.hidden = true;
        brain.target_id = Some(1);
        let pos = Vec3::new(0.0, 0.0, 0.0);
        let ctx = StepContext {
            tick: 0,
            players: &players,
            ..StepContext::default()
        };
        let action = step(&mut brain, &pos, &params(), &ctx);
        assert_eq!(brain.phase, StaticPhase::Track);
        assert_eq!(brain.target_id, Some(2), "it switches to the next-closest");
        assert!(action.teleport.is_some());
        assert!(!brain.hidden);
    }
}
