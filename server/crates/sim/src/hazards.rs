//! Deterministic enemy hazards for M2: turret projectiles and crumbling tiles.
//!
//! Owns all projectile and tile state and advances it once per server tick, alongside
//! the enemy behaviours from [`crate::rules`]. `Hazards::step` steps enemy movement for
//! every enemy (chaser, turret, mimic and corrupter) and replaces the server's direct
//! call to [`crate::rules::step_enemy`]. Only `+ - * /`, `sqrt` and `clamp` are used on
//! floats, so results are bit-identical on every target.

use protocol::{
    DEATH_ENEMY, DEATH_PROJECTILE, ENEMY_HUNTING, LevelData, PLATFORM_CRUMBLE, Platform, ProjectileView, TICK_HZ,
    TILE_GONE, TILE_WARN, TileState, Vec3,
};

use crate::controller::player_box;
use crate::rules::{
    BEACON_HEIGHT, BEHAVIOR_CORRUPTER, BEHAVIOR_TURRET, Enemy, enemy_state, enemy_touches, nearest_target_within,
    player_centre, step_behavior,
};
use crate::world::World;

/// Ticks a projectile survives after being fired before it is removed (4 s at 20 Hz).
pub const PROJECTILE_LIFETIME_TICKS: u32 = 80;

/// Projectiles kill when they come within this distance of the closest point on the player box.
pub const PROJECTILE_RADIUS: f32 = 0.35;

/// Ticks a platform stays `TILE_WARN` before going `TILE_GONE` (0.6 s at 20 Hz).
pub const TILE_WARN_TICKS: u32 = 12;

/// Ticks a platform stays `TILE_GONE` before returning to normal (3 s at 20 Hz).
pub const TILE_GONE_TICKS: u32 = 60;

/// Most projectiles alive at once; extra shots are dropped on the floor.
pub const MAX_PROJECTILES: usize = 64;

/// Feet must be within this height of the platform top to count as standing on it.
const STAND_HEIGHT_TOLERANCE: f32 = 0.05;

/// A flying projectile. `ticks_alive` counts from 0 on the firing tick.
#[derive(Debug, Clone, PartialEq)]
struct Projectile {
    id: u32,
    pos: Vec3,
    vel: Vec3,
    ticks_alive: u32,
}

/// A platform that is currently `TILE_WARN` or `TILE_GONE`, with ticks left in that state.
#[derive(Debug, Clone, PartialEq)]
struct TileEntry {
    id: u16,
    state: u8,
    ticks_left: u32,
}

/// Owns projectiles and tile states. Call [`Hazards::step`] once per server tick.
///
/// `new`/`reset` produce an empty state (fresh level); `tiles` only reports
/// non-normal platforms, sorted by id; `projectiles` reports live projectiles in id
/// order. Projectile ids increase monotonically from 0 and wrap around `u32`.
#[derive(Debug, Default)]
pub struct Hazards {
    projectiles: Vec<Projectile>,
    tiles: Vec<TileEntry>,
    next_projectile_id: u32,
}

impl Hazards {
    /// Create an empty `Hazards`.
    pub fn new() -> Self {
        Self::default()
    }

    /// Drop all projectiles and tile state. Call when the level changes.
    pub fn reset(&mut self) {
        self.projectiles.clear();
        self.tiles.clear();
        self.next_projectile_id = 0;
    }

