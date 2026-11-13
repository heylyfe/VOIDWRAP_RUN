//! Pure, deterministic M1 game rules: gift pickups, the beacon, and chaser enemies.
//!
//! Everything here is a plain function over protocol data so the authoritative server
//! and the client-side predictor can run the exact same code. Only `+ - * /` and
//! `sqrt` are used, so results are bit-identical on every target.

use protocol::{Aabb, Gift, TICK_HZ, Vec3};

use crate::controller::player_box;
use crate::world::overlaps;

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

/// Half extent of the chaser's cubic hit box, centred on its position.
pub const CHASER_HALF_SIZE: f32 = 0.5;

/// Ticks a freshly spawned chaser stays dormant (3 s at 20 Hz).
pub const CHASER_GRACE_TICKS: u32 = 60;

/// Height of the player's centre above their feet.
const PLAYER_CENTRE_HEIGHT: f32 = 0.9;

/// Per-enemy tuning: flight speed, aggro radius, hit box half size and grace ticks.
///
/// [`EnemyParams::default`] is exactly the M1 chaser, so an enemy spawned without
/// explicit params behaves as it always did.
#[derive(Debug, Clone, PartialEq)]
pub struct EnemyParams {
    /// Flight speed in units per second.
    pub speed: f32,
    /// 3D distance within which the enemy pursues a target.
    pub aggro: f32,
    /// Half extent of the cubic hit box, centred on the enemy position.
    pub size: f32,
    /// Ticks the enemy stays dormant after spawning.
    pub grace_ticks: u32,
}

impl Default for EnemyParams {
    fn default() -> Self {
        Self {
            speed: CHASER_SPEED,
            aggro: CHASER_AGGRO_RADIUS,
            size: CHASER_HALF_SIZE,
            grace_ticks: CHASER_GRACE_TICKS,
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
}

/// Euclidean distance between two points.
fn distance(a: &Vec3, b: &Vec3) -> f32 {
    let dx = a.x - b.x;
    let dy = a.y - b.y;
    let dz = a.z - b.z;
    (dx * dx + dy * dy + dz * dz).sqrt()
}

/// The player's centre point, derived from their feet position.
fn player_centre(player_feet: &Vec3) -> Vec3 {
    Vec3::new(player_feet.x, player_feet.y + PLAYER_CENTRE_HEIGHT, player_feet.z)
}

/// Move `pos` toward `target` by at most `max_step`, never overshooting.
fn move_toward(pos: &mut Vec3, target: &Vec3, max_step: f32) {
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
/// full grace period.
pub fn spawn_enemy(id: u32, kind: u8, pos: Vec3) -> Enemy {
    spawn_enemy_with(id, kind, pos, EnemyParams::default())
}

/// Create an enemy with explicit [`EnemyParams`]; `active_in` starts at
/// `params.grace_ticks`.
pub fn spawn_enemy_with(id: u32, kind: u8, pos: Vec3, params: EnemyParams) -> Enemy {
    let active_in = params.grace_ticks;
    Enemy {
        id,
        kind,
        home: pos.clone(),
        pos,
        active_in,
        params,
    }
}

/// Advance one enemy by one server tick (`dt = 1 / TICK_HZ`).
///
/// `targets` are the centres of living players. While `active_in > 0` the enemy only
/// counts down. Afterwards it flies straight at the nearest target inside
/// `enemy.params.aggro` (ties go to the lowest index), or drifts home at half speed.
/// Unknown kinds do nothing at all.
pub fn step_enemy(enemy: &mut Enemy, targets: &[Vec3]) {
    if enemy.kind != ENEMY_KIND_CHASER {
        return;
    }
    if enemy.active_in > 0 {
        enemy.active_in -= 1;
        return;
    }

    let step = enemy.params.speed / TICK_HZ as f32;
    let aggro = enemy.params.aggro;

    let mut nearest: Option<(usize, f32)> = None;
    for (index, target) in targets.iter().enumerate() {
        let d = distance(&enemy.pos, target);
        if d > aggro {
            continue;
        }
        match nearest {
            Some((_, best)) if best <= d => {}
            _ => nearest = Some((index, d)),
        }
    }

    match nearest {
        Some((index, _)) => {
            let target = targets[index].clone();
            move_toward(&mut enemy.pos, &target, step);
        }
        None => {
            let home = enemy.home.clone();
            move_toward(&mut enemy.pos, &home, step * 0.5);
        }
    }
}

/// True when the enemy's cube (half extent `enemy.params.size`) overlaps the
/// player's collision box.
pub fn enemy_touches(enemy: &Enemy, player_feet: &Vec3) -> bool {
    let half = enemy.params.size;
    let cube = Aabb {
        min: Vec3::new(enemy.pos.x - half, enemy.pos.y - half, enemy.pos.z - half),
        max: Vec3::new(enemy.pos.x + half, enemy.pos.y + half, enemy.pos.z + half),
    };
    overlaps(&cube, &player_box(player_feet))
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
}
