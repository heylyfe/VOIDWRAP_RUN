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

/// A single enemy. `home` is where it drifts back to when nothing is in range.
#[derive(Debug, Clone, PartialEq)]
pub struct Enemy {
    pub id: u32,
    pub kind: u8,
    pub pos: Vec3,
    pub home: Vec3,
    pub active_in: u32,
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
    distance(&player_centre(player_feet), &gift.pos) <= GIFT_RADIUS
}

/// True when the player stands on the beacon: close horizontally and roughly level.
pub fn beacon_reached(player_feet: &Vec3, beacon: &Vec3) -> bool {
    let dx = player_feet.x - beacon.x;
    let dz = player_feet.z - beacon.z;
    let horizontal = (dx * dx + dz * dz).sqrt();
    horizontal <= BEACON_RADIUS && (player_feet.y - beacon.y).abs() <= BEACON_HEIGHT
}

/// Create an enemy sitting at `pos`, with `pos` as its home and a full grace period.
pub fn spawn_enemy(id: u32, kind: u8, pos: Vec3) -> Enemy {
    Enemy {
        id,
        kind,
        home: pos.clone(),
        pos,
        active_in: CHASER_GRACE_TICKS,
    }
}

/// Advance one enemy by one server tick (`dt = 1 / TICK_HZ`).
///
/// `targets` are the centres of living players. While `active_in > 0` the enemy only
/// counts down. Afterwards it flies straight at the nearest target inside
/// [`CHASER_AGGRO_RADIUS`] (ties go to the lowest index), or drifts home at half speed.
/// Unknown kinds do nothing at all.
pub fn step_enemy(enemy: &mut Enemy, targets: &[Vec3]) {
    if enemy.kind != ENEMY_KIND_CHASER {
        return;
    }
    if enemy.active_in > 0 {
        enemy.active_in -= 1;
        return;
    }

    let step = CHASER_SPEED / TICK_HZ as f32;

    let mut nearest: Option<(usize, f32)> = None;
    for (index, target) in targets.iter().enumerate() {
        let d = distance(&enemy.pos, target);
        if d > CHASER_AGGRO_RADIUS {
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

/// True when the enemy's cube overlaps the player's collision box.
pub fn enemy_touches(enemy: &Enemy, player_feet: &Vec3) -> bool {
    let half = CHASER_HALF_SIZE;
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
        };
        let far = Gift {
            id: 1,
            pos: Vec3::new(0.0, 2.1, 0.0),
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
}
