//! Integration tests for the chunk assembler, using only the public API of `sim::levelgen`.
//!
//! Small inline catalogs exercise rotation/join math, the stacking rule, loop closing,
//! sealing, the fallback degradation path and reachability-failure detection; the shared
//! catalog is then checked across 30 seeds x levels 1..=12 (valid, non-fallback, gift counts
//! per amendment A3, altar frequency, determinism).

use protocol::{
    Aabb, EVENT_CRUMBLE, EVENT_HIGH_RISE, EVENT_ICE, EVENT_NONE, Gift, LevelData, PLATFORM_NORMAL, Platform, Vec3,
};
use sim::levelgen::{
    Catalog, Facing, OpenSocket, event_for, facing_bridge, generate, generate_for, generate_for_with_bias,
    generate_for_with_spacing, generate_with_catalog, hub, parse_catalog, parse_catalog_lenient, plan_join,
    platform_pair_ok, validate,
};

fn inline_catalog() -> Catalog {
    let json = r#"{"version":1,"chunks":[
      {"id":"hub","name":"Test Hub","role":"hub","min_level":1,"difficulty":1,"tags":[],
       "platforms":[{"kind":"NORMAL","min":[-8,-1,-7],"max":[8,0,7]}],
       "sockets":[{"id":"n","pos":[0,0,7],"facing":"N","width":4},
                  {"id":"e","pos":[8,0,0],"facing":"E","width":4},
                  {"id":"s","pos":[0,0,-7],"facing":"S","width":4},
                  {"id":"w","pos":[-8,0,0],"facing":"W","width":4}],
       "gift_slots":[{"pos":[4,0.3,-4],"elevated":false}],
       "enemy_slots":[[5,2,0],[3.5,2,3.5],[0,2,5],[-3.5,2,3.5],[-5,2,0],[-3.5,2,-3.5],[0,2,-5],[3.5,2,-3.5]],
       "altar_slots":[[0,0,0]],
       "beacon":[0,0,0]},
      {"id":"straight","name":"Straight","role":"path","min_level":1,"difficulty":1,"tags":[],
       "platforms":[{"kind":"NORMAL","min":[-4,-1,-1.5],"max":[4,0,1.5]}],
       "sockets":[{"id":"w","pos":[-4,0,0],"facing":"W","width":3},
                  {"id":"e","pos":[4,0,0],"facing":"E","width":3}],
       "gift_slots":[{"pos":[-2,0.3,0],"elevated":false},{"pos":[2,0.3,0],"elevated":false}],
       "enemy_slots":[],"altar_slots":[]},
      {"id":"ramp","name":"Ramp","role":"path","min_level":1,"difficulty":1,"tags":["ramp","climb"],
       "platforms":[{"kind":"NORMAL","min":[-4,-1,-1.5],"max":[0,0,1.5]},
                    {"kind":"RAMP_PX","min":[0,0,-1.5],"max":[4,1.0,1.5]}],
       "sockets":[{"id":"w","pos":[-4,0,0],"facing":"W","width":3},
                  {"id":"e","pos":[4,1.0,0],"facing":"E","width":3}],
       "gift_slots":[{"pos":[-2,0.3,0],"elevated":false}],
       "enemy_slots":[],"altar_slots":[]},
      {"id":"junction","name":"Junction","role":"junction","min_level":1,"difficulty":2,"tags":[],
       "platforms":[{"kind":"NORMAL","min":[-4,-1,-4],"max":[4,0,4]}],
       "sockets":[{"id":"w","pos":[-4,0,0],"facing":"W","width":4},
                  {"id":"n","pos":[0,0,4],"facing":"N","width":4},
                  {"id":"e","pos":[4,0,0],"facing":"E","width":4}],
       "gift_slots":[{"pos":[0,0.3,2],"elevated":false}],
       "enemy_slots":[],"altar_slots":[]},
      {"id":"dead_end","name":"Dead End","role":"dead_end","min_level":1,"difficulty":1,"tags":[],
       "platforms":[{"kind":"NORMAL","min":[-2.5,-1,-2.5],"max":[2.5,0,2.5]}],
       "sockets":[{"id":"w","pos":[-2.5,0,0],"facing":"W","width":3}],
       "gift_slots":[{"pos":[1.5,0.3,1.5],"elevated":false},{"pos":[-1.5,0.3,1.5],"elevated":false},
                     {"pos":[1.5,0.3,-1.5],"elevated":false},{"pos":[-1.5,0.3,-1.5],"elevated":false}],
       "enemy_slots":[],"altar_slots":[]}
    ]}"#;
    parse_catalog(json).expect("inline catalog must parse")
}

