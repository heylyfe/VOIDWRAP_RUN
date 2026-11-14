//! Lure — the redesigned fog-gift ambush (cue tier 1, min level 3, cap 2;
//! tags fog/ambush/collapse).
//!
//! It kept its star silhouette and purple-fog identity and lost “does nothing”.
//!
//! - DORMANT: sits in fog emitting a soft gift-like glimmer; the fog pulses
//!   slowly ([`glimmer_intensity`]) — that pulse IS the telegraph. No movement,
//!   no chase. At gameplay distance it reads as a collectible.
//! - TRIGGER: a player inside the short radius gets a brief flare-up
//!   ([`LURE_FLARE_TICKS`], 0.6 s — tier 1, under 1 s), then a small shockwave
//!   (horizontal knockback, no damage), then the Lure pursues the nearest
//!   player for a few seconds before re-dorming elsewhere.
//! - The ambush layer: distinct from Static (hunts) and Coil (random leaps).
//!   Its whole threat is being mistaken for a gift.
//! - One class sees the true silhouette through the glimmer
//!   ([`lure_sees_true_silhouette`]); a jump-pad launch escapes the pursuit
//!   (a grappler using their ability also pulls past it).
//! - During the collapse phase it re-dorms aggressively when players rush the
//!   exit: pursuit lasts [`LURE_COLLAPSE_PURSUE_TICKS`] instead of
//!   [`LURE_PURSUE_TICKS`].
//!
//! Pursuit is deliberately slower than `RUN_SPEED` (8.5 u/s vs 9.0 u/s), so a
//! walking player outruns it; the flare shockwave is the dangerous part.

use protocol::Vec3;

use super::{SIM_DT, TickContext, nearest_player_where, seed_from_pos, unit_f32};
use crate::rules::{flat_direction, horizontal_distance, move_toward};

/// Radius that triggers the flare when a player walks inside (units).
pub const LURE_TRIGGER_RADIUS: f32 = 2.2;
/// Flare-up duration, in ticks (0.6 s at 60 Hz; tier 1).
pub const LURE_FLARE_TICKS: u32 = 36;
/// Shockwave radius (units).
pub const LURE_SHOCKWAVE_RADIUS: f32 = 3.0;
/// Horizontal impulse of the shockwave (u/s).
pub const LURE_SHOCKWAVE_IMPULSE: f32 = 7.0;
/// Normal pursuit duration, in ticks (3.0 s at 60 Hz).
pub const LURE_PURSUE_TICKS: u32 = 180;
/// Collapse-phase pursuit duration — re-dorms aggressively (1.0 s at 60 Hz).
pub const LURE_COLLAPSE_PURSUE_TICKS: u32 = 60;
/// Pursued speed (u/s); slower than a walking player's run speed.
pub const LURE_PURSUE_SPEED: f32 = 8.5;
/// Re-dorm hop duration, in ticks (0.5 s at 60 Hz).
pub const LURE_REDORM_TICKS: u32 = 30;
/// Re-dorm hop apex height (units).
pub const LURE_REDORM_HEIGHT: f32 = 1.5;
/// Fog pulse period, in ticks (2.0 s at 60 Hz).
pub const LURE_GLIMMER_PERIOD: u32 = 120;
/// Minimum distance of the re-dorm spot from where it was (units).
pub const LURE_REDORM_MIN_DIST: f32 = 8.0;

/// Lure phases.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LurePhase {
    /// Sitting in fog, glimmering like a gift. No movement, no chase.
    Dormant,
    /// Brief flare-up (tier-1 telegraph) before the shockwave.
    Flare,
    /// Pursuing the nearest player; ends after the timer or an escape.
    Pursue,
    /// Hop to a new spot and go dormant again.
    ReDorm,
}

/// Lure brain state.
#[derive(Debug, Clone, PartialEq)]
pub struct LureBrain {
    /// Current phase.
    pub phase: LurePhase,
    /// Phase countdown in ticks.
    pub timer: u32,
    /// Fog pulse counter; drives [`glimmer_intensity`]. VFX only.
    pub glimmer_ticks: u32,
    /// The player being pursued (nearest at trigger time).
    pub prey: Option<u32>,
    /// Hop start position (set when the re-dorm begins).
    pub redorm_from: Vec3,
    /// Hop end position (random map spot, ≥ [`LURE_REDORM_MIN_DIST`] away when possible).
    pub redorm_to: Vec3,
    /// Random seed for re-dorm placement.
    pub seed: u32,
}

