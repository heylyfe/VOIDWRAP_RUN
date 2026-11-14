//! Pure, deterministic game rules: gift pickups, the beacon, and enemy behaviours.
//!
//! Everything here is a plain function over protocol data so the authoritative server
//! and the client-side predictor can run the exact same code. Only `+ - * /` and
//! `sqrt` are used, so results are bit-identical on every target.

use protocol::{ENEMY_DISGUISED, ENEMY_DORMANT, ENEMY_HUNTING, Gift, TICK_HZ, Vec3};

use crate::controller::player_box;

/// A gift is collected when the player centre is within this distance of it.
pub const GIFT_RADIUS: f32 = 1.1;

/// Horizontal radius in which the beacon counts as reached.
pub const BEACON_RADIUS: f32 = 1.5;

/// Vertical tolerance for standing on the beacon's platform.
pub const BEACON_HEIGHT: f32 = 1.0;

/// Enemy kind tag for the flying chaser.
pub const ENEMY_KIND_CHASER: u8 = 1;

/// Chaser flight speed in units per second (the player runs at 9.0).
pub const CHASER_SPEED: f32 = 5.5;

/// A chaser only pursues targets within this 3D distance.
pub const CHASER_AGGRO_RADIUS: f32 = 22.0;

/// Body radius of the default chaser (the hit sphere is this times [`ENEMY_HIT_FORGIVENESS`]).
pub const CHASER_HALF_SIZE: f32 = 0.5;

/// Ticks a freshly spawned chaser stays dormant (3 s at 20 Hz).
pub const CHASER_GRACE_TICKS: u32 = 60;

/// Behaviour tag: flying chaser (the M1 default).
pub const BEHAVIOR_CHASER: u8 = 0;

/// Behaviour tag: stationary turret that fires projectiles at players in range.
pub const BEHAVIOR_TURRET: u8 = 1;

/// Behaviour tag: gift mimic that reveals itself when a player gets close.
pub const BEHAVIOR_MIMIC: u8 = 2;

/// Behaviour tag: chaser that corrupts the nearest valid platform to crumble.
pub const BEHAVIOR_CORRUPTER: u8 = 3;

/// Default turret firing cadence in ticks (1 s at 20 Hz).
pub const TURRET_FIRE_INTERVAL_TICKS: u32 = 20;

/// Default turret projectile speed in units per second.
pub const TURRET_PROJECTILE_SPEED: f32 = 12.0;

/// Default distance at which a mimic reveals itself.
pub const MIMIC_REVEAL_RADIUS: f32 = 3.0;

/// Default corrupter corruption cadence in ticks (2 s at 20 Hz).
pub const CORRUPTER_INTERVAL_TICKS: u32 = 40;

/// Height of the player's centre above their feet.
const PLAYER_CENTRE_HEIGHT: f32 = 0.9;

/// Per-enemy tuning: flight speed, aggro radius, body radius and grace ticks.
///
/// [`EnemyParams::default`] is exactly the M1 chaser, so an enemy spawned without
/// explicit params behaves as it always did.
#[derive(Debug, Clone, PartialEq)]
pub struct EnemyParams {
    /// Flight speed in units per second.
    pub speed: f32,
    /// 3D distance within which the enemy pursues a target.
    pub aggro: f32,
    /// Body radius: the client draws the enemy this big; see [`enemy_hit_radius`].
    pub size: f32,
    /// Ticks the enemy stays dormant after spawning (does not apply to mimics).
    pub grace_ticks: u32,
    /// Which behaviour drives this enemy (`BEHAVIOR_*`).
    pub behavior: u8,
    /// Ticks between shots for `BEHAVIOR_TURRET`.
    pub fire_interval_ticks: u32,
    /// Projectile speed in units per second for `BEHAVIOR_TURRET`.
    pub projectile_speed: f32,
    /// Distance within which a `BEHAVIOR_MIMIC` reveals itself.
    pub reveal_radius: f32,
    /// Ticks between platform corruption for `BEHAVIOR_CORRUPTER`.
    pub corrupt_interval_ticks: u32,
}

impl Default for EnemyParams {
    fn default() -> Self {
        Self {
            speed: CHASER_SPEED,
            aggro: CHASER_AGGRO_RADIUS,
            size: CHASER_HALF_SIZE,
            grace_ticks: CHASER_GRACE_TICKS,
            behavior: BEHAVIOR_CHASER,
            fire_interval_ticks: TURRET_FIRE_INTERVAL_TICKS,
            projectile_speed: TURRET_PROJECTILE_SPEED,
            reveal_radius: MIMIC_REVEAL_RADIUS,
            corrupt_interval_ticks: CORRUPTER_INTERVAL_TICKS,
        }
    }
}