/// FNV-1a over the raw level fields; no trait bounds required on the protocol types.
fn hash_level(l: &LevelData) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut mix = |x: u64| {
        h ^= x;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    };
    mix(l.level as u64);
    mix(l.seed as u64);
    mix(l.event as u64);
    mix(l.kill_y.to_bits() as u64);
    for p in &l.platforms {
        mix(p.id as u64);
        mix(p.kind as u64);
        mix(p.bounds.min.x.to_bits() as u64);
        mix(p.bounds.min.y.to_bits() as u64);
        mix(p.bounds.min.z.to_bits() as u64);
        mix(p.bounds.max.x.to_bits() as u64);
        mix(p.bounds.max.y.to_bits() as u64);
        mix(p.bounds.max.z.to_bits() as u64);
    }
    for g in &l.gifts {
        mix(g.id as u64);
        mix(g.kind as u64);
        mix(g.pos.x.to_bits() as u64);
        mix(g.pos.y.to_bits() as u64);
        mix(g.pos.z.to_bits() as u64);
    }
    for a in &l.altars {
        mix(a.id as u64);
        mix(a.pos.x.to_bits() as u64);
        mix(a.pos.y.to_bits() as u64);
        mix(a.pos.z.to_bits() as u64);
    }
    for c in [l.spawn.x, l.spawn.y, l.spawn.z, l.beacon.x, l.beacon.y, l.beacon.z] {
        mix(c.to_bits() as u64);
    }
    h
}

fn eligible_normal_area(l: &LevelData) -> f32 {
    l.platforms
        .iter()
        .filter(|p| p.kind == PLATFORM_NORMAL)
        .map(|p| (p.bounds.max.x - p.bounds.min.x) * (p.bounds.max.z - p.bounds.min.z))
        .sum()
}

/// Two platforms: p0 (spawn deck) and a far island at x in [30, 36]. The gift/beacon are
/// placed on the island or on the deck to exercise reachability detection.
fn island_level(beacon_far: bool, gift_far: bool) -> LevelData {
    let p0 = Platform {
        id: 0,
        kind: PLATFORM_NORMAL,
        bounds: Aabb {
            min: Vec3::new(-6.0, -1.0, -6.0),
            max: Vec3::new(6.0, 0.0, 6.0),
        },
    };
    let p1 = Platform {
        id: 1,
        kind: PLATFORM_NORMAL,
        bounds: Aabb {
            min: Vec3::new(30.0, -1.0, -1.5),
            max: Vec3::new(36.0, 0.0, 1.5),
        },
    };
    let beacon = if beacon_far {
        Vec3::new(33.0, 0.0, 0.0)
    } else {
        Vec3::new(0.0, 0.0, 5.0)
    };
    let gifts = vec![
        Gift {
            id: 0,
            pos: Vec3::new(-3.0, 0.3, -3.0),
            kind: protocol::GIFT_NORMAL,
        },
        Gift {
            id: 1,
            pos: if gift_far {
                Vec3::new(33.0, 0.3, 0.0)
            } else {
                Vec3::new(3.0, 0.3, -3.0)
            },
            kind: protocol::GIFT_NORMAL,
        },
    ];
    LevelData {
        level: 1,
        seed: 1,
        spawn: Vec3::ZERO,
        beacon,
        kill_y: -16.0,
        event: EVENT_NONE,
        platforms: vec![p0, p1],
        gifts,
        altars: Vec::new(),
    }
}

// ---------------- inline catalogs: rotation / join / stacking / loop / seal ----------------

