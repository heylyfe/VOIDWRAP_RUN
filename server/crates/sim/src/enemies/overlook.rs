//! Overlook — the teleporting marker and one-way laser (cue tier 2,
//! min level 12, cap 1).
//!
//! Flanks around a marked player, teleporting to random offsets and leaving a
//! glowing mark at each spot, then locks a 1.6 s sight line onto the marked
//! player (tier 2: decal-class telegraph, exact hit line, ≥ 1.0 s), then fires a
//! slow tracking laser that ONLY the marked player can see and be harmed by.
//!
//! Teammates can draw the laser's path during the collapse phase: the brain
//! records the beam tip every tick (up to [`OVERLOOK_PATH_MAX`] entries) and
//! [`OverlookBrain::drawn_path`] returns it only when asked with
//! `collapse_phase == true`.
//!
//! Glider is never attracted (registry relation `glider: ignores`): gliders are
//! never marked, so the laser never targets them. Striker counters by closing
//! and killing it (kill plumbing is out of scope for the brain; noted).

use protocol::Vec3;

use super::{TickContext, nearest_player_where, unit_f32};
use crate::rules::flat_direction;

/// Ticks between teleports while flanking (0.75 s at 60 Hz).
pub const OVERLOOK_FLANK_INTERVAL_TICKS: u32 = 45;
/// Teleports before the sight line locks.
pub const OVERLOOK_FLANKS: u32 = 4;
/// Teleport offset radius around the marked player (units).
pub const OVERLOOK_TELEPORT_RADIUS: f32 = 6.0;
/// Sight-line telegraph duration, in ticks (1.6 s at 60 Hz; tier 2: ≥ 1.0 s).
pub const OVERLOOK_SIGHT_LINE_TICKS: u32 = 96;
/// Laser duration once fired, in ticks (3.0 s at 60 Hz).
pub const OVERLOOK_LASER_TICKS: u32 = 180;
/// Laser length (units).
pub const OVERLOOK_LASER_RANGE: f32 = 30.0;
/// Laser beam radius — the exact hit volume shown during the telegraph.
pub const OVERLOOK_LASER_RADIUS: f32 = 0.6;
/// Fraction of the way the beam turns toward the marked player each tick.
pub const OVERLOOK_LASER_STEER: f32 = 0.10;
/// Cooldown after the laser ends, in ticks (2.0 s at 60 Hz).
pub const OVERLOOK_COOLDOWN_TICKS: u32 = 120;
/// Cap on stored glowing marks (oldest dropped).
pub const OVERLOOK_MARKS_MAX: usize = 8;
/// Cap on recorded laser path points (3.0 s at 60 Hz).
pub const OVERLOOK_PATH_MAX: usize = 180;

/// Overlook phases.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OverlookPhase {
    /// Teleports around the marked player, leaving glowing marks.
    Flank,
    /// 1.6 s sight line to the marked player; the glowing mark flashes.
    Mark,
    /// Slow tracking laser; only the marked player is harmed.
    Fire,
    /// Cooldown before the next hunt.
    Cooldown,
}

/// Overlook brain state.
#[derive(Debug, Clone, PartialEq)]
pub struct OverlookBrain {
    /// Current phase.
    pub phase: OverlookPhase,
    /// Phase countdown in ticks.
    pub timer: u32,
    /// The player the laser is for (and only for).
    pub marked: Option<u32>,
    /// Glowing marks left at each teleport spot (newest last).
    pub marks: Vec<Vec3>,
    /// Unit horizontal direction of the current sight line / laser.
    pub sight_dir: Vec3,
    /// Locked laser direction (unit, horizontal); steered during `Fire`.
    pub laser_dir: Vec3,
    /// Recorded beam tips; exposed to teammates only during collapse.
    pub laser_path: Vec<Vec3>,
    /// Random seed for teleport placement.
    pub seed: u32,
    /// Teleports left in this hunt.
    pub flanks_left: u32,
    /// Ticks until the next teleport.
    pub flank_in: u32,
}

