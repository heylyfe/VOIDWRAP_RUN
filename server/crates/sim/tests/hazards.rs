//! Integration tests for `sim::hazards`: the nine reworked enemies, the heavy-attack
//! mutex, ward immunity rules, crumble/fretted tiles and determinism.
//!
//! Everything here runs against the public crate API; small levels are built inline.
//! Timing expectations come from the design (20 Hz server): 0.6 s = 12 ticks,
//! 0.75 s = 15 ticks, 0.9 s = 18 ticks, 1.1 s = 22 ticks, 2.0 s = 40 ticks,
//! 2.5 s = 50 ticks, 4.0 s = 80 ticks.

use protocol::{
    Aabb, DEATH_ENEMY, DEATH_PROJECTILE, ENEMY_ATTACK, ENEMY_DISGUISED, ENEMY_DORMANT,
    ENEMY_FLAG_HEAVY, ENEMY_HUNTING, ENEMY_RECOVER, ENEMY_TELEGRAPH, LevelData, Platform,
    PLATFORM_CRUMBLE, TILE_FRETTED, TILE_GONE, TILE_WARN, Vec3,
};
use sim::hazards::{HazardEffect, Hazards, PlayerSense};
use sim::rules::{
    spawn_enemy_with, Brain, Enemy, EnemyParams, LanternLeechBrain, LeechPhase, SnapshrikePhase,
    BEHAVIOR_CORRUPTER, BEHAVIOR_GALE_WIGHT, BEHAVIOR_LANTERN_LEECH, BEHAVIOR_MIMIC,
    BEHAVIOR_MOTELING, BEHAVIOR_PALLBEARER, BEHAVIOR_RIFT_WARDEN, BEHAVIOR_SNAPSHRIKE,
    BEHAVIOR_TURRET,
};
use sim::world::World;

fn v3(x: f32, y: f32, z: f32) -> Vec3 {
    Vec3::new(x, y, z)
}

fn assert_vec3_near(a: &Vec3, b: &Vec3) {
    assert!((a.x - b.x).abs() < 1e-4, "x mismatch: {} vs {}", a.x, b.x);
    assert!((a.y - b.y).abs() < 1e-4, "y mismatch: {} vs {}", a.y, b.y);
    assert!((a.z - b.z).abs() < 1e-4, "z mismatch: {} vs {}", a.z, b.z);
}

fn platform(id: u16, kind: u8, min: [f32; 3], max: [f32; 3]) -> Platform {
    Platform {
        id,
        kind,
        bounds: Aabb {
            min: v3(min[0], min[1], min[2]),
            max: v3(max[0], max[1], max[2]),
        },
    }
}

fn test_level(platforms: Vec<Platform>, seed: u32) -> LevelData {
    LevelData {
        seed,
        level: 1,
        kill_y: -20.0,
        platforms,
        spawn: Vec3::ZERO,
        beacon: Vec3::new(0.0, -100.0, 0.0),
        gifts: Vec::new(),
        event: protocol::EVENT_NONE,
        altars: Vec::new(),
    }
}

fn empty_level(seed: u32) -> LevelData {
    test_level(Vec::new(), seed)
}

fn params(behavior: u8) -> EnemyParams {
    EnemyParams {
        behavior,
        speed_mul: 1.0,
        size: 0.5,
        grace_ticks: 0,
    }
}

fn enemy(id: u32, behavior: u8, x: f32, y: f32, z: f32) -> Enemy {
    spawn_enemy_with(id, 0, v3(x, y, z), params(behavior))
}

fn sense(id: u32, x: f32, y: f32, z: f32) -> PlayerSense {
    PlayerSense {
        id,
        feet: v3(x, y, z),
        vel: v3(0.0, 0.0, 0.0),
        warded: false,
    }
}

fn sense_vel(id: u32, feet: Vec3, vel: Vec3, warded: bool) -> PlayerSense {
    PlayerSense { id, feet, vel, warded }
}

fn tick(
    hz: &mut Hazards,
    enemies: &mut [Enemy],
    level: &LevelData,
    world: &World,
    players: &[PlayerSense],
    pickups: &[Vec3],
) -> Vec<HazardEffect> {
    hz.step(enemies, world, level, players, pickups, 1.0)
}

/// Burn the level-wide 4.0 s no-harm window with an empty roster.
fn warm(hz: &mut Hazards, level: &LevelData, world: &World) {
    for _ in 0..80 {
        let mut none = Vec::new();
        hz.step(&mut none, world, level, &[], &[], 1.0);
    }
}

fn kills(fx: &[HazardEffect]) -> Vec<u32> {
    fx.iter()
        .filter_map(|e| match e {
            HazardEffect::Kill { player, .. } => Some(*player),
            _ => None,
        })
        .collect()
}

fn kill_causes(fx: &[HazardEffect]) -> Vec<u8> {
    fx.iter()
        .filter_map(|e| match e {
            HazardEffect::Kill { cause, .. } => Some(*cause),
            _ => None,
        })
        .collect()
}

fn shove_count(fx: &[HazardEffect]) -> usize {
    fx.iter()
        .filter(|e| matches!(e, HazardEffect::Shove { .. }))
        .count()
}

fn view_of(hz: &Hazards, enemies: &[Enemy], id: u32) -> protocol::EnemyView {
    hz.views(enemies)
        .into_iter()
        .find(|v| v.id == id)
        .expect("view exists")
}

fn has_heavy(v: &protocol::EnemyView) -> bool {
    v.flags & ENEMY_FLAG_HEAVY != 0
}

#[test]
fn level_grace_suppresses_all_effects() {
    let level = empty_level(11);
    let world = World::from_level(&level);
    let mut hz = Hazards::new();
    let mut enemies = vec![enemy(1, BEHAVIOR_PALLBEARER, 0.5, 0.0, 0.0)];
    let players = vec![sense(1, 0.0, 0.0, 0.0)];
    for _ in 0..79 {
        let fx = tick(&mut hz, &mut enemies, &level, &world, &players, &[]);
        assert!(fx.is_empty(), "the first 4.0 s of a level never hurt");
    }
}

