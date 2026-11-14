//! Pure, deterministic game rules: gift pickups, the beacon, enemy spawning and
//! enemy data shared by the server and the client-side predictor.
//!
//! Everything here is a plain function over protocol data so the authoritative
//! server and the client-side predictor can run the exact same code. Only
//! `+ - * /`, `sqrt`, `abs`, `min`, `max` and `clamp` are used on floats, so
//! results are bit-identical on every target.

use protocol::{Aabb, Gift, Vec3};

use crate::controller::player_box;

/// A gift is collected when the player centre is within this distance of it.
pub const GIFT_RADIUS: f32 = 1.1;
/// Horizontal radius in which the beacon counts as reached.
pub const BEACON_RADIUS: f32 = 1.5;
/// Vertical tolerance for standing on the beacon's platform.
pub const BEACON_HEIGHT: f32 = 1.0;

/// Behaviour tag: pallbearer (behaviour order 0..=8, one per enemy id).
/// Legacy kind tag used by the hand-made test levels (kind is a content index now).
pub const ENEMY_KIND_CHASER: u8 = 0;

pub const BEHAVIOR_PALLBEARER: u8 = 0;
/// Behaviour tag: snapshrike.
pub const BEHAVIOR_SNAPSHRIKE: u8 = 1;
/// Behaviour tag: moteling.
pub const BEHAVIOR_MOTELING: u8 = 2;
/// Behaviour tag: rift warden.
pub const BEHAVIOR_RIFT_WARDEN: u8 = 3;
/// Behaviour tag: gale wight.
pub const BEHAVIOR_GALE_WIGHT: u8 = 4;
/// Behaviour tag: lantern leech.
pub const BEHAVIOR_LANTERN_LEECH: u8 = 5;
/// Behaviour tag: turret (bolt sentry).
pub const BEHAVIOR_TURRET: u8 = 6;
/// Behaviour tag: mimic (false gift).
pub const BEHAVIOR_MIMIC: u8 = 7;
/// Behaviour tag: corrupter (crumble wisp).
pub const BEHAVIOR_CORRUPTER: u8 = 8;

/// Ticks a freshly spawned enemy stays dormant when spawned without explicit params (4 s at 20 Hz).
pub const DEFAULT_GRACE_TICKS: u32 = 80;
/// Pallbearer spawn dormancy (design: the lantern glows, wake at 4.5 s).
pub const PALLBEARER_WAKE_TICKS: u32 = 90;

/// Fraction of the drawn body radius that actually shoves (and that `enemy_touches` uses).
pub const ENEMY_HIT_FORGIVENESS: f32 = 0.85;

/// Per-enemy tuning supplied at spawn (content + curses). Per-enemy design numbers live
/// in `crate::hazards` as named constants.
#[derive(Debug, Clone, PartialEq)]
pub struct EnemyParams {
    /// Which state machine drives this enemy (`BEHAVIOR_*`).
    pub behavior: u8,
    /// Phase-2 / curse speed multiplier applied to the enemy's movement.
    pub speed_mul: f32,
    /// Drawn body radius, reported in `EnemyView.radius`.
    pub size: f32,
    /// Ticks the enemy stays dormant after spawning.
    pub grace_ticks: u32,
}

impl Default for EnemyParams {
    fn default() -> Self {
        Self {
            behavior: BEHAVIOR_PALLBEARER,
            speed_mul: 1.0,
            size: 1.0,
            grace_ticks: DEFAULT_GRACE_TICKS,
        }
    }
}

/// Pallbearer phases.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PallbearerPhase {
    /// Airborne drift toward the target's breadcrumb from 3 s ago.
    Hunt,
    /// Stops, rears back; the telegraph before the lunge.
    Kneel,
    /// Committed dash toward the locked point; lethal on contact.
    Lunge,
    /// 2 s of harmless drift after the lunge.
    Recover,
}

