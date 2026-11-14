//! Metronome — the counting beacon (cue tier 1, min level 10, cap 1).
//!
//! A beacon shows a number that ticks up. The marked player must use their
//! class ability exactly on their number or be struck. Numbers are assigned
//! deterministically per cycle (`1 + cycle % 4`), there is no RNG.
//!
//! Telegraph: the arm flashes for 0.5 s ([`METRONOME_ARM_FLASH_TICKS`]) before
//! each swing resolves — squarely inside tier 1's 0.35–0.95 s band. The window
//! during which the ability must be used IS the flash.
//!
//! Relations (registry): `bulwark: immune` (the ward ignores the beat — never
//! struck); `glider: counters` (gliding crosses the swing — a glider that is
//! gliding at any point during the window is not struck even without ability
//! use). No class has an `ignores` relation against Metronome in the frozen
//! registry, so none is implemented here.

use protocol::Vec3;

use super::{TickContext, nearest_player_where};

/// Ticks per number on the beacon (0.5 s at 60 Hz).
pub const METRONOME_BEAT_TICKS: u32 = 30;
/// Arm-flash telegraph / ability window, in ticks (0.5 s at 60 Hz).
pub const METRONOME_ARM_FLASH_TICKS: u32 = 30;
/// Rest between cycles, in ticks (1.0 s at 60 Hz).
pub const METRONOME_REST_TICKS: u32 = 60;
/// Highest number a cycle can target; cycles through `1..=METRONOME_NUMBER_MAX`.
pub const METRONOME_NUMBER_MAX: u32 = 4;

/// Metronome phases.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MetronomePhase {
    /// Resting between cycles; marks the nearest player when the rest ends.
    Rest,
    /// The beacon is ticking up toward the target number.
    Count,
    /// The arm flashes; the ability window is open; the strike resolves at the end.
    Flash,
}

/// Metronome brain state. The beacon is the enemy's own position.
#[derive(Debug, Clone, PartialEq)]
pub struct MetronomeBrain {
    /// Current phase.
    pub phase: MetronomePhase,
    /// Phase countdown in ticks (Flash duration when in `Flash`).
    pub timer: u32,
    /// The player marked this cycle.
    pub marked: Option<u32>,
    /// Number currently shown on the beacon.
    pub display: u32,
    /// The number on which the marked player must use their class ability.
    pub target_number: u32,
    /// Ticks until the number advances.
    pub count_in: u32,
    /// True once ability use was seen during the Flash window.
    pub ability_seen: bool,
    /// True once gliding was seen during the Flash window (glider counter).
    pub gliding_seen: bool,
    /// Cycle counter; drives the deterministic `target_number` sequence.
    pub cycle: u32,
}

impl Default for MetronomeBrain {
    fn default() -> Self {
        Self {
            phase: MetronomePhase::Rest,
            timer: 0,
            marked: None,
            display: 1,
            target_number: 1,
            count_in: METRONOME_BEAT_TICKS,
            ability_seen: false,
            gliding_seen: false,
            cycle: 0,
        }
    }
}

/// What happened this tick.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct MetronomeStep {
    /// True while the arm-flash telegraph is up (VFX hook; also the test hook).
    pub telegraphing: bool,
    /// The number currently on the beacon (HUD hook).
    pub display: u32,
    /// The player to strike this tick; the caller applies damage/debuff.
    /// `None` when the ability was used on the number, the class is immune
    /// (bulwark), or the glider crossed it.
    pub struck: Option<u32>,
}

