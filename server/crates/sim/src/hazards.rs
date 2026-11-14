//! Deterministic enemy hazards for M3: the nine reworked enemies, the heavy-attack
//! mutex, crumble tiles and corrupter fretting.
//!
//! Owns all projectile and tile state and advances it once per server tick,
//! alongside the enemy state machines. Only `+ - * /`, `sqrt`, `abs`, `min`,
//! `max` and `clamp` are used on floats; randomness comes from an internal
//! xorshift seeded from `level.seed`, so results are bit-identical on every target.
//!
//! Design tick conversions (20 Hz, exact): 4.0 s = 80, 4.5 s = 90, 2.2 s = 44,
//! 2.0 s = 40, 1.6 s = 32, 1.4 s = 28, 1.3 s = 26, 1.2 s = 24, 1.1 s = 22,
//! 0.95 s = 19, 0.9 s = 18, 0.8 s = 16, 0.75 s = 15, 0.6 s = 12, 0.5 s = 10,
//! 0.45 s = 9, 3.5 s = 70, 8 s = 160, 0.25 s = 5.

use protocol::{
    DEATH_ENEMY, DEATH_PROJECTILE, ENEMY_ATTACK, ENEMY_DISGUISED, ENEMY_DORMANT, ENEMY_FLAG_HEAVY, ENEMY_HUNTING,
    ENEMY_RECOVER, ENEMY_TELEGRAPH, LevelData, PLATFORM_CRUMBLE, Platform, ProjectileView, TICK_HZ, TILE_FRETTED,
    TILE_GONE, TILE_WARN, TileState, Vec3,
};