#[test]
fn dormant_enemy_never_acts() {
    let level = empty_level(12);
    let world = World::from_level(&level);
    let mut hz = Hazards::new();
    warm(&mut hz, &level, &world);
    let p = EnemyParams {
        grace_ticks: 5,
        ..params(BEHAVIOR_SNAPSHRIKE)
    };
    let mut enemies = vec![spawn_enemy_with(1, 0, v3(1.0, 0.0, 0.0), p)];
    let players = vec![sense(1, 0.0, 0.0, 0.0)];
    // A freshly spawned enemy reports ENEMY_DORMANT (active_in > 0).
    assert_eq!(view_of(&hz, &enemies, 1).state, ENEMY_DORMANT);
    let start = enemies[0].pos.clone();
    for _ in 0..5 {
        let fx = tick(&mut hz, &mut enemies, &level, &world, &players, &[]);
        assert!(fx.is_empty());
        assert_eq!(enemies[0].pos, start, "dormant enemy stays put");
    }
    tick(&mut hz, &mut enemies, &level, &world, &players, &[]);
    assert_ne!(view_of(&hz, &enemies, 1).state, ENEMY_DORMANT);
}

#[test]
fn pallbearer_hunt_kneel_lunge_recover_sequence() {
    let level = empty_level(21);
    let world = World::from_level(&level);
    let mut hz = Hazards::new();
    warm(&mut hz, &level, &world);
    let mut enemies = vec![enemy(1, BEHAVIOR_PALLBEARER, 4.0, 0.0, 0.0)];
    let players = vec![sense(1, 0.0, 0.0, 0.0)];
    let mut kneel_at = None;
    let mut kill_at = None;
    let mut saw_attack = false;
    let mut killed_any = false;
    for t in 0..400 {
        let fx = tick(&mut hz, &mut enemies, &level, &world, &players, &[]);
        if !kills(&fx).is_empty() {
            killed_any = true;
        }
        let v = view_of(&hz, &enemies, 1);
        match v.state {
            ENEMY_TELEGRAPH => {
                if kneel_at.is_none() {
                    kneel_at = Some(t);
                }
            }
            ENEMY_ATTACK => {
                saw_attack = true;
                if kill_at.is_none() && kills(&fx).contains(&1) {
                    kill_at = Some(t);
                    break;
                }
            }
            _ => {}
        }
    }
    let kneel = kneel_at.expect("pallbearer rears back before lunging");
    let kill = kill_at.expect("lunge connects with a stationary target");
    assert!(saw_attack);
    assert!(
        kill - kneel >= 12,
        ">= 0.6 s telegraph before the lethal lunge"
    );
    assert!(killed_any);
    // After the lunge it drifts harmlessly, then returns to hunting.
    let mut saw_recover = false;
    let mut saw_hunt = false;
    for _ in 0..120 {
        tick(&mut hz, &mut enemies, &level, &world, &[], &[]);
        let v = view_of(&hz, &enemies, 1);
        if v.state == ENEMY_RECOVER {
            saw_recover = true;
        }
        if saw_recover && v.state == ENEMY_HUNTING {
            saw_hunt = true;
            break;
        }
    }
    assert!(saw_recover && saw_hunt);
}

#[test]
fn pallbearer_targets_nearer_of_two_players() {
    let level = empty_level(22);
    let world = World::from_level(&level);
    let mut hz = Hazards::new();
    warm(&mut hz, &level, &world);
    let mut enemies = vec![enemy(1, BEHAVIOR_PALLBEARER, 0.0, 0.0, 0.0)];
    let players = vec![sense(1, 2.0, 0.0, 0.0), sense(2, 9.0, 0.0, 0.0)];
    // Pallbearers stay dormant for their first 4.5 s (PALLBEARER_WAKE_TICKS = 90);
    // warm() used an empty roster, so burn the wake window with players present.
    for _ in 0..90 {
        tick(&mut hz, &mut enemies, &level, &world, &players, &[]);
    }
    tick(&mut hz, &mut enemies, &level, &world, &players, &[]);
    assert!(
        enemies[0].pos.x > 0.1 && enemies[0].pos.x < 1.0,
        "moves toward the nearer player, got {}",
        enemies[0].pos.x
    );
}

#[test]
fn snapshrike_lock_dash_kills_stationary_player() {
    let level = empty_level(31);
    let world = World::from_level(&level);
    let mut hz = Hazards::new();
    warm(&mut hz, &level, &world);
    let mut enemies = vec![enemy(1, BEHAVIOR_SNAPSHRIKE, 6.0, 0.0, 0.0)];
    let players = vec![sense(1, 0.0, 0.0, 0.0)];
    let mut tele_at = None;
    let mut kill_at = None;
    for t in 0..120 {
        let fx = tick(&mut hz, &mut enemies, &level, &world, &players, &[]);
        let v = view_of(&hz, &enemies, 1);
        if v.state == ENEMY_TELEGRAPH && tele_at.is_none() {
            tele_at = Some(t);
        }
        if kills(&fx).contains(&1) {
            kill_at = Some(t);
            break;
        }
    }
    let tele = tele_at.expect("the shrike locks on before dashing");
    let kill = kill_at.expect("the dash connects");
    assert!(kill - tele >= 12, ">= 0.6 s telegraph before the dash");
    assert!(kill - tele >= 14, "the full 0.75 s lock plays out");
}