/// Snapshrike phases.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SnapshrikePhase {
    /// Orbits its nearest target at 4-7 u.
    Stalk,
    /// 0.75 s aim; tracks for the first 0.4 s, frozen for the last 0.35 s.
    Lock,
    /// Straight dash at 13.5 u/s; lethal on contact.
    Dash,
    /// 0.6 s stunned after its dash was blocked by a platform.
    Stunned,
    /// 1.3 s of harmless drift after the dash.
    Recover,
}

/// Moteling phases.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MotelingPhase {
    /// 3.6 u/s drift; engages when a player is within 20 u.
    Wander,
    /// Circles the marked player at 5 u radius.
    Orbit,
    /// 2.0 s collapsing ring (heavy); lethal unless the marked player stands still.
    Inhale,
    /// 4.0 s slow drift after an inhale cycle.
    Cooldown,
}

/// Rift warden phases.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RiftPhase {
    /// 1.6 u/s hover toward the nearest player; shoves on contact.
    Haunt,
    /// 1.1 s rune telegraph.
    Ignite,
    /// Radial displacement pulse (3.5 u, 8 u/s + 2 u/s up, 0.9 s stagger).
    Shove,
    /// 3.5 s cooldown (slow creep).
    Settle,
}

/// Gale wight phases.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GalePhase {
    /// Wanders at map-top + 6 u altitude; at least 4 s between volleys.
    Cruise,
    /// 0.9 s circle tracking the target with 0.35 s position lag (heavy).
    Mark,
    /// 0.6 s fixed circle (heavy).
    Lock,
    /// Shell is falling; 1.1 s flight time.
    Fall,
    /// 3.5 s between volleys.
    Downtime,
}

/// Lantern leech phases.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LeechPhase {
    /// 3.2 u/s drift within 24 u of any player; hunts gift-pickup events.
    Prowl,
    /// Locked onto the newest pickup point; accelerates to 9.5 u/s.
    Siphon,
    /// Sits on the gorge point; rear-back then sting (lethal 1.5 u).
    Gorge,
    /// Leaves the scene; 3.0 s minimum before the next siphon.
    Slide,
}

/// Turret phases.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TurretPhase {
    /// Rotates toward the nearest player with clear line of sight.
    Seek,
    /// 0.95 s charge; aim line to the predicted intercept.
    Charge,
    /// Bolt leaves the muzzle (2 ticks, for VFX).
    Fire,
    /// 1.4 s venting smoke; cannot fire.
    Vent,
}

/// Mimic phases.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MimicPhase {
    /// Sits as a gift prop; shimmers every 3.0 s.
    Cached,
    /// 0.9 s reveal telegraph when a player comes within 4 u.
    Hiss,
    /// 8.5 u/s hop toward the triggering player; lethal on contact.
    Lunge,
    /// 2.5 s inert wobble after the lunge.
    Spent,
}

/// Corrupter phases.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CorrupterPhase {
    /// 3.0 u/s drift toward the marked platform; aura frets it after 2 s.
    Drift,
    /// 1.5 s withdrawal after a platform is fretted.
    ReForm,
}

/// Pallbearer brain state.
#[derive(Debug, Clone, PartialEq)]
pub struct PallbearerBrain {
    pub phase: PallbearerPhase,
    /// Phase countdown in ticks.
    pub timer: u32,
    /// Point locked at the start of the kneel; the lunge goes there.
    pub locked: Vec3,
}

impl Default for PallbearerBrain {
    fn default() -> Self {
        Self {
            phase: PallbearerPhase::Hunt,
            timer: 0,
            locked: Vec3::new(0.0, 0.0, 0.0),
        }
    }
}