/// Advance Metronome by one tick. The beacon does not move.
pub fn step_metronome(enemy_pos: &Vec3, brain: &mut MetronomeBrain, ctx: &TickContext) -> MetronomeStep {
    let mut out = MetronomeStep {
        display: brain.display,
        ..Default::default()
    };

    match brain.phase {
        MetronomePhase::Rest => {
            if brain.timer > 0 {
                brain.timer -= 1;
                return out;
            }
            if let Some(p) = nearest_player_where(ctx.players, enemy_pos, |_| true) {
                brain.marked = Some(p.id);
                brain.target_number = 1 + (brain.cycle % METRONOME_NUMBER_MAX);
                brain.display = 1;
                brain.ability_seen = false;
                brain.gliding_seen = false;
                out.display = brain.display;
                if brain.target_number == 1 {
                    brain.phase = MetronomePhase::Flash;
                    brain.timer = METRONOME_ARM_FLASH_TICKS;
                } else {
                    brain.phase = MetronomePhase::Count;
                    brain.count_in = METRONOME_BEAT_TICKS;
                }
            } else {
                brain.timer = METRONOME_BEAT_TICKS;
            }
        }
        MetronomePhase::Count => {
            if brain.count_in > 0 {
                brain.count_in -= 1;
            }
            if brain.count_in == 0 {
                brain.display += 1;
                if brain.display == brain.target_number {
                    brain.phase = MetronomePhase::Flash;
                    brain.timer = METRONOME_ARM_FLASH_TICKS;
                } else {
                    brain.count_in = METRONOME_BEAT_TICKS;
                }
                out.display = brain.display;
            }
        }
        MetronomePhase::Flash => {
            out.telegraphing = true;
            if let Some(id) = brain.marked
                && let Some(p) = ctx.players.iter().find(|p| p.id == id)
            {
                if p.used_ability {
                    brain.ability_seen = true;
                }
                if p.class == "glider" && p.gliding {
                    brain.gliding_seen = true;
                }
            }
            if brain.timer > 0 {
                brain.timer -= 1;
            }
            if brain.timer == 0 {
                let mut struck = None;
                if let Some(id) = brain.marked
                    && let Some(p) = ctx.players.iter().find(|p| p.id == id)
                {
                    let immune = p.class == "bulwark";
                    let answered = brain.ability_seen || (p.class == "glider" && brain.gliding_seen);
                    if !immune && !answered {
                        struck = Some(id);
                    }
                }
                out.struck = struck;
                brain.phase = MetronomePhase::Rest;
                brain.timer = METRONOME_REST_TICKS;
                brain.cycle += 1;
                brain.marked = None;
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

    fn run_into_flash(brain: &mut MetronomeBrain, players: &[super::super::PlayerSample<'static>]) {
        let pos = Vec3::new(0.0, 0.0, 0.0);
        let mut guard = 0;
        loop {
            guard += 1;
            assert!(guard < 10_000, "metronome must reach its Flash phase");
            step_metronome(&pos, brain, &ctx(players));
            if brain.phase == MetronomePhase::Flash {
                return;
            }
        }
    }

    #[test]
    fn ability_on_the_number_answers_the_beat() {
        let mut brain = MetronomeBrain::default();
        let quiet = vec![sample_player(3, 4.0, 0.0, "striker")];
        run_into_flash(&mut brain, &quiet);

        let pos = Vec3::new(0.0, 0.0, 0.0);
        let mut with_ability = sample_player(3, 4.0, 0.0, "striker");
        with_ability.used_ability = true;
        let on_the_number = vec![with_ability];
        let out = step_metronome(&pos, &mut brain, &ctx(&on_the_number));
        assert!(out.telegraphing);

        let mut telegraph_ticks = 1_u32;
        let mut guard = 0;
        let struck = loop {
            guard += 1;
            assert!(guard < 10_000);
            let out = step_metronome(&pos, &mut brain, &ctx(&quiet));
            if out.telegraphing {
                telegraph_ticks += 1;
            }
            if brain.phase == MetronomePhase::Rest {
                break out.struck;
            }
        };
        assert_eq!(struck, None, "ability used on the number: no strike");
        assert_eq!(telegraph_ticks, METRONOME_ARM_FLASH_TICKS);
    }

    #[test]
    fn bulwark_is_immune_and_glider_crossing_answers() {
        let pos = Vec3::new(0.0, 0.0, 0.0);
        for class in ["bulwark", "glider"] {
            let mut brain = MetronomeBrain::default();
            let mut p = sample_player(5, 4.0, 0.0, class);
            if class == "glider" {
                p.gliding = true; // crosses the swing; no ability use needed
            }
            let players = vec![p];
            run_into_flash(&mut brain, &players);
            let mut guard = 0;
            let stuck = loop {
                guard += 1;
                assert!(guard < 10_000);
                let out = step_metronome(&pos, &mut brain, &ctx(&players));
                if brain.phase == MetronomePhase::Rest {
                    break out.struck;
                }
            };
            assert_eq!(stuck, None, "{class} must not be struck");
        }
    }

    #[test]
    fn striker_off_the_number_is_struck() {
        let mut brain = MetronomeBrain::default();
        let quiet = vec![sample_player(3, 4.0, 0.0, "striker")];
        run_into_flash(&mut brain, &quiet);
        let pos = Vec3::new(0.0, 0.0, 0.0);
        let mut guard = 0;
        let struck = loop {
            guard += 1;
            assert!(guard < 10_000);
            let out = step_metronome(&pos, &mut brain, &ctx(&quiet));
            if brain.phase == MetronomePhase::Rest {
                break out.struck;
            }
        };
        assert_eq!(struck, Some(3));
    }

    #[test]
    fn arm_flash_meets_tier1_minimum_and_counts_exactly() {
        // Tier 1: wind-up visible for 0.35–0.95 s before the hit resolves.
        let flash_seconds = METRONOME_ARM_FLASH_TICKS as f32 / 60.0;
        assert!((0.35..=0.95).contains(&flash_seconds));

        let mut brain = MetronomeBrain::default();
        let quiet = vec![sample_player(3, 4.0, 0.0, "striker")];
        run_into_flash(&mut brain, &quiet);
        let pos = Vec3::new(0.0, 0.0, 0.0);
        let mut telegraph_ticks = 0_u32;
        let mut guard = 0;
        loop {
            guard += 1;
            assert!(guard < 10_000);
            let out = step_metronome(&pos, &mut brain, &ctx(&quiet));
            if out.telegraphing {
                telegraph_ticks += 1;
            }
            if brain.phase == MetronomePhase::Rest {
                break;
            }
        }
        assert_eq!(telegraph_ticks, METRONOME_ARM_FLASH_TICKS);
    }
}