/// A single enemy. `home` is where it drifts back to when nothing is in range.
#[derive(Debug, Clone, PartialEq)]
pub struct Enemy {
    pub id: u32,
    pub kind: u8,
    pub pos: Vec3,
    pub home: Vec3,
    pub active_in: u32,
    pub params: EnemyParams,
    /// Ticks until a turret may fire / a corrupter may corrupt again (counting down).
    pub cooldown_ticks: u32,
    /// Mimics only: set once a target has got close enough to reveal the enemy for good.
    pub revealed: bool,
}

/// Euclidean distance between two points.
pub(crate) fn distance(a: &Vec3, b: &Vec3) -> f32 {
    let dx = a.x - b.x;
    let dy = a.y - b.y;
    let dz = a.z - b.z;
    (dx * dx + dy * dy + dz * dz).sqrt()
}

/// The player's centre point, derived from their feet position.
pub(crate) fn player_centre(player_feet: &Vec3) -> Vec3 {
    Vec3::new(player_feet.x, player_feet.y + PLAYER_CENTRE_HEIGHT, player_feet.z)
}

/// Move `pos` toward `target` by at most `max_step`, never overshooting.
pub(crate) fn move_toward(pos: &mut Vec3, target: &Vec3, max_step: f32) {
    let dx = target.x - pos.x;
    let dy = target.y - pos.y;
    let dz = target.z - pos.z;
    let d = (dx * dx + dy * dy + dz * dz).sqrt();
    if d <= max_step {
        *pos = target.clone();
        return;
    }
    let scale = max_step / d;
    pos.x += dx * scale;
    pos.y += dy * scale;
    pos.z += dz * scale;
}

/// Index of the nearest target within `aggro`, or `None`. Ties go to the lowest index.
pub(crate) fn nearest_target_within(pos: &Vec3, targets: &[Vec3], aggro: f32) -> Option<usize> {
    let mut nearest: Option<(usize, f32)> = None;
    for (index, target) in targets.iter().enumerate() {
        let d = distance(pos, target);
        if d > aggro {
            continue;
        }
        match nearest {
            Some((_, best)) if best <= d => {}
            _ => nearest = Some((index, d)),
        }
    }
    nearest.map(|(index, _)| index)
}

/// True when any target is within `radius` of `pos`.
pub(crate) fn target_within(pos: &Vec3, targets: &[Vec3], radius: f32) -> bool {
    targets.iter().any(|target| distance(pos, target) <= radius)
}

/// Chaser-family movement shared by chaser/mimic/corrupter: while grace lasts only
/// count down; afterwards fly at the nearest target in aggro, else drift home at
/// half speed.
pub(crate) fn step_hunter(enemy: &mut Enemy, targets: &[Vec3]) {
    if enemy.active_in > 0 {
        enemy.active_in -= 1;
        return;
    }
    let step = enemy.params.speed / TICK_HZ as f32;
    let aggro = enemy.params.aggro;
    match nearest_target_within(&enemy.pos, targets, aggro) {
        Some(index) => {
            let target = targets[index].clone();
            move_toward(&mut enemy.pos, &target, step);
        }
        None => {
            let home = enemy.home.clone();
            move_toward(&mut enemy.pos, &home, step * 0.5);
        }
    }
}

