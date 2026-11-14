//! Blight — the squelching infection debuff (cue tier 3, min level 8, cap 1).
//!
//! Ambient enemy: no per-attack wind-up. The telegraph is the infected ground
//! itself — a discoloured aura on Blight that spreads one tile per beat and
//! never disappears, so the level is readable without watching Blight.
//!
//! Punish: crossing infected ground (or being hit by Blight) INFECTS: the
//! victim is slowed to [`INFECTION_SPEED_MUL`] and the infection pauses Rewind's
//! and Kite's signature timers while it lasts (registry synergy hook
//! `infection_pauses_timer`).
//!
//! Cleanse: a jump-pad launch or a support-class ability. Glider is immune
//! (airborne, per registry); a grappler using their ability is clean while
//! grappling above it (registry relation `grappler: counters`).

use protocol::Vec3;

use super::{SIM_DT, TickContext, nearest_player_where};
use crate::rules::{horizontal_distance, move_toward};

/// Approach speed while stalking a player (u/s).
pub const BLIGHT_APPROACH_SPEED: f32 = 2.6;
/// Radius of the infected aura at spawn (units; one tile = 1.0 unit).
pub const BLIGHT_INFECT_RADIUS_START: f32 = 2.0;
/// Maximum radius the infected aura spreads to (units).
pub const BLIGHT_INFECT_RADIUS_MAX: f32 = 6.0;
/// Ticks per tile of aura spread (one beat, 1.0 s at 60 Hz).
pub const BLIGHT_INFECT_SPREAD_TICKS: u32 = 60;
/// Infection duration once applied (6.0 s at 60 Hz).
pub const BLIGHT_INFECTION_TICKS: u32 = 360;
/// Movement multiplier applied to infected players.
pub const INFECTION_SPEED_MUL: f32 = 0.6;

/// Blight phases. Tier 3: ambient, no wind-up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlightPhase {
    /// Squelching approach; the aura does the rest.
    Stalk,
}

/// Blight brain state.
#[derive(Debug, Clone, PartialEq)]
pub struct BlightBrain {
    /// Current phase.
    pub phase: BlightPhase,
    /// Radius of the infected ground (units). The persistent telegraph.
    pub aura_radius: f32,
    /// Ticks until the aura spreads one tile.
    pub spread_in: u32,
    /// Infected players: `(player_id, ticks remaining)`.
    pub infected: Vec<(u32, u32)>,
}

impl Default for BlightBrain {
    fn default() -> Self {
        Self {
            phase: BlightPhase::Stalk,
            aura_radius: BLIGHT_INFECT_RADIUS_START,
            spread_in: BLIGHT_INFECT_SPREAD_TICKS,
            infected: Vec::new(),
        }
    }
}

/// What happened this tick.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct BlightStep {
    /// Players newly infected this tick.
    pub newly_infected: Vec<u32>,
    /// Players cleansed this tick (jump-pad launch or support ability).
    pub cleansed: Vec<u32>,
}

/// True while Blight's infection is on the player.
///
/// Rewind's rewind clock and Kite's tether clock must not decrement while this
/// is true for the afflicted player (registry synergy hook
/// `infection_pauses_timer`). The predicate is exported so batch A can gate
/// their timer decrement without duplicating the rule.
pub fn infection_pauses_timer(infected_ticks: u32) -> bool {
    infected_ticks > 0
}

impl BlightBrain {
    /// True while the player is infected.
    pub fn is_infected(&self, player_id: u32) -> bool {
        self.infected.iter().any(|&(id, t)| id == player_id && t > 0)
    }

    /// Infection ticks remaining for the player (0 = clean).
    pub fn infected_ticks(&self, player_id: u32) -> u32 {
        self.infected
            .iter()
            .find(|&&(id, _)| id == player_id)
            .map(|&(_, t)| t)
            .unwrap_or(0)
    }

    /// Movement multiplier for the player ([`INFECTION_SPEED_MUL`] while infected).
    pub fn speed_mul_for(&self, player_id: u32) -> f32 {
        if self.is_infected(player_id) {
            INFECTION_SPEED_MUL
        } else {
            1.0
        }
    }
}

/// Apply or refresh the infection; returns true if freshly applied.
fn apply_infection(brain: &mut BlightBrain, id: u32) -> bool {
    if let Some(entry) = brain.infected.iter_mut().find(|e| e.0 == id) {
        entry.1 = BLIGHT_INFECTION_TICKS;
        false
    } else {
        brain.infected.push((id, BLIGHT_INFECTION_TICKS));
        true
    }
}

/// Remove the infection; returns true if the player was infected.
fn remove_infection(brain: &mut BlightBrain, id: u32) -> bool {
    let before = brain.infected.len();
    brain.infected.retain(|&(eid, _)| eid != id);
    before != brain.infected.len()
}