impl Default for LureBrain {
    fn default() -> Self {
        Self {
            phase: LurePhase::Dormant,
            timer: 0,
            glimmer_ticks: 0,
            prey: None,
            redorm_from: Vec3::new(0.0, 0.0, 0.0),
            redorm_to: Vec3::new(0.0, 0.0, 0.0),
            seed: 0,
        }
    }
}

/// What happened this tick.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct LureStep {
    /// True while flaring (tier-1 telegraph; VFX/test hook).
    pub telegraphing: bool,
    /// Shockwave impulse per affected player, `(id, impulse)`, emitted once at
    /// the end of the flare. The caller applies the knockback.
    pub shockwave: Option<Vec<(u32, Vec3)>>,
    /// True while pursuing.
    pub pursuing: bool,
    /// True on the tick the pursued player escaped (jump-pad launch, or a
    /// grappler using their ability); the Lure begins re-dorming.
    pub escaped: bool,
}

/// True for the class that sees the true star silhouette through the glimmer.
///
/// The ticket does not name the class; this is mapped to `glider`, the Lure's
/// `immune` relation in the frozen registry (“crosses above”). Flagged in
/// NOTES for design confirmation.
pub fn lure_sees_true_silhouette(class: &str) -> bool {
    class == "glider"
}

/// Fog pulse intensity in `[0.35, 0.85]`; a triangle wave over
/// [`LURE_GLIMMER_PERIOD`] ticks. This pulse IS the DORMANT telegraph.
pub fn glimmer_intensity(brain: &LureBrain) -> f32 {
    let t = brain.glimmer_ticks as f32 / LURE_GLIMMER_PERIOD as f32;
    let tri = if t < 0.5 { t * 2.0 } else { (1.0 - t) * 2.0 };
    0.35 + 0.5 * tri
}

/// Deterministic re-dorm spot in map bounds; rerolls up to 8 times to land
/// at least [`LURE_REDORM_MIN_DIST`] away.
fn pick_redorm(brain: &mut LureBrain, ctx: &TickContext, from: &Vec3) -> Vec3 {
    let mut candidate = *from;
    for _ in 0..8 {
        let x = ctx.map_min.x + unit_f32(&mut brain.seed) * (ctx.map_max.x - ctx.map_min.x);
        let z = ctx.map_min.z + unit_f32(&mut brain.seed) * (ctx.map_max.z - ctx.map_min.z);
        candidate = Vec3::new(x, from.y, z);
        if horizontal_distance(&candidate, from) >= LURE_REDORM_MIN_DIST {
            break;
        }
    }
    candidate
}