#[test]
fn rotation_math_joins_facing_sockets() {
    let cat = inline_catalog();
    assert_eq!(cat.chunks.len(), 5);
    let straight = cat.chunks.iter().find(|c| c.id == "straight").expect("straight chunk");

    // Anchor: hub's E socket at (8, 0, 0) facing E. straight's W socket (idx 0) joins as-is.
    let anchor = OpenSocket {
        pos: Vec3::new(8.0, 0.0, 0.0),
        facing: Facing::E,
        width: 4.0,
        failures: 0,
    };
    let plan = plan_join(straight, 0, 0, &anchor).expect("straight W socket joins the hub E socket");
    let w = plan
        .sockets
        .iter()
        .find(|s| s.facing == Facing::W)
        .expect("joined socket stays in placement");
    assert!((w.pos.x - 8.0).abs() < 1e-4 && (w.pos.z - 0.0).abs() < 1e-4);
    let e = plan.sockets.iter().find(|s| s.facing == Facing::E).expect("far socket");
    assert!((e.pos.x - 16.0).abs() < 1e-4);
    assert_eq!(plan.platforms.len(), 1);
    assert!((plan.platforms[0].1.min.x - 8.0).abs() < 1e-4);
    assert!((plan.platforms[0].1.max.x - 16.0).abs() < 1e-4);

    // |dy| > 0.5 must reject the join.
    let high = OpenSocket {
        pos: Vec3::new(8.0, 0.6, 0.0),
        facing: Facing::E,
        width: 4.0,
        failures: 0,
    };
    assert!(plan_join(straight, 0, 0, &high).is_none());

    // Wrong facing must reject the join.
    let north = OpenSocket {
        pos: Vec3::new(0.0, 0.0, 7.0),
        facing: Facing::N,
        width: 4.0,
        failures: 0,
    };
    assert!(plan_join(straight, 0, 0, &north).is_none());

    // Three quarter-turns bring straight's W socket around to S, facing the hub's N socket.
    let plan3 = plan_join(straight, 0, 3, &north).expect("3 quarter turns bring W to S");
    let joined = plan3
        .sockets
        .iter()
        .find(|s| s.facing == Facing::S)
        .expect("joined socket faces south");
    assert!((joined.pos.x - 0.0).abs() < 1e-4 && (joined.pos.z - 7.0).abs() < 1e-4);
}

#[test]
fn stacking_rule_rejects_cramped_layers_but_allows_resting_contact() {
    let a = Aabb {
        min: Vec3::new(0.0, 0.0, 0.0),
        max: Vec3::new(4.0, 1.0, 4.0),
    };
    let intersecting = Aabb {
        min: Vec3::new(1.0, 0.0, 1.0),
        max: Vec3::new(5.0, 1.0, 5.0),
    };
    assert!(!platform_pair_ok(&a, &intersecting), "volume intersection rejected");
    let one_u_gap = Aabb {
        min: Vec3::new(0.0, 2.0, 0.0),
        max: Vec3::new(4.0, 3.0, 4.0),
    };
    assert!(!platform_pair_ok(&a, &one_u_gap), "1 u clearance rejected");
    let resting = Aabb {
        min: Vec3::new(0.0, 1.0, 0.0),
        max: Vec3::new(4.0, 2.0, 4.0),
    };
    assert!(platform_pair_ok(&a, &resting), "resting contact allowed");
    let four_u = Aabb {
        min: Vec3::new(0.0, 5.0, 0.0),
        max: Vec3::new(4.0, 6.0, 4.0),
    };
    assert!(platform_pair_ok(&a, &four_u), "exactly 4.0 u clearance allowed");
    let side_by_side = Aabb {
        min: Vec3::new(10.0, 0.0, 0.0),
        max: Vec3::new(14.0, 1.0, 4.0),
    };
    assert!(platform_pair_ok(&a, &side_by_side), "no XZ overlap: fine at any height");
}