#[test]
fn snapshrike_aim_is_frozen_for_final_035s_and_a_perpendicular_step_dodges() {
    let level = empty_level(32);
    let world = World::from_level(&level);
    let mut hz = Hazards::new();
    warm(&mut hz, &level, &world);
    let mut enemies = vec![enemy(1, BEHAVIOR_SNAPSHRIKE, 6.0, 0.0, 0.0)];
    let mut players = vec![sense_vel(1, v3(0.0, 0.0, 0.0), v3(0.0, 0.0, 6.0), false)];
    let mut targets = Vec::new();
    for t in 0..40 {
        if t == 3 {
            // Change velocity during the lock so the tracked aim provably moves.
            players[0].vel = v3(0.0, 0.0, -6.0);
        }
        players[0].feet.z += players[0].vel.z * 0.05;
        let _ = tick(&mut hz, &mut enemies, &level, &world, &players, &[]);
        let v = view_of(&hz, &enemies, 1);
        if v.state == ENEMY_TELEGRAPH {
            targets.push(v.target);
        } else if !targets.is_empty() {
            break;
        }
    }
    let n = targets.len();
    assert!(n >= 13, "lock telegraph ~0.75 s, got {n} samples");
    let last7 = &targets[n - 7..];
    for t in last7 {
        assert_eq!(t, &last7[0], "aim frozen for the final 0.35 s");
    }
    assert_ne!(targets[0], last7[0], "tracked aim moved before freezing");
    // A perpendicular step during the frozen window dodges the dash. Step along
    // the true XZ perpendicular of the frozen aim axis, not a raw +x offset.
    let aim = *targets.last().expect("telegraph was sampled");
    let dx = aim.x - enemies[0].pos.x;
    let dz = aim.z - enemies[0].pos.z;
    let len = (dx * dx + dz * dz).sqrt();
    players[0].feet.x += -dz / len * 2.6;
    players[0].feet.z += dx / len * 2.6;
    let mut killed = false;
    for _ in 0..30 {
        let fx = tick(&mut hz, &mut enemies, &level, &world, &players, &[]);
        if !kills(&fx).is_empty() {
            killed = true;
            break;
        }
    }
    assert!(!killed, "a perpendicular step dodges the frozen aim");
}

#[test]
fn snapshrike_dash_blocked_by_platform_stuns_it() {
    let level = test_level(
        vec![platform(3, 0, [4.0, 0.0, -1.0], [6.0, 2.0, 1.0])],
        33,
    );
    let world = World::from_level(&level);
    let mut hz = Hazards::new();
    warm(&mut hz, &level, &world);
    let mut enemies = vec![enemy(1, BEHAVIOR_SNAPSHRIKE, 0.0, 0.0, 0.0)];
    let players = vec![sense(1, 10.0, 0.0, 0.0)];
    let mut stunned = false;
    let mut killed = false;
    for _ in 0..120 {
        let fx = tick(&mut hz, &mut enemies, &level, &world, &players, &[]);
        if !kills(&fx).is_empty() {
            killed = true;
        }
        if let Brain::Snapshrike(b) = &enemies[0].brain {
            if b.phase == SnapshrikePhase::Stunned {
                stunned = true;
            }
        }
    }
    assert!(stunned, "dashing into a platform stuns the shrike");
    assert!(!killed, "the shrike never reaches a sheltered target");
}

#[test]
fn moteling_stationary_marked_player_survives() {
    let level = empty_level(41);
    let world = World::from_level(&level);
    let mut hz = Hazards::new();
    warm(&mut hz, &level, &world);
    let mut enemies = vec![enemy(1, BEHAVIOR_MOTELING, 5.0, 0.0, 0.0)];
    let players = vec![sense(1, 0.0, 0.0, 0.0)];
    let mut saw_heavy_telegraph = false;
    let mut kill_count = 0usize;
    for _ in 0..400 {
        let fx = tick(&mut hz, &mut enemies, &level, &world, &players, &[]);
        let v = view_of(&hz, &enemies, 1);
        if v.state == ENEMY_TELEGRAPH && has_heavy(&v) {
            saw_heavy_telegraph = true;
        }
        kill_count += kills(&fx).len();
    }
    assert!(saw_heavy_telegraph, "inhale wind-up sets the heavy flag");
    assert_eq!(kill_count, 0, "standing still survives the moteling");
}

#[test]
fn moteling_moving_marked_player_dies_after_full_telegraph() {
    let level = empty_level(42);
    let world = World::from_level(&level);
    let mut hz = Hazards::new();
    warm(&mut hz, &level, &world);
    let mut enemies = vec![enemy(1, BEHAVIOR_MOTELING, 5.0, 0.0, 0.0)];
    let players = vec![sense_vel(1, v3(0.0, 0.0, 0.0), v3(2.2, 0.0, 0.0), false)];
    let mut tele_at = None;
    let mut kill_at = None;
    let mut cause = None;
    for t in 0..400 {
        let fx = tick(&mut hz, &mut enemies, &level, &world, &players, &[]);
        let v = view_of(&hz, &enemies, 1);
        if v.state == ENEMY_TELEGRAPH && has_heavy(&v) && tele_at.is_none() {
            tele_at = Some(t);
        }
        if let Some(c) = kill_causes(&fx).first() {
            if kill_at.is_none() {
                kill_at = Some(t);
                cause = Some(*c);
            }
        }
        if kill_at.is_some() {
            break;
        }
    }
    let tele = tele_at.expect("the collapsing ring telegraphs");
    let kill = kill_at.expect("a moving marked player is shrieked down");
    assert!(
        kill - tele >= 12,
        ">= 0.6 s telegraph before the shriek lands"
    );
    assert_eq!(cause, Some(DEATH_ENEMY));
}

#[test]
fn moteling_marks_the_fastest_player() {
    let level = empty_level(43);
    let world = World::from_level(&level);
    let mut hz = Hazards::new();
    warm(&mut hz, &level, &world);
    let mut enemies = vec![enemy(1, BEHAVIOR_MOTELING, 0.0, 0.0, 0.0)];
    let players = vec![
        sense(1, 3.0, 0.0, 0.0),
        sense_vel(2, v3(-3.0, 0.0, 0.0), v3(4.0, 0.0, 0.0), false),
    ];
    tick(&mut hz, &mut enemies, &level, &world, &players, &[]);
    match &enemies[0].brain {
        Brain::Moteling(b) => assert_eq!(b.marked, Some(2), "marks the fastest in-range player"),
        other => panic!("expected a moteling brain, got {other:?}"),
    }
}

