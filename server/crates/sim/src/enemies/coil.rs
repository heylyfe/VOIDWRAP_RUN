//! Coil — the winding shockwave leap (cue tier 1, min level 4, cap 2).
//! Deliberate pair with Peal via the OVERTUNED debuff.
//!
//! Coils in place, winds up for 0.8 s ([`COIL_WIND_TICKS`]) while a player
//! stays near, then releases a radial shockwave that inflicts OVERTUNED (the
//! debuff that makes Peal's final ring lethal instead of a debuff — registry
//! synergy hook `peal_lethal_when_overtuned`). Afterwards it recovers and
//! leaps to a random map spot (deterministic RNG — see NOTES).
//!
//! The longer a player stays near while it winds, the tighter it coils: every
//! [`COIL_TIGHTEN_INTERVAL_TICKS`] of continuous proximity shaves
//! [`COIL_TIGHTEN_BONUS_TICKS`] off the remaining wind. The wind never drops
//! below [`COIL_MIN_WIND_TICKS`], so the tier-1 wind-up promise holds even
//! under tightening.
//!
//! Glider is never attracted (registry relation `glider: ignores`): gliders do
//! not trigger the wind-up and are not chased. Striker dashes out (escape by
//! leaving the radius before the release).

use protocol::Vec3;

use super::{SIM_DT, TickContext, nearest_player_where, seed_from_pos, unit_f32};
use crate::rules::{horizontal_distance, move_toward};

/// Aggro radius for the slow drift toward a player (units).
pub const COIL_AGGRO_RADIUS: f32 = 18.0;
/// Drift speed while coiled (u/s).
pub const COIL_DRIFT_SPEED: f32 = 1.8;
/// Radius inside which staying near starts the wind-up (units).
pub const COIL_TAUNT_RADIUS: f32 = 4.0;
/// Wind-up duration, in ticks (0.8 s at 60 Hz; tier 1).
pub const COIL_WIND_TICKS: u32 = 48;
/// Hard floor for the wind-up under tightening pressure (0.35 s; tier-1 minimum).
pub const COIL_MIN_WIND_TICKS: u32 = 21;
/// Continuous-proximity cadence for tightening (20 ticks ≈ 0.33 s).
///
/// A full "every second" cadence cannot fit inside the frozen 0.8 s tier-1
/// wind; the cadence is tightened so the mechanic exists at all. The tier-1
/// floor is preserved regardless.
pub const COIL_TIGHTEN_INTERVAL_TICKS: u32 = 20;
/// Ticks removed from the remaining wind per tighten step.
pub const COIL_TIGHTEN_BONUS_TICKS: u32 = 8;
/// Shockwave radius (units).
pub const COIL_SHOCKWAVE_RADIUS: f32 = 5.0;
/// Recovery pause before the leap (1.0 s at 60 Hz).
pub const COIL_RECOVER_TICKS: u32 = 60;
/// Airborne leap duration (0.8 s at 60 Hz).
pub const COIL_LEAP_TICKS: u32 = 48;
/// Leap arc apex height above the lerp line (units).
pub const COIL_LEAP_HEIGHT: f32 = 4.0;
/// Cooldown between wind-ups after a release (2.5 s at 60 Hz).
pub const COIL_RELEASE_COOLDOWN_TICKS: u32 = 150;

/// Coil phases.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoilPhase {
    /// Coiled; slow drift toward the nearest attracted player.
    Coiled,
    /// Winding up; telegraph for the release.
    Wind,
    /// One tick: the shockwave resolves and OVERTUNED is inflicted.
    Release,
    /// Recovering before the leap.
    Recover,
    /// Airborne arc to the chosen map spot.
    Leap,
}

/// Coil brain state.
#[derive(Debug, Clone, PartialEq)]
pub struct CoilBrain {
    /// Current phase.
    pub phase: CoilPhase,
    /// Phase countdown in ticks.
    pub timer: u32,
    /// Continuous ticks a player has stayed in the taunt radius.
    pub proximity_ticks: u32,
    /// Ticks until the next tighten step.
    pub tighten_in: u32,
    /// Cooldown between wind-ups.
    pub cooldown_in: u32,
    /// Leap arc start (current position when the leap began).
    pub leap_from: Vec3,
    /// Leap arc end (random map spot).
    pub leap_to: Vec3,
    /// Random seed for leap placement.
    pub seed: u32,
}

impl Default for CoilBrain {
    fn default() -> Self {
        Self {
            phase: CoilPhase::Coiled,
            timer: 0,
            proximity_ticks: 0,
            tighten_in: COIL_TIGHTEN_INTERVAL_TICKS,
            cooldown_in: 0,
            leap_from: Vec3::new(0.0, 0.0, 0.0),
            leap_to: Vec3::new(0.0, 0.0, 0.0),
            seed: 0,
        }
    }
}