/// Snapshrike brain state.
#[derive(Debug, Clone, PartialEq)]
pub struct SnapshrikeBrain {
    pub phase: SnapshrikePhase,
    /// Phase countdown in ticks.
    pub timer: u32,
    /// Dash-to-dash cooldown, ticks remaining.
    pub dash_cooldown: u32,
    /// Predicted intercept (telegraph anchor); frozen during the last 0.35 s of `Lock`.
    pub aim: Vec3,
    /// Locked dash direction.
    pub dash_dir: Vec3,
    /// Distance covered during the current dash, in units.
    pub dash_dist: f32,
    /// The player currently locked onto.
    pub target_id: Option<u32>,
}

impl Default for SnapshrikeBrain {
    fn default() -> Self {
        Self {
            phase: SnapshrikePhase::Stalk,
            timer: 0,
            dash_cooldown: 0,
            aim: Vec3::new(0.0, 0.0, 0.0),
            dash_dir: Vec3::new(1.0, 0.0, 0.0),
            dash_dist: 0.0,
            target_id: None,
        }
    }
}

/// Moteling brain state.
#[derive(Debug, Clone, PartialEq)]
pub struct MotelingBrain {
    pub phase: MotelingPhase,
    /// Phase countdown in ticks.
    pub timer: u32,
    /// Ticks spent in orbit (drives the 6.0 s inhale cadence).
    pub orbit_ticks: u32,
    /// The marked player id (re-marked after each inhale cycle).
    pub marked: Option<u32>,
    /// Ticks spent waiting for the heavy slot while frozen at the first frame.
    pub heavy_wait: u32,
    /// True once the heavy mutex has been granted for this inhale.
    pub acquired: bool,
    /// Last known position of the marked player (ring anchor).
    pub anchor: Vec3,
    /// Wander target while drifting.
    pub wander_goal: Option<Vec3>,
}

impl Default for MotelingBrain {
    fn default() -> Self {
        Self {
            phase: MotelingPhase::Wander,
            timer: 0,
            orbit_ticks: 0,
            marked: None,
            heavy_wait: 0,
            acquired: false,
            anchor: Vec3::new(0.0, 0.0, 0.0),
            wander_goal: None,
        }
    }
}

/// Rift warden brain state.
#[derive(Debug, Clone, PartialEq)]
pub struct RiftWardenBrain {
    pub phase: RiftPhase,
    /// Phase countdown in ticks.
    pub timer: u32,
}

impl Default for RiftWardenBrain {
    fn default() -> Self {
        Self {
            phase: RiftPhase::Haunt,
            timer: 0,
        }
    }
}

/// Gale wight brain state. One `Mark`/`Lock`/`Fall` pipeline per shell; the whole
/// volley holds the heavy mutex.
#[derive(Debug, Clone, PartialEq)]
pub struct GaleWightBrain {
    pub phase: GalePhase,
    /// Phase countdown in ticks.
    pub timer: u32,
    /// Shells still to be launched in this volley.
    pub shells_left: u32,
    /// The player this shell is tracking.
    pub target_id: Option<u32>,
    /// The (possibly tracking) circle anchor; fixed during `Lock`.
    pub anchor: Vec3,
    /// Recent target feet positions used for the 0.35 s position lag.
    pub lag: Vec<Vec3>,
    /// Cruise waypoint.
    pub cruise_goal: Option<Vec3>,
    /// Ticks spent cruising (must reach 4.0 s before the next volley).
    pub cruise_elapsed: u32,
    /// Ticks spent waiting for the heavy slot while frozen at the first frame.
    pub heavy_wait: u32,
    /// True once the heavy mutex has been granted for this volley.
    pub acquired: bool,
}

impl Default for GaleWightBrain {
    fn default() -> Self {
        Self {
            phase: GalePhase::Cruise,
            timer: 0,
            shells_left: 0,
            target_id: None,
            anchor: Vec3::new(0.0, 0.0, 0.0),
            lag: Vec::new(),
            cruise_goal: None,
            cruise_elapsed: 0,
            heavy_wait: 0,
            acquired: false,
        }
    }
}