#[test]
fn facing_bridge_geometry() {
    let a = OpenSocket {
        pos: Vec3::new(0.0, 2.0, 0.0),
        facing: Facing::E,
        width: 3.0,
        failures: 0,
    };
    let b = OpenSocket {
        pos: Vec3::new(8.0, 2.2, 0.3),
        facing: Facing::W,
        width: 3.0,
        failures: 0,
    };
    let Some((_kind, bounds)) = facing_bridge(&a, &b) else {
        panic!("facing sockets within 12 u must produce a bridge");
    };
    assert!((bounds.min.x - 0.0).abs() < 1e-4 && (bounds.max.x - 8.0).abs() < 1e-4);
    assert!((bounds.max.y - 2.2).abs() < 1e-4 && (bounds.min.y - 1.2).abs() < 1e-4);
    assert!((bounds.max.z - bounds.min.z - 3.5).abs() < 1e-4, "bridge is 3.5 u wide");

    // dy > 0.5 rejects.
    let b_high = OpenSocket {
        pos: Vec3::new(8.0, 3.0, 0.3),
        facing: Facing::W,
        width: 3.0,
        failures: 0,
    };
    assert!(facing_bridge(&a, &b_high).is_none());
    // Beyond 12 u rejects.
    let b_far = OpenSocket {
        pos: Vec3::new(14.0, 2.0, 0.0),
        facing: Facing::W,
        width: 3.0,
        failures: 0,
    };
    assert!(facing_bridge(&a, &b_far).is_none());
    // Perpendicular offset > 0.75 rejects.
    let b_off = OpenSocket {
        pos: Vec3::new(8.0, 2.0, 1.5),
        facing: Facing::W,
        width: 3.0,
        failures: 0,
    };
    assert!(facing_bridge(&a, &b_off).is_none());
    // Not facing each other rejects.
    let b_north = OpenSocket {
        pos: Vec3::new(8.0, 2.0, 0.0),
        facing: Facing::N,
        width: 3.0,
        failures: 0,
    };
    assert!(facing_bridge(&a, &b_north).is_none());
    // Facing away rejects (along <= 0).
    let b_behind = OpenSocket {
        pos: Vec3::new(-8.0, 2.0, 0.0),
        facing: Facing::W,
        width: 3.0,
        failures: 0,
    };
    assert!(facing_bridge(&a, &b_behind).is_none());
}

#[test]
fn inline_catalog_assembles_valid_levels() {
    let cat = inline_catalog();
    for level in 1..=3u32 {
        for seed in 0..12u32 {
            let built = generate_with_catalog(&cat, seed, level, 1);
            if let Err(e) = validate(&built.data) {
                panic!("inline catalog seed {seed} level {level}: {e}");
            }
        }
    }
}

#[test]
fn inline_catalog_is_deterministic() {
    let cat = inline_catalog();
    let a = generate_with_catalog(&cat, 42, 2, 1);
    let b = generate_with_catalog(&cat, 42, 2, 1);
    assert_eq!(hash_level(&a.data), hash_level(&b.data));
    assert_eq!(a.enemy_spawns.len(), b.enemy_spawns.len());
    assert!(a.enemy_spawns.iter().zip(&b.enemy_spawns).all(|(x, y)| x == y));
    let c = generate_for(42, 2, 1);
    let d = generate_for(42, 2, 1);
    assert_eq!(hash_level(&c.data), hash_level(&d.data));
}

#[test]
fn inline_catalog_gifts_respect_the_area_cap() {
    let cat = inline_catalog();
    for seed in 0..8u32 {
        let built = generate_with_catalog(&cat, seed, 1, 1);
        let eligible = eligible_normal_area(&built.data);
        assert!(
            (built.data.gifts.len() as f32) * 9.0 <= eligible + 1e-3,
            "seed {seed}: gift density exceeds one gift per 9 u^2"
        );
    }
}

#[test]
fn broken_hub_degrades_to_builtin_fallback() {
    // Two intersecting boxes in the hub: no intra-chunk check in the parser catches this,
    // the catalog fallback must refuse it and degrade to the guaranteed-valid builtin map.
    let json = r#"{"version":1,"chunks":[
      {"id":"hub","name":"Broken Hub","role":"hub","min_level":1,"difficulty":1,"tags":[],
       "platforms":[{"kind":"NORMAL","min":[-6,-1,-6],"max":[6,0,6]},
                    {"kind":"NORMAL","min":[-3,-1,-3],"max":[3,0,3]}],
       "sockets":[],"gift_slots":[],
       "enemy_slots":[[5,2,0],[3.5,2,3.5],[0,2,5],[-3.5,2,3.5],[-5,2,0],[-3.5,2,-3.5],[0,2,-5],[3.5,2,-3.5]],
       "altar_slots":[],"beacon":[0,0,0]}
    ]}"#;
    let cat = parse_catalog(json).expect("parser has no intra-chunk overlap check");
    let built = generate_with_catalog(&cat, 9, 1, 1);
    assert_eq!(
        built.data.platforms.len(),
        21,
        "broken hub must degrade to the builtin fallback (1 plaza + 20 arms)"
    );
    validate(&built.data).expect("builtin fallback must be valid");
}