#[test]
fn rift_warden_ignite_then_radial_shove() {
    let level = empty_level(51);
    let world = World::from_level(&level);
    let mut hz = Hazards::new();
    warm(&mut hz, &level, &world);
    let mut enemies = vec![enemy(1, BEHAVIOR_RIFT_WARDEN, 3.0, 0.0, 0.0)];
    let players = vec![sense(1, 0.0, 0.0, 0.0)];
    let mut tele_at = None;
    let mut shove = None;
    let mut killed_any = false;
    for t in 0..200 {
        let fx = tick(&mut hz, &mut enemies, &level, &world, &players, &[]);
        if !kills(&fx).is_empty() {
            killed_any = true;
        }
        let v = view_of(&hz, &enemies, 1);
        if v.state == ENEMY_TELEGRAPH && tele_at.is_none() {
            tele_at = Some(t);
        }
        for e in &fx {
            if let HazardEffect::Shove {
                player,
                vel,
                stagger_ticks,
            } = e
            {
                assert_eq!(*player, 1);
                shove = Some((t, vel.clone(), *stagger_ticks));
            }
        }
        if shove.is_some() {
            break;
        }
    }
    let (t, vel, stagger) = shove.expect("the ignition wave displaces the player");
    let tele = tele_at.expect("1.1 s rune telegraph");
    assert!(t - tele >= 12, "shove follows the >= 0.6 s telegraph");
    assert_eq!(stagger, 18, "0.9 s stagger");
    assert!((vel.y - 2.0).abs() < 1e-6, "shove launches upward");
    assert!((vel.x + 8.0).abs() < 1e-6, "8 u/s radially away");
    assert_eq!(vel.z, 0.0);
    assert!(!killed_any, "the rift warden never kills");
}

#[test]
fn rift_warden_pulse_staggers_nearby_enemies() {
    let level = empty_level(52);
    let world = World::from_level(&level);
    let mut hz = Hazards::new();
    warm(&mut hz, &level, &world);
    // The moteling orbits out to a 5 u ring (design) and would be out of the
    // 3.5 u pulse by the time the warden ignites. Use a stationary turret
    // instead: knockback must be radial (8 u/s + 2 u/s up, 0.9 s stagger).
    let mut enemies = vec![
        enemy(1, BEHAVIOR_RIFT_WARDEN, 2.5, 0.0, 0.0),
        enemy(2, BEHAVIOR_TURRET, 1.0, 0.0, 0.0),
    ];
    let players = vec![sense(1, 0.0, 0.0, 0.0)];
    for _ in 0..200 {
        let fx = tick(&mut hz, &mut enemies, &level, &world, &players, &[]);
        if fx
            .iter()
            .any(|e| matches!(e, HazardEffect::Shove { player: 1, .. }))
        {
            break;
        }
    }
    assert_eq!(
        enemies[1].knock_ticks, 18,
        "nearby enemies are staggered too"
    );
    let away_x = enemies[1].pos.x - enemies[0].pos.x;
    let away_z = enemies[1].pos.z - enemies[0].pos.z;
    let len = (away_x * away_x + away_z * away_z).sqrt();
    assert!(len > 1e-3, "the turret is clear of the warden");
    assert!(
        (enemies[1].knock_vel.x - away_x / len * 8.0).abs() < 1e-4,
        "enemy knockback is radial (away from the warden)"
    );
    assert!((enemies[1].knock_vel.z - away_z / len * 8.0).abs() < 1e-4);
    assert!((enemies[1].knock_vel.y - 2.0).abs() < 1e-6);
}

#[test]
fn rift_warden_ward_blocks_player_shove_but_not_enemy_pulse() {
    let level = empty_level(53);
    let world = World::from_level(&level);
    let mut hz = Hazards::new();
    warm(&mut hz, &level, &world);
    let mut enemies = vec![enemy(1, BEHAVIOR_RIFT_WARDEN, 2.5, 0.0, 0.0)];
    let players = vec![sense_vel(1, v3(0.0, 0.0, 0.0), v3(0.0, 0.0, 0.0), true)];
    for _ in 0..200 {
        let fx = tick(&mut hz, &mut enemies, &level, &world, &players, &[]);
        assert_eq!(
            shove_count(&fx),
            0,
            "warded players are immune to the rift shove"
        );
    }
}

#[test]
fn gale_wight_volley_and_shell_blast_kills_anchor_player() {
    let level = empty_level(61);
    let world = World::from_level(&level);
    let mut hz = Hazards::new();
    warm(&mut hz, &level, &world);
    let mut enemies = vec![enemy(1, BEHAVIOR_GALE_WIGHT, 0.0, 0.0, 0.0)];
    let players = vec![sense(1, 0.0, 0.0, 0.0)];
    let mut tele_at = None;
    let mut saw_attack = false;
    let mut saw_shell = false;
    let mut kill_at = None;
    for t in 0..600 {
        let fx = tick(&mut hz, &mut enemies, &level, &world, &players, &[]);
        let v = view_of(&hz, &enemies, 1);
        match v.state {
            ENEMY_TELEGRAPH if has_heavy(&v) => {
                if tele_at.is_none() {
                    tele_at = Some(t);
                }
            }
            ENEMY_ATTACK => saw_attack = true,
            _ => {}
        }
        if !hz.projectiles().is_empty() {
            saw_shell = true;
        }
        if !kills(&fx).is_empty() {
            kill_at = Some(t);
            break;
        }
    }
    let tele = tele_at.expect("mark + lock heavy telegraph");
    let kill = kill_at.expect("the shell blast reaches the anchor");
    assert!(saw_attack, "shell fall is reported as attack");
    assert!(saw_shell, "the shell is a projectile in flight");
    assert!(
        kill - tele >= 30,
        "the full 1.5 s mark+lock telegraph plays first ({} < 30)",
        kill - tele
    );
}