/// Lantern leech brain state.
#[derive(Debug, Clone, PartialEq)]
pub struct LanternLeechBrain {
    pub phase: LeechPhase,
    /// Phase countdown / ramp timer in ticks.
    pub timer: u32,
    /// The locked pickup point (siphon goal / gorge point).
    pub goal: Vec3,
    /// Direction the leech drifts away in during `Slide`.
    pub away: Vec3,
}

impl Default for LanternLeechBrain {
    fn default() -> Self {
        Self {
            phase: LeechPhase::Prowl,
            timer: 0,
            goal: Vec3::new(0.0, 0.0, 0.0),
            away: Vec3::new(1.0, 0.0, 0.0),
        }
    }
}

/// Turret brain state.
#[derive(Debug, Clone, PartialEq)]
pub struct TurretBrain {
    pub phase: TurretPhase,
    /// Phase countdown in ticks.
    pub timer: u32,
    /// The player being tracked.
    pub target_id: Option<u32>,
    /// Predicted intercept (telegraph anchor); frozen if line of sight breaks.
    pub aim: Vec3,
}

impl Default for TurretBrain {
    fn default() -> Self {
        Self {
            phase: TurretPhase::Seek,
            timer: 0,
            target_id: None,
            aim: Vec3::new(0.0, 0.0, 0.0),
        }
    }
}

/// Mimic brain state.
#[derive(Debug, Clone, PartialEq)]
pub struct MimicBrain {
    pub phase: MimicPhase,
    /// Phase countdown in ticks.
    pub timer: u32,
    /// Shimmer cycle counter (0..60; the last 10 ticks shimmer).
    pub shimmer: u32,
    /// The triggering player (locked during `Hiss`).
    pub trigger: Option<u32>,
    /// Last known position of the triggering player (telegraph anchor).
    pub anchor: Vec3,
    /// Locked lunge direction.
    pub lunge_dir: Vec3,
}

impl Default for MimicBrain {
    fn default() -> Self {
        Self {
            phase: MimicPhase::Cached,
            timer: 0,
            shimmer: 0,
            trigger: None,
            anchor: Vec3::new(0.0, 0.0, 0.0),
            lunge_dir: Vec3::new(1.0, 0.0, 0.0),
        }
    }
}

/// Corrupter brain state.
#[derive(Debug, Clone, PartialEq)]
pub struct CorrupterBrain {
    pub phase: CorrupterPhase,
    /// Phase countdown in ticks.
    pub timer: u32,
    /// The platform currently inside the aura (fret target).
    pub mark: Option<u16>,
    /// Continuous ticks the marked platform has spent inside the aura.
    pub fret_ticks: u32,
    /// Direction the corrupter withdraws in during `ReForm`.
    pub away: Vec3,
}

impl Default for CorrupterBrain {
    fn default() -> Self {
        Self {
            phase: CorrupterPhase::Drift,
            timer: 0,
            mark: None,
            fret_ticks: 0,
            away: Vec3::new(1.0, 0.0, 0.0),
        }
    }
}

/// Per-enemy state machine payload; the variant matches `params.behavior`.
#[derive(Debug, Clone, PartialEq)]
pub enum Brain {
    Pallbearer(PallbearerBrain),
    Snapshrike(SnapshrikeBrain),
    Moteling(MotelingBrain),
    RiftWarden(RiftWardenBrain),
    GaleWight(GaleWightBrain),
    LanternLeech(LanternLeechBrain),
    Turret(TurretBrain),
    Mimic(MimicBrain),
    Corrupter(CorrupterBrain),
}