// ---------------- parser ----------------

#[test]
fn lenient_parser_drops_only_the_broken_chunk() {
    let json = r#"{"version":1,"chunks":[
      {"id":"hub","name":"Test Hub","role":"hub","min_level":1,"difficulty":1,"tags":[],
       "platforms":[{"kind":"NORMAL","min":[-8,-1,-7],"max":[8,0,7]}],
       "sockets":[{"id":"n","pos":[0,0,7],"facing":"N","width":4}],
       "gift_slots":[],
       "enemy_slots":[[5,2,0],[3.5,2,3.5],[0,2,5],[-3.5,2,3.5],[-5,2,0],[-3.5,2,-3.5],[0,2,-5],[3.5,2,-3.5]],
       "altar_slots":[],"beacon":[0,0,0]},
      {"id":"badsock","name":"Bad Socket","role":"path","min_level":1,"difficulty":1,"tags":[],
       "platforms":[{"kind":"NORMAL","min":[-4,-1,-1.5],"max":[4,0,1.5]}],
       "sockets":[{"id":"w","pos":[-4,0,9],"facing":"W","width":3}],
       "gift_slots":[],"enemy_slots":[],"altar_slots":[]},
      {"id":"straight","name":"Straight","role":"path","min_level":1,"difficulty":1,"tags":[],
       "platforms":[{"kind":"NORMAL","min":[-4,-1,-1.5],"max":[4,0,1.5]}],
       "sockets":[{"id":"w","pos":[-4,0,0],"facing":"W","width":3},
                  {"id":"e","pos":[4,0,0],"facing":"E","width":3}],
       "gift_slots":[],"enemy_slots":[],"altar_slots":[]}
    ]}"#;
    let err = parse_catalog(json).expect_err("strict parse must reject the broken chunk");
    assert!(err.contains("badsock"), "{err}");
    let cat = parse_catalog_lenient(json).expect("lenient parse keeps the rest");
    let ids: Vec<&str> = cat.chunks.iter().map(|c| c.id.as_str()).collect();
    assert_eq!(ids, ["hub", "straight"]);
}

#[test]
fn lenient_parser_without_a_hub_is_an_error_not_a_panic() {
    let json = r#"{"version":1,"chunks":[
      {"id":"hub","name":"Broken Hub","role":"hub","min_level":1,"difficulty":1,"tags":[],
       "platforms":[{"kind":"NORMAL","min":[-8,-1,-7],"max":[8,0,7]}],
       "sockets":[{"id":"n","pos":[0,0,99],"facing":"N","width":4}],
       "gift_slots":[],
       "enemy_slots":[[5,2,0],[3.5,2,3.5],[0,2,5],[-3.5,2,3.5],[-5,2,0],[-3.5,2,-3.5],[0,2,-5],[3.5,2,-3.5]],
       "altar_slots":[],"beacon":[0,0,0]},
      {"id":"straight","name":"Straight","role":"path","min_level":1,"difficulty":1,"tags":[],
       "platforms":[{"kind":"NORMAL","min":[-4,-1,-1.5],"max":[4,0,1.5]}],
       "sockets":[{"id":"w","pos":[-4,0,0],"facing":"W","width":3},
                  {"id":"e","pos":[4,0,0],"facing":"E","width":3}],
       "gift_slots":[],"enemy_slots":[],"altar_slots":[]}
    ]}"#;
    assert!(parse_catalog(json).is_err());
    assert!(
        parse_catalog_lenient(json).is_err(),
        "lenient parse without a hub must fail, not panic"
    );
    assert!(parse_catalog_lenient("not json").is_err());
}

// ---------------- validate: reachability failures ----------------

#[test]
fn validate_passes_a_connected_handbuilt_level() {
    validate(&island_level(false, false)).expect("connected level is valid");
}

#[test]
fn validate_reports_unreachable_gift() {
    let lvl = island_level(false, true);
    let err = validate(&lvl).expect_err("gift on a far island must fail validation");
    assert!(err.contains("not reachable"), "{err}");
}