    /// Advance tiles, enemies, projectiles and crumble detection by one server tick.
    ///
    /// `players` are the feet positions of the living players; everything the step
    /// function needs (target centres, aim points) is derived from them. Enemy
    /// behaviour and movement for all enemies is stepped here; this replaces the
    /// server's call to [`crate::rules::step_enemy`]. `world` is consulted for the
    /// current set of solid platforms (projectiles removed inside them); `level`
    /// supplies the platforms (ids, kinds, bounds, beacon, kill_y).
    pub fn step(&mut self, enemies: &mut [Enemy], world: &World, level: &LevelData, players: &[Vec3]) {
        // 1. Age existing tile state so newly marked platforms always get their full duration.
        //    Ids that returned to normal this tick are remembered so a standing player does
        //    not re-warn them until the following tick (back to normal must be observable).
        let expired = self.age_tiles();

        // 2. Enemies: behaviour movement/timers, turret firing, corrupter marking.
        let centres: Vec<Vec3> = players.iter().map(player_centre).collect();
        for enemy in enemies.iter_mut() {
            // Grace ticks take no action this tick at all: step_behavior counts the
            // enemy's grace down but the enemy does not fire/corrupt yet (the same
            // way movement waits out grace inside step_hunter).
            let was_in_grace = enemy.active_in > 0;
            step_behavior(enemy, &centres);
            if was_in_grace {
                continue;
            }
            match enemy.params.behavior {
                BEHAVIOR_TURRET => step_turret(self, enemy, &centres),
                BEHAVIOR_CORRUPTER => step_corrupter(self, enemy, level),
                _ => {}
            }
        }

        // 3. Projectiles: straight-line flight, then removal for age, void or walls.
        let dt = 1.0 / TICK_HZ as f32;
        for p in self.projectiles.iter_mut() {
            p.pos.x += p.vel.x * dt;
            p.pos.y += p.vel.y * dt;
            p.pos.z += p.vel.z * dt;
            p.ticks_alive += 1;
        }
        let kill_y = level.kill_y;
        self.projectiles.retain(|p| {
            p.pos.y >= kill_y && p.ticks_alive < PROJECTILE_LIFETIME_TICKS && !point_in_solid(&p.pos, world)
        });

        // 4. Players standing on a crumbling platform warn it. A platform that just
        //    returned to normal this tick is not re-warned until the next tick.
        for platform in &level.platforms {
            if platform.kind != PLATFORM_CRUMBLE {
                continue;
            }
            if self.tiles.iter().any(|t| t.id == platform.id) {
                continue;
            }
            if expired.contains(&platform.id) {
                continue;
            }
            let top = platform.bounds.max.y;
            let standing = players.iter().any(|feet| {
                feet.x >= platform.bounds.min.x
                    && feet.x <= platform.bounds.max.x
                    && feet.z >= platform.bounds.min.z
                    && feet.z <= platform.bounds.max.z
                    && (feet.y - top).abs() <= STAND_HEIGHT_TOLERANCE
            });
            if standing {
                self.tiles.push(TileEntry {
                    id: platform.id,
                    state: TILE_WARN,
                    ticks_left: TILE_WARN_TICKS,
                });
            }
        }
    }

    /// Deaths caused by this hazard set for a player standing at `feet`.
    ///
    /// `Some(DEATH_ENEMY)` for an active, non-disguised enemy touch (checked first),
    /// otherwise `Some(DEATH_PROJECTILE)` for a projectile within [`PROJECTILE_RADIUS`]
    /// of the player box, otherwise `None`.
    pub fn hit(&self, enemies: &[Enemy], feet: &Vec3) -> Option<u8> {
        for enemy in enemies {
            if enemy_state(enemy) == ENEMY_HUNTING && enemy_touches(enemy, feet) {
                return Some(DEATH_ENEMY);
            }
        }
        let b = player_box(feet);
        for p in &self.projectiles {
            let cx = p.pos.x.clamp(b.min.x, b.max.x);
            let cy = p.pos.y.clamp(b.min.y, b.max.y);
            let cz = p.pos.z.clamp(b.min.z, b.max.z);
            let dx = p.pos.x - cx;
            let dy = p.pos.y - cy;
            let dz = p.pos.z - cz;
            if dx * dx + dy * dy + dz * dz <= PROJECTILE_RADIUS * PROJECTILE_RADIUS {
                return Some(DEATH_PROJECTILE);
            }
        }
        None
    }

    /// Views of all live projectiles, in spawn (id) order.
    pub fn projectiles(&self) -> Vec<ProjectileView> {
        self.projectiles
            .iter()
            .map(|p| ProjectileView {
                id: p.id,
                pos: p.pos.clone(),
            })
            .collect()
    }

    /// Views of every non-normal platform (`TILE_WARN` / `TILE_GONE`), sorted by id.
    pub fn tiles(&self) -> Vec<TileState> {
        let mut out: Vec<TileState> = self
            .tiles
            .iter()
            .map(|t| TileState {
                id: t.id,
                state: t.state,
            })
            .collect();
        out.sort_by_key(|t| t.id);
        out
    }

    /// Age existing tile entries one tick; WARN -> GONE after [`TILE_WARN_TICKS`],
    /// then removed entirely (back to normal) after [`TILE_GONE_TICKS`].
    /// Returns the ids that returned to normal this tick.
    fn age_tiles(&mut self) -> Vec<u16> {
        let mut expired = Vec::new();
        let mut i = 0;
        while i < self.tiles.len() {
            self.tiles[i].ticks_left -= 1;
            if self.tiles[i].ticks_left == 0 {
                if self.tiles[i].state == TILE_WARN {
                    self.tiles[i].state = TILE_GONE;
                    self.tiles[i].ticks_left = TILE_GONE_TICKS;
                    i += 1;
                } else {
                    expired.push(self.tiles[i].id);
                    self.tiles.remove(i);
                }
            } else {
                i += 1;
            }
        }
        expired
    }