impl Default for OverlookBrain {
    fn default() -> Self {
        Self {
            phase: OverlookPhase::Flank,
            timer: 0,
            marked: None,
            marks: Vec::new(),
            sight_dir: Vec3::new(0.0, 0.0, 1.0),
            laser_dir: Vec3::new(1.0, 0.0, 0.0),
            laser_path: Vec::new(),
            seed: 0,
            flanks_left: OVERLOOK_FLANKS,
            flank_in: 0,
        }
    }
}

impl OverlookBrain {
    /// The laser path for teammates to draw. Empty outside the collapse phase
    /// or before the laser has fired.
    pub fn drawn_path(&self, collapse_phase: bool) -> &[Vec3] {
        if collapse_phase { &self.laser_path } else { &[] }
    }
}

/// What happened this tick.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct OverlookStep {
    /// True while the sight line telegraph is up (VFX/test hook).
    pub telegraphing: bool,
    /// True when the laser crossed the MARKED player this tick.
    pub marked_hit: bool,
    /// True when the laser path may be drawn by teammates (collapse phase).
    pub path_public: bool,
}

/// Random point in a circle around the player, rejection-sampled (no trig).
fn teleport_offset(brain: &mut OverlookBrain) -> Vec3 {
    for _ in 0..8 {
        let x = (unit_f32(&mut brain.seed) * 2.0 - 1.0) * OVERLOOK_TELEPORT_RADIUS;
        let z = (unit_f32(&mut brain.seed) * 2.0 - 1.0) * OVERLOOK_TELEPORT_RADIUS;
        if x * x + z * z <= OVERLOOK_TELEPORT_RADIUS * OVERLOOK_TELEPORT_RADIUS {
            return Vec3::new(x, 0.0, z);
        }
    }
    Vec3::new(OVERLOOK_TELEPORT_RADIUS, 0.0, 0.0)
}

/// Normalised horizontal direction; falls back to +X.
fn normalize_flat(d: Vec3) -> Vec3 {
    let d2 = d.x * d.x + d.z * d.z;
    if d2 <= 1e-9 {
        return Vec3::new(1.0, 0.0, 0.0);
    }
    let inv = 1.0 / d2.sqrt();
    Vec3::new(d.x * inv, 0.0, d.z * inv)
}