impl Brain {
    /// Default brain for a behaviour tag; unknown behaviours fall back to pallbearer.
    pub fn for_behavior(behavior: u8) -> Self {
        match behavior {
            BEHAVIOR_SNAPSHRIKE => Brain::Snapshrike(SnapshrikeBrain::default()),
            BEHAVIOR_MOTELING => Brain::Moteling(MotelingBrain::default()),
            BEHAVIOR_RIFT_WARDEN => Brain::RiftWarden(RiftWardenBrain::default()),
            BEHAVIOR_GALE_WIGHT => Brain::GaleWight(GaleWightBrain::default()),
            BEHAVIOR_LANTERN_LEECH => Brain::LanternLeech(LanternLeechBrain::default()),
            BEHAVIOR_TURRET => Brain::Turret(TurretBrain::default()),
            BEHAVIOR_MIMIC => Brain::Mimic(MimicBrain::default()),
            BEHAVIOR_CORRUPTER => Brain::Corrupter(CorrupterBrain::default()),
            _ => Brain::Pallbearer(PallbearerBrain::default()),
        }
    }
}

/// A single enemy. `home` is where it drifts back to when nothing is in range.
#[derive(Debug, Clone, PartialEq)]
pub struct Enemy {
    /// Unique enemy id chosen by the server.
    pub id: u32,
    /// Content kind id (`design/content.json`); reported verbatim in `EnemyView.kind`.
    pub kind: u8,
    /// World position of the enemy's centre.
    pub pos: Vec3,
    /// Home position it drifts back to when idle.
    pub home: Vec3,
    /// Ticks until the enemy becomes active (spawn grace); 0 = active.
    pub active_in: u32,
    /// Spawn-time tuning (behaviour, speed multiplier, body radius, grace).
    pub params: EnemyParams,
    /// Knockback velocity (u/s) applied while `knock_ticks` lasts (Bulwark pulse, rift shove).
    pub knock_vel: Vec3,
    /// Ticks left of the knockback stagger; while set, the brain does not run.
    pub knock_ticks: u8,
    /// Per-enemy state machine payload.
    pub brain: Brain,
}

/// Euclidean distance between two points.
pub(crate) fn distance(a: &Vec3, b: &Vec3) -> f32 {
    let dx = a.x - b.x;
    let dy = a.y - b.y;
    let dz = a.z - b.z;
    (dx * dx + dy * dy + dz * dz).sqrt()
}

/// Horizontal (XZ) distance between two points.
pub(crate) fn horizontal_distance(a: &Vec3, b: &Vec3) -> f32 {
    let dx = a.x - b.x;
    let dz = a.z - b.z;
    (dx * dx + dz * dz).sqrt()
}

/// The player's centre point, derived from their feet position.
pub(crate) fn player_centre(player_feet: &Vec3) -> Vec3 {
    Vec3::new(player_feet.x, player_feet.y + 0.9, player_feet.z)
}

/// Move `pos` toward `target` by at most `max_step` (3D), never overshooting.
pub(crate) fn move_toward(pos: &mut Vec3, target: &Vec3, max_step: f32) {
    let dx = target.x - pos.x;
    let dy = target.y - pos.y;
    let dz = target.z - pos.z;
    let d = (dx * dx + dy * dy + dz * dz).sqrt();
    if d <= max_step {
        *pos = *target;
        return;
    }
    if d <= 1e-9 {
        return;
    }
    let scale = max_step / d;
    pos.x += dx * scale;
    pos.y += dy * scale;
    pos.z += dz * scale;
}

/// Normalised 3D direction from `from` to `to`; falls back to +X when they coincide.
pub(crate) fn dir_normalized(from: &Vec3, to: &Vec3) -> Vec3 {
    let dx = to.x - from.x;
    let dy = to.y - from.y;
    let dz = to.z - from.z;
    let d = (dx * dx + dy * dy + dz * dz).sqrt();
    if d <= 1e-9 {
        return Vec3::new(1.0, 0.0, 0.0);
    }
    Vec3::new(dx / d, dy / d, dz / d)
}

/// Normalised horizontal (XZ) direction from `from` to `to`; falls back to +X when coincident.
pub(crate) fn flat_direction(from: &Vec3, to: &Vec3) -> Vec3 {
    let dx = to.x - from.x;
    let dz = to.z - from.z;
    let d = (dx * dx + dz * dz).sqrt();
    if d <= 1e-9 {
        return Vec3::new(1.0, 0.0, 0.0);
    }
    Vec3::new(dx / d, 0.0, dz / d)
}