#[test]
fn gale_wight_shell_is_stopped_by_a_platform_top() {
    let level = test_level(
        vec![
            // Floor the player stands on.
            platform(1, 0, [-20.0, -1.0, -20.0], [20.0, 0.0, 20.0]),
            // Shelter directly above the player.
            platform(2, 0, [-2.0, 8.0, -2.0], [2.0, 9.0, 2.0]),
        ],
        62,
    );
    let world = World::from_level(&level);
    let mut hz = Hazards::new();
    warm(&mut hz, &level, &world);
    let mut enemies = vec![enemy(1, BEHAVIOR_GALE_WIGHT, 0.0, 0.0, 0.0)];
    let players = vec![sense(1, 0.0, 0.0, 0.0)];
    let mut killed = false;
    let mut shoved = false;
    for _ in 0..600 {
        let fx = tick(&mut hz, &mut enemies, &level, &world, &players, &[]);
        if !kills(&fx).is_empty() {
            killed = true;
        }
        if shove_count(&fx) > 0 {
            shoved = true;
        }
    }
    assert!(!killed, "shelter stops the shell blast");
    assert!(!shoved, "and keeps the knockback ring out");
}

#[test]
fn gale_wight_blast_bypasses_ward() {
    let level = empty_level(63);
    let world = World::from_level(&level);
    let mut hz = Hazards::new();
    warm(&mut hz, &level, &world);
    let mut enemies = vec![enemy(1, BEHAVIOR_GALE_WIGHT, 0.0, 0.0, 0.0)];
    let players = vec![sense_vel(1, v3(0.0, 0.0, 0.0), v3(0.0, 0.0, 0.0), true)];
    let mut causes = Vec::new();
    for _ in 0..600 {
        let fx = tick(&mut hz, &mut enemies, &level, &world, &players, &[]);
        causes.extend(kill_causes(&fx));
        if !causes.is_empty() {
            break;
        }
    }
    assert!(!causes.is_empty(), "shell blasts are the ward exception");
    assert!(
        causes.iter().all(|c| *c == DEATH_ENEMY),
        "blast kills report DEATH_ENEMY"
    );
}

#[test]
fn lantern_leech_siphons_the_newest_pickup() {
    let level = empty_level(71);
    let world = World::from_level(&level);
    let mut hz = Hazards::new();
    warm(&mut hz, &level, &world);
    let mut enemies = vec![enemy(1, BEHAVIOR_LANTERN_LEECH, 0.0, 0.0, 0.0)];
    let _ = tick(
        &mut hz,
        &mut enemies,
        &level,
        &world,
        &[],
        &[v3(5.0, 0.0, 0.0), v3(9.0, 0.0, 0.0)],
    );
    match &enemies[0].brain {
        Brain::LanternLeech(b) => {
            assert_eq!(b.phase, LeechPhase::Siphon, "locks onto the pickup");
            assert_vec3_near(&b.goal, &v3(9.0, 0.0, 0.0));
        }
        other => panic!("expected a leech brain, got {other:?}"),
    }
}

#[test]
fn lantern_leech_gorge_rearback_telegraphs_the_sting() {
    let level = empty_level(72);
    let world = World::from_level(&level);
    let mut hz = Hazards::new();
    warm(&mut hz, &level, &world);
    let mut enemies = vec![enemy(1, BEHAVIOR_LANTERN_LEECH, 0.0, 0.0, 0.0)];
    enemies[0].brain = Brain::LanternLeech(LanternLeechBrain {
        phase: LeechPhase::Gorge,
        timer: 40, // 2.0 s of gorge
        goal: v3(0.0, 0.0, 0.0),
        away: v3(1.0, 0.0, 0.0),
    });
    let players = vec![sense(1, 0.5, 0.0, 0.0)]; // within 1.5 u of the gorge point
    let mut tele_at = None;
    let mut kill_at = None;
    let mut cause = None;
    for t in 0..80 {
        let fx = tick(&mut hz, &mut enemies, &level, &world, &players, &[]);
        let v = view_of(&hz, &enemies, 1);
        if v.state == ENEMY_TELEGRAPH && tele_at.is_none() {
            tele_at = Some(t);
        }
        if let Some(c) = kill_causes(&fx).first() {
            if kill_at.is_none() {
                kill_at = Some(t);
                cause = Some(*c);
            }
        }
    }
    let tele = tele_at.expect("rear-back telegraph before the sting");
    let kill = kill_at.expect("sting lands inside 1.5 u");
    assert!(kill - tele >= 12, "0.6 s rear-back precedes the sting");
    assert_eq!(cause, Some(DEATH_ENEMY));
}

#[test]
fn lantern_leech_flies_to_pickup_and_gorges_end_to_end() {
    let level = empty_level(73);
    let world = World::from_level(&level);
    let mut hz = Hazards::new();
    warm(&mut hz, &level, &world);
    let mut enemies = vec![enemy(1, BEHAVIOR_LANTERN_LEECH, 6.0, 0.0, 0.0)];
    let players = vec![sense(1, 0.0, 0.0, 0.0)];
    let mut saw_siphon = false;
    let mut killed = false;
    for t in 0..400 {
        let pickups: &[Vec3] = if t == 0 { &[v3(0.0, 0.0, 0.0)] } else { &[] };
        let fx = tick(&mut hz, &mut enemies, &level, &world, &players, pickups);
        if matches!(
            &enemies[0].brain,
            Brain::LanternLeech(b) if b.phase == LeechPhase::Siphon
        ) {
            saw_siphon = true;
        }
        if !kills(&fx).is_empty() {
            killed = true;
            break;
        }
    }
    assert!(saw_siphon, "the leech enters siphon flight");
    assert!(killed, "the leech reaches the gift and stings");
}