    /// Push a new projectile if at most [`MAX_PROJECTILES`] are alive; ids wrap around.
    fn spawn_projectile(&mut self, pos: Vec3, vel: Vec3) {
        if self.projectiles.len() >= MAX_PROJECTILES {
            return;
        }
        let id = self.next_projectile_id;
        self.next_projectile_id = self.next_projectile_id.wrapping_add(1);
        self.projectiles.push(Projectile {
            id,
            pos,
            vel,
            ticks_alive: 0,
        });
    }

    /// Mark `id` WARN unless it is already non-normal.
    fn mark_warn(&mut self, id: u16) {
        if self.tiles.iter().any(|t| t.id == id) {
            return;
        }
        self.tiles.push(TileEntry {
            id,
            state: TILE_WARN,
            ticks_left: TILE_WARN_TICKS,
        });
    }
}

/// True when `p` is inside any of the world's current solid platforms (inclusive).
fn point_in_solid(p: &Vec3, world: &World) -> bool {
    world.solids.iter().any(|b| {
        p.x >= b.min.x && p.x <= b.max.x && p.y >= b.min.y && p.y <= b.max.y && p.z >= b.min.z && p.z <= b.max.z
    })
}

/// True when the platform's top face contains the beacon: horizontally inside the
/// top rectangle and within [`BEACON_HEIGHT`] of its height.
fn platform_contains_beacon(platform: &Platform, beacon: &Vec3) -> bool {
    let top = platform.bounds.max.y;
    beacon.x >= platform.bounds.min.x
        && beacon.x <= platform.bounds.max.x
        && beacon.z >= platform.bounds.min.z
        && beacon.z <= platform.bounds.max.z
        && (beacon.y - top).abs() <= BEACON_HEIGHT
}

/// Id of the nearest platform whose top-face centre is closest to `pos`, excluding
/// platform id 0 (the spawn), the platform holding the beacon and any platform that
/// is not currently normal. Ties go to the platform appearing first in `level.platforms`.
fn nearest_corruptible(level: &LevelData, tiles: &[TileEntry], pos: &Vec3) -> Option<u16> {
    let mut best: Option<(u16, f32)> = None;
    for platform in &level.platforms {
        if platform.id == 0 {
            continue;
        }
        if tiles.iter().any(|t| t.id == platform.id) {
            continue;
        }
        if platform_contains_beacon(platform, &level.beacon) {
            continue;
        }
        let cx = (platform.bounds.min.x + platform.bounds.max.x) * 0.5;
        let cy = platform.bounds.max.y;
        let cz = (platform.bounds.min.z + platform.bounds.max.z) * 0.5;
        let dx = cx - pos.x;
        let dy = cy - pos.y;
        let dz = cz - pos.z;
        let d = (dx * dx + dy * dy + dz * dz).sqrt();
        let replace = match best {
            None => true,
            Some((_, best_d)) => d < best_d,
        };
        if replace {
            best = Some((platform.id, d));
        }
    }
    best.map(|(id, _)| id)
}

/// Turret behaviour: after grace, fire at the nearest target's centre (feet + 0.9 up)
/// every `fire_interval_ticks`. Target centres are passed in via `targets`.
fn step_turret(hazards: &mut Hazards, enemy: &mut Enemy, targets: &[Vec3]) {
    if enemy.active_in != 0 {
        return; // still in grace period
    }
    if enemy.cooldown_ticks > 0 {
        enemy.cooldown_ticks -= 1;
    }
    if enemy.cooldown_ticks != 0 {
        return; // not time yet
    }
    let Some(index) = nearest_target_within(&enemy.pos, targets, enemy.params.aggro) else {
        return; // nobody in range; cadence counter stays at zero
    };
    enemy.cooldown_ticks = enemy.params.fire_interval_ticks;
    let target = targets[index].clone();
    let dx = target.x - enemy.pos.x;
    let dy = target.y - enemy.pos.y;
    let dz = target.z - enemy.pos.z;
    let d = (dx * dx + dy * dy + dz * dz).sqrt();
    if d <= 0.0 {
        return; // target exactly on the turret: shot fizzles, cadence consumed
    }
    let scale = enemy.params.projectile_speed / d;
    hazards.spawn_projectile(enemy.pos.clone(), Vec3::new(dx * scale, dy * scale, dz * scale));
}

/// Corrupter behaviour: after grace, mark the nearest valid platform `TILE_WARN`
/// every `corrupt_interval_ticks`.
fn step_corrupter(hazards: &mut Hazards, enemy: &mut Enemy, level: &LevelData) {
    if enemy.active_in != 0 {
        return; // still in grace period
    }
    if enemy.cooldown_ticks > 0 {
        enemy.cooldown_ticks -= 1;
    }
    if enemy.cooldown_ticks != 0 {
        return; // not time yet
    }
    let Some(id) = nearest_corruptible(level, &hazards.tiles, &enemy.pos) else {
        return; // no valid platform; try again next tick
    };
    hazards.mark_warn(id);
    enemy.cooldown_ticks = enemy.params.corrupt_interval_ticks;
}