/// Squared distance from a point to an AABB (0 when the point is inside).
pub(crate) fn aabb_distance(p: &Vec3, b: &Aabb) -> f32 {
    let dx = (b.min.x - p.x).max(0.0).max(p.x - b.max.x);
    let dy = (b.min.y - p.y).max(0.0).max(p.y - b.max.y);
    let dz = (b.min.z - p.z).max(0.0).max(p.z - b.max.z);
    (dx * dx + dy * dy + dz * dz).sqrt()
}

/// True when the segment `a`..`b` intersects (or touches) the AABB `bounds`.
pub(crate) fn segment_intersects_aabb(a: &Vec3, b: &Vec3, bounds: &Aabb) -> bool {
    let mut t_min = 0.0_f32;
    let mut t_max = 1.0_f32;
    if !clip_axis(a.x, b.x - a.x, bounds.min.x, bounds.max.x, &mut t_min, &mut t_max) {
        return false;
    }
    if !clip_axis(a.y, b.y - a.y, bounds.min.y, bounds.max.y, &mut t_min, &mut t_max) {
        return false;
    }
    if !clip_axis(a.z, b.z - a.z, bounds.min.z, bounds.max.z, &mut t_min, &mut t_max) {
        return false;
    }
    true
}

/// Clip the slab `[min, max]` on one axis into the `[t_min, t_max]` window; false = no hit.
fn clip_axis(origin: f32, dir: f32, min: f32, max: f32, t_min: &mut f32, t_max: &mut f32) -> bool {
    if dir.abs() <= 1e-9 {
        return origin >= min && origin <= max;
    }
    let inv = 1.0 / dir;
    let mut t0 = (min - origin) * inv;
    let mut t1 = (max - origin) * inv;
    if t0 > t1 {
        std::mem::swap(&mut t0, &mut t1);
    }
    if t0 > *t_min {
        *t_min = t0;
    }
    if t1 < *t_max {
        *t_max = t1;
    }
    *t_min <= *t_max
}

/// True when a sphere at `centre` with `radius` overlaps the player's collision box.
pub(crate) fn sphere_hits_player_box(centre: &Vec3, radius: f32, feet: &Vec3) -> bool {
    let b = player_box(feet);
    let dx = centre.x - centre.x.clamp(b.min.x, b.max.x);
    let dy = centre.y - centre.y.clamp(b.min.y, b.max.y);
    let dz = centre.z - centre.z.clamp(b.min.z, b.max.z);
    dx * dx + dy * dy + dz * dz <= radius * radius
}

/// True when the player's centre is within [`GIFT_RADIUS`] of the gift centre.
pub fn gift_touched(player_feet: &Vec3, gift: &Gift) -> bool {
    gift_touched_within(player_feet, gift, GIFT_RADIUS)
}

/// True when the player's centre is within `radius` of the gift centre.
pub fn gift_touched_within(player_feet: &Vec3, gift: &Gift, radius: f32) -> bool {
    distance(&player_centre(player_feet), &gift.pos) <= radius
}

/// True when the player stands on the beacon: close horizontally and roughly level.
pub fn beacon_reached(player_feet: &Vec3, beacon: &Vec3) -> bool {
    beacon_reached_within(player_feet, beacon, BEACON_RADIUS)
}

/// True when the player is within `radius` horizontally of the beacon and roughly
/// level with it. The vertical tolerance stays [`BEACON_HEIGHT`].
pub fn beacon_reached_within(player_feet: &Vec3, beacon: &Vec3, radius: f32) -> bool {
    let dx = player_feet.x - beacon.x;
    let dz = player_feet.z - beacon.z;
    let horizontal = (dx * dx + dz * dz).sqrt();
    horizontal <= radius && (player_feet.y - beacon.y).abs() <= BEACON_HEIGHT
}