#[test]
fn turret_charges_fires_and_kills_with_a_projectile() {
    let level = empty_level(81);
    let world = World::from_level(&level);
    let mut hz = Hazards::new();
    warm(&mut hz, &level, &world);
    let mut enemies = vec![enemy(1, BEHAVIOR_TURRET, 0.0, 1.0, 0.0)];
    let players = vec![sense(1, 6.0, 0.0, 0.0)]; // velocity 0: intercept == player centre
    let mut tele_at = None;
    let mut attack_at = None;
    let mut kill_at = None;
    let mut cause = None;
    for t in 0..200 {
        let fx = tick(&mut hz, &mut enemies, &level, &world, &players, &[]);
        let v = view_of(&hz, &enemies, 1);
        match v.state {
            ENEMY_TELEGRAPH => {
                if tele_at.is_none() {
                    assert_vec3_near(&v.target, &v3(6.0, 0.9, 0.0));
                    tele_at = Some(t);
                }
            }
            ENEMY_ATTACK => attack_at = Some(t),
            _ => {}
        }
        if let Some(c) = kill_causes(&fx).first() {
            kill_at = Some(t);
            cause = Some(*c);
            break;
        }
    }
    let tele = tele_at.expect("0.95 s charge telegraph");
    let kill = kill_at.expect("bolt connects");
    assert!(attack_at.is_some(), "muzzle flash reads as attack");
    assert_eq!(cause, Some(DEATH_PROJECTILE));
    assert!(kill - tele >= 12, "challenged by no smaller telegraph");
}

#[test]
fn turret_needs_line_of_sight_to_the_target() {
    let level = test_level(
        vec![platform(4, 0, [2.0, 0.0, -1.0], [3.0, 2.0, 1.0])],
        82,
    );
    let world = World::from_level(&level);
    let mut hz = Hazards::new();
    warm(&mut hz, &level, &world);
    let mut enemies = vec![enemy(1, BEHAVIOR_TURRET, 0.0, 1.0, 0.0)];
    let players = vec![sense(1, 6.0, 0.0, 0.0)];
    for _ in 0..40 {
        let fx = tick(&mut hz, &mut enemies, &level, &world, &players, &[]);
        assert!(kills(&fx).is_empty());
    }
    assert!(
        hz.projectiles().is_empty(),
        "no bolt is ever fired without sight"
    );
    assert_eq!(
        view_of(&hz, &enemies, 1).state,
        ENEMY_HUNTING,
        "the sentry keeps seeking"
    );
}

#[test]
fn turret_bolt_is_blocked_by_the_ward() {
    let level = empty_level(83);
    let world = World::from_level(&level);
    let mut hz = Hazards::new();
    warm(&mut hz, &level, &world);
    let mut enemies = vec![enemy(1, BEHAVIOR_TURRET, 0.0, 1.0, 0.0)];
    let players = vec![sense_vel(1, v3(6.0, 0.0, 0.0), v3(0.0, 0.0, 0.0), true)];
    let mut kill_count = 0usize;
    for _ in 0..200 {
        let fx = tick(&mut hz, &mut enemies, &level, &world, &players, &[]);
        kill_count += kills(&fx).len();
    }
    assert_eq!(kill_count, 0, "warded players are immune to bolts");
}

#[test]
fn mimic_disguised_then_hiss_then_lunge() {
    let level = empty_level(91);
    let world = World::from_level(&level);
    let mut hz = Hazards::new();
    warm(&mut hz, &level, &world);
    let mut enemies = vec![enemy(1, BEHAVIOR_MIMIC, 0.0, 0.0, 0.0)];
    let mut players = vec![sense(1, 7.0, 0.0, 0.0)];
    for _ in 0..3 {
        let fx = tick(&mut hz, &mut enemies, &level, &world, &players, &[]);
        assert!(fx.is_empty());
        assert_eq!(view_of(&hz, &enemies, 1).state, ENEMY_DISGUISED);
    }
    players[0].feet = v3(2.5, 0.0, 0.0); // step within the 4 u trigger
    let mut tele_at = None;
    let mut kill_at = None;
    for t in 0..100 {
        let fx = tick(&mut hz, &mut enemies, &level, &world, &players, &[]);
        let v = view_of(&hz, &enemies, 1);
        if v.state == ENEMY_TELEGRAPH {
            if tele_at.is_none() {
                assert_vec3_near(&v.target, &v3(2.5, 0.9, 0.0));
                tele_at = Some(t);
            }
        }
        if !kills(&fx).is_empty() {
            kill_at = Some(t);
            break;
        }
    }
    let tele = tele_at.expect("the hiss is a telegraph");
    let kill = kill_at.expect("the lunge closes the distance");
    assert!(kill - tele >= 12, "0.9 s hiss precedes the lunge");
}

#[test]
fn mimic_ward_blocks_the_lunge() {
    let level = empty_level(92);
    let world = World::from_level(&level);
    let mut hz = Hazards::new();
    warm(&mut hz, &level, &world);
    let mut enemies = vec![enemy(1, BEHAVIOR_MIMIC, 0.0, 0.0, 0.0)];
    let players = vec![sense_vel(1, v3(2.5, 0.0, 0.0), v3(0.0, 0.0, 0.0), true)];
    let mut kill_count = 0usize;
    for _ in 0..100 {
        let fx = tick(&mut hz, &mut enemies, &level, &world, &players, &[]);
        kill_count += kills(&fx).len();
    }
    assert_eq!(kill_count, 0);
}

#[test]
fn warded_snapshrike_dash_never_hits_nor_shoves() {
    let level = empty_level(93);
    let world = World::from_level(&level);
    let mut hz = Hazards::new();
    warm(&mut hz, &level, &world);
    let mut enemies = vec![enemy(1, BEHAVIOR_SNAPSHRIKE, 6.0, 0.0, 0.0)];
    let players = vec![sense_vel(1, v3(0.0, 0.0, 0.0), v3(0.0, 0.0, 0.0), true)];
    let mut kills_n = 0usize;
    let mut shoves_n = 0usize;
    for _ in 0..150 {
        let fx = tick(&mut hz, &mut enemies, &level, &world, &players, &[]);
        kills_n += kills(&fx).len();
        shoves_n += shove_count(&fx);
    }
    assert_eq!(kills_n, 0);
    assert_eq!(shoves_n, 0);
}