#[cfg(test)]
mod tests {
    use super::*;
    use protocol::{Aabb, ENEMY_DISGUISED, EVENT_NONE, PLATFORM_NORMAL};

    use crate::rules::{
        BEHAVIOR_CHASER, BEHAVIOR_MIMIC, ENEMY_KIND_CHASER, EnemyParams, enemy_state, spawn_enemy_with,
    };

    const EPS: f32 = 1e-4;

    fn feet(x: f32, y: f32, z: f32) -> Vec3 {
        Vec3::new(x, y, z)
    }

    fn platform(id: u16, kind: u8, min: [f32; 3], max: [f32; 3]) -> Platform {
        Platform {
            id,
            kind,
            bounds: Aabb {
                min: Vec3::new(min[0], min[1], min[2]),
                max: Vec3::new(max[0], max[1], max[2]),
            },
        }
    }

    fn test_level(platforms: Vec<Platform>, beacon: Vec3, kill_y: f32) -> LevelData {
        LevelData {
            level: 1,
            seed: 42,
            spawn: Vec3::new(0.0, 0.0, 0.0),
            beacon,
            kill_y,
            platforms,
            gifts: Vec::new(),
            event: EVENT_NONE,
            altars: Vec::new(),
        }
    }

    fn turret_params(grace_ticks: u32, fire_interval_ticks: u32, projectile_speed: f32) -> EnemyParams {
        EnemyParams {
            behavior: BEHAVIOR_TURRET,
            grace_ticks,
            fire_interval_ticks,
            projectile_speed,
            ..EnemyParams::default()
        }
    }

    #[test]
    fn turret_fires_on_cadence_and_moves_straight() {
        let params = turret_params(0, 3, 10.0);
        let mut enemies = vec![spawn_enemy_with(1, ENEMY_KIND_CHASER, feet(0.0, 0.9, 0.0), params)];
        let level = test_level(vec![], feet(50.0, 0.0, 50.0), -100.0);
        let world = World::from_level(&level);
        let players = [feet(10.0, 0.0, 0.0)]; // centre (10, 0.9, 0)
        let mut hazards = Hazards::new();

        hazards.step(&mut enemies, &world, &level, &players); // tick 1: fires
        let shots = hazards.projectiles();
        assert_eq!(shots.len(), 1);
        assert!((shots[0].pos.x - 0.5).abs() < EPS, "10 u/s over 1/20 s from x=0");
        assert!((shots[0].pos.y - 0.9).abs() < EPS);
        assert!(shots[0].pos.z.abs() < EPS);

        hazards.step(&mut enemies, &world, &level, &players); // tick 2: no shot
        hazards.step(&mut enemies, &world, &level, &players); // tick 3: no shot
        assert_eq!(hazards.projectiles().len(), 1, "cadence: one shot every 3 ticks");
        assert!((hazards.projectiles()[0].pos.x - 1.5).abs() < EPS);

        hazards.step(&mut enemies, &world, &level, &players); // tick 4: fires again
        let shots = hazards.projectiles();
        assert_eq!(shots.len(), 2);
        assert!((shots[1].pos.x - 0.5).abs() < EPS);
        assert!((shots[0].pos.x - 2.0).abs() < EPS);
    }

    #[test]
    fn turret_aim_rises_toward_target_centre() {
        // Turret at feet height of nobody: target's centre (feet + 0.9) is above it,
        // so the shot must gain height over time.
        let params = turret_params(0, 100, 10.0);
        let mut enemies = vec![spawn_enemy_with(1, ENEMY_KIND_CHASER, feet(0.0, 0.0, 0.0), params)];
        let level = test_level(vec![], feet(50.0, 0.0, 50.0), -100.0);
        let world = World::from_level(&level);
        let players = [feet(10.0, 0.0, 0.0)]; // centre (10, 0.9, 0)
        let mut hazards = Hazards::new();
        hazards.step(&mut enemies, &world, &level, &players);
        let shots = hazards.projectiles();
        assert_eq!(shots.len(), 1);
        assert!(shots[0].pos.x > 0.0);
        assert!(shots[0].pos.y > 0.0, "aimed at feet + 0.9, not at the feet");
    }

    #[test]
    fn turret_respects_grace_period() {
        let params = turret_params(2, 1, 10.0);
        let mut enemies = vec![spawn_enemy_with(1, ENEMY_KIND_CHASER, feet(0.0, 0.9, 0.0), params)];
        let level = test_level(vec![], feet(50.0, 0.0, 50.0), -100.0);
        let world = World::from_level(&level);
        let players = [feet(10.0, 0.0, 0.0)];
        let mut hazards = Hazards::new();
        hazards.step(&mut enemies, &world, &level, &players);
        hazards.step(&mut enemies, &world, &level, &players);
        assert!(hazards.projectiles().is_empty(), "grace: no firing yet");
        hazards.step(&mut enemies, &world, &level, &players);
        assert_eq!(hazards.projectiles().len(), 1, "fires once grace elapses");
    }