/// Advance Lure by one tick.
pub fn step_lure(enemy_pos: &mut Vec3, brain: &mut LureBrain, ctx: &TickContext) -> LureStep {
    if brain.seed == 0 {
        brain.seed = seed_from_pos(enemy_pos);
    }
    let mut out = LureStep::default();
    brain.glimmer_ticks = (brain.glimmer_ticks + 1) % LURE_GLIMMER_PERIOD;

    match brain.phase {
        LurePhase::Dormant => {
            if let Some(p) = nearest_player_where(ctx.players, enemy_pos, |_| true)
                && horizontal_distance(enemy_pos, &p.feet) <= LURE_TRIGGER_RADIUS
            {
                brain.prey = Some(p.id);
                brain.phase = LurePhase::Flare;
                brain.timer = LURE_FLARE_TICKS;
            }
        }
        LurePhase::Flare => {
            out.telegraphing = true;
            if brain.timer > 0 {
                brain.timer -= 1;
            }
            if brain.timer == 0 {
                let mut hits = Vec::new();
                for p in ctx.players {
                    if horizontal_distance(enemy_pos, &p.feet) <= LURE_SHOCKWAVE_RADIUS {
                        let dir = flat_direction(enemy_pos, &Vec3::new(p.feet.x, enemy_pos.y, p.feet.z));
                        hits.push((
                            p.id,
                            Vec3::new(dir.x * LURE_SHOCKWAVE_IMPULSE, 0.0, dir.z * LURE_SHOCKWAVE_IMPULSE),
                        ));
                    }
                }
                out.shockwave = Some(hits);
                brain.phase = LurePhase::Pursue;
                brain.timer = if ctx.collapse_phase {
                    LURE_COLLAPSE_PURSUE_TICKS
                } else {
                    LURE_PURSUE_TICKS
                };
            }
        }
        LurePhase::Pursue => {
            out.pursuing = true;
            let mut escaped = false;
            if let Some(id) = brain.prey
                && let Some(p) = ctx.players.iter().find(|p| p.id == id)
                && (p.jump_launched || (p.class == "grappler" && p.used_ability))
            {
                escaped = true;
            }
            if escaped {
                out.escaped = true;
                brain.phase = LurePhase::ReDorm;
                brain.timer = LURE_REDORM_TICKS;
                brain.redorm_from = *enemy_pos;
                brain.redorm_to = pick_redorm(brain, ctx, enemy_pos);
                return out;
            }
            if let Some(p) = nearest_player_where(ctx.players, enemy_pos, |_| true) {
                let goal = Vec3::new(p.feet.x, enemy_pos.y, p.feet.z);
                move_toward(enemy_pos, &goal, LURE_PURSUE_SPEED * SIM_DT);
            }
            if brain.timer > 0 {
                brain.timer -= 1;
            }
            if brain.timer == 0 {
                brain.phase = LurePhase::ReDorm;
                brain.timer = LURE_REDORM_TICKS;
                brain.redorm_from = *enemy_pos;
                brain.redorm_to = pick_redorm(brain, ctx, enemy_pos);
            }
        }
        LurePhase::ReDorm => {
            let p = 1.0 - brain.timer as f32 / LURE_REDORM_TICKS as f32;
            let h = LURE_REDORM_HEIGHT * 4.0 * p * (1.0 - p);
            enemy_pos.x = brain.redorm_from.x + (brain.redorm_to.x - brain.redorm_from.x) * p;
            enemy_pos.y = brain.redorm_from.y + (brain.redorm_to.y - brain.redorm_from.y) * p + h;
            enemy_pos.z = brain.redorm_from.z + (brain.redorm_to.z - brain.redorm_from.z) * p;
            if brain.timer > 0 {
                brain.timer -= 1;
            }
            if brain.timer == 0 {
                *enemy_pos = brain.redorm_to;
                brain.phase = LurePhase::Dormant;
                brain.prey = None;
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::enemies::testutil::{arena, sample_player};

    fn ctx<'a>(players: &'a [super::super::PlayerSample<'static>], collapse: bool) -> TickContext<'a> {
        let (min, max) = arena();
        TickContext {
            players,
            collapse_phase: collapse,
            map_min: min,
            map_max: max,
        }
    }

    #[test]
    fn lure_triggers_shockwave_then_pursues() {
        let mut pos = Vec3::new(0.0, 0.0, 0.0);
        let mut brain = LureBrain::default();
        let players = vec![sample_player(1, 1.0, 0.0, "striker")];

        let _out = step_lure(&mut pos, &mut brain, &ctx(&players, false));
        assert_eq!(brain.phase, LurePhase::Flare);
        assert_eq!(brain.prey, Some(1));

        let mut guard = 0;
        loop {
            guard += 1;
            assert!(guard < 10_000, "a shockwave fires before the pursuit begins");
            let out = step_lure(&mut pos, &mut brain, &ctx(&players, false));
            if out.shockwave.is_some() {
                assert_eq!(brain.phase, LurePhase::Pursue);
                break;
            }
        }
        let before = horizontal_distance(&pos, &Vec3::new(1.0, 0.0, 0.0));
        step_lure(&mut pos, &mut brain, &ctx(&players, false));
        let after = horizontal_distance(&pos, &Vec3::new(1.0, 0.0, 0.0));
        assert!(after < before, "the pursuit closes in");
    }

    #[test]
    fn jump_pad_launch_escapes_the_pursuit() {
        let mut pos = Vec3::new(0.0, 0.0, 0.0);
        let mut brain = LureBrain::default();
        let players = vec![sample_player(1, 1.0, 0.0, "striker")];
        step_lure(&mut pos, &mut brain, &ctx(&players, false));
        let mut guard = 0;
        loop {
            guard += 1;
            assert!(guard < 10_000);
            let out = step_lure(&mut pos, &mut brain, &ctx(&players, false));
            if out.shockwave.is_some() {
                break;
            }
        }
        let mut launched = sample_player(1, 1.0, 0.0, "striker");
        launched.jump_launched = true;
        let launched_players = vec![launched];
        let out = step_lure(&mut pos, &mut brain, &ctx(&launched_players, false));
        assert!(out.escaped);
        assert_eq!(brain.phase, LurePhase::ReDorm);
    }

    #[test]
    fn grappler_ability_pulls_past_the_pursuit() {
        let mut pos = Vec3::new(0.0, 0.0, 0.0);
        let mut brain = LureBrain::default();
        let players = vec![sample_player(1, 1.0, 0.0, "grappler")];
        step_lure(&mut pos, &mut brain, &ctx(&players, false));
        let mut guard = 0;
        loop {
            guard += 1;
            assert!(guard < 10_000);
            let out = step_lure(&mut pos, &mut brain, &ctx(&players, false));
            if out.shockwave.is_some() {
                break;
            }
        }
        let mut grappling = sample_player(1, 1.0, 0.0, "grappler");
        grappling.used_ability = true;
        let grapplers = vec![grappling];
        let out = step_lure(&mut pos, &mut brain, &ctx(&grapplers, false));
        assert!(out.escaped);
    }

    #[test]
    fn only_glider_sees_through_the_glimmer_and_flare_meets_tier1() {
        assert!(lure_sees_true_silhouette("glider"));
        assert!(!lure_sees_true_silhouette("striker"));
        assert!(!lure_sees_true_silhouette("bulwark"));

        // Tier 1: telegraph visible for 0.35–0.95 s, under 1 s by design.
        let seconds = LURE_FLARE_TICKS as f32 / 60.0;
        assert!((0.35..1.0).contains(&seconds));

        let mut pos = Vec3::new(0.0, 0.0, 0.0);
        let mut brain = LureBrain::default();
        let players = vec![sample_player(1, 1.0, 0.0, "striker")];
        step_lure(&mut pos, &mut brain, &ctx(&players, false));
        let mut telegraph_ticks = 0_u32;
        let mut guard = 0;
        loop {
            guard += 1;
            assert!(guard < 10_000);
            let out = step_lure(&mut pos, &mut brain, &ctx(&players, false));
            if out.telegraphing {
                telegraph_ticks += 1;
            }
            if brain.phase != LurePhase::Flare {
                break;
            }
        }
        assert_eq!(telegraph_ticks, LURE_FLARE_TICKS);
    }

    #[test]
    fn lure_redorms_aggressively_during_collapse() {
        let mut pos = Vec3::new(0.0, 0.0, 0.0);
        let mut brain = LureBrain::default();
        let players = vec![sample_player(1, 1.0, 0.0, "striker")];
        step_lure(&mut pos, &mut brain, &ctx(&players, true));
        let mut guard = 0;
        loop {
            guard += 1;
            assert!(guard < 10_000);
            let out = step_lure(&mut pos, &mut brain, &ctx(&players, true));
            if out.shockwave.is_some() {
                assert_eq!(brain.timer, LURE_COLLAPSE_PURSUE_TICKS);
                break;
            }
        }
    }

    #[test]
    fn dormant_glimmer_pulse_stays_in_range() {
        let brain = LureBrain::default();
        for ticks in [0, brain.glimmer_ticks + LURE_GLIMMER_PERIOD / 4] {
            let mut b = brain.clone();
            b.glimmer_ticks = ticks;
            let i = glimmer_intensity(&b);
            assert!((0.0..=1.0).contains(&i));
        }
    }
}