/// Create an enemy sitting at `pos`, with `pos` as its home, default params and a
/// full default spawn grace (pallbearers wait 4.5 s).
pub fn spawn_enemy(id: u32, kind: u8, pos: Vec3) -> Enemy {
    spawn_enemy_with(id, kind, pos, EnemyParams::default())
}

/// Create an enemy with explicit [`EnemyParams`]; `active_in` starts at
/// `params.grace_ticks`, with a 4.5 s floor for pallbearers.
pub fn spawn_enemy_with(id: u32, kind: u8, pos: Vec3, params: EnemyParams) -> Enemy {
    let active_in = if params.behavior == BEHAVIOR_PALLBEARER {
        params.grace_ticks.max(PALLBEARER_WAKE_TICKS)
    } else {
        params.grace_ticks
    };
    Enemy {
        id,
        kind,
        home: pos,
        pos,
        active_in,
        brain: Brain::for_behavior(params.behavior),
        knock_vel: Vec3::new(0.0, 0.0, 0.0),
        knock_ticks: 0,
        params,
    }
}

/// Fraction of the drawn body radius that actually kills for `enemy_touches`.
pub fn enemy_hit_radius(enemy: &Enemy) -> f32 {
    enemy.params.size * ENEMY_HIT_FORGIVENESS
}