    #[test]
    fn turret_ignores_targets_beyond_aggro() {
        let mut params = turret_params(0, 1, 10.0);
        params.aggro = 5.0;
        let mut enemies = vec![spawn_enemy_with(1, ENEMY_KIND_CHASER, feet(0.0, 0.9, 0.0), params)];
        let level = test_level(vec![], feet(50.0, 0.0, 50.0), -100.0);
        let world = World::from_level(&level);
        let players = [feet(10.0, 0.0, 0.0)]; // 10 units away, outside aggro
        let mut hazards = Hazards::new();
        for _ in 0..5 {
            hazards.step(&mut enemies, &world, &level, &players);
        }
        assert!(hazards.projectiles().is_empty());
    }

    #[test]
    fn turret_never_moves() {
        let params = turret_params(0, 1, 10.0);
        let mut enemies = vec![spawn_enemy_with(1, ENEMY_KIND_CHASER, feet(3.0, 1.0, -2.0), params)];
        let level = test_level(vec![], feet(50.0, 0.0, 50.0), -100.0);
        let world = World::from_level(&level);
        let players = [feet(0.0, 0.0, 0.0)];
        let mut hazards = Hazards::new();
        for _ in 0..10 {
            hazards.step(&mut enemies, &world, &level, &players);
        }
        assert_eq!(enemies[0].pos, Vec3::new(3.0, 1.0, -2.0));
    }

    #[test]
    fn projectile_is_removed_after_lifetime() {
        let mut params = turret_params(0, 1000, 10.0); // fires once, never re-fires
        params.aggro = 2000.0; // target is far away but must be inside aggro
        let mut enemies = vec![spawn_enemy_with(1, ENEMY_KIND_CHASER, feet(0.0, 0.9, 0.0), params)];
        let level = test_level(vec![], feet(50.0, 0.0, 50.0), -100.0);
        let world = World::from_level(&level);
        let players = [feet(1000.0, 0.0, 0.0)]; // far away, never reached
        let mut hazards = Hazards::new();
        for tick in 1..=PROJECTILE_LIFETIME_TICKS {
            hazards.step(&mut enemies, &world, &level, &players);
            if tick < PROJECTILE_LIFETIME_TICKS {
                assert!(
                    !hazards.projectiles().is_empty(),
                    "projectile must live {PROJECTILE_LIFETIME_TICKS} ticks"
                );
            }
        }
        assert!(hazards.projectiles().is_empty(), "removed once lifetime elapses");
    }

    #[test]
    fn projectile_is_removed_beyond_kill_y() {
        let params = turret_params(0, 1000, 10.0);
        let mut enemies = vec![spawn_enemy_with(1, ENEMY_KIND_CHASER, feet(0.0, 0.9, 0.0), params)];
        let level = test_level(vec![], feet(50.0, 0.0, 50.0), 5.0); // kill_y above the shot
        let world = World::from_level(&level);
        let players = [feet(10.0, 0.0, 0.0)];
        let mut hazards = Hazards::new();
        hazards.step(&mut enemies, &world, &level, &players);
        assert!(hazards.projectiles().is_empty());
    }

    #[test]
    fn projectile_is_removed_when_entering_a_solid_platform() {
        let params = turret_params(0, 1000, 10.0);
        let mut enemies = vec![spawn_enemy_with(1, ENEMY_KIND_CHASER, feet(0.0, 0.9, 0.0), params)];
        let wall = platform(9, PLATFORM_NORMAL, [2.0, 0.0, -1.0], [4.0, 2.0, 1.0]);
        let level = test_level(vec![wall], feet(50.0, 0.0, 50.0), -100.0);
        let world = World::from_level(&level);
        let players = [feet(10.0, 0.0, 0.0)];
        let mut hazards = Hazards::new();
        for _ in 0..10 {
            hazards.step(&mut enemies, &world, &level, &players);
        }
        assert!(
            hazards.projectiles().is_empty(),
            "projectile must not pass through solids"
        );
    }

    #[test]
    fn projectile_kills_player_in_its_path() {
        let params = turret_params(0, 100, 10.0);
        let mut enemies = vec![spawn_enemy_with(1, ENEMY_KIND_CHASER, feet(0.0, 0.9, 0.0), params)];
        let level = test_level(vec![], feet(50.0, 0.0, 50.0), -100.0);
        let world = World::from_level(&level);
        let player = feet(5.0, 0.0, 0.0); // standing still, in the line of fire
        let mut hazards = Hazards::new();
        let mut death = None;
        for _ in 0..40 {
            hazards.step(&mut enemies, &world, &level, std::slice::from_ref(&player));
            if let Some(d) = hazards.hit(&enemies, &player) {
                death = Some(d);
                break;
            }
        }
        assert_eq!(death, Some(DEATH_PROJECTILE));
    }