#[test]
fn warded_moteling_inhale_never_kills() {
    let level = empty_level(94);
    let world = World::from_level(&level);
    let mut hz = Hazards::new();
    warm(&mut hz, &level, &world);
    let mut enemies = vec![enemy(1, BEHAVIOR_MOTELING, 5.0, 0.0, 0.0)];
    let players = vec![sense_vel(1, v3(0.0, 0.0, 0.0), v3(2.2, 0.0, 0.0), true)];
    let mut kills_n = 0usize;
    for _ in 0..400 {
        let fx = tick(&mut hz, &mut enemies, &level, &world, &players, &[]);
        kills_n += kills(&fx).len();
    }
    assert_eq!(kills_n, 0, "ward blocks the shriek Kill");
}

#[test]
fn corrupter_frets_the_marked_platform() {
    let level = test_level(
        vec![platform(9, 0, [-2.0, -1.0, -2.0], [2.0, 0.0, 2.0])],
        101,
    );
    let world = World::from_level(&level);
    let mut hz = Hazards::new();
    warm(&mut hz, &level, &world);
    // Corrupter and player both stand on the platform.
    let mut enemies = vec![enemy(1, BEHAVIOR_CORRUPTER, 1.0, 0.0, 1.0)];
    let players = vec![sense(1, 0.0, 0.0, 0.0)];
    let mut fretted_at = None;
    let mut killed = false;
    for t in 0..200 {
        let fx = tick(&mut hz, &mut enemies, &level, &world, &players, &[]);
        if !kills(&fx).is_empty() {
            killed = true;
        }
        if hz
            .tiles()
            .iter()
            .any(|s| s.id == 9 && s.state == TILE_FRETTED)
        {
            fretted_at = Some(t);
            break;
        }
    }
    let t = fretted_at.expect("the aura frets the platform");
    assert!(t >= 40, "fretting takes strictly more than 2.0 s inside the aura (t={t})");
    assert!(!killed, "the corrupter is never lethal");
}

#[test]
fn fretted_tile_clears_after_eight_seconds() {
    let level = test_level(
        vec![platform(9, 0, [-2.0, -1.0, -2.0], [2.0, 0.0, 2.0])],
        102,
    );
    let world = World::from_level(&level);
    let mut hz = Hazards::new();
    warm(&mut hz, &level, &world);
    let mut enemies = vec![enemy(1, BEHAVIOR_CORRUPTER, 0.5, 0.0, 0.0)];
    let players = vec![sense(1, 0.0, 0.0, 0.0)];
    for _ in 0..200 {
        let _ = tick(&mut hz, &mut enemies, &level, &world, &players, &[]);
        if hz
            .tiles()
            .iter()
            .any(|s| s.id == 9 && s.state == TILE_FRETTED)
        {
            break;
        }
    }
    assert!(hz
        .tiles()
        .iter()
        .any(|s| s.id == 9 && s.state == TILE_FRETTED));
    // No re-fretting once the wisp is gone.
    enemies.clear();
    for _ in 0..165 {
        hz.step(&mut Vec::new(), &world, &level, &players, &[], 1.0);
    }
    assert!(
        !hz.tiles().iter().any(|s| s.id == 9 && s.state == TILE_FRETTED),
        "TILE_FRETTED expires after its 8 s"
    );
}

#[test]
fn crumble_platform_warns_then_goes_gone() {
    let level = test_level(
        vec![platform(7, PLATFORM_CRUMBLE, [-2.0, -1.0, -2.0], [2.0, 0.0, 2.0])],
        111,
    );
    let world = World::from_level(&level);
    let mut hz = Hazards::new();
    warm(&mut hz, &level, &world);
    let mut enemies = Vec::new();
    let players = vec![sense(1, 0.0, 0.0, 0.0)];
    tick(&mut hz, &mut enemies, &level, &world, &players, &[]);
    assert!(
        hz.tiles().iter().any(|s| s.id == 7 && s.state == TILE_WARN),
        "standing on a crumble platform marks it TILE_WARN"
    );
    let mut saw_gone = false;
    for _ in 0..15 {
        tick(&mut hz, &mut enemies, &level, &world, &players, &[]);
        if hz.tiles().iter().any(|s| s.id == 7 && s.state == TILE_GONE) {
            saw_gone = true;
            break;
        }
    }
    assert!(saw_gone, "TILE_WARN turns TILE_GONE after 0.6 s");
    // Long enough for TILE_GONE (3 s) to lapse again, plus slack for the re-warn.
    for _ in 0..62 {
        tick(&mut hz, &mut enemies, &level, &world, &players, &[]);
    }
    assert!(
        !hz.tiles().iter().any(|s| s.id == 7 && s.state == TILE_GONE),
        "TILE_GONE expires after its 3 s"
    );
}

#[test]
fn pulse_interrupt_releases_the_heavy_slot() {
    let level = empty_level(121);
    let world = World::from_level(&level);
    let mut hz = Hazards::new();
    warm(&mut hz, &level, &world);
    let mut enemies = vec![enemy(1, BEHAVIOR_MOTELING, 2.0, 0.0, 0.0)];
    let players = vec![sense(1, 0.0, 0.0, 0.0)];
    let mut acquired = false;
    for _ in 0..400 {
        let _ = tick(&mut hz, &mut enemies, &level, &world, &players, &[]);
        let v = view_of(&hz, &enemies, 1);
        if v.state == ENEMY_TELEGRAPH && has_heavy(&v) {
            acquired = true;
            break;
        }
    }
    assert!(acquired, "the moteling holds the heavy slot during inhale");
    // The moteling has orbited away from its spawn point; the pulse still blows
    // it directly away from the pulse centre at 7 u/s (3D direction).
    let away_x = enemies[0].pos.x;
    let away_y = enemies[0].pos.y;
    let away_z = enemies[0].pos.z;
    let d = (away_x * away_x + away_y * away_y + away_z * away_z).sqrt();
    hz.pulse(&mut enemies, &v3(0.0, 0.0, 0.0), 10.0);
    assert_eq!(enemies[0].knock_ticks, 16, "0.8 s of Bulwark knockback");
    assert!(d > 1e-3, "the moteling is clear of the pulse centre");
    assert!(
        (enemies[0].knock_vel.x - 7.0 * away_x / d).abs() < 1e-6
            && (enemies[0].knock_vel.y - 7.0 * away_y / d).abs() < 1e-6
            && (enemies[0].knock_vel.z - 7.0 * away_z / d).abs() < 1e-6,
        "blown directly away at 7 u/s"
    );
    let v = view_of(&hz, &enemies, 1);
    assert_eq!(v.state, ENEMY_RECOVER, "interrupted into cooldown");
    assert!(!has_heavy(&v), "the heavy slot is released");
}