/// Distance from `p` to segment `a`..`b` (sqrt only).
fn point_segment_distance(p: &Vec3, a: &Vec3, b: &Vec3) -> f32 {
    let abx = b.x - a.x;
    let aby = b.y - a.y;
    let abz = b.z - a.z;
    let apx = p.x - a.x;
    let apy = p.y - a.y;
    let apz = p.z - a.z;
    let ab_len2 = abx * abx + aby * aby + abz * abz;
    let t = if ab_len2 > 1e-9 {
        ((apx * abx + apy * aby + apz * abz) / ab_len2).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let cx = a.x + abx * t;
    let cy = a.y + aby * t;
    let cz = a.z + abz * t;
    let dx = p.x - cx;
    let dy = p.y - cy;
    let dz = p.z - cz;
    (dx * dx + dy * dy + dz * dz).sqrt()
}

/// Seed the RNG from the spawn position once (the step API has no enemy id).
fn seed_if_needed(enemy_pos: &Vec3, brain: &mut OverlookBrain) {
    if brain.seed == 0 {
        brain.seed = super::seed_from_pos(enemy_pos);
    }
}

/// Advance Overlook by one tick.
pub fn step_overlook(enemy_pos: &mut Vec3, brain: &mut OverlookBrain, ctx: &TickContext) -> OverlookStep {
    seed_if_needed(enemy_pos, brain);
    let mut out = OverlookStep::default();

    match brain.phase {
        OverlookPhase::Flank => {
            if brain.marked.is_none() {
                // Gliders are never attracted and never marked.
                brain.marked = nearest_player_where(ctx.players, enemy_pos, |p| p.class != "glider").map(|p| p.id);
                if brain.marked.is_none() {
                    brain.phase = OverlookPhase::Cooldown;
                    brain.timer = 30;
                    return out;
                }
                brain.flanks_left = OVERLOOK_FLANKS;
                brain.flank_in = 0;
            }
            if brain.flank_in > 0 {
                brain.flank_in -= 1;
                return out;
            }
            let prey = ctx.players.iter().find(|p| Some(p.id) == brain.marked).map(|p| p.feet);
            let Some(prey) = prey else {
                brain.marked = None;
                brain.phase = OverlookPhase::Cooldown;
                brain.timer = 30;
                return out;
            };
            let off = teleport_offset(brain);
            enemy_pos.x = prey.x + off.x;
            enemy_pos.z = prey.z + off.z;
            brain.marks.push(*enemy_pos);
            if brain.marks.len() > OVERLOOK_MARKS_MAX {
                brain.marks.remove(0);
            }
            brain.flanks_left = brain.flanks_left.saturating_sub(1);
            if brain.flanks_left == 0 {
                brain.phase = OverlookPhase::Mark;
                brain.timer = OVERLOOK_SIGHT_LINE_TICKS;
            } else {
                brain.flank_in = OVERLOOK_FLANK_INTERVAL_TICKS;
            }
        }
        OverlookPhase::Mark => {
            out.telegraphing = true;
            if let Some(prey) = ctx.players.iter().find(|p| Some(p.id) == brain.marked).map(|p| p.feet) {
                brain.sight_dir = flat_direction(enemy_pos, &prey);
            }
            if brain.timer > 0 {
                brain.timer -= 1;
            }
            if brain.timer == 0 {
                brain.phase = OverlookPhase::Fire;
                brain.timer = OVERLOOK_LASER_TICKS;
                brain.laser_dir = brain.sight_dir;
                brain.laser_path.clear();
            }
        }
        OverlookPhase::Fire => {
            let prey_feet = ctx.players.iter().find(|p| Some(p.id) == brain.marked).map(|p| p.feet);
            if let Some(prey) = prey_feet {
                let desired = flat_direction(enemy_pos, &prey);
                brain.laser_dir = normalize_flat(Vec3::new(
                    brain.laser_dir.x + (desired.x - brain.laser_dir.x) * OVERLOOK_LASER_STEER,
                    0.0,
                    brain.laser_dir.z + (desired.z - brain.laser_dir.z) * OVERLOOK_LASER_STEER,
                ));
                // ONLY the marked player can see and be harmed by the laser.
                let beam_end = Vec3::new(
                    enemy_pos.x + brain.laser_dir.x * OVERLOOK_LASER_RANGE,
                    enemy_pos.y,
                    enemy_pos.z + brain.laser_dir.z * OVERLOOK_LASER_RANGE,
                );
                // The beam is a horizontal line at the enemy's own height, so measure
                // the player's FEET against it and require them to be within a body
                // height of that plane. Using the player's centre put the point 0.9 u
                // above the beam, outside the 0.6 u radius, and the beam never hit.
                let centre = Vec3::new(prey.x, enemy_pos.y, prey.z);
                if point_segment_distance(&centre, enemy_pos, &beam_end) <= OVERLOOK_LASER_RADIUS
                    && (prey.y - enemy_pos.y).abs() <= 1.0
                {
                    out.marked_hit = true;
                }
            }
            let tip = Vec3::new(
                enemy_pos.x + brain.laser_dir.x * OVERLOOK_LASER_RANGE,
                enemy_pos.y,
                enemy_pos.z + brain.laser_dir.z * OVERLOOK_LASER_RANGE,
            );
            brain.laser_path.push(tip);
            if brain.laser_path.len() > OVERLOOK_PATH_MAX {
                brain.laser_path.remove(0);
            }
            out.path_public = ctx.collapse_phase;
            if brain.timer > 0 {
                brain.timer -= 1;
            }
            if brain.timer == 0 {
                brain.phase = OverlookPhase::Cooldown;
                brain.timer = OVERLOOK_COOLDOWN_TICKS;
                brain.marked = None;
                brain.flanks_left = OVERLOOK_FLANKS;
            }
        }
        OverlookPhase::Cooldown => {
            if brain.timer > 0 {
                brain.timer -= 1;
            } else {
                brain.phase = OverlookPhase::Flank;
                brain.flank_in = 0;
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

    fn run_into_phase(
        brain: &mut OverlookBrain,
        pos: &mut Vec3,
        players: &[super::super::PlayerSample<'static>],
        target: OverlookPhase,
    ) {
        let mut guard = 0;
        loop {
            guard += 1;
            assert!(guard < 10_000, "overlook must reach {target:?}");
            step_overlook(pos, brain, &ctx(players, false));
            if brain.phase == target {
                return;
            }
        }
    }

    #[test]
    fn overlook_flanks_marks_and_laser_hits_only_the_marked() {
        let mut brain = OverlookBrain::default();
        let mut pos = Vec3::new(5.0, 0.0, 5.0);
        let players = vec![sample_player(1, 9.0, 5.0, "striker")];
        run_into_phase(&mut brain, &mut pos, &players, OverlookPhase::Mark);
        assert_eq!(brain.marks.len(), OVERLOOK_FLANKS as usize);

        run_into_phase(&mut brain, &mut pos, &players, OverlookPhase::Fire);
        let mut hit = false;
        for _ in 0..5 {
            let out = step_overlook(&mut pos, &mut brain, &ctx(&players, false));
            if out.marked_hit {
                hit = true;
            }
        }
        assert!(hit, "beam starts aimed at the marked player");
        assert_eq!(brain.drawn_path(false).len(), 0, "path hidden outside collapse");
        assert!(!brain.drawn_path(true).is_empty(), "path drawable during collapse");
    }

    #[test]
    fn overlook_only_harms_the_marked_player() {
        let mut brain = OverlookBrain::default();
        let mut pos = Vec3::new(5.0, 0.0, 5.0);
        let marked = vec![sample_player(1, 9.0, 5.0, "striker")];
        run_into_phase(&mut brain, &mut pos, &marked, OverlookPhase::Fire);
        // The marked player is gone; an unmarked player sits in the beam line.
        let bystanders = vec![sample_player(2, 9.0, 5.0, "striker")];
        for _ in 0..5 {
            let out = step_overlook(&mut pos, &mut brain, &ctx(&bystanders, false));
            assert!(!out.marked_hit, "unmarked players are never harmed");
        }
    }

    #[test]
    fn glider_is_never_attracted_and_never_marked() {
        let mut brain = OverlookBrain::default();
        let mut pos = Vec3::new(5.0, 0.0, 5.0);
        let gliders = vec![sample_player(4, 6.0, 5.0, "glider")];
        let mut guard = 0;
        while guard < 300 {
            guard += 1;
            step_overlook(&mut pos, &mut brain, &ctx(&gliders, false));
        }
        assert!(brain.marked.is_none());
    }

    #[test]
    fn sight_line_telegraph_meets_tier2_and_lasts_full_duration() {
        // Tier 2: decal/line for 1.0–2.0 s before the hit.
        let seconds = OVERLOOK_SIGHT_LINE_TICKS as f32 / 60.0;
        assert!((1.0..=2.0).contains(&seconds));

        let mut brain = OverlookBrain::default();
        let mut pos = Vec3::new(5.0, 0.0, 5.0);
        let players = vec![sample_player(1, 9.0, 5.0, "striker")];
        run_into_phase(&mut brain, &mut pos, &players, OverlookPhase::Mark);
        let mut telegraph_ticks = 0_u32;
        let mut guard = 0;
        loop {
            guard += 1;
            assert!(guard < 10_000);
            let out = step_overlook(&mut pos, &mut brain, &ctx(&players, false));
            if out.telegraphing {
                telegraph_ticks += 1;
            }
            if brain.phase == OverlookPhase::Fire {
                break;
            }
        }
        assert_eq!(telegraph_ticks, OVERLOOK_SIGHT_LINE_TICKS);
    }
}