    #[test]
    fn projectile_count_is_capped() {
        let mut params = turret_params(0, 1, 1.0); // fires every tick, slow shots linger
        params.aggro = 2000.0; // 1000-unit target must be inside aggro
        let mut enemies = vec![spawn_enemy_with(1, ENEMY_KIND_CHASER, feet(0.0, 0.9, 0.0), params)];
        let level = test_level(vec![], feet(50.0, 0.0, 50.0), -100.0);
        let world = World::from_level(&level);
        let players = [feet(1000.0, 0.0, 0.0)];
        let mut hazards = Hazards::new();
        for _ in 0..(MAX_PROJECTILES as u32 + 6) {
            hazards.step(&mut enemies, &world, &level, &players);
        }
        assert_eq!(hazards.projectiles().len(), MAX_PROJECTILES);
    }

    #[test]
    fn mimic_is_harmless_disguised_then_hunts() {
        let params = EnemyParams {
            behavior: BEHAVIOR_MIMIC,
            speed: 5.0,
            aggro: 10.0,
            reveal_radius: 3.0,
            ..EnemyParams::default()
        };
        let enemy = spawn_enemy_with(1, ENEMY_KIND_CHASER, feet(0.0, 0.0, 0.0), params);
        assert_eq!(enemy.active_in, 0, "mimics do not wait out a grace period");
        let mut enemies = vec![enemy];
        let level = test_level(vec![], feet(50.0, 0.0, 50.0), -100.0);
        let world = World::from_level(&level);
        let mut hazards = Hazards::new();

        // Target far away: stays disguised, sits still and is harmless on touch.
        let far = [feet(9.9, 0.0, 0.0)];
        hazards.step(&mut enemies, &world, &level, &far);
        assert_eq!(enemy_state(&enemies[0]), ENEMY_DISGUISED);
        assert_eq!(enemies[0].pos, feet(0.0, 0.0, 0.0));
        assert_eq!(hazards.hit(&enemies, &feet(0.0, 0.0, 0.0)), None);

        // Target steps close: reveals and moves like a chaser.
        let near = [feet(1.0, 0.0, 0.0)];
        hazards.step(&mut enemies, &world, &level, &near);
        assert_eq!(enemy_state(&enemies[0]), ENEMY_HUNTING);
        assert!(enemies[0].pos.x > 0.0);

        // Target retreats: never disguises again.
        hazards.step(&mut enemies, &world, &level, &far);
        assert_eq!(enemy_state(&enemies[0]), ENEMY_HUNTING);

        // Revealed mimic on top of the player is lethal.
        enemies[0].pos = feet(0.0, 0.9, 0.0);
        assert_eq!(hazards.hit(&enemies, &feet(0.0, 0.0, 0.0)), Some(DEATH_ENEMY));
    }

    #[test]
    fn corrupter_marks_nearest_valid_platform_never_spawn_or_beacon() {
        // Platform 0 (spawn, nearest) and platform 4 (beacon, second nearest) must never
        // be picked; platform 7, the farthest, is the only valid choice.
        let platforms = vec![
            platform(0, PLATFORM_NORMAL, [-5.0, -1.0, -5.0], [5.0, 0.0, 5.0]),
            platform(4, PLATFORM_NORMAL, [-5.0, -1.0, 5.0], [5.0, 0.0, 15.0]),
            platform(7, PLATFORM_CRUMBLE, [-5.0, -1.0, 25.0], [5.0, 0.0, 35.0]),
        ];
        let beacon = feet(0.0, 0.0, 10.0); // on top-face centre of platform 4
        let level = test_level(platforms, beacon, -100.0);
        let world = World::from_level(&level);
        let params = EnemyParams {
            behavior: BEHAVIOR_CORRUPTER,
            grace_ticks: 0,
            corrupt_interval_ticks: 5,
            ..EnemyParams::default()
        };
        let mut enemies = vec![spawn_enemy_with(1, ENEMY_KIND_CHASER, feet(0.0, 3.0, 0.0), params)];
        let mut hazards = Hazards::new();
        hazards.step(&mut enemies, &world, &level, &[]);
        assert_eq!(
            hazards.tiles(),
            vec![TileState {
                id: 7,
                state: TILE_WARN
            }]
        );

        // Already WARN platforms are never re-picked; 0 and the beacon platform are excluded forever.
        for _ in 0..10 {
            hazards.step(&mut enemies, &world, &level, &[]);
        }
        assert_eq!(hazards.tiles().len(), 1, "nothing else may be marked");
        assert!(hazards.tiles().iter().all(|t| t.id == 7));
    }