/// Advance Blight by one tick.
pub fn step_blight(enemy_pos: &mut Vec3, brain: &mut BlightBrain, ctx: &TickContext) -> BlightStep {
    let mut out = BlightStep::default();

    // Infected ground spreads one tile per beat, up to the cap.
    if brain.spread_in > 0 {
        brain.spread_in -= 1;
    } else if brain.aura_radius < BLIGHT_INFECT_RADIUS_MAX {
        brain.aura_radius = (brain.aura_radius + 1.0).min(BLIGHT_INFECT_RADIUS_MAX);
        brain.spread_in = BLIGHT_INFECT_SPREAD_TICKS;
    }

    // Tick existing infections down BEFORE applying new ones.
    for entry in &mut brain.infected {
        entry.1 = entry.1.saturating_sub(1);
    }
    brain.infected.retain(|&(_, t)| t > 0);

    // Cleanse hooks: jump-pad launch or a support-class ability.
    for p in ctx.players {
        if (p.jump_launched || p.used_support_ability) && remove_infection(brain, p.id) {
            out.cleansed.push(p.id);
        }
    }

    // Crossing the infected ground applies it.
    // Glider is immune (airborne). A grappler mid-grapple is over the infection.
    for p in ctx.players {
        if p.class == "glider" {
            continue;
        }
        if p.class == "grappler" && p.used_ability {
            continue;
        }
        // A cleanse this tick wins: the escape must not be undone by the aura
        // pass that runs a few lines later in the same tick.
        if out.cleansed.contains(&p.id) {
            continue;
        }
        if horizontal_distance(enemy_pos, &p.feet) <= brain.aura_radius && apply_infection(brain, p.id) {
            out.newly_infected.push(p.id);
        }
    }

    // Squelching approach toward the nearest player.
    if let Some(target) = nearest_player_where(ctx.players, enemy_pos, |_| true) {
        let goal = Vec3::new(target.feet.x, enemy_pos.y, target.feet.z);
        move_toward(enemy_pos, &goal, BLIGHT_APPROACH_SPEED * SIM_DT);
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
    fn blight_infects_slows_and_pauses_timers_then_jump_pad_cleanses() {
        let mut pos = Vec3::new(0.0, 0.0, 0.0);
        let mut brain = BlightBrain::default();
        let players = vec![sample_player(7, 1.0, 0.0, "striker")];

        let out = step_blight(&mut pos, &mut brain, &ctx(&players));
        assert!(out.newly_infected.contains(&7));
        assert_eq!(brain.infected_ticks(7), BLIGHT_INFECTION_TICKS);
        assert!(infection_pauses_timer(brain.infected_ticks(7)));
        assert_eq!(brain.speed_mul_for(7), INFECTION_SPEED_MUL);

        // A jump-pad launch cleanses the infection.
        let mut launched = sample_player(7, 1.0, 0.0, "striker");
        launched.jump_launched = true;
        let players = vec![launched];
        let out = step_blight(&mut pos, &mut brain, &ctx(&players));
        assert!(out.cleansed.contains(&7));
        assert!(!brain.is_infected(7));
        assert!(!infection_pauses_timer(brain.infected_ticks(7)));
        assert_eq!(brain.speed_mul_for(7), 1.0);
    }

    #[test]
    fn support_ability_cleanses_infection() {
        let mut pos = Vec3::new(0.0, 0.0, 0.0);
        let mut brain = BlightBrain::default();
        let players = vec![sample_player(9, 0.5, 0.0, "bulwark")];
        step_blight(&mut pos, &mut brain, &ctx(&players));
        assert!(brain.is_infected(9));

        let mut support = sample_player(9, 0.5, 0.0, "bulwark");
        support.used_support_ability = true;
        let players = vec![support];
        let out = step_blight(&mut pos, &mut brain, &ctx(&players));
        assert!(out.cleansed.contains(&9));
        assert!(!brain.is_infected(9));
    }

    #[test]
    fn glider_and_grappling_grappler_are_not_infected() {
        let mut pos = Vec3::new(0.0, 0.0, 0.0);
        let mut brain = BlightBrain::default();
        let mut grappler = sample_player(2, 0.5, 0.0, "grappler");
        grappler.used_ability = true;
        let players = vec![sample_player(1, 0.5, 0.0, "glider"), grappler];
        let out = step_blight(&mut pos, &mut brain, &ctx(&players));
        assert!(out.newly_infected.is_empty());
    }

    #[test]
    fn blight_telegraph_is_a_persistent_spreading_aura() {
        // Tier 3 promise: the level is readable without watching Blight.
        let mut pos = Vec3::new(0.0, 0.0, 0.0);
        let mut brain = BlightBrain::default();
        let r0 = brain.aura_radius;
        assert!(r0 > 0.0, "aura is the telegraph: present from spawn");
        let players: Vec<super::super::PlayerSample<'static>> = Vec::new();
        // `spread_in` starts at a full interval, so the first spread lands on the
        // tick after it elapses.
        for _ in 0..=BLIGHT_INFECT_SPREAD_TICKS {
            step_blight(&mut pos, &mut brain, &ctx(&players));
        }
        assert!(brain.aura_radius >= r0 + 1.0, "aura spreads one tile per beat");
        assert!(brain.aura_radius > 0.0);
    }
}