#[test]
fn validate_reports_unreachable_beacon() {
    let lvl = island_level(true, false);
    let err = validate(&lvl).expect_err("beacon on a far island must fail validation");
    assert!(err.contains("beacon is not reachable"), "{err}");
}

// ---------------- hub / events ----------------

#[test]
fn hub_is_a_lone_spawn_platform() {
    let h = hub();
    assert_eq!(h.level, 0);
    assert_eq!(h.platforms.len(), 1);
    assert_eq!(h.platforms[0].id, 0);
    assert!(h.gifts.is_empty());
    assert!(h.altars.is_empty());
    assert_eq!(h.event, EVENT_NONE);
    // Level 0 / level 1 always get EVENT_NONE; level >= 2 stays deterministic.
    for seed in 0..64u32 {
        assert_eq!(event_for(seed, 0), EVENT_NONE);
        assert_eq!(event_for(seed, 1), EVENT_NONE);
    }
    let mut non_none = 0u32;
    for seed in 0..64u32 {
        let e = event_for(seed, 5);
        assert!(matches!(e, EVENT_NONE | EVENT_HIGH_RISE | EVENT_ICE | EVENT_CRUMBLE));
        assert_eq!(event_for(seed, 5), e, "event_for must be deterministic");
        if e != EVENT_NONE {
            non_none += 1;
        }
    }
    assert!(non_none > 0, "about 35% of levels should get an event");
}

// ---------------- real catalog ----------------

#[test]
fn real_catalog_generates_valid_non_fallback_levels() {
    for level in 1..=12u32 {
        for seed in 0..30u32 {
            let built = generate(seed, level);
            if let Err(e) = validate(&built.data) {
                panic!("seed {seed} level {level}: {e}");
            }
            // The fallback maps have at most 26 platforms (hub + 20 arms, or 1 + 20).
            assert!(
                built.data.platforms.len() >= 30,
                "seed {seed} level {level}: fallback-sized map ({} platforms)",
                built.data.platforms.len()
            );
        }
    }
}

#[test]
fn real_catalog_is_deterministic() {
    for (seed, level) in [(1u32, 1u32), (7, 3), (42, 6), (99, 10)] {
        let a = generate(seed, level);
        let b = generate(seed, level);
        assert_eq!(hash_level(&a.data), hash_level(&b.data), "seed {seed} level {level}");
        assert_eq!(a.enemy_spawns.len(), b.enemy_spawns.len());
        assert!(a.enemy_spawns.iter().zip(&b.enemy_spawns).all(|(x, y)| x == y));
        let c = generate_for(seed, level, 3);
        let d = generate_for(seed, level, 3);
        assert_eq!(hash_level(&c.data), hash_level(&d.data));
    }
}

#[test]
fn gifts_scale_per_a3() {
    for level in [1u32, 3, 6, 9] {
        for seed in 0..20u32 {
            let built = generate(seed, level);
            let a3 = (46u32 + 10 * level.saturating_sub(1)).min(400); // players = 1
            let by_area = (eligible_normal_area(&built.data) / 9.0).floor() as u32;
            let expected = a3.min(by_area);
            let got = built.data.gifts.len() as u32;
            assert!(
                got <= expected,
                "seed {seed} level {level}: {got} gifts exceeds A3 cap {expected}"
            );
            assert!(
                got * 5 >= expected * 4,
                "seed {seed} level {level}: {got} gifts, expected ~{expected} (>= 80%)"
            );
        }
    }
}

#[test]
fn altar_frequency_on_levels_3_and_up_is_between_25_and_75_percent() {
    let mut hits = 0u32;
    let mut total = 0u32;
    for level in 3..=12u32 {
        for seed in 0..30u32 {
            total += 1;
            if !generate(seed, level).data.altars.is_empty() {
                hits += 1;
            }
        }
    }
    let frac = hits as f32 / total as f32;
    assert!(
        (0.25..=0.75).contains(&frac),
        "altar frequency {frac:.2} ({hits}/{total}) outside [0.25, 0.75]"
    );
}

#[test]
fn no_altars_below_level_3() {
    for level in 1..=2u32 {
        for seed in 0..10u32 {
            assert!(generate(seed, level).data.altars.is_empty());
        }
    }
}