#[test]
fn heavy_mutex_holds_over_10000_ticks_full_roster_x2() {
    let level = empty_level(99);
    let world = World::from_level(&level);
    let mut hz = Hazards::new();
    warm(&mut hz, &level, &world);
    let behaviors = [
        BEHAVIOR_PALLBEARER,
        BEHAVIOR_SNAPSHRIKE,
        BEHAVIOR_MOTELING,
        BEHAVIOR_RIFT_WARDEN,
        BEHAVIOR_GALE_WIGHT,
        BEHAVIOR_LANTERN_LEECH,
        BEHAVIOR_TURRET,
        BEHAVIOR_MIMIC,
        BEHAVIOR_CORRUPTER,
    ];
    let mut enemies: Vec<Enemy> = Vec::new();
    let mut id = 1u32;
    for b in behaviors {
        for _ in 0..2 {
            let a = id as f32 * 2.399963; // golden angle
            let r = 3.0 + (id as f32 % 9.0);
            let y = if b == BEHAVIOR_GALE_WIGHT { 3.0 } else { 0.0 };
            enemies.push(enemy(id, b, a.cos() * r, y, a.sin() * r));
            id += 1;
        }
    }
    assert_eq!(enemies.len(), 18);
    let mut players: Vec<PlayerSense> = (0..4).map(|i| sense(i + 1, 6.0, 0.0, 0.0)).collect();
    let mut heavy_max = 0usize;
    for t in 0..10_000u32 {
        let phi = t as f32 * 0.01;
        for (i, p) in players.iter_mut().enumerate() {
            let ph = phi + i as f32 * std::f32::consts::FRAC_PI_2;
            let r = 5.0 + i as f32;
            p.feet = v3(ph.cos() * r, 0.0, ph.sin() * r);
            p.vel = v3(-ph.sin() * r * 0.2, 0.0, ph.cos() * r * 0.2);
        }
        let fx = tick(&mut hz, &mut enemies, &level, &world, &players, &[]);
        for e in &fx {
            if let HazardEffect::Shove { vel, .. } = e {
                assert!(
                    vel.x.is_finite() && vel.y.is_finite() && vel.z.is_finite(),
                    "non-finite shove velocity at tick {t}"
                );
            }
        }
        let views = hz.views(&enemies);
        for v in &views {
            assert!(
                v.pos.x.is_finite() && v.pos.y.is_finite() && v.pos.z.is_finite(),
                "non-finite enemy position at tick {t}"
            );
        }
        let heavy = views.iter().filter(|v| has_heavy(v)).count();
        heavy_max = heavy_max.max(heavy);
        assert!(heavy <= 1, "tick {t}: two heavy wind-ups at once");
    }
    assert!(heavy_max >= 1, "the soak should exercise heavy wind-ups");
}

#[test]
fn determinism_same_inputs_same_effects() {
    fn run() -> Vec<Vec<HazardEffect>> {
        let level = empty_level(123);
        let world = World::from_level(&level);
        let mut hz = Hazards::new();
        warm(&mut hz, &level, &world);
        let mut enemies = vec![
            enemy(1, BEHAVIOR_PALLBEARER, 5.0, 0.0, 0.0),
            enemy(2, BEHAVIOR_MOTELING, 0.0, 0.0, 4.0),
            enemy(3, BEHAVIOR_GALE_WIGHT, -4.0, 2.0, 2.0),
            enemy(4, BEHAVIOR_MIMIC, 2.0, 0.0, 2.0),
        ];
        let mut players: Vec<PlayerSense> =
            (0..3).map(|i| sense(i + 1, 3.0 + i as f32, 0.0, 1.0)).collect();
        let mut out = Vec::new();
        for t in 0..300u32 {
            let phi = t as f32 * 0.02;
            for (i, p) in players.iter_mut().enumerate() {
                p.feet = v3(3.0 * phi.cos() + i as f32, 0.0, 3.0 * phi.sin());
                p.vel = v3(-0.06, 0.0, 0.06);
            }
            out.push(tick(&mut hz, &mut enemies, &level, &world, &players, &[]));
        }
        out
    }
    assert_eq!(run(), run(), "same inputs must produce same effects");
}

/// Regression (found in M3 integration): hub enemy slots lie ON the floor, and a turret whose centre sat on the floor
/// surface never had line of sight (the sight line grazed the floor's top face). The server now spawns enemies with
/// their centre at least at chest height; a turret standing on a floor at that height must hit a player in the open.
#[test]
fn turret_standing_on_a_floor_fires_at_a_player_in_the_open() {
    let floor = platform(0, protocol::PLATFORM_NORMAL, [-30.0, -1.0, -30.0], [30.0, 0.0, 30.0]);
    let level = test_level(vec![floor], 7);
    let world = World::from_level(&level);
    let mut enemies = vec![enemy(1, BEHAVIOR_TURRET, 0.0, 1.0, 0.0)];
    let mut hazards = Hazards::new();
    let player = sense(9, 8.0, 0.0, 0.0);
    let mut killed = false;
    for _ in 0..400 {
        let fx = hazards.step(&mut enemies, &world, &level, std::slice::from_ref(&player), &[], 1.0);
        if fx.iter().any(|e| matches!(e, HazardEffect::Kill { player: 9, cause } if *cause == DEATH_PROJECTILE)) {
            killed = true;
            break;
        }
    }
    assert!(killed, "a turret standing on a floor must be able to shoot a standing player 8 u away");
}