/// Behaviour stepping shared by [`step_enemy`] and `crate::hazards`: turret grace
/// countdown, mimic reveal and chaser-family movement. Firing turrets and
/// corrupting corrupters need level context and are handled in `crate::hazards`.
/// Unknown behaviours do nothing.
pub(crate) fn step_behavior(enemy: &mut Enemy, targets: &[Vec3]) {
    match enemy.params.behavior {
        BEHAVIOR_TURRET => {
            if enemy.active_in > 0 {
                enemy.active_in -= 1;
            }
        }
        BEHAVIOR_MIMIC => {
            if !enemy.revealed {
                if target_within(&enemy.pos, targets, enemy.params.reveal_radius) {
                    enemy.revealed = true;
                } else {
                    return;
                }
            }
            step_hunter(enemy, targets);
        }
        BEHAVIOR_CORRUPTER | BEHAVIOR_CHASER => step_hunter(enemy, targets),
        _ => {}
    }
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
/// full grace period (mimics never get one).
pub fn spawn_enemy(id: u32, kind: u8, pos: Vec3) -> Enemy {
    spawn_enemy_with(id, kind, pos, EnemyParams::default())
}

/// Create an enemy with explicit [`EnemyParams`]; `active_in` starts at
/// `params.grace_ticks`, except for mimics which never wait.
pub fn spawn_enemy_with(id: u32, kind: u8, pos: Vec3, params: EnemyParams) -> Enemy {
    let active_in = if params.behavior == BEHAVIOR_MIMIC {
        0
    } else {
        params.grace_ticks
    };
    Enemy {
        id,
        kind,
        home: pos.clone(),
        pos,
        active_in,
        params,
        cooldown_ticks: 0,
        revealed: false,
    }
}

/// The protocol state to report for `enemy`: `ENEMY_DISGUISED` for an unrevealed
/// mimic, `ENEMY_DORMANT` while grace lasts (and for unknown behaviours), else
/// `ENEMY_HUNTING`.
pub fn enemy_state(enemy: &Enemy) -> u8 {
    match enemy.params.behavior {
        BEHAVIOR_MIMIC => {
            if enemy.revealed {
                ENEMY_HUNTING
            } else {
                ENEMY_DISGUISED
            }
        }
        BEHAVIOR_CHASER | BEHAVIOR_TURRET | BEHAVIOR_CORRUPTER => {
            if enemy.active_in > 0 {
                ENEMY_DORMANT
            } else {
                ENEMY_HUNTING
            }
        }
        _ => ENEMY_DORMANT,
    }
}

/// Advance one enemy by one server tick (`dt = 1 / TICK_HZ`).
///
/// `targets` are the centres of living players. While `active_in > 0` the enemy only
/// counts down. Afterwards it flies straight at the nearest target inside
/// `enemy.params.aggro` (ties go to the lowest index), or drifts home at half speed.
/// Unknown kinds (and unknown behaviours) do nothing at all.
///
/// Turret firing and corrupter platform marking need level context; the server must
/// call `crate::hazards::Hazards::step`, which dispatches behaviour for every enemy
/// (regardless of `kind`) and replaces this function for all of them.
pub fn step_enemy(enemy: &mut Enemy, targets: &[Vec3]) {
    if enemy.kind != ENEMY_KIND_CHASER {
        return;
    }
    step_behavior(enemy, targets);
}

/// Fraction of the drawn body radius that actually kills. Slightly under 1 so a hit
/// always looks like a hit: grazing the glowing edge of the model is survivable.
pub const ENEMY_HIT_FORGIVENESS: f32 = 0.85;

/// Radius of the lethal sphere around the enemy centre.
pub fn enemy_hit_radius(enemy: &Enemy) -> f32 {
    enemy.params.size * ENEMY_HIT_FORGIVENESS
}

/// True when the enemy's hit sphere (see [`enemy_hit_radius`]) overlaps the player's
/// collision box. The sphere matches the drawn body, so there are no invisible corners.
pub fn enemy_touches(enemy: &Enemy, player_feet: &Vec3) -> bool {
    let b = player_box(player_feet);
    let c = &enemy.pos;
    let dx = c.x - c.x.clamp(b.min.x, b.max.x);
    let dy = c.y - c.y.clamp(b.min.y, b.max.y);
    let dz = c.z - c.z.clamp(b.min.z, b.max.z);
    let r = enemy_hit_radius(enemy);
    dx * dx + dy * dy + dz * dz <= r * r
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn chaser_waits_out_grace_ticks() {
        let mut enemy = spawn_enemy(0, ENEMY_KIND_CHASER, Vec3::new(0.0, 0.0, 0.0));
        assert_eq!(enemy.active_in, CHASER_GRACE_TICKS);
        let targets = [Vec3::new(10.0, 0.0, 0.0)];
        for _ in 0..CHASER_GRACE_TICKS {
            step_enemy(&mut enemy, &targets);
        }
        assert_eq!(enemy.active_in, 0);
        assert!(enemy.pos.x.abs() < EPS);
        assert!(enemy.pos.y.abs() < EPS);
        assert!(enemy.pos.z.abs() < EPS);
    }

    #[test]
    fn chaser_closes_exactly_one_step() {
        let mut enemy = spawn_enemy(0, ENEMY_KIND_CHASER, Vec3::new(0.0, 0.0, 0.0));
        enemy.active_in = 0;
        let targets = [Vec3::new(10.0, 0.0, 0.0)];
        step_enemy(&mut enemy, &targets);
        let step = CHASER_SPEED / TICK_HZ as f32;
        assert!((enemy.pos.x - step).abs() < EPS);
        assert!(enemy.pos.y.abs() < EPS);
        assert!(enemy.pos.z.abs() < EPS);
    }

    #[test]
    fn chaser_never_overshoots() {
        let mut enemy = spawn_enemy(0, ENEMY_KIND_CHASER, Vec3::new(0.0, 0.0, 0.0));
        enemy.active_in = 0;
        let targets = [Vec3::new(0.1, 0.0, 0.0)];
        step_enemy(&mut enemy, &targets);
        assert!((enemy.pos.x - 0.1).abs() < EPS);
        assert!(enemy.pos.y.abs() < EPS);
        assert!(enemy.pos.z.abs() < EPS);
    }

    #[test]
    fn chaser_picks_nearest_target() {
        let mut enemy = spawn_enemy(0, ENEMY_KIND_CHASER, Vec3::new(0.0, 0.0, 0.0));
        enemy.active_in = 0;
        let targets = [Vec3::new(10.0, 0.0, 0.0), Vec3::new(0.0, 0.0, 5.0)];
        step_enemy(&mut enemy, &targets);
        let step = CHASER_SPEED / TICK_HZ as f32;
        assert!(enemy.pos.x.abs() < EPS);
        assert!((enemy.pos.z - step).abs() < EPS);
    }

    #[test]
    fn chaser_ignores_far_targets_and_returns_home() {
        let mut enemy = spawn_enemy(0, ENEMY_KIND_CHASER, Vec3::new(0.0, 0.0, 0.0));
        enemy.active_in = 0;
        enemy.pos = Vec3::new(2.0, 0.0, 0.0);
        let targets = [Vec3::new(30.0, 0.0, 0.0)];
        step_enemy(&mut enemy, &targets);
        let half_step = CHASER_SPEED / TICK_HZ as f32 * 0.5;
        assert!((enemy.pos.x - (2.0 - half_step)).abs() < EPS);
        assert!(enemy.pos.y.abs() < EPS);
        assert!(enemy.pos.z.abs() < EPS);
    }

    #[test]
    fn unknown_kind_does_nothing() {
        let mut enemy = spawn_enemy(0, 99, Vec3::new(0.0, 0.0, 0.0));
        enemy.active_in = 0;
        let targets = [Vec3::new(1.0, 0.0, 0.0)];
        step_enemy(&mut enemy, &targets);
        assert!(enemy.pos.x.abs() < EPS);
        assert!(enemy.pos.y.abs() < EPS);
        assert!(enemy.pos.z.abs() < EPS);
    }

    #[test]
    fn enemy_touches_player_box() {
        let mut enemy = spawn_enemy(0, ENEMY_KIND_CHASER, Vec3::new(0.0, 0.9, 0.0));
        let player = feet(0.0, 0.0, 0.0);
        assert!(enemy_touches(&enemy, &player));
        enemy.pos = Vec3::new(1.9, 0.9, 0.0);
        assert!(!enemy_touches(&enemy, &player));
    }

    #[test]
    fn hit_sphere_has_no_cube_corners() {
        // A big enemy diagonal to the player: a cube of half size 1.3 would reach the
        // player box here, the drawn sphere does not.
        let params = EnemyParams {
            size: 1.3,
            ..EnemyParams::default()
        };
        let enemy = spawn_enemy_with(1, ENEMY_KIND_CHASER, Vec3::new(1.4, 2.9, 1.4), params);
        assert!(!enemy_touches(&enemy, &feet(0.0, 0.0, 0.0)));
    }

    #[test]
    fn grazing_the_body_edge_is_survivable() {
        let params = EnemyParams {
            size: 1.0,
            ..EnemyParams::default()
        };
        // Player box edge at x = 0.4 (half width); the body surface (radius 1.0) overlaps it
        // by 0.05, but the lethal sphere is 0.85.
        let edge = 0.4 + 0.95;
        let enemy = spawn_enemy_with(1, ENEMY_KIND_CHASER, Vec3::new(edge, 0.9, 0.0), params);
        assert!(!enemy_touches(&enemy, &feet(0.0, 0.0, 0.0)));
    }

    #[test]
    fn spawn_enemy_uses_default_params() {
        let enemy = spawn_enemy(3, ENEMY_KIND_CHASER, Vec3::new(1.0, 2.0, 3.0));
        assert_eq!(enemy.params, EnemyParams::default());
        assert_eq!(enemy.params.speed, CHASER_SPEED);
        assert_eq!(enemy.params.aggro, CHASER_AGGRO_RADIUS);
        assert_eq!(enemy.params.size, CHASER_HALF_SIZE);
        assert_eq!(enemy.params.grace_ticks, CHASER_GRACE_TICKS);
        assert_eq!(enemy.active_in, CHASER_GRACE_TICKS);
        assert_eq!(enemy.home, Vec3::new(1.0, 2.0, 3.0));
        assert_eq!(enemy.pos, Vec3::new(1.0, 2.0, 3.0));
    }

    #[test]
    fn tuned_enemy_params_change_speed_and_hit_size() {
        let params = EnemyParams {
            speed: 11.0,
            aggro: 40.0,
            size: 1.5,
            grace_ticks: 0,
            ..EnemyParams::default()
        };
        let mut enemy = spawn_enemy_with(7, ENEMY_KIND_CHASER, Vec3::new(0.0, 0.0, 0.0), params.clone());
        assert_eq!(enemy.active_in, 0, "grace_ticks comes from the params");
        assert_eq!(enemy.params, params);

        let targets = [Vec3::new(10.0, 0.0, 0.0)];
        step_enemy(&mut enemy, &targets);
        let step = 11.0 / TICK_HZ as f32;
        assert!((enemy.pos.x - step).abs() < EPS, "tuned speed must be used");

        // The same position is only a hit for the bigger box.
        let player = feet(0.0, 0.0, 0.0);
        let mut big = spawn_enemy_with(8, ENEMY_KIND_CHASER, Vec3::new(1.5, 0.9, 0.0), params);
        assert!(enemy_touches(&big, &player));
        big.params.size = CHASER_HALF_SIZE;
        assert!(!enemy_touches(&big, &player));
    }

    #[test]
    fn tuned_aggro_radius_is_used() {
        let params = EnemyParams {
            speed: CHASER_SPEED,
            aggro: 4.0,
            size: CHASER_HALF_SIZE,
            grace_ticks: 0,
            ..EnemyParams::default()
        };
        let mut enemy = spawn_enemy_with(0, ENEMY_KIND_CHASER, Vec3::new(0.0, 0.0, 0.0), params);
        // 10 units away: outside the tuned 4.0 aggro, so it drifts home instead.
        let targets = [Vec3::new(10.0, 0.0, 0.0)];
        step_enemy(&mut enemy, &targets);
        assert!(enemy.pos.x.abs() < EPS, "target outside tuned aggro must be ignored");
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
        // The vertical tolerance is not part of the radius.
        assert!(!beacon_reached_within(&feet(0.0, 3.0, 0.0), &beacon, 100.0));
    }

    #[test]
    fn default_params_are_m1_chaser_plus_new_fields() {
        let d = EnemyParams::default();
        assert_eq!(d.speed, CHASER_SPEED);
        assert_eq!(d.aggro, CHASER_AGGRO_RADIUS);
        assert_eq!(d.size, CHASER_HALF_SIZE);
        assert_eq!(d.grace_ticks, CHASER_GRACE_TICKS);
        assert_eq!(d.behavior, BEHAVIOR_CHASER);
        assert_eq!(d.fire_interval_ticks, TURRET_FIRE_INTERVAL_TICKS);
        assert_eq!(d.projectile_speed, TURRET_PROJECTILE_SPEED);
        assert_eq!(d.reveal_radius, MIMIC_REVEAL_RADIUS);
        assert_eq!(d.corrupt_interval_ticks, CORRUPTER_INTERVAL_TICKS);
    }

    #[test]
    fn enemy_state_tracks_grace_for_chasers() {
        let mut chaser = spawn_enemy(0, ENEMY_KIND_CHASER, Vec3::new(0.0, 0.0, 0.0));
        assert_eq!(enemy_state(&chaser), ENEMY_DORMANT);
        chaser.active_in = 0;
        assert_eq!(enemy_state(&chaser), ENEMY_HUNTING);
    }

    #[test]
    fn enemy_state_tracks_turret_and_corrupter_grace() {
        for behavior in [BEHAVIOR_TURRET, BEHAVIOR_CORRUPTER] {
            let params = EnemyParams {
                behavior,
                ..EnemyParams::default()
            };
            let mut enemy = spawn_enemy_with(0, ENEMY_KIND_CHASER, Vec3::new(0.0, 0.0, 0.0), params);
            assert_eq!(enemy_state(&enemy), ENEMY_DORMANT);
            enemy.active_in = 0;
            assert_eq!(enemy_state(&enemy), ENEMY_HUNTING);
        }
    }

    #[test]
    fn unknown_behaviour_is_dormant() {
        let params = EnemyParams {
            behavior: 99,
            grace_ticks: 0,
            ..EnemyParams::default()
        };
        let enemy = spawn_enemy_with(0, ENEMY_KIND_CHASER, Vec3::new(0.0, 0.0, 0.0), params);
        assert_eq!(enemy_state(&enemy), ENEMY_DORMANT);
    }

    #[test]
    fn mimics_never_spawn_in_grace_and_start_disguised() {
        let params = EnemyParams {
            behavior: BEHAVIOR_MIMIC,
            grace_ticks: 100,
            ..EnemyParams::default()
        };
        let enemy = spawn_enemy_with(0, ENEMY_KIND_CHASER, Vec3::new(0.0, 0.0, 0.0), params);
        assert_eq!(enemy.active_in, 0);
        assert_eq!(enemy_state(&enemy), ENEMY_DISGUISED);
    }

    #[test]
    fn enemy_state_reports_revealed_mimic_as_hunting() {
        let params = EnemyParams {
            behavior: BEHAVIOR_MIMIC,
            ..EnemyParams::default()
        };
        let mut enemy = spawn_enemy_with(0, ENEMY_KIND_CHASER, Vec3::new(0.0, 0.0, 0.0), params);
        assert_eq!(enemy_state(&enemy), ENEMY_DISGUISED);
        enemy.revealed = true;
        assert_eq!(enemy_state(&enemy), ENEMY_HUNTING);
    }

    #[test]
    fn step_enemy_turret_never_moves() {
        let params = EnemyParams {
            behavior: BEHAVIOR_TURRET,
            grace_ticks: 0,
            ..EnemyParams::default()
        };
        let mut turret = spawn_enemy_with(0, ENEMY_KIND_CHASER, Vec3::new(1.0, 2.0, 3.0), params);
        step_enemy(&mut turret, &[Vec3::new(0.0, 0.0, 0.0)]);
        assert_eq!(turret.pos, Vec3::new(1.0, 2.0, 3.0));
    }

    #[test]
    fn step_enemy_mimic_waits_disguised_until_close() {
        let params = EnemyParams {
            behavior: BEHAVIOR_MIMIC,
            reveal_radius: 3.0,
            speed: 5.0,
            ..EnemyParams::default()
        };
        let mut mimic = spawn_enemy_with(0, ENEMY_KIND_CHASER, Vec3::new(0.0, 0.0, 0.0), params);
        // Far target: no reveal, no movement.
        step_enemy(&mut mimic, &[Vec3::new(9.0, 0.0, 0.0)]);
        assert_eq!(mimic.pos, Vec3::new(0.0, 0.0, 0.0));
        assert_eq!(enemy_state(&mimic), ENEMY_DISGUISED);
        // Close target: reveals and chases.
        step_enemy(&mut mimic, &[Vec3::new(1.0, 0.0, 0.0)]);
        assert_eq!(enemy_state(&mimic), ENEMY_HUNTING);
        assert!(mimic.pos.x > 0.0);
    }

    #[test]
    fn step_enemy_corrupter_chases_after_grace() {
        let params = EnemyParams {
            behavior: BEHAVIOR_CORRUPTER,
            grace_ticks: 1,
            speed: 5.0,
            ..EnemyParams::default()
        };
        let mut corrupter = spawn_enemy_with(0, ENEMY_KIND_CHASER, Vec3::new(0.0, 0.0, 0.0), params);
        step_enemy(&mut corrupter, &[Vec3::new(10.0, 0.0, 0.0)]);
        assert!(corrupter.pos.x.abs() < EPS, "grace period: no movement");
        step_enemy(&mut corrupter, &[Vec3::new(10.0, 0.0, 0.0)]);
        assert!(corrupter.pos.x > 0.0, "grace over: chases");
    }
}