use crate::rules::{
    Brain, CorrupterBrain, CorrupterPhase, ENEMY_HIT_FORGIVENESS, Enemy, EnemyParams, GaleWightBrain, GalePhase,
    LanternLeechBrain, LeechPhase, MimicBrain, MimicPhase, MotelingBrain, MotelingPhase, PallbearerBrain,
    PallbearerPhase, RiftWardenBrain, RiftPhase, SnapshrikeBrain, SnapshrikePhase, TurretBrain, TurretPhase,
    aabb_distance, dir_normalized, distance, flat_direction, horizontal_distance, move_toward, player_centre,
    segment_intersects_aabb, sphere_hits_player_box,
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
/// Most projectiles alive at once; extra shots are dropped.
pub const MAX_PROJECTILES: usize = 64;

const DT: f32 = 1.0 / TICK_HZ as f32;
const STAND_HEIGHT_TOLERANCE: f32 = 0.05;
/// Global no-harm window: nothing can hurt anyone during the first 4.0 s of a level.
const LEVEL_GRACE_TICKS: u32 = 80;

// Shared tracking cadence.
const CRUMB_SAMPLE_TICKS: u32 = 5; // breadcrumbs sampled at 0.25 s
const CRUMB_KEEP_SAMPLES: usize = 80; // 4.0 s of breadcrumbs
const CRUMB_LAG_SAMPLES: usize = 12; // breadcrumb target is 3.0 s old
const SPEED_HISTORY_TICKS: usize = 30; // 1.5 s of horizontal-speed samples
const HEAVY_WAIT_MAX_TICKS: u32 = 40; // a blocked heavy waits frozen for at most 2.0 s

// Pallbearer.
const PALLBEARER_HUNT_SPEED: f32 = 5.2;
const PALLBEARER_FAST_SPEED: f32 = 6.0; // level 14+
const PALLBEARER_LUNGE_SPEED: f32 = 11.0;
const PALLBEARER_LUNGE_TICKS: u32 = 9; // 0.45 s
const PALLBEARER_KNEEL_TICKS: u32 = 24; // 1.2 s telegraph
const PALLBEARER_RECOVER_TICKS: u32 = 40; // 2.0 s
const PALLBEARER_RECOVER_DRIFT: f32 = 2.0;
const PALLBEARER_HUNT_SHOVE: f32 = 4.0;
const PALLBEARER_LUNGE_RADIUS: f32 = 1.1;
const PALLBEARER_KNEEL_RANGE: f32 = 2.0;
const PALLBEARER_FAST_LEVEL: u32 = 14;

// Snapshrike.
const SNAPSHRIKE_ORBIT_SPEED: f32 = 6.8;
const SNAPSHRIKE_ORBIT_MIN: f32 = 4.0;
const SNAPSHRIKE_ORBIT_MAX: f32 = 7.0;
const SNAPSHRIKE_LOCK_TICKS: u32 = 15; // 0.75 s
const SNAPSHRIKE_TRACK_TICKS: u32 = 8; // tracks for the first 0.4 s
const SNAPSHRIKE_DASH_SPEED: f32 = 13.5;
const SNAPSHRIKE_DASH_MAX_DIST: f32 = 11.0;
const SNAPSHRIKE_DASH_TICKS: u32 = 16; // 0.8 s
const SNAPSHRIKE_DASH_RADIUS: f32 = 0.7;
const SNAPSHRIKE_STUN_TICKS: u32 = 12; // 0.6 s
const SNAPSHRIKE_RECOVER_TICKS: u32 = 26; // 1.3 s
const SNAPSHRIKE_RECOVER_DRIFT: f32 = 1.5;
const SNAPSHRIKE_DASH_COOLDOWN_TICKS: u32 = 44; // 2.2 s dash-to-dash
const SNAPSHRIKE_STALK_SHOVE: f32 = 5.0;

// Moteling.
const MOTELING_WANDER_SPEED: f32 = 3.6;
const MOTELING_ORBIT_SPEED: f32 = 5.5;
const MOTELING_ORBIT_RADIUS: f32 = 5.0;
const MOTELING_AGGRO_RADIUS: f32 = 20.0;
const MOTELING_INHALE_TICKS: u32 = 40; // 2.0 s, HEAVY
const MOTELING_ORBIT_PERIOD_TICKS: u32 = 120; // an inhale every 6.0 s
const MOTELING_COOLDOWN_TICKS: u32 = 80; // 4.0 s
const MOTELING_SHRIEK_RADIUS: f32 = 4.0; // lethal if the marked player is moving; single target
const MOTELING_STILL_SPEED: f32 = 1.0;
const MOTELING_DRIFT_HOME: f32 = 1.8;

// Rift warden.
const RIFT_HAUNT_SPEED: f32 = 1.6;
const RIFT_TRIGGER_RADIUS: f32 = 4.0;
const RIFT_IGNITE_TICKS: u32 = 22; // 1.1 s
const RIFT_SHOVE_TICKS: u32 = 4; // displacement window
const RIFT_SHOVE_RADIUS: f32 = 3.5;
const RIFT_SHOVE_SPEED: f32 = 8.0;
const RIFT_SHOVE_UP: f32 = 2.0;
const RIFT_STAGGER_TICKS: u8 = 18; // 0.9 s
const RIFT_SETTLE_TICKS: u32 = 70; // 3.5 s
const RIFT_CONTACT_SHOVE: f32 = 4.0;
const RIFT_CREEP_SPEED: f32 = 0.8;

// Gale wight.
const GALE_CRUISE_SPEED: f32 = 7.0;
const GALE_CRUISE_MIN_TICKS: u32 = 80; // at least 4.0 s between volleys
const GALE_CRUISE_ALTITUDE: f32 = 6.0; // map-top + 6
const GALE_MARK_TICKS: u32 = 18; // 0.9 s (telegraph)
const GALE_LOCK_TICKS: u32 = 12; // 0.6 s (telegraph)
const GALE_LAG_TICKS: usize = 7; // 0.35 s position lag
const GALE_FALL_TICKS: u32 = 22; // 1.1 s fall
const GALE_SHELL_SPAWN_HEIGHT: f32 = 12.0;
const GALE_FALL_SPEED: f32 = GALE_SHELL_SPAWN_HEIGHT / 1.1;
const GALE_BLAST_RADIUS: f32 = 2.1; // lethal
const GALE_RING_RADIUS: f32 = 4.2; // knockback ring
const GALE_RING_PUSH: f32 = 7.0;
const GALE_DOWNTIME_TICKS: u32 = 70; // 3.5 s

const GALE_MAX_SHELLS: u32 = 3;

// Lantern leech.
const LEECH_PROWL_SPEED: f32 = 3.2;
const LEECH_PROWL_RADIUS: f32 = 24.0;
const LEECH_SIPHON_RANGE: f32 = 30.0;
const LEECH_TOP_SPEED: f32 = 9.5;
const LEECH_RAMP_TICKS: u32 = 32; // accelerates over 1.6 s
const LEECH_FLARE_TICKS: u32 = 16; // flares 0.8 s before arrival
const LEECH_ARRIVE_DIST: f32 = 0.6;
const LEECH_GORGE_TICKS: u32 = 40; // sits 2.0 s
const LEECH_REARBACK_TICKS: u32 = 12; // rears back 0.6 s before the sting
const LEECH_STING_RADIUS: f32 = 1.5;
const LEECH_STING_EVERY_TICKS: u32 = 20; // 1.0 s between stings
const LEECH_SLIDE_TICKS: u32 = 60; // 3.0 s minimum before the next siphon

// Turret.
const TURRET_CHARGE_TICKS: u32 = 19; // 0.95 s
const TURRET_VENT_TICKS: u32 = 28; // 1.4 s
const TURRET_BOLT_SPEED: f32 = 13.0;
const TURRET_FIRE_TICKS: u32 = 2; // 0.1 s, for VFX

// Mimic.
const MIMIC_TRIGGER_RADIUS: f32 = 4.0;
const MIMIC_SHIMMER_PERIOD: u32 = 60; // every 3.0 s
const MIMIC_SHIMMER_TICKS: u32 = 10; // 0.5 s
const MIMIC_HISS_TICKS: u32 = 18; // 0.9 s
const MIMIC_LUNGE_SPEED: f32 = 8.5;
const MIMIC_LUNGE_TICKS: u32 = 12; // 0.6 s
const MIMIC_LUNGE_RADIUS: f32 = 0.7;
const MIMIC_SPENT_TICKS: u32 = 50; // 2.5 s

// Corrupter.
const CORRUPTER_DRIFT_SPEED: f32 = 3.0;
const CORRUPTER_TARGET_RANGE: f32 = 18.0;
const CORRUPTER_AURA_RADIUS: f32 = 2.0;
const CORRUPTER_FRET_TICKS: u32 = 41; // strictly more than 2.0 s inside the aura
const CORRUPTER_FRET_DURATION: u32 = 160; // fretted for 8 s
const CORRUPTER_REFORM_TICKS: u32 = 30; // 1.5 s

const RNG_SEED: u64 = 0x243F_6A88_85A3_08D3;

/// What the server needs to know about a player this tick.
#[derive(Debug, Clone, PartialEq)]
pub struct PlayerSense {
    /// Server player id (echoed back in effects).
    pub id: u32,
    /// Feet position.
    pub feet: Vec3,
    /// Velocity (u/s); the horizontal part drives moteling's stillness check.
    pub vel: Vec3,
    /// True while Bulwark's Aegis is active: immune to every Kill and Shove
    /// except gale_wight shell blasts.
    pub warded: bool,
}

/// Something an enemy (or its projectile) did to a player this tick.
#[derive(Debug, Clone, PartialEq)]
pub enum HazardEffect {
    /// Lethal contact/blast. `cause` is `DEATH_ENEMY` or `DEATH_PROJECTILE`.
    Kill { player: u32, cause: u8 },
    /// Knockback velocity (u/s); the server writes it into the player's velocity.
    /// `stagger_ticks` is in server ticks (0 = no stagger).
    Shove { player: u32, vel: Vec3, stagger_ticks: u8 },
}

/// A flying projectile: turret bolts and falling gale-wight shells.
#[derive(Debug, Clone, PartialEq)]
struct Projectile {
    id: u32,
    pos: Vec3,
    vel: Vec3,
    ticks_alive: u32,
    kind: ProjectileKind,
    /// Shells only: vertical level at which they detonate (the ground under the target).
    impact_y: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ProjectileKind {
    Bolt,
    Shell,
}

/// A platform that is currently `TILE_WARN`, `TILE_GONE` or `TILE_FRETTED`.
#[derive(Debug, Clone, PartialEq)]
struct TileEntry {
    id: u16,
    state: u8,
    ticks_left: u32,
}

/// Bundles the immutable world data the enemy step functions need.
struct Ctx<'a> {
    world: &'a World,
    level: &'a LevelData,
    players: &'a [PlayerSense],
    pickups: &'a [Vec3],
    speed_mul: f32,
    damage_ok: bool,
    /// Snapshot of every enemy's start-of-tick position; index matches the `enemies` slice.
    positions: &'a [Vec3],
}

/// Owns projectiles and tile states. Call [`Hazards::step`] once per server tick.
#[derive(Debug, Default)]
pub struct Hazards {
    projectiles: Vec<Projectile>,
    tiles: Vec<TileEntry>,
    next_projectile_id: u32,
    ticks_elapsed: u32,
    heavy_holder: Option<u32>,
    crumb_timer: u32,
    crumbs: Vec<(u32, Vec<Vec3>)>,
    speed_history: Vec<(u32, Vec<f32>)>,
    pending_enemy_shoves: Vec<(usize, Vec3)>,
    rng: u64,
    rng_seeded: bool,
}

impl Hazards {
    /// Create an empty `Hazards`.
    pub fn new() -> Self {
        Self::default()
    }

    /// Drop all projectiles and tile state and rewind the clock. Call when the level changes.
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// Advance tiles, enemies, projectiles and crumble detection by one server tick.
    ///
    /// `players` are the living players; everything else (aim points, stillness
    /// checks, target centres) is derived from them. `pickups` are the positions
    /// of gifts collected this tick (lantern_leech). `speed_mul` is the phase-2
    /// enemy speed multiplier (1.0 or 1.1). Effects are ordered by enemy spawn
    /// order, then by player order. The first 4.0 s of a level and dormant
    /// enemies never produce effects.
    pub fn step(
        &mut self,
        enemies: &mut [Enemy],
        world: &World,
        level: &LevelData,
        players: &[PlayerSense],
        pickups: &[Vec3],
        speed_mul: f32,
    ) -> Vec<HazardEffect> {
        if !self.rng_seeded {
            let seed = u64::from(level.seed).wrapping_add(1).wrapping_mul(0x9E37_79B9_7F4A_7C15);
            self.rng = RNG_SEED ^ seed | 1;
            self.rng_seeded = true;
        }
        let damage_ok = self.ticks_elapsed >= LEVEL_GRACE_TICKS;
        self.ticks_elapsed = self.ticks_elapsed.saturating_add(1);

        let expired = self.age_tiles();
        self.sample_histories(players);

        // Snapshot positions so the rift warden can find its neighbours without
        // holding a second live borrow of the enemy slice.
        let enemy_positions: Vec<Vec3> = enemies.iter().map(|enemy| enemy.pos.clone()).collect();
        let ctx = Ctx {
            world,
            level,
            players,
            pickups,
            speed_mul,
            damage_ok,
            positions: &enemy_positions,
        };
        let mut fx: Vec<HazardEffect> = Vec::new();
        for i in 0..enemies.len() {
            if let Some(enemy) = enemies.get_mut(i) {
                step_one(self, i, enemy, &ctx, &mut fx);
            }
        }
        // Rift warden shoves ripple through other enemies (not the warden itself).
        for (index, vel) in self.take_pending_enemy_shoves() {
            if let Some(other) = enemies.get_mut(index) {
                other.knock_vel = vel;
                other.knock_ticks = RIFT_STAGGER_TICKS;
            }
        }
        self.step_projectiles(&ctx, &mut fx);
        self.mark_crumble(level, players, &expired);

        if !damage_ok {
            fx.clear();
        }
        fx
    }

    /// Bulwark pulse: knock every enemy within `radius` of `centre` directly away at
    /// 7 u/s for 0.8 s and interrupt its current state into its cooldown state.
    /// A heavy wind-up interrupted this way releases the heavy slot.
    pub fn pulse(&mut self, enemies: &mut [Enemy], centre: &Vec3, radius: f32) {
        for enemy in enemies.iter_mut() {
            let d = distance(&enemy.pos, centre);
            if d > radius {
                continue;
            }
            let dir = if d > 1e-6 {
                Vec3::new(
                    (enemy.pos.x - centre.x) / d,
                    (enemy.pos.y - centre.y) / d,
                    (enemy.pos.z - centre.z) / d,
                )
            } else {
                Vec3::new(1.0, 0.0, 0.0)
            };
            enemy.knock_vel = Vec3::new(dir.x * 7.0, dir.y * 7.0, dir.z * 7.0);
            enemy.knock_ticks = 16;
            if self.heavy_holder == Some(enemy.id) {
                self.heavy_holder = None;
            }
            cooldown_state(enemy);
        }
    }

    /// Views of every enemy, for the per-tick snapshot. Telegraphing enemies report
    /// `ENEMY_TELEGRAPH` with `prog` 0..=255 and their anchor in `target`; attacking
    /// enemies report `ENEMY_ATTACK` with `prog` 255; others zero/`None`.
    pub fn views(&self, enemies: &[Enemy]) -> Vec<protocol::EnemyView> {
        enemies.iter().map(view_of).collect()
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

    /// Views of every non-normal platform (`TILE_WARN` / `TILE_GONE` / `TILE_FRETTED`), sorted by id.
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

    fn take_pending_enemy_shoves(&mut self) -> Vec<(usize, Vec3)> {
        core::mem::take(&mut self.pending_enemy_shoves)
    }

    fn sample_histories(&mut self, players: &[PlayerSense]) {
        self.crumb_timer += 1;
        let do_crumb = self.crumb_timer >= CRUMB_SAMPLE_TICKS;
        if do_crumb {
            self.crumb_timer = 0;
        }
        for player in players {
            let hs = (player.vel.x * player.vel.x + player.vel.z * player.vel.z).sqrt();
            push_capped(&mut self.speed_history, player.id, hs, SPEED_HISTORY_TICKS);
            if do_crumb {
                push_capped(&mut self.crumbs, player.id, player.feet.clone(), CRUMB_KEEP_SAMPLES);
            }
        }
    }

    fn breadcrumb_anchor(&self, player_id: u32, fallback: &Vec3) -> Vec3 {
        match self.crumbs.iter().find(|entry| entry.0 == player_id) {
            Some((_, samples)) if !samples.is_empty() => {
                let index = samples.len().saturating_sub(1 + CRUMB_LAG_SAMPLES);
                samples.get(index).cloned().unwrap_or_else(|| fallback.clone())
            }
            _ => fallback.clone(),
        }
    }

    fn avg_speed(&self, player_id: u32) -> f32 {
        match self.speed_history.iter().find(|entry| entry.0 == player_id) {
            Some((_, samples)) if !samples.is_empty() => {
                let sum: f32 = samples.iter().sum();
                sum / samples.len() as f32
            }
            _ => 0.0,
        }
    }

    fn next_rng(&mut self) -> u64 {
        let mut x = self.rng;
        if x == 0 {
            x = RNG_SEED;
        }
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.rng = x;
        x
    }

    fn rng_flat_direction(&mut self) -> Vec3 {
        let dx = (self.next_rng() % 2001) as f32 / 1000.0 - 1.0;
        let dz = (self.next_rng() % 2001) as f32 / 1000.0 - 1.0;
        let d = (dx * dx + dz * dz).sqrt();
        if d <= 1e-6 {
            Vec3::new(1.0, 0.0, 0.0)
        } else {
            Vec3::new(dx / d, 0.0, dz / d)
        }
    }

    fn random_target(&mut self, players: &[PlayerSense]) -> Option<u32> {
        if players.is_empty() {
            return None;
        }
        let index = (self.next_rng() % players.len() as u64) as usize;
        players.get(index).map(|p| p.id)
    }

    fn age_tiles(&mut self) -> Vec<u16> {
        let mut expired = Vec::new();
        let mut i = 0;
        while i < self.tiles.len() {
            self.tiles[i].ticks_left = self.tiles[i].ticks_left.saturating_sub(1);
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

    /// Mark `id` as fretted, replacing any existing non-normal state.
    fn mark_fretted(&mut self, id: u16, duration: u32) {
        if self.tiles.iter().any(|t| t.id == id) {
            return;
        }
        self.tiles.push(TileEntry {
            id,
            state: TILE_FRETTED,
            ticks_left: duration,
        });
    }

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

    fn mark_crumble(&mut self, level: &LevelData, players: &[PlayerSense], expired: &[u16]) {
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
            let standing = players.iter().any(|player| {
                player.feet.x >= platform.bounds.min.x
                    && player.feet.x <= platform.bounds.max.x
                    && player.feet.z >= platform.bounds.min.z
                    && player.feet.z <= platform.bounds.max.z
                    && (player.feet.y - top).abs() <= STAND_HEIGHT_TOLERANCE
            });
            if standing {
                self.mark_warn(platform.id);
            }
        }
    }

    fn spawn_projectile(&mut self, pos: Vec3, vel: Vec3, kind: ProjectileKind, impact_y: f32) -> bool {
        if self.projectiles.len() >= MAX_PROJECTILES {
            return false;
        }
        let id = self.next_projectile_id;
        self.next_projectile_id = self.next_projectile_id.wrapping_add(1);
        self.projectiles.push(Projectile {
            id,
            pos,
            vel,
            ticks_alive: 0,
            kind,
            impact_y,
        });
        true
    }

    fn step_projectiles(&mut self, ctx: &Ctx, fx: &mut Vec<HazardEffect>) {
        for p in self.projectiles.iter_mut() {
            p.pos.x += p.vel.x * DT;
            p.pos.y += p.vel.y * DT;
            p.pos.z += p.vel.z * DT;
            p.ticks_alive += 1;
        }
        // Find shells that hit the ground or a platform top face; they detonate there.
        let mut shell_hits: Vec<usize> = Vec::new();
        for (i, p) in self.projectiles.iter().enumerate() {
            if p.kind != ProjectileKind::Shell {
                continue;
            }
            if point_in_solid(&p.pos, ctx.world) || p.pos.y <= p.impact_y {
                shell_hits.push(i);
            }
        }
        for i in &shell_hits {
            if let Some(p) = self.projectiles.get(*i) {
                let blast = Vec3::new(p.pos.x, p.pos.y.max(p.impact_y), p.pos.z);
                blast_effects(ctx, fx, &blast);
            }
        }
        for i in shell_hits.iter().rev() {
            if *i < self.projectiles.len() {
                self.projectiles.remove(*i);
            }
        }
        // Turret bolts kill on contact; shells never kill on the way down.
        for p in ctx.players {
            for projectile in &self.projectiles {
                if projectile.kind != ProjectileKind::Bolt {
                    continue;
                }
                if p.warded {
                    continue;
                }
                if sphere_hits_player_box(&projectile.pos, PROJECTILE_RADIUS, &p.feet) {
                    fx.push(HazardEffect::Kill {
                        player: p.id,
                        cause: DEATH_PROJECTILE,
                    });
                }
            }
        }
        let kill_y = ctx.level.kill_y;
        self.projectiles.retain(|p| {
            p.pos.y >= kill_y && p.ticks_alive < PROJECTILE_LIFETIME_TICKS && !point_in_solid(&p.pos, ctx.world)
        });
    }
}

/// Unused since the rift-warden shove loop was rebuilt (see `step`). The FN edit
/// tool cannot delete items; the lead should delete this function.
#[allow(dead_code)]
fn index_alive(enemies: &[Enemy], index: usize) -> bool {
    enemies.get(index).is_some()
}

fn point_in_solid(p: &Vec3, world: &World) -> bool {
    world.solids.iter().any(|b| {
        p.x >= b.min.x && p.x <= b.max.x && p.y >= b.min.y && p.y <= b.max.y && p.z >= b.min.z && p.z <= b.max.z
    })
}

fn has_los(world: &World, from: &Vec3, to: &Vec3) -> bool {
    !world.solids.iter().any(|b| segment_intersects_aabb(from, to, b))
}

fn predict_intercept(from: &Vec3, target_pos: &Vec3, target_vel: &Vec3, speed: f32) -> Vec3 {
    let d = distance(from, target_pos);
    let t = if speed > 1e-6 { d / speed } else { 0.0 };
    Vec3::new(
        target_pos.x + target_vel.x * t,
        target_pos.y + target_vel.y * t,
        target_pos.z + target_vel.z * t,
    )
}

fn nearest_index_3d(players: &[PlayerSense], from: &Vec3) -> Option<usize> {
    nearest_index_within(players, from, f32::INFINITY, false)
}

fn nearest_index_within(players: &[PlayerSense], from: &Vec3, range: f32, horizontal: bool) -> Option<usize> {
    let mut best: Option<(usize, f32)> = None;
    for (i, player) in players.iter().enumerate() {
        let centre = player_centre(&player.feet);
        let d = if horizontal {
            horizontal_distance(from, &centre)
        } else {
            distance(from, &centre)
        };
        if d > range {
            continue;
        }
        match best {
            Some((_, bd)) if bd <= d => {}
            _ => best = Some((i, d)),
        }
    }
    best.map(|(i, _)| i)
}

/// Gale-wight shell blast: lethal inside 2.1 u, knockback 7 u/s in the 2.1-4.2 u ring.
/// Warded players are NOT immune to this (the one exception in the roster).
fn blast_effects(ctx: &Ctx, fx: &mut Vec<HazardEffect>, blast: &Vec3) {
    for player in ctx.players {
        let centre = player_centre(&player.feet);
        let d = distance(blast, &centre);
        if d <= GALE_BLAST_RADIUS {
            fx.push(HazardEffect::Kill {
                player: player.id,
                cause: DEATH_ENEMY,
            });
            continue;
        }
        if d <= GALE_RING_RADIUS {
            let dir = flat_direction(blast, &centre);
            fx.push(HazardEffect::Shove {
                player: player.id,
                vel: Vec3::new(dir.x * GALE_RING_PUSH, 0.0, dir.z * GALE_RING_PUSH),
                stagger_ticks: 0,
            });
        }
    }
}

fn push_kill(fx: &mut Vec<HazardEffect>, player: &PlayerSense, cause: u8) {
    if player.warded {
        return; // ward: immune to every Kill except gale blasts
    }
    fx.push(HazardEffect::Kill { player: player.id, cause });
}

fn push_shove(fx: &mut Vec<HazardEffect>, player: &PlayerSense, vel: Vec3, stagger_ticks: u8) {
    if player.warded {
        return; // ward: immune to every Shove except gale blasts
    }
    fx.push(HazardEffect::Shove {
        player: player.id,
        vel,
        stagger_ticks,
    });
}

fn push_capped<T>(entries: &mut Vec<(u32, Vec<T>)>, id: u32, value: T, cap: usize) {
    match entries.iter_mut().find(|entry| entry.0 == id) {
        Some(entry) => {
            entry.1.push(value);
            if entry.1.len() > cap {
                entry.1.remove(0);
            }
        }
        None => entries.push((id, vec![value])),
    }
}

fn step_one(h: &mut Hazards, index: usize, e: &mut Enemy, ctx: &Ctx, fx: &mut Vec<HazardEffect>) {
    if e.knock_ticks > 0 {
        e.pos.x += e.knock_vel.x * DT;
        e.pos.y += e.knock_vel.y * DT;
        e.pos.z += e.knock_vel.z * DT;
        e.knock_ticks -= 1;
        return;
    }
    if e.active_in > 0 {
        e.active_in -= 1;
        return;
    }
    let id = e.id;
    let speed_mul = e.params.speed_mul * ctx.speed_mul;
    let contact_radius = e.params.size * ENEMY_HIT_FORGIVENESS;
    let home = e.home.clone();
    match &mut e.brain {
        Brain::Pallbearer(b) => pallbearer_step(h, &mut e.pos, b, speed_mul, contact_radius, &home, ctx, fx),
        Brain::Snapshrike(b) => snapshrike_step(h, id, &mut e.pos, b, speed_mul, contact_radius, &home, ctx, fx),
        Brain::Moteling(b) => moteling_step(h, id, &mut e.pos, b, speed_mul, &home, ctx, fx),
        Brain::RiftWarden(b) => rift_warden_step(h, index, &mut e.pos, b, speed_mul, contact_radius, &home, ctx, fx),
        Brain::GaleWight(b) => gale_wight_step(h, id, &mut e.pos, b, speed_mul, &home, ctx),
        Brain::LanternLeech(b) => lantern_leech_step(&mut e.pos, b, speed_mul, &home, ctx, fx),
        Brain::Turret(b) => turret_step(h, &mut e.pos, b, speed_mul, ctx),
        Brain::Mimic(b) => mimic_step(&mut e.pos, b, speed_mul, ctx, fx),
        Brain::Corrupter(b) => corrupter_step(h, &mut e.pos, b, speed_mul, &home, ctx),
    }
    // The pallbearer never descends below y = 0.
    if e.pos.y < 0.0 {
        e.pos.y = 0.0;
    }
}

#[allow(clippy::too_many_arguments)]
fn pallbearer_step(
    h: &mut Hazards,
    pos: &mut Vec3,
    b: &mut PallbearerBrain,
    speed_mul: f32,
    contact_radius: f32,
    home: &Vec3,
    ctx: &Ctx,
    fx: &mut Vec<HazardEffect>,
) {
    match b.phase {
        PallbearerPhase::Hunt => {
            let target = nearest_index_3d(ctx.players, pos);
            match target.and_then(|i| ctx.players.get(i)) {
                None => {
                    move_toward(pos, home, PALLBEARER_RECOVER_DRIFT * speed_mul * DT);
                }
                Some(target) => {
                    let centre = player_centre(&target.feet);
                    let anchor = h.breadcrumb_anchor(target.id, &target.feet);
                    let speed = if ctx.level.level >= PALLBEARER_FAST_LEVEL {
                        PALLBEARER_FAST_SPEED
                    } else {
                        PALLBEARER_HUNT_SPEED
                    };
                    move_toward(pos, &anchor, speed * speed_mul * DT);
                    if sphere_hits_player_box(pos, contact_radius, &target.feet) {
                        let dir = flat_direction(pos, &centre);
                        push_shove(
                            fx,
                            target,
                            Vec3::new(dir.x * PALLBEARER_HUNT_SHOVE, 0.0, dir.z * PALLBEARER_HUNT_SHOVE),
                            0,
                        );
                    }
                    if distance(pos, &centre) <= PALLBEARER_KNEEL_RANGE {
                        b.phase = PallbearerPhase::Kneel;
                        b.timer = PALLBEARER_KNEEL_TICKS;
                        b.locked = centre;
                    }
                }
            }
        }
        PallbearerPhase::Kneel => {
            b.timer = b.timer.saturating_sub(1);
            if b.timer == 0 {
                b.phase = PallbearerPhase::Lunge;
                b.timer = PALLBEARER_LUNGE_TICKS;
            }
        }
        PallbearerPhase::Lunge => {
            move_toward(pos, &b.locked, PALLBEARER_LUNGE_SPEED * speed_mul * DT);
            for player in ctx.players {
                if sphere_hits_player_box(pos, PALLBEARER_LUNGE_RADIUS, &player.feet) {
                    push_kill(fx, player, DEATH_ENEMY);
                }
            }
            b.timer = b.timer.saturating_sub(1);
            if b.timer == 0 {
                b.phase = PallbearerPhase::Recover;
                b.timer = PALLBEARER_RECOVER_TICKS;
            }
        }
        PallbearerPhase::Recover => {
            let away = flat_direction(&b.locked, pos);
            pos.x += away.x * PALLBEARER_RECOVER_DRIFT * speed_mul * DT;
            pos.z += away.z * PALLBEARER_RECOVER_DRIFT * speed_mul * DT;
            b.timer = b.timer.saturating_sub(1);
            if b.timer == 0 {
                b.phase = PallbearerPhase::Hunt;
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn snapshrike_step(
    h: &mut Hazards,
    id: u32,
    pos: &mut Vec3,
    b: &mut SnapshrikeBrain,
    speed_mul: f32,
    contact_radius: f32,
    home: &Vec3,
    ctx: &Ctx,
    fx: &mut Vec<HazardEffect>,
) {
    match b.phase {
        SnapshrikePhase::Stalk => {
            b.dash_cooldown = b.dash_cooldown.saturating_sub(1);
            let target = nearest_index_within(ctx.players, pos, f32::INFINITY, true);
            match target.and_then(|i| ctx.players.get(i)) {
                None => move_toward(pos, home, SNAPSHRIKE_ORBIT_SPEED * speed_mul * DT),
                Some(target) => {
                    let centre = player_centre(&target.feet);
                    let d = horizontal_distance(pos, &centre);
                    if d > SNAPSHRIKE_ORBIT_MAX {
                        move_toward(pos, &centre, SNAPSHRIKE_ORBIT_SPEED * speed_mul * DT);
                    } else if d < SNAPSHRIKE_ORBIT_MIN {
                        let away = flat_direction(&centre, pos);
                        pos.x += away.x * SNAPSHRIKE_ORBIT_SPEED * speed_mul * DT;
                        pos.z += away.z * SNAPSHRIKE_ORBIT_SPEED * speed_mul * DT;
                    } else {
                        // Orbit counter-clockwise at 4-7 u.
                        let r = flat_direction(&centre, pos);
                        pos.x += -r.z * SNAPSHRIKE_ORBIT_SPEED * speed_mul * DT;
                        pos.z += r.x * SNAPSHRIKE_ORBIT_SPEED * speed_mul * DT;
                    }
                    if sphere_hits_player_box(pos, contact_radius, &target.feet) {
                        let dir = flat_direction(pos, &centre);
                        push_shove(
                            fx,
                            target,
                            Vec3::new(dir.x * SNAPSHRIKE_STALK_SHOVE, 0.0, dir.z * SNAPSHRIKE_STALK_SHOVE),
                            0,
                        );
                    }
                    if b.dash_cooldown == 0 {
                        b.phase = SnapshrikePhase::Lock;
                        b.timer = SNAPSHRIKE_LOCK_TICKS;
                        b.target_id = Some(target.id);
                        b.aim = predict_intercept(pos, &centre, &target.vel, SNAPSHRIKE_DASH_SPEED);
                    }
                }
            }
        }
        SnapshrikePhase::Lock => {
            if b.timer > SNAPSHRIKE_LOCK_TICKS - SNAPSHRIKE_TRACK_TICKS {
                // Tracking window: the last 0.35 s the line is frozen.
                if let Some(target) = b
                    .target_id
                    .and_then(|tid| ctx.players.iter().find(|p| p.id == tid))
                {
                    let centre = player_centre(&target.feet);
                    b.aim = predict_intercept(pos, &centre, &target.vel, SNAPSHRIKE_DASH_SPEED);
                }
            }
            b.timer = b.timer.saturating_sub(1);
            if b.timer == 0 {
                b.phase = SnapshrikePhase::Dash;
                b.timer = SNAPSHRIKE_DASH_TICKS;
                b.dash_dir = dir_normalized(pos, &b.aim);
                b.dash_dist = 0.0;
                b.dash_cooldown = SNAPSHRIKE_DASH_COOLDOWN_TICKS;
            }
        }
        SnapshrikePhase::Dash => {
            let step_len = SNAPSHRIKE_DASH_SPEED * speed_mul * DT;
            let next = Vec3::new(
                pos.x + b.dash_dir.x * step_len,
                pos.y + b.dash_dir.y * step_len,
                pos.z + b.dash_dir.z * step_len,
            );
            if point_in_solid(&next, ctx.world) {
                // Blocked by a platform: the dash ends early and the shrike is stunned.
                b.phase = SnapshrikePhase::Stunned;
                b.timer = SNAPSHRIKE_STUN_TICKS;
            } else {
                *pos = next;
                b.dash_dist += step_len;
                for player in ctx.players {
                    if sphere_hits_player_box(pos, SNAPSHRIKE_DASH_RADIUS, &player.feet) {
                        push_kill(fx, player, DEATH_ENEMY);
                    }
                }
                b.timer = b.timer.saturating_sub(1);
                if b.timer == 0 || b.dash_dist >= SNAPSHRIKE_DASH_MAX_DIST {
                    b.phase = SnapshrikePhase::Recover;
                    b.timer = SNAPSHRIKE_RECOVER_TICKS;
                }
            }
        }
        SnapshrikePhase::Stunned => {
            b.timer = b.timer.saturating_sub(1);
            if b.timer == 0 {
                b.phase = SnapshrikePhase::Recover;
                b.timer = SNAPSHRIKE_RECOVER_TICKS;
            }
        }
        SnapshrikePhase::Recover => {
            pos.x -= b.dash_dir.x * SNAPSHRIKE_RECOVER_DRIFT * speed_mul * DT;
            pos.y -= b.dash_dir.y * SNAPSHRIKE_RECOVER_DRIFT * speed_mul * DT;
            pos.z -= b.dash_dir.z * SNAPSHRIKE_RECOVER_DRIFT * speed_mul * DT;
            b.timer = b.timer.saturating_sub(1);
            if b.timer == 0 {
                b.phase = SnapshrikePhase::Stalk;
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn moteling_step(
    h: &mut Hazards,
    id: u32,
    pos: &mut Vec3,
    b: &mut MotelingBrain,
    speed_mul: f32,
    home: &Vec3,
    ctx: &Ctx,
    fx: &mut Vec<HazardEffect>,
) {
    match b.phase {
        MotelingPhase::Wander => {
            match &b.wander_goal {
                Some(goal) if distance(pos, goal) > 1.0 => {
                    let goal = goal.clone();
                    move_toward(pos, &goal, MOTELING_WANDER_SPEED * speed_mul * DT);
                }
                _ => {
                    let dir = h.rng_flat_direction();
                    let reach = 2.0 + (h.next_rng() % 7000) as f32 / 1000.0;
                    b.wander_goal = Some(Vec3::new(pos.x + dir.x * reach, home.y, pos.z + dir.z * reach));
                }
            }
            if let Some(target) = moteling_pick_target(h, ctx.players, pos) {
                b.marked = Some(target);
                b.phase = MotelingPhase::Orbit;
                b.orbit_ticks = 0;
                b.acquired = false;
                b.heavy_wait = 0;
            }
        }
        MotelingPhase::Orbit => {
            if b.marked.is_none() {
                b.marked = moteling_pick_target(h, ctx.players, pos);
                if b.marked.is_none() {
                    b.phase = MotelingPhase::Wander;
                    return;
                }
            }
            let Some(marked) = b.marked else {
                b.phase = MotelingPhase::Wander;
                return;
            };
            let found = ctx.players.iter().find(|p| p.id == marked);
            let Some(target) = found else {
                b.marked = None; // target died
                b.phase = MotelingPhase::Wander;
                return;
            };
            let centre = player_centre(&target.feet);
            b.anchor = centre.clone();
            b.orbit_ticks += 1;
            let dx = pos.x - centre.x;
            let dz = pos.z - centre.z;
            let len = (dx * dx + dz * dz).sqrt();
            let (rx, rz) = if len > 1e-4 { (dx / len, dz / len) } else { (1.0, 0.0) };
            let corr = (MOTELING_ORBIT_RADIUS - len).clamp(-2.5, 2.5);
            pos.x += (-rz * MOTELING_ORBIT_SPEED + rx * corr) * speed_mul * DT;
            pos.z += (rx * MOTELING_ORBIT_SPEED + rz * corr) * speed_mul * DT;
            if b.orbit_ticks >= MOTELING_ORBIT_PERIOD_TICKS {
                b.phase = MotelingPhase::Inhale;
                b.timer = MOTELING_INHALE_TICKS;
                b.acquired = false;
                b.heavy_wait = 0;
            }
        }
        MotelingPhase::Inhale => {
            let Some(marked) = b.marked else {
                b.phase = MotelingPhase::Cooldown;
                b.timer = MOTELING_COOLDOWN_TICKS;
                return;
            };
            let found = ctx.players.iter().find(|p| p.id == marked);
            let Some(target) = found else {
                b.marked = None;
                release_heavy(h, id, b);
                b.phase = MotelingPhase::Cooldown;
                b.timer = MOTELING_COOLDOWN_TICKS;
                return;
            };
            b.anchor = player_centre(&target.feet);
            if !b.acquired {
                if h.heavy_holder.is_none() {
                    b.acquired = true;
                    h.heavy_holder = Some(id);
                } else {
                    // Heavy slot is taken: wait frozen at the first frame.
                    b.heavy_wait += 1;
                    if b.heavy_wait > HEAVY_WAIT_MAX_TICKS {
                        b.marked = None;
                        b.orbit_ticks = 0;
                        b.heavy_wait = 0;
                        b.phase = MotelingPhase::Cooldown;
                        b.timer = MOTELING_COOLDOWN_TICKS;
                    }
                    return;
                }
            }
            b.timer = b.timer.saturating_sub(1);
            if b.timer == 0 {
                release_heavy(h, id, b);
                let hs = (target.vel.x * target.vel.x + target.vel.z * target.vel.z).sqrt();
                // The ring snaps shut: only a marked player who kept moving is hurt.
                // Routed through push_kill so a warded marked player survives.
                if hs > MOTELING_STILL_SPEED {
                    push_kill(fx, target, DEATH_ENEMY);
                }
                b.marked = None;
                b.orbit_ticks = 0;
                b.heavy_wait = 0;
                b.phase = MotelingPhase::Cooldown;
                b.timer = MOTELING_COOLDOWN_TICKS;
            }
        }
        MotelingPhase::Cooldown => {
            move_toward(pos, home, MOTELING_DRIFT_HOME * speed_mul * DT);
            b.timer = b.timer.saturating_sub(1);
            if b.timer == 0 {
                b.phase = MotelingPhase::Orbit;
                b.orbit_ticks = MOTELING_ORBIT_PERIOD_TICKS / 2; // re-approaches the ring before the next inhale
                b.marked = None;
            }
        }
    }
    // Virtue: never report the shriek radius anywhere except the Kill effect.
    let _ = MOTELING_SHRIEK_RADIUS;
}

fn moteling_pick_target(h: &Hazards, players: &[PlayerSense], from: &Vec3) -> Option<u32> {
    let mut best: Option<(u32, f32)> = None;
    for player in players {
        let centre = player_centre(&player.feet);
        let d = distance(from, &centre);
        if d > MOTELING_AGGRO_RADIUS {
            continue;
        }
        let speed = h.avg_speed(player.id);
        let replace = match best {
            None => true,
            Some((best_id, best_speed)) => {
                speed > best_speed || (speed == best_speed && player.id < best_id)
            }
        };
        if replace {
            best = Some((player.id, speed));
        }
    }
    best.map(|(id, _)| id)
}

fn release_heavy(h: &mut Hazards, id: u32, b: &mut MotelingBrain) {
    if h.heavy_holder == Some(id) {
        h.heavy_holder = None;
    }
    b.acquired = false;
}

#[allow(clippy::too_many_arguments)]
fn rift_warden_step(
    h: &mut Hazards,
    self_index: usize,
    pos: &mut Vec3,
    b: &mut RiftWardenBrain,
    speed_mul: f32,
    contact_radius: f32,
    home: &Vec3,
    ctx: &Ctx,
    fx: &mut Vec<HazardEffect>,
) {
    match b.phase {
        RiftPhase::Haunt => {
            let target = nearest_index_3d(ctx.players, pos);
            match target.and_then(|i| ctx.players.get(i)) {
                None => move_toward(pos, home, RIFT_HAUNT_SPEED * speed_mul * DT),
                Some(target) => {
                    let centre = player_centre(&target.feet);
                    move_toward(pos, &centre, RIFT_HAUNT_SPEED * speed_mul * DT);
                    if sphere_hits_player_box(pos, contact_radius, &target.feet) {
                        let dir = flat_direction(pos, &centre);
                        push_shove(
                            fx,
                            target,
                            Vec3::new(dir.x * RIFT_CONTACT_SHOVE, 0.0, dir.z * RIFT_CONTACT_SHOVE),
                            0,
                        );
                    }
                    if distance(pos, &centre) <= RIFT_TRIGGER_RADIUS {
                        b.phase = RiftPhase::Ignite;
                        b.timer = RIFT_IGNITE_TICKS;
                    }
                }
            }
        }
        RiftPhase::Ignite => {
            b.timer = b.timer.saturating_sub(1);
            if b.timer == 0 {
                b.phase = RiftPhase::Shove;
                b.timer = RIFT_SHOVE_TICKS;
                // Radial displacement: players are shoved for real, enemies are knocked back.
                for player in ctx.players {
                    let centre = player_centre(&player.feet);
                    let d = distance(pos, &centre);
                    if d > RIFT_SHOVE_RADIUS {
                        continue;
                    }
                    let dir = flat_direction(pos, &centre);
                    push_shove(
                        fx,
                        player,
                        Vec3::new(dir.x * RIFT_SHOVE_SPEED, RIFT_SHOVE_UP, dir.z * RIFT_SHOVE_SPEED),
                        RIFT_STAGGER_TICKS,
                    );
                }
                for (index, vel) in enemies_from_ctx_players(ctx, pos, self_index, RIFT_SHOVE_RADIUS) {
                    h.pending_enemy_shoves.push((index, vel));
                }
            }
        }
        RiftPhase::Shove => {
            b.timer = b.timer.saturating_sub(1);
            if b.timer == 0 {
                b.phase = RiftPhase::Settle;
                b.timer = RIFT_SETTLE_TICKS;
            }
        }
        RiftPhase::Settle => {
            move_toward(pos, home, RIFT_CREEP_SPEED * speed_mul * DT);
            b.timer = b.timer.saturating_sub(1);
            if b.timer == 0 {
                b.phase = RiftPhase::Haunt;
            }
        }
    }
}

/// Collect `(index, knockback velocity)` pairs for every enemy other than
/// `self_index` within `radius` of `centre`. Used by the rift warden's pulse; the
/// velocities are queued in [`Hazards::take_pending_enemy_shoves`] and applied after
/// the enemy loop, because stepping an enemy cannot hold a second live borrow of the
/// slice. Y carries the same upward kick as the player shove (`+2 u/s`).
fn enemies_from_ctx_players(ctx: &Ctx, centre: &Vec3, self_index: usize, radius: f32) -> Vec<(usize, Vec3)> {
    let mut out = Vec::new();
    for (index, pos) in ctx.positions.iter().enumerate() {
        if index == self_index {
            continue;
        }
        if distance(centre, pos) > radius {
            continue;
        }
        let dir = flat_direction(centre, pos);
        out.push((
            index,
            Vec3::new(dir.x * RIFT_SHOVE_SPEED, RIFT_SHOVE_UP, dir.z * RIFT_SHOVE_SPEED),
        ));
    }
    out
}

fn gale_wight_step(
    h: &mut Hazards,
    id: u32,
    pos: &mut Vec3,
    b: &mut GaleWightBrain,
    speed_mul: f32,
    home: &Vec3,
    ctx: &Ctx,
) {
    match b.phase {
        GalePhase::Cruise => {
            b.cruise_elapsed = b.cruise_elapsed.saturating_add(1);
            let y_target = map_top(ctx.level) + GALE_CRUISE_ALTITUDE;
            let goal_ok = match &b.cruise_goal {
                Some(goal) => {
                    horizontal_distance(pos, goal) < 1.5 && (goal.y - y_target).abs() < 0.5
                }
                None => true,
            };
            if goal_ok {
                let dir = h.rng_flat_direction();
                let reach = 6.0 + (h.next_rng() % 8000) as f32 / 1000.0;
                b.cruise_goal = Some(Vec3::new(pos.x + dir.x * reach, y_target, pos.z + dir.z * reach));
            }
            if let Some(goal) = b.cruise_goal.clone() {
                move_toward(pos, &goal, GALE_CRUISE_SPEED * speed_mul * DT);
            }
            if b.cruise_elapsed >= GALE_CRUISE_MIN_TICKS {
                let count = match ctx.players.len() {
                    0 => 0,
                    1 => 2,
                    n => (n as u32).min(GALE_MAX_SHELLS),
                };
                if count == 0 {
                    b.cruise_elapsed = 0;
                    return;
                }
                b.shells_left = count;
                b.phase = GalePhase::Mark;
                b.timer = GALE_MARK_TICKS;
                b.lag.clear();
                b.target_id = h.random_target(ctx.players);
                b.acquired = false;
                b.heavy_wait = 0;
            }
        }
        GalePhase::Mark => {
            if !b.acquired {
                if h.heavy_holder.is_none() {
                    b.acquired = true;
                    h.heavy_holder = Some(id);
                } else {
                    // Heavy slot taken: wait frozen at the first frame for up to 2.0 s.
                    b.heavy_wait += 1;
                    if b.heavy_wait > HEAVY_WAIT_MAX_TICKS {
                        b.phase = GalePhase::Downtime;
                        b.timer = GALE_DOWNTIME_TICKS;
                        b.shells_left = 0;
                        b.cruise_elapsed = 0;
                        b.heavy_wait = 0;
                    }
                    return;
                }
            }
            if let Some(target) = b.target_id.and_then(|tid| ctx.players.iter().find(|p| p.id == tid)) {
                b.lag.push(target.feet.clone());
                if b.lag.len() > GALE_LAG_TICKS + 1 {
                    b.lag.remove(0);
                }
                let index = b.lag.len().saturating_sub(1 + GALE_LAG_TICKS);
                b.anchor = b.lag.get(index).cloned().unwrap_or_else(|| target.feet.clone());
            }
            b.timer = b.timer.saturating_sub(1);
            if b.timer == 0 {
                b.phase = GalePhase::Lock;
                b.timer = GALE_LOCK_TICKS;
            }
        }
        GalePhase::Lock => {
            b.timer = b.timer.saturating_sub(1);
            if b.timer == 0 {
                let spawn = Vec3::new(b.anchor.x, b.anchor.y + GALE_SHELL_SPAWN_HEIGHT, b.anchor.z);
                let vel = Vec3::new(0.0, -GALE_FALL_SPEED, 0.0);
                h.spawn_projectile(spawn, vel, ProjectileKind::Shell, b.anchor.y);
                b.phase = GalePhase::Fall;
                b.timer = GALE_FALL_TICKS;
                b.shells_left = b.shells_left.saturating_sub(1);
            }
        }
        GalePhase::Fall => {
            b.timer = b.timer.saturating_sub(1);
            if b.timer == 0 {
                if b.shells_left > 0 {
                    b.phase = GalePhase::Mark;
                    b.timer = GALE_MARK_TICKS;
                    b.lag.clear();
                    b.target_id = h.random_target(ctx.players);
                } else {
                    if h.heavy_holder == Some(id) {
                        h.heavy_holder = None; // volley over: heavy slot released
                    }
                    b.acquired = false;
                    b.heavy_wait = 0;
                    b.phase = GalePhase::Downtime;
                    b.timer = GALE_DOWNTIME_TICKS;
                }
            }
        }
        GalePhase::Downtime => {
            move_toward(pos, home, RIFT_CREEP_SPEED * speed_mul * DT);
            b.timer = b.timer.saturating_sub(1);
            if b.timer == 0 {
                b.phase = GalePhase::Cruise;
                b.cruise_elapsed = 0;
                b.acquired = false;
                b.heavy_wait = 0;
            }
        }
    }
}

fn map_top(level: &LevelData) -> f32 {
    level.platforms.iter().map(|p| p.bounds.max.y).fold(0.0, f32::max)
}

#[allow(clippy::too_many_arguments)]
fn lantern_leech_step(
    pos: &mut Vec3,
    b: &mut LanternLeechBrain,
    speed_mul: f32,
    home: &Vec3,
    ctx: &Ctx,
    fx: &mut Vec<HazardEffect>,
) {
    match b.phase {
        LeechPhase::Prowl => {
            let target = nearest_index_3d(ctx.players, pos);
            match target.and_then(|i| ctx.players.get(i)) {
                None => move_toward(pos, home, LEECH_PROWL_SPEED * speed_mul * DT),
                Some(target) => {
                    let centre = player_centre(&target.feet);
                    let d = distance(pos, &centre);
                    if d > LEECH_PROWL_RADIUS {
                        move_toward(pos, &centre, LEECH_PROWL_SPEED * speed_mul * DT);
                    } else {
                        // Drift tangentially around the player.
                        let r = flat_direction(&centre, pos);
                        pos.x += -r.z * LEECH_PROWL_SPEED * speed_mul * DT;
                        pos.z += r.x * LEECH_PROWL_SPEED * speed_mul * DT;
                    }
                }
            }
            apply_pickup_lock(pos, b, ctx.pickups);
        }
        LeechPhase::Siphon => {
            b.timer = b.timer.saturating_add(1);
            let ramp = (b.timer as f32 / LEECH_RAMP_TICKS as f32).clamp(0.0, 1.0);
            let speed = LEECH_PROWL_SPEED + (LEECH_TOP_SPEED - LEECH_PROWL_SPEED) * ramp;
            let d = distance(pos, &b.goal);
            if d <= LEECH_ARRIVE_DIST {
                b.phase = LeechPhase::Gorge;
                b.timer = LEECH_GORGE_TICKS;
                b.away = flat_direction(&b.goal, pos);
                return;
            }
            let dir = dir_normalized(pos, &b.goal);
            pos.x += dir.x * speed * speed_mul * DT;
            pos.y += dir.y * speed * speed_mul * DT;
            pos.z += dir.z * speed * speed_mul * DT;
            // The newest pickup re-targets it; no stacking, no queue.
            apply_pickup_lock(pos, b, ctx.pickups);
        }
        LeechPhase::Gorge => {
            // Ticks elapsed since the gorge began, before this tick's decrement,
            // so the rear-back telegraph spans elapsed 0..12 and the first sting
            // lands exactly 0.6 s (12 ticks) into the gorge.
            let elapsed = LEECH_GORGE_TICKS - b.timer;
            if elapsed == LEECH_REARBACK_TICKS
                || elapsed == LEECH_REARBACK_TICKS + LEECH_STING_EVERY_TICKS
            {
                for player in ctx.players {
                    let centre = player_centre(&player.feet);
                    if distance(pos, &centre) <= LEECH_STING_RADIUS {
                        push_kill(fx, player, DEATH_ENEMY);
                    }
                }
            }
            b.timer = b.timer.saturating_sub(1);
            if b.timer == 0 {
                b.phase = LeechPhase::Slide;
                b.timer = LEECH_SLIDE_TICKS;
            }
        }
        LeechPhase::Slide => {
            pos.x += b.away.x * LEECH_PROWL_SPEED * speed_mul * DT;
            pos.z += b.away.z * LEECH_PROWL_SPEED * speed_mul * DT;
            b.timer = b.timer.saturating_sub(1);
            if b.timer == 0 {
                b.phase = LeechPhase::Prowl;
            }
        }
    }
}

/// Lock onto the newest pickup within 30 u (used in Prowl and Siphon).
fn apply_pickup_lock(pos: &Vec3, b: &mut LanternLeechBrain, pickups: &[Vec3]) {
    if let Some(newest) = pickups.last() {
        if distance(pos, newest) <= LEECH_SIPHON_RANGE {
            b.phase = LeechPhase::Siphon;
            b.timer = 0;
            b.goal = newest.clone();
        }
    }
}

fn turret_step(
    h: &mut Hazards,
    pos: &mut Vec3,
    b: &mut TurretBrain,
    speed_mul: f32,
    ctx: &Ctx,
) {
    let _ = speed_mul;
    match b.phase {
        TurretPhase::Seek => {
            let mut best: Option<(usize, f32)> = None;
            for (i, player) in ctx.players.iter().enumerate() {
                let centre = player_centre(&player.feet);
                if !has_los(ctx.world, pos, &centre) {
                    continue;
                }
                let d = distance(pos, &centre);
                match best {
                    Some((_, bd)) if bd <= d => {}
                    _ => best = Some((i, d)),
                }
            }
            if let Some((i, _)) = best {
                if let Some(target) = ctx.players.get(i) {
                    let centre = player_centre(&target.feet);
                    b.aim = predict_intercept(pos, &centre, &target.vel, TURRET_BOLT_SPEED);
                    b.target_id = Some(target.id);
                    b.phase = TurretPhase::Charge;
                    b.timer = TURRET_CHARGE_TICKS;
                }
            }
        }
        TurretPhase::Charge => {
            // If line of sight breaks mid-charge, the turret fires at the last
            // predicted point anyway.
            if let Some(target) = b.target_id.and_then(|tid| ctx.players.iter().find(|p| p.id == tid)) {
                let centre = player_centre(&target.feet);
                if has_los(ctx.world, pos, &centre) {
                    b.aim = predict_intercept(pos, &centre, &target.vel, TURRET_BOLT_SPEED);
                }
            }
            b.timer = b.timer.saturating_sub(1);
            if b.timer == 0 {
                let dx = b.aim.x - pos.x;
                let dz = b.aim.z - pos.z;
                let d = (dx * dx + dz * dz).sqrt();
                let dir = if d > 1e-6 {
                    Vec3::new(dx / d, 0.0, dz / d)
                } else {
                    Vec3::new(1.0, 0.0, 0.0)
                };
                // Flat bolt at muzzle height.
                h.spawn_projectile(pos.clone(), Vec3::new(dir.x * TURRET_BOLT_SPEED, 0.0, dir.z * TURRET_BOLT_SPEED), ProjectileKind::Bolt, 0.0);
                b.phase = TurretPhase::Fire;
                b.timer = TURRET_FIRE_TICKS;
            }
        }
        TurretPhase::Fire => {
            b.timer = b.timer.saturating_sub(1);
            if b.timer == 0 {
                b.phase = TurretPhase::Vent;
                b.timer = TURRET_VENT_TICKS;
            }
        }
        TurretPhase::Vent => {
            b.timer = b.timer.saturating_sub(1);
            if b.timer == 0 {
                b.phase = TurretPhase::Seek;
            }
        }
    }
}

fn mimic_step(
    pos: &mut Vec3,
    b: &mut MimicBrain,
    speed_mul: f32,
    ctx: &Ctx,
    fx: &mut Vec<HazardEffect>,
) {
    let _ = speed_mul;
    match b.phase {
        MimicPhase::Cached => {
            b.shimmer = (b.shimmer + 1) % MIMIC_SHIMMER_PERIOD;
            if let Some(i) = nearest_index_within(ctx.players, pos, MIMIC_TRIGGER_RADIUS, false) {
                if let Some(target) = ctx.players.get(i) {
                    b.phase = MimicPhase::Hiss;
                    b.timer = MIMIC_HISS_TICKS;
                    b.trigger = Some(target.id);
                    b.anchor = player_centre(&target.feet);
                }
            }
        }
        MimicPhase::Hiss => {
            if let Some(target) = b.trigger.and_then(|tid| ctx.players.iter().find(|p| p.id == tid)) {
                b.anchor = player_centre(&target.feet);
            }
            b.timer = b.timer.saturating_sub(1);
            if b.timer == 0 {
                b.phase = MimicPhase::Lunge;
                b.timer = MIMIC_LUNGE_TICKS;
                b.lunge_dir = flat_direction(pos, &b.anchor);
            }
        }
        MimicPhase::Lunge => {
            // Ground-hugging hop; y never changes.
            pos.x += b.lunge_dir.x * MIMIC_LUNGE_SPEED * DT;
            pos.z += b.lunge_dir.z * MIMIC_LUNGE_SPEED * DT;
            for player in ctx.players {
                if sphere_hits_player_box(pos, MIMIC_LUNGE_RADIUS, &player.feet) {
                    push_kill(fx, player, DEATH_ENEMY);
                }
            }
            b.timer = b.timer.saturating_sub(1);
            if b.timer == 0 {
                b.phase = MimicPhase::Spent;
                b.timer = MIMIC_SPENT_TICKS;
                b.shimmer = 0;
            }
        }
        MimicPhase::Spent => {
            b.timer = b.timer.saturating_sub(1);
            if b.timer == 0 {
                b.phase = MimicPhase::Cached;
            }
        }
    }
}

fn corrupter_step(
    h: &mut Hazards,
    pos: &mut Vec3,
    b: &mut CorrupterBrain,
    speed_mul: f32,
    home: &Vec3,
    ctx: &Ctx,
) {
    match b.phase {
        CorrupterPhase::Drift => {
            let target = nearest_index_3d(ctx.players, pos);
            let mut best_platform: Option<(usize, f32)> = None;
            if let Some(i) = target.and_then(|i| ctx.players.get(i)) {
                for (pi, platform) in ctx.level.platforms.iter().enumerate() {
                    let d = aabb_distance(&i.feet, &platform.bounds);
                    if d > CORRUPTER_TARGET_RANGE {
                        continue;
                    }
                    match best_platform {
                        Some((_, bd)) if bd <= d => {}
                        _ => best_platform = Some((pi, d)),
                    }
                }
            }
            match best_platform.and_then(|(pi, _)| ctx.level.platforms.get(pi).map(|p| (pi, p))) {
                Some((_, platform)) => {
                    let cx = (platform.bounds.min.x + platform.bounds.max.x) * 0.5;
                    let cz = (platform.bounds.min.z + platform.bounds.max.z) * 0.5;
                    let top = Vec3::new(cx, platform.bounds.max.y, cz);
                    let id = platform.id;
                    if b.mark != Some(id) {
                        b.mark = Some(id);
                        b.fret_ticks = 0;
                    }
                    move_toward(pos, &top, CORRUPTER_DRIFT_SPEED * speed_mul * DT);
                    if aabb_distance(pos, &platform.bounds) <= CORRUPTER_AURA_RADIUS {
                        b.fret_ticks += 1;
                        if b.fret_ticks > CORRUPTER_FRET_TICKS {
                            h.mark_fretted(id, CORRUPTER_FRET_DURATION);
                            b.away = flat_direction(&top, pos);
                            b.phase = CorrupterPhase::ReForm;
                            b.timer = CORRUPTER_REFORM_TICKS;
                            b.mark = None;
                            b.fret_ticks = 0;
                        }
                    } else {
                        b.fret_ticks = 0;
                    }
                }
                None => {
                    b.mark = None;
                    b.fret_ticks = 0;
                    match target.and_then(|i| ctx.players.get(i)) {
                        Some(player) => {
                            let centre = player_centre(&player.feet);
                            move_toward(pos, &centre, CORRUPTER_DRIFT_SPEED * speed_mul * DT);
                        }
                        None => move_toward(pos, home, CORRUPTER_DRIFT_SPEED * speed_mul * DT),
                    }
                }
            }
        }
        CorrupterPhase::ReForm => {
            pos.x += b.away.x * CORRUPTER_DRIFT_SPEED * speed_mul * DT;
            pos.z += b.away.z * CORRUPTER_DRIFT_SPEED * speed_mul * DT;
            b.timer = b.timer.saturating_sub(1);
            if b.timer == 0 {
                b.phase = CorrupterPhase::Drift;
            }
        }
    }
}

/// Interrupt an enemy into its post-attack cooldown state (Bulwark pulse).
fn cooldown_state(enemy: &mut Enemy) {
    match &mut enemy.brain {
        Brain::Pallbearer(b) => {
            b.phase = PallbearerPhase::Recover;
            b.timer = PALLBEARER_RECOVER_TICKS;
        }
        Brain::Snapshrike(b) => {
            b.phase = SnapshrikePhase::Recover;
            b.timer = SNAPSHRIKE_RECOVER_TICKS;
        }
        Brain::Moteling(b) => {
            b.phase = MotelingPhase::Cooldown;
            b.timer = MOTELING_COOLDOWN_TICKS;
            b.marked = None;
            b.orbit_ticks = 0;
            b.acquired = false;
            b.heavy_wait = 0;
        }
        Brain::RiftWarden(b) => {
            b.phase = RiftPhase::Settle;
            b.timer = RIFT_SETTLE_TICKS;
        }
        Brain::GaleWight(b) => {
            b.phase = GalePhase::Downtime;
            b.timer = GALE_DOWNTIME_TICKS;
            b.shells_left = 0;
            b.target_id = None;
            b.lag.clear();
            b.cruise_elapsed = 0;
            b.acquired = false;
            b.heavy_wait = 0;
        }
        Brain::LanternLeech(b) => {
            b.phase = LeechPhase::Slide;
            b.timer = LEECH_SLIDE_TICKS;
        }
        Brain::Turret(b) => {
            b.phase = TurretPhase::Vent;
            b.timer = TURRET_VENT_TICKS;
        }
        Brain::Mimic(b) => {
            b.phase = MimicPhase::Spent;
            b.timer = MIMIC_SPENT_TICKS;
        }
        Brain::Corrupter(b) => {
            b.phase = CorrupterPhase::ReForm;
            b.timer = CORRUPTER_REFORM_TICKS;
            b.mark = None;
            b.fret_ticks = 0;
        }
    }
}

fn telegraph_prog(elapsed: u32, total: u32) -> u8 {
    if total == 0 {
        return 255;
    }
    ((elapsed * 255) / total).min(255) as u8
}

fn view_of(enemy: &Enemy) -> protocol::EnemyView {
    const ZERO: fn() -> Vec3 = || Vec3::new(0.0, 0.0, 0.0);
    if enemy.active_in > 0 {
        return protocol::EnemyView {
            id: enemy.id,
            kind: enemy.kind,
            pos: enemy.pos.clone(),
            radius: enemy.params.size,
            state: ENEMY_DORMANT,
            target: ZERO(),
            prog: 0,
            flags: 0,
        };
    }
    let (state, target, prog, flags) = match &enemy.brain {
        Brain::Pallbearer(b) => match b.phase {
            PallbearerPhase::Hunt => (ENEMY_HUNTING, ZERO(), 0, 0),
            PallbearerPhase::Kneel => {
                let elapsed = PALLBEARER_KNEEL_TICKS - b.timer;
                (ENEMY_TELEGRAPH, b.locked.clone(), telegraph_prog(elapsed, PALLBEARER_KNEEL_TICKS), 0)
            }
            PallbearerPhase::Lunge => (ENEMY_ATTACK, b.locked.clone(), 255, 0),
            PallbearerPhase::Recover => (ENEMY_RECOVER, ZERO(), 0, 0),
        },
        Brain::Snapshrike(b) => match b.phase {
            SnapshrikePhase::Stalk => (ENEMY_HUNTING, ZERO(), 0, 0),
            SnapshrikePhase::Lock => {
                let elapsed = SNAPSHRIKE_LOCK_TICKS - b.timer;
                (ENEMY_TELEGRAPH, b.aim.clone(), telegraph_prog(elapsed, SNAPSHRIKE_LOCK_TICKS), 0)
            }
            SnapshrikePhase::Dash => (ENEMY_ATTACK, b.aim.clone(), 255, 0),
            SnapshrikePhase::Stunned => (ENEMY_RECOVER, ZERO(), 0, 0),
            SnapshrikePhase::Recover => (ENEMY_RECOVER, ZERO(), 0, 0),
        },
        Brain::Moteling(b) => match b.phase {
            MotelingPhase::Wander | MotelingPhase::Orbit => (ENEMY_HUNTING, ZERO(), 0, 0),
            MotelingPhase::Inhale => {
                // Frozen at the first frame while waiting for the heavy slot: prog 0, no flag.
                let prog = if b.acquired {
                    telegraph_prog(MOTELING_INHALE_TICKS - b.timer, MOTELING_INHALE_TICKS)
                } else {
                    0
                };
                let flags = if b.acquired { ENEMY_FLAG_HEAVY } else { 0 };
                (ENEMY_TELEGRAPH, b.anchor.clone(), prog, flags)
            }
            MotelingPhase::Cooldown => (ENEMY_RECOVER, ZERO(), 0, 0),
        },
        Brain::RiftWarden(b) => match b.phase {
            RiftPhase::Haunt => (ENEMY_HUNTING, ZERO(), 0, 0),
            RiftPhase::Ignite => {
                let elapsed = RIFT_IGNITE_TICKS - b.timer;
                (ENEMY_TELEGRAPH, enemy.pos.clone(), telegraph_prog(elapsed, RIFT_IGNITE_TICKS), 0)
            }
            RiftPhase::Shove => (ENEMY_ATTACK, enemy.pos.clone(), 255, 0),
            RiftPhase::Settle => (ENEMY_RECOVER, ZERO(), 0, 0),
        },
        Brain::GaleWight(b) => match b.phase {
            GalePhase::Cruise => (ENEMY_HUNTING, ZERO(), 0, 0),
            GalePhase::Mark => {
                let prog = if b.acquired {
                    telegraph_prog(GALE_MARK_TICKS - b.timer, GALE_MARK_TICKS + GALE_LOCK_TICKS)
                } else {
                    0
                };
                let flags = if b.acquired { ENEMY_FLAG_HEAVY } else { 0 };
                (ENEMY_TELEGRAPH, b.anchor.clone(), prog, flags)
            }
            GalePhase::Lock => {
                let elapsed = GALE_MARK_TICKS + (GALE_LOCK_TICKS - b.timer);
                (
                    ENEMY_TELEGRAPH,
                    b.anchor.clone(),
                    telegraph_prog(elapsed, GALE_MARK_TICKS + GALE_LOCK_TICKS),
                    ENEMY_FLAG_HEAVY,
                )
            }
            GalePhase::Fall => (ENEMY_ATTACK, b.anchor.clone(), 255, 0),
            GalePhase::Downtime => (ENEMY_RECOVER, ZERO(), 0, 0),
        },
        Brain::LanternLeech(b) => match b.phase {
            LeechPhase::Prowl => (ENEMY_HUNTING, ZERO(), 0, 0),
            LeechPhase::Siphon => {
                // Before the flare: roaming. Inside the 0.8 s flare window: telegraph with ramping prog.
                let speed = LEECH_TOP_SPEED.max(LEECH_PROWL_SPEED);
                let d = distance(&enemy.pos, &b.goal);
                let eta_ticks = (d / speed) * TICK_HZ as f32;
                if eta_ticks <= LEECH_FLARE_TICKS as f32 {
                    let prog = ((LEECH_FLARE_TICKS as f32 - eta_ticks) * 255.0 / LEECH_FLARE_TICKS as f32)
                        .clamp(0.0, 255.0) as u8;
                    (ENEMY_TELEGRAPH, b.goal.clone(), prog, 0)
                } else {
                    (ENEMY_HUNTING, ZERO(), 0, 0)
                }
            }
            LeechPhase::Gorge => {
                let elapsed = LEECH_GORGE_TICKS - b.timer;
                if elapsed < LEECH_REARBACK_TICKS {
                    (ENEMY_TELEGRAPH, b.goal.clone(), telegraph_prog(elapsed, LEECH_REARBACK_TICKS), 0)
                } else {
                    (ENEMY_ATTACK, b.goal.clone(), 255, 0)
                }
            }
            LeechPhase::Slide => (ENEMY_RECOVER, ZERO(), 0, 0),
        },
        Brain::Turret(b) => match b.phase {
            TurretPhase::Seek => (ENEMY_HUNTING, ZERO(), 0, 0),
            TurretPhase::Charge => {
                let elapsed = TURRET_CHARGE_TICKS - b.timer;
                (ENEMY_TELEGRAPH, b.aim.clone(), telegraph_prog(elapsed, TURRET_CHARGE_TICKS), 0)
            }
            TurretPhase::Fire => (ENEMY_ATTACK, b.aim.clone(), 255, 0),
            TurretPhase::Vent => (ENEMY_RECOVER, ZERO(), 0, 0),
        },
        Brain::Mimic(b) => match b.phase {
            MimicPhase::Cached => {
                let prog = if b.shimmer >= MIMIC_SHIMMER_PERIOD - MIMIC_SHIMMER_TICKS {
                    telegraph_prog(b.shimmer - (MIMIC_SHIMMER_PERIOD - MIMIC_SHIMMER_TICKS), MIMIC_SHIMMER_TICKS)
                } else {
                    0
                };
                (ENEMY_DISGUISED, ZERO(), prog, 0)
            }
            MimicPhase::Hiss => {
                let elapsed = MIMIC_HISS_TICKS - b.timer;
                (ENEMY_TELEGRAPH, b.anchor.clone(), telegraph_prog(elapsed, MIMIC_HISS_TICKS), 0)
            }
            MimicPhase::Lunge => (ENEMY_ATTACK, b.anchor.clone(), 255, 0),
            MimicPhase::Spent => (ENEMY_RECOVER, ZERO(), 0, 0),
        },
        Brain::Corrupter(b) => match b.phase {
            CorrupterPhase::Drift => (ENEMY_HUNTING, ZERO(), 0, 0),
            CorrupterPhase::ReForm => (ENEMY_RECOVER, ZERO(), 0, 0),
        },
    };
    protocol::EnemyView {
        id: enemy.id,
        kind: enemy.kind,
        pos: enemy.pos.clone(),
        radius: enemy.params.size,
        state,
        target,
        prog,
        flags,
    }
}