/// True when the enemy's hit sphere overlaps the player's collision box.
pub fn enemy_touches(enemy: &Enemy, player_feet: &Vec3) -> bool {
    sphere_hits_player_box(&enemy.pos, enemy_hit_radius(enemy), player_feet)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::controller::spawn_state;

    const EPS: f32 = 1e-4;

    fn feet(x: f32, y: f32, z: f32) -> Vec3 {
        Vec3::new(x, y, z)
    }

    #[test]
    fn gift_touched_uses_centre_distance() {
        let player = feet(0.0, 0.0, 0.0);
        let near = Gift {
            id: 0,
            pos: Vec3::new(0.0, 1.9, 0.0),
            kind: protocol::GIFT_NORMAL,
        };
        let far = Gift {
            id: 1,
            pos: Vec3::new(0.0, 2.1, 0.0),
            kind: protocol::GIFT_NORMAL,
        };
        assert!(gift_touched(&player, &near));
        assert!(!gift_touched(&player, &far));
    }

    #[test]
    fn beacon_reached_checks_radius_and_height() {
        let beacon = Vec3::new(0.0, 0.0, 0.0);
        assert!(beacon_reached(&feet(0.0, 0.0, 0.0), &beacon));
        assert!(!beacon_reached(&feet(2.0, 0.0, 0.0), &beacon));
        assert!(!beacon_reached(&feet(0.0, 3.0, 0.0), &beacon));
    }

    #[test]
    fn radius_helpers_take_explicit_radii() {
        let player = feet(0.0, 0.0, 0.0);
        let gift = Gift {
            id: 0,
            pos: Vec3::new(0.0, 1.9, 0.0),
            kind: protocol::GIFT_NORMAL,
        };
        assert!(gift_touched(&player, &gift));
        assert!(gift_touched_within(&player, &gift, GIFT_RADIUS));
        assert!(gift_touched_within(&player, &gift, 1.0));
        assert!(!gift_touched_within(&player, &gift, 0.5));

        let beacon = Vec3::new(0.0, 0.0, 0.0);
        assert!(beacon_reached(&player, &beacon));
        assert!(beacon_reached_within(&player, &beacon, BEACON_RADIUS));
        assert!(!beacon_reached_within(&feet(2.0, 0.0, 0.0), &beacon, BEACON_RADIUS));
        assert!(beacon_reached_within(&feet(2.0, 0.0, 0.0), &beacon, 2.5));
        assert!(!beacon_reached_within(&feet(0.0, 3.0, 0.0), &beacon, 100.0));
    }

    #[test]
    fn enemy_touches_player_box() {
        let mut enemy = spawn_enemy(0, 1, Vec3::new(0.0, 0.9, 0.0));
        let player = feet(0.0, 0.0, 0.0);
        assert!(enemy_touches(&enemy, &player));
        enemy.pos = Vec3::new(2.4, 0.9, 0.0);
        assert!(!enemy_touches(&enemy, &player));
    }

    #[test]
    fn spawn_enemy_uses_default_params() {
        let enemy = spawn_enemy(3, 7, Vec3::new(1.0, 2.0, 3.0));
        assert_eq!(enemy.params, EnemyParams::default());
        assert_eq!(enemy.params.speed_mul, 1.0);
        assert_eq!(enemy.params.grace_ticks, DEFAULT_GRACE_TICKS);
        assert_eq!(enemy.active_in, PALLBEARER_WAKE_TICKS, "pallbearer floor is 4.5 s");
        assert_eq!(enemy.home, Vec3::new(1.0, 2.0, 3.0));
        assert_eq!(enemy.pos, Vec3::new(1.0, 2.0, 3.0));
    }

    #[test]
    fn spawn_enemy_with_honours_grace_and_pallbearer_floor() {
        let params = EnemyParams {
            behavior: BEHAVIOR_SNAPSHRIKE,
            speed_mul: 1.5,
            size: 0.7,
            grace_ticks: 20,
        };
        let shrike = spawn_enemy_with(1, 2, Vec3::new(0.0, 0.0, 0.0), params);
        assert_eq!(shrike.active_in, 20);
        assert_eq!(shrike.params.speed_mul, 1.5);

        let pallbearer = spawn_enemy_with(
            2,
            3,
            Vec3::new(0.0, 0.0, 0.0),
            EnemyParams {
                behavior: BEHAVIOR_PALLBEARER,
                speed_mul: 1.0,
                size: 1.0,
                grace_ticks: 10,
            },
        );
        assert_eq!(pallbearer.active_in, PALLBEARER_WAKE_TICKS);
    }

    #[test]
    fn for_behavior_unknown_falls_back_to_pallbearer() {
        assert!(matches!(Brain::for_behavior(99), Brain::Pallbearer(_)));
        assert!(matches!(Brain::for_behavior(BEHAVIOR_CORRUPTER), Brain::Corrupter(_)));
    }

    #[test]
    fn segment_intersects_aabb_basic_cases() {
        let wall = Aabb {
            min: Vec3::new(4.0, 0.0, -1.0),
            max: Vec3::new(6.0, 1.0, 1.0),
        };
        let a = Vec3::new(0.0, 0.5, 0.0);
        let b = Vec3::new(10.0, 0.5, 0.0);
        assert!(segment_intersects_aabb(&a, &b, &wall));
        let over = Vec3::new(10.0, 5.0, 0.0);
        assert!(!segment_intersects_aabb(&Vec3::new(0.0, 5.0, 0.0), &over, &wall));
        assert!(
            segment_intersects_aabb(&Vec3::new(5.0, 0.5, 0.0), &b, &wall),
            "start inside"
        );
        assert!(!segment_intersects_aabb(
            &Vec3::new(0.0, 3.0, 0.0),
            &Vec3::new(3.5, 3.0, 0.0),
            &wall
        ));
    }

    #[test]
    fn aabb_distance_is_zero_inside() {
        let bounds = Aabb {
            min: Vec3::new(0.0, 0.0, 0.0),
            max: Vec3::new(2.0, 2.0, 2.0),
        };
        assert!(aabb_distance(&Vec3::new(1.0, 1.0, 1.0), &bounds).abs() < EPS);
        assert!((aabb_distance(&Vec3::new(5.0, 1.0, 1.0), &bounds) - 3.0).abs() < EPS);
    }

    #[test]
    fn spawn_state_is_usable() {
        let s = spawn_state(feet(1.0, 0.0, 2.0));
        assert_eq!(s.pos, feet(1.0, 0.0, 2.0));
    }
}