/// What happened this tick.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct CoilStep {
    /// True while winding (VFX/test hook).
    pub telegraphing: bool,
    /// Players caught in the shockwave this tick; the caller applies OVERTUNED
    /// to each via `enemies::apply_overtuned`.
    pub overtuned_ids: Vec<u32>,
    /// True while airborne on the leap arc.
    pub leaping: bool,
}

/// Advance Coil by one tick.
pub fn step_coil(enemy_pos: &mut Vec3, brain: &mut CoilBrain, ctx: &TickContext) -> CoilStep {
    if brain.seed == 0 {
        brain.seed = seed_from_pos(enemy_pos);
    }
    let mut out = CoilStep::default();

    match brain.phase {
        CoilPhase::Coiled => {
            if brain.cooldown_in > 0 {
                brain.cooldown_in -= 1;
            }
            // Gliders are never attracted: no wind-up, no chase.
            let target = nearest_player_where(ctx.players, enemy_pos, |p| p.class != "glider");
            if let Some(p) = target {
                let d = horizontal_distance(enemy_pos, &p.feet);
                if d <= COIL_TAUNT_RADIUS && brain.cooldown_in == 0 {
                    brain.phase = CoilPhase::Wind;
                    brain.timer = COIL_WIND_TICKS;
                    brain.proximity_ticks = 0;
                    brain.tighten_in = COIL_TIGHTEN_INTERVAL_TICKS;
                } else if d <= COIL_AGGRO_RADIUS {
                    let goal = Vec3::new(p.feet.x, enemy_pos.y, p.feet.z);
                    move_toward(enemy_pos, &goal, COIL_DRIFT_SPEED * SIM_DT);
                }
            }
        }
        CoilPhase::Wind => {
            out.telegraphing = true;
            let someone_near = ctx
                .players
                .iter()
                .any(|p| p.class != "glider" && horizontal_distance(enemy_pos, &p.feet) <= COIL_TAUNT_RADIUS);
            if someone_near {
                brain.proximity_ticks += 1;
                if brain.tighten_in > 0 {
                    brain.tighten_in -= 1;
                }
                if brain.tighten_in == 0 {
                    // Tightening only ever SHORTENS the wind. Clamping with `.max` on
                    // every pass would push a nearly-finished wind back up to the floor
                    // and the coil would never release.
                    if brain.timer > COIL_MIN_WIND_TICKS {
                        brain.timer = (brain.timer - COIL_TIGHTEN_BONUS_TICKS).max(COIL_MIN_WIND_TICKS);
                    }
                    brain.tighten_in = COIL_TIGHTEN_INTERVAL_TICKS;
                }
            } else {
                brain.proximity_ticks = 0;
            }
            if brain.timer > 0 {
                brain.timer -= 1;
            }
            if brain.timer == 0 {
                brain.phase = CoilPhase::Release;
            }
        }
        CoilPhase::Release => {
            out.overtuned_ids = ctx
                .players
                .iter()
                .filter(|p| horizontal_distance(enemy_pos, &p.feet) <= COIL_SHOCKWAVE_RADIUS)
                .map(|p| p.id)
                .collect();
            brain.phase = CoilPhase::Recover;
            brain.timer = COIL_RECOVER_TICKS;
            brain.cooldown_in = COIL_RELEASE_COOLDOWN_TICKS;
        }
        CoilPhase::Recover => {
            if brain.timer > 0 {
                brain.timer -= 1;
            }
            if brain.timer == 0 {
                let rx = unit_f32(&mut brain.seed);
                let rz = unit_f32(&mut brain.seed);
                brain.leap_from = *enemy_pos;
                brain.leap_to = Vec3::new(
                    ctx.map_min.x + rx * (ctx.map_max.x - ctx.map_min.x),
                    enemy_pos.y,
                    ctx.map_min.z + rz * (ctx.map_max.z - ctx.map_min.z),
                );
                brain.phase = CoilPhase::Leap;
                brain.timer = COIL_LEAP_TICKS;
            }
        }
        CoilPhase::Leap => {
            out.leaping = true;
            let p = 1.0 - brain.timer as f32 / COIL_LEAP_TICKS as f32;
            let h = COIL_LEAP_HEIGHT * 4.0 * p * (1.0 - p);
            enemy_pos.x = brain.leap_from.x + (brain.leap_to.x - brain.leap_from.x) * p;
            enemy_pos.y = brain.leap_from.y + (brain.leap_to.y - brain.leap_from.y) * p + h;
            enemy_pos.z = brain.leap_from.z + (brain.leap_to.z - brain.leap_from.z) * p;
            if brain.timer > 0 {
                brain.timer -= 1;
            }
            if brain.timer == 0 {
                *enemy_pos = brain.leap_to;
                brain.phase = CoilPhase::Coiled;
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::enemies::testutil::{arena, sample_player};

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
    fn coil_winds_then_releases_and_overtunes_players_in_radius() {
        let mut pos = Vec3::new(0.0, 0.0, 0.0);
        let mut brain = CoilBrain::default();
        let players = vec![sample_player(3, 1.5, 0.0, "striker")];
        let _out = step_coil(&mut pos, &mut brain, &ctx(&players));
        assert_eq!(brain.phase, CoilPhase::Wind);

        let mut overtuned: Vec<u32> = Vec::new();
        let mut guard = 0;
        loop {
            guard += 1;
            assert!(guard < 10_000);
            let out = step_coil(&mut pos, &mut brain, &ctx(&players));
            if !out.overtuned_ids.is_empty() {
                overtuned = out.overtuned_ids;
            }
            if brain.phase == CoilPhase::Recover {
                break;
            }
        }
        assert!(overtuned.contains(&3));
    }

    #[test]
    fn striker_dashes_out_and_is_not_overtuned() {
        let mut pos = Vec3::new(0.0, 0.0, 0.0);
        let mut brain = CoilBrain::default();
        let near = vec![sample_player(3, 1.5, 0.0, "striker")];
        step_coil(&mut pos, &mut brain, &ctx(&near));
        assert_eq!(brain.phase, CoilPhase::Wind);

        let far = vec![sample_player(3, 30.0, 0.0, "striker")];
        let mut overtuned: Vec<u32> = Vec::new();
        let mut guard = 0;
        loop {
            guard += 1;
            assert!(guard < 10_000);
            let out = step_coil(&mut pos, &mut brain, &ctx(&far));
            if !out.overtuned_ids.is_empty() {
                overtuned = out.overtuned_ids;
            }
            if brain.phase == CoilPhase::Recover {
                break;
            }
        }
        assert!(overtuned.is_empty(), "leaving the radius avoids OVERTUNED");
    }

    #[test]
    fn glider_is_never_attracted() {
        let mut pos = Vec3::new(0.0, 0.0, 0.0);
        let mut brain = CoilBrain::default();
        let gliders = vec![sample_player(4, 1.0, 0.0, "glider")];
        for _ in 0..120 {
            step_coil(&mut pos, &mut brain, &ctx(&gliders));
        }
        assert_eq!(brain.phase, CoilPhase::Coiled);
    }

    #[test]
    fn wind_meets_tier1_minimum_and_tightening_holds_the_floor() {
        // Tier 1: wind-up visible for 0.35–0.95 s before the hit resolves.
        let seconds = COIL_WIND_TICKS as f32 / 60.0;
        assert!((0.35..=0.95).contains(&seconds));

        let mut pos = Vec3::new(0.0, 0.0, 0.0);
        let mut brain = CoilBrain::default();
        let players = vec![sample_player(3, 1.5, 0.0, "striker")];
        step_coil(&mut pos, &mut brain, &ctx(&players)); // -> Wind
        let mut telegraph_ticks = 0_u32;
        let mut guard = 0;
        loop {
            guard += 1;
            assert!(guard < 10_000);
            let out = step_coil(&mut pos, &mut brain, &ctx(&players));
            if out.telegraphing {
                telegraph_ticks += 1;
            }
            if brain.phase != CoilPhase::Wind {
                break;
            }
        }
        assert!(telegraph_ticks <= COIL_WIND_TICKS);
        // The floor is on the whole wind-up, not on the countdown: tightening may
        // shorten the wind but never below the tier-1 minimum.
        assert!(
            telegraph_ticks >= COIL_MIN_WIND_TICKS,
            "the wind-up is never shorter than the tier-1 floor (got {telegraph_ticks})"
        );
    }

    #[test]
    fn leap_target_is_deterministic() {
        let players: Vec<super::super::PlayerSample<'static>> = Vec::new();
        let (min, max) = arena();
        let leap_for = |x: f32| {
            let mut pos = Vec3::new(x, 0.0, 7.0);
            let mut brain = CoilBrain {
                phase: CoilPhase::Recover,
                timer: 1,
                ..CoilBrain::default()
            };
            let ctx = TickContext {
                players: &players,
                collapse_phase: false,
                map_min: min,
                map_max: max,
            };
            step_coil(&mut pos, &mut brain, &ctx);
            brain.leap_to
        };
        assert_eq!(leap_for(3.0), leap_for(3.0));
    }
}