/// T-055 task 5: a level-20 map must generate + validate in < 150 ms in release builds.
/// Run with: cargo test --release --test levelgen -- --ignored --nocapture
#[test]
#[ignore = "timing check; run in release mode"]
fn generate_level_20_within_budget() {
    let start = std::time::Instant::now();
    let built = generate(12345, 20);
    let check = validate(&built.data);
    let elapsed = start.elapsed();
    println!(
        "level 20: generate + validate in {elapsed:?} ({} platforms)",
        built.data.platforms.len()
    );
    assert!(check.is_ok(), "{check:?}");
    assert!(elapsed.as_millis() < 150, "level 20 took {elapsed:?}");
}

// ------------------------------------------------- T-159 theme bias ----

/// The share of placed chunks that carry `tag`, over `seeds` at `level`.
///
/// `GeneratedLevel::chunks` is the catalog index of every chunk the level was built from,
/// so this is the level's real tag mix rather than a proxy for it.
fn theme_share(seeds: &[u32], level: u32, bias: Option<&str>, tag: &str) -> (usize, usize) {
    let cat = sim::levelgen::catalog().expect("the shared catalog parses");
    let (mut themed, mut total) = (0usize, 0usize);
    for &seed in seeds {
        let built = generate_for_with_bias(seed, level, 1, 1.0, bias);
        for &ci in &built.chunks {
            total += 1;
            if cat.chunks[ci].tags.iter().any(|t| t == tag) {
                themed += 1;
            }
        }
    }
    (themed, total)
}

/// The hard requirement: an unpolled run generates exactly the levels it generated before
/// the poll existed. `None` must not perturb a single float, so the whole `LevelData` and
/// every side output has to match field for field.
#[test]
fn no_theme_bias_is_byte_identical_to_the_unbiased_generator() {
    for level in 1..=8u32 {
        for seed in [0u32, 1, 7, 1234, 99_991, 4_294_967_295] {
            let plain = generate_for_with_spacing(seed, level, 2, 1.0);
            let unbiased = generate_for_with_bias(seed, level, 2, 1.0, None);
            assert_eq!(
                plain.data, unbiased.data,
                "seed {seed} level {level}: level data drifted"
            );
            assert_eq!(
                plain.enemy_spawns, unbiased.enemy_spawns,
                "seed {seed} level {level}: spawns"
            );
            assert_eq!(plain.plan, unbiased.plan, "seed {seed} level {level}: plan");
            assert_eq!(plain.chunks, unbiased.chunks, "seed {seed} level {level}: chunk list");
        }
    }
}

/// A theme bias has to move the tag mix, not just the RNG: the polled theme must be
/// measurably over-represented in the chunks the generator actually places.
#[test]
fn a_theme_bias_shifts_the_tag_mix() {
    let seeds: Vec<u32> = (0..40u32).collect();
    for tag in ["theme:ruin", "theme:void", "theme:frost"] {
        let (plain_hits, plain_total) = theme_share(&seeds, 6, None, tag);
        let (biased_hits, biased_total) = theme_share(&seeds, 6, Some(tag), tag);
        let plain = plain_hits as f32 / plain_total as f32;
        let biased = biased_hits as f32 / biased_total as f32;
        println!(
            "{tag}: unbiased {plain:.3} ({plain_hits}/{plain_total}), biased {biased:.3} ({biased_hits}/{biased_total})"
        );
        assert!(
            biased > plain * 1.5,
            "{tag}: bias moved the share from {plain:.3} to {biased:.3}, expected at least 1.5x"
        );
    }
}

/// A bias for a tag no chunk carries must be inert: the weights are unchanged, so the
/// level is the unbiased level.
#[test]
fn a_bias_no_chunk_carries_changes_nothing() {
    for seed in [0u32, 3, 77, 4242] {
        let plain = generate_for_with_spacing(seed, 5, 1, 1.0);
        let unknown = generate_for_with_bias(seed, 5, 1, 1.0, Some("theme:does-not-exist"));
        assert_eq!(
            plain.data, unknown.data,
            "seed {seed}: an unmatched bias changed the level"
        );
        assert_eq!(
            plain.chunks, unknown.chunks,
            "seed {seed}: an unmatched bias changed the chunk list"
        );
    }
}
