use protocol::{Aabb, InputFrame, LevelData, Platform, Vec3, PLATFORM_JUMP_PAD, PLATFORM_NORMAL, EVENT_NONE};
use sim::controller::{spawn_state, step};
use sim::world::World;
#[test]
fn probe() {
    let p = |id, kind, min: [f32;3], max: [f32;3]| Platform { id, kind, bounds: Aabb { min: Vec3::new(min[0],min[1],min[2]), max: Vec3::new(max[0],max[1],max[2]) } };
    let level = LevelData { level: 1, seed: 1, spawn: Vec3::ZERO, beacon: Vec3::new(0.0,-50.0,0.0), kill_y: -30.0,
        platforms: vec![p(0, PLATFORM_NORMAL, [-10.0,-1.0,-2.0],[0.0,0.0,2.0]), p(1, PLATFORM_JUMP_PAD, [0.0,-1.0,-1.5],[3.0,0.0,1.5])],
        gifts: vec![], event: EVENT_NONE, altars: vec![] };
    let w = World::from_level(&level);
    let mut s = spawn_state(Vec3::new(-6.0, 0.0, 0.0));
    let mut max_y = 0.0f32;
    for i in 0..120 { step(&mut s, &InputFrame { seq: i, move_x: 127, move_z: 0, buttons: 0 }, &w); max_y = max_y.max(s.pos.y); }
    println!("walk onto pad: max_y {max_y:.2} final {:?}", s.pos);
    let mut s = spawn_state(Vec3::new(1.5, 3.0, 0.0));
    let mut max_y = 0.0f32; let mut landed = false;
    for i in 0..120 { step(&mut s, &InputFrame { seq: i, move_x: 0, move_z: 0, buttons: 0 }, &w); if s.grounded { landed = true; } if landed { max_y = max_y.max(s.pos.y); } }
    println!("drop onto pad: max_y after landing {max_y:.2}");
}