    #[test]
    fn corrupter_waits_out_grace_before_corrupting() {
        let platforms = vec![platform(7, PLATFORM_NORMAL, [-5.0, -1.0, 25.0], [5.0, 0.0, 35.0])];
        let level = test_level(platforms, feet(50.0, 0.0, 50.0), -100.0);
        let world = World::from_level(&level);
        let params = EnemyParams {
            behavior: BEHAVIOR_CORRUPTER,
            grace_ticks: 2,
            corrupt_interval_ticks: 5,
            ..EnemyParams::default()
        };
        let mut enemies = vec![spawn_enemy_with(1, ENEMY_KIND_CHASER, feet(0.0, 3.0, 0.0), params)];
        let mut hazards = Hazards::new();
        hazards.step(&mut enemies, &world, &level, &[]);
        hazards.step(&mut enemies, &world, &level, &[]);
        assert!(hazards.tiles().is_empty(), "grace: no corruption yet");
        hazards.step(&mut enemies, &world, &level, &[]);
        assert_eq!(
            hazards.tiles(),
            vec![TileState {
                id: 7,
                state: TILE_WARN
            }]
        );
    }

    #[test]
    fn crumble_platform_becomes_warn_when_stood_on() {
        let platforms = vec![platform(2, PLATFORM_CRUMBLE, [-5.0, 10.0, -5.0], [5.0, 11.0, 5.0])];
        let level = test_level(platforms, feet(50.0, 0.0, 50.0), -100.0);
        let world = World::from_level(&level);
        let mut enemies: Vec<Enemy> = Vec::new();
        let mut hazards = Hazards::new();

        let standing = [feet(0.0, 11.0, 0.0)];
        hazards.step(&mut enemies, &world, &level, &standing);
        assert_eq!(
            hazards.tiles(),
            vec![TileState {
                id: 2,
                state: TILE_WARN
            }]
        );

        // Off by more than the tolerance in height: no warning for a new platform,
        // but the existing WARN keeps ticking (it is already non-normal).
        let airborne = [feet(0.0, 11.06, 0.0)];
        let mut hazards2 = Hazards::new();
        hazards2.step(&mut enemies, &world, &level, &airborne);
        assert!(hazards2.tiles().is_empty(), "feet too high above the top");
    }

    #[test]
    fn normal_platform_never_crumbles_from_standing() {
        let platforms = vec![platform(2, PLATFORM_NORMAL, [-5.0, 10.0, -5.0], [5.0, 11.0, 5.0])];
        let level = test_level(platforms, feet(50.0, 0.0, 50.0), -100.0);
        let world = World::from_level(&level);
        let mut enemies: Vec<Enemy> = Vec::new();
        let mut hazards = Hazards::new();
        let standing = [feet(0.0, 11.0, 0.0)];
        hazards.step(&mut enemies, &world, &level, &standing);
        assert!(hazards.tiles().is_empty());
    }

    #[test]
    fn standing_tolerance_accepts_within_point_zero_five() {
        let platforms = vec![platform(2, PLATFORM_CRUMBLE, [-5.0, 10.0, -5.0], [5.0, 11.0, 5.0])];
        let level = test_level(platforms, feet(50.0, 0.0, 50.0), -100.0);
        let world = World::from_level(&level);
        let mut enemies: Vec<Enemy> = Vec::new();
        let mut hazards = Hazards::new();
        let standing = [feet(0.0, 11.04, 0.0)]; // 0.04 above the top
        hazards.step(&mut enemies, &world, &level, &standing);
        assert_eq!(
            hazards.tiles(),
            vec![TileState {
                id: 2,
                state: TILE_WARN
            }]
        );
    }

    #[test]
    fn tile_warn_gone_normal_cycle_timing() {
        let platforms = vec![platform(2, PLATFORM_CRUMBLE, [-5.0, 10.0, -5.0], [5.0, 11.0, 5.0])];
        let level = test_level(platforms, feet(50.0, 0.0, 50.0), -100.0);
        let world = World::from_level(&level);
        let mut enemies: Vec<Enemy> = Vec::new();
        let mut hazards = Hazards::new();

        let standing = [feet(0.0, 11.0, 0.0)];
        hazards.step(&mut enemies, &world, &level, &standing); // becomes WARN
        assert_eq!(
            hazards.tiles(),
            vec![TileState {
                id: 2,
                state: TILE_WARN
            }]
        );

        // 11 aging ticks keep it WARN (it was created with 12).
        for _ in 0..11 {
            hazards.step(&mut enemies, &world, &level, &[]);
        }
        assert_eq!(
            hazards.tiles(),
            vec![TileState {
                id: 2,
                state: TILE_WARN
            }]
        );

        // The 12th aging tick flips WARN -> GONE.
        hazards.step(&mut enemies, &world, &level, &[]);
        assert_eq!(
            hazards.tiles(),
            vec![TileState {
                id: 2,
                state: TILE_GONE
            }]
        );

        // 59 aging ticks keep it GONE; the 60th removes it entirely.
        for _ in 0..59 {
            hazards.step(&mut enemies, &world, &level, &[]);
        }
        assert_eq!(
            hazards.tiles(),
            vec![TileState {
                id: 2,
                state: TILE_GONE
            }]
        );

        hazards.step(&mut enemies, &world, &level, &[]);
        assert!(hazards.tiles().is_empty(), "back to normal");

        // With the player standing again, a returned-to-normal platform crumbles anew.
        hazards.step(&mut enemies, &world, &level, &standing);
        assert_eq!(
            hazards.tiles(),
            vec![TileState {
                id: 2,
                state: TILE_WARN
            }]
        );
    }

    #[test]
    fn standing_does_not_reset_gone_back_to_warn() {
        let platforms = vec![platform(2, PLATFORM_CRUMBLE, [-5.0, 10.0, -5.0], [5.0, 11.0, 5.0])];
        let level = test_level(platforms, feet(50.0, 0.0, 50.0), -100.0);
        let world = World::from_level(&level);
        let mut enemies: Vec<Enemy> = Vec::new();
        let mut hazards = Hazards::new();

        let standing = [feet(0.0, 11.0, 0.0)];
        hazards.step(&mut enemies, &world, &level, &standing); // WARN (12 ticks)
        // Player stands through the whole WARN -> GONE -> normal cycle.
        for _ in 0..12 {
            hazards.step(&mut enemies, &world, &level, &standing);
        }
        assert_eq!(
            hazards.tiles(),
            vec![TileState {
                id: 2,
                state: TILE_GONE
            }]
        );
        for _ in 0..60 {
            hazards.step(&mut enemies, &world, &level, &standing);
        }
        assert!(hazards.tiles().is_empty(), "GONE must run its full course, not reset");
    }

    #[test]
    fn tiles_are_sorted_by_id() {
        // Level order is 5 then 2; tiles() must report 2 then 5.
        let platforms = vec![
            platform(5, PLATFORM_CRUMBLE, [-5.0, 10.0, -5.0], [5.0, 11.0, 5.0]),
            platform(2, PLATFORM_CRUMBLE, [20.0, 10.0, -5.0], [30.0, 11.0, 5.0]),
        ];
        let level = test_level(platforms, feet(50.0, 0.0, 50.0), -100.0);
        let world = World::from_level(&level);
        let mut enemies: Vec<Enemy> = Vec::new();
        let mut hazards = Hazards::new();
        let players = [feet(0.0, 11.0, 0.0), feet(25.0, 11.0, 0.0)];
        hazards.step(&mut enemies, &world, &level, &players);
        let ids: Vec<u16> = hazards.tiles().iter().map(|t| t.id).collect();
        assert_eq!(ids, vec![2, 5]);
    }

    #[test]
    fn hit_ignores_dormant_and_unknown_behaviour_enemies() {
        let dormant = spawn_enemy_with(1, ENEMY_KIND_CHASER, feet(0.0, 0.9, 0.0), EnemyParams::default());
        let weird = spawn_enemy_with(
            2,
            ENEMY_KIND_CHASER,
            feet(0.0, 0.9, 0.0),
            EnemyParams {
                behavior: 99,
                grace_ticks: 0,
                ..EnemyParams::default()
            },
        );
        let mut enemies = vec![dormant, weird];
        let player = feet(0.0, 0.0, 0.0);
        let level = test_level(vec![], feet(50.0, 0.0, 50.0), -100.0);
        let world = World::from_level(&level);
        let mut hazards = Hazards::new();
        hazards.step(&mut enemies, &world, &level, std::slice::from_ref(&player));
        assert_eq!(hazards.hit(&enemies, &player), None);
    }

    #[test]
    fn chaser_behaviour_still_steps_through_hazards() {
        let params = EnemyParams {
            behavior: BEHAVIOR_CHASER,
            grace_ticks: 0,
            speed: 5.0,
            ..EnemyParams::default()
        };
        let mut enemies = vec![spawn_enemy_with(1, ENEMY_KIND_CHASER, feet(0.0, 0.0, 0.0), params)];
        let level = test_level(vec![], feet(50.0, 0.0, 50.0), -100.0);
        let world = World::from_level(&level);
        let mut hazards = Hazards::new();
        hazards.step(&mut enemies, &world, &level, &[feet(10.0, 0.0, 0.0)]);
        assert!(enemies[0].pos.x > 0.0, "chaser behaviour stepped via Hazards::step");
    }
}
