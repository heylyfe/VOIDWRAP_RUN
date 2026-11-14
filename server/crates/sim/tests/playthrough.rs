//! Playthrough bot: proves a generated or hand-authored level is completable with the REAL
//! movement controller (same code the server and the WASM client run), frame by frame — gifts
//! collected, then the beacon reached — using only running and jumping (no dash).
//!
//! Generated maps are far larger than the hand-authored ones, so the generated-levels test
//! has the bot collect 8 seed-chosen gifts per (seed, level) and then the beacon; the
//! hand-authored levels are still played in full. The bot plans a route over platforms
//! (edge = horizontal gap <= 4.0 and rise <= 1.5), then steers toward the next platform and
//! jumps when it is about to leave the current one, when the next platform is higher, or when
//! it is blocked by a wall. Enemies are not simulated: this test is about level geometry and
//! movement, not combat.

use protocol::{BTN_JUMP, ControllerState, InputFrame, LevelData, Platform, Vec3};
use sim::controller::{spawn_state, step};
use sim::levelgen;
use sim::levels::{LEVEL_COUNT, level};
use sim::rules::{beacon_reached, gift_touched};
use sim::world::World;
use std::collections::VecDeque;

const MAX_FRAMES_PER_GOAL: u32 = 60 * 60;

fn gap_xz(a: &Platform, b: &Platform) -> f32 {
    let gx = (b.bounds.min.x - a.bounds.max.x)
        .max(a.bounds.min.x - b.bounds.max.x)
        .max(0.0);
    let gz = (b.bounds.min.z - a.bounds.max.z)
        .max(a.bounds.min.z - b.bounds.max.z)
        .max(0.0);
    (gx * gx + gz * gz).sqrt()
}

fn inside_xz(p: &Platform, x: f32, z: f32, margin: f32) -> bool {
    x >= p.bounds.min.x + margin
        && x <= p.bounds.max.x - margin
        && z >= p.bounds.min.z + margin
        && z <= p.bounds.max.z - margin
}

/// The platform the player is standing on (feet on its top, inside its x/z extent).
fn standing_on(level: &LevelData, s: &ControllerState) -> Option<usize> {
    if !s.grounded {
        return None;
    }
    level
        .platforms
        .iter()
        .position(|p| inside_xz(p, s.pos.x, s.pos.z, -0.4) && (p.bounds.max.y - s.pos.y).abs() < 1e-3)
}

/// The platform under a point that floats above it (gift) or sits on it (beacon).
fn platform_under(level: &LevelData, x: f32, y: f32, z: f32) -> usize {
    level
        .platforms
        .iter()
        .enumerate()
        .filter(|(_, p)| inside_xz(p, x, z, 0.0) && p.bounds.max.y <= y + 1e-3)
        .max_by(|a, b| a.1.bounds.max.y.total_cmp(&b.1.bounds.max.y))
        .map(|(i, _)| i)
        .expect("goal floats above some platform")
}

fn route(level: &LevelData, from: usize, to: usize) -> Vec<usize> {
    let n = level.platforms.len();
    let mut prev = vec![usize::MAX; n];
    let mut queue = VecDeque::from([from]);
    prev[from] = from;
    while let Some(i) = queue.pop_front() {
        if i == to {
            break;
        }
        let a = &level.platforms[i];
        for (j, b) in level.platforms.iter().enumerate() {
            if prev[j] == usize::MAX && gap_xz(a, b) <= 4.0 && b.bounds.max.y <= a.bounds.max.y + 1.5 {
                prev[j] = i;
                queue.push_back(j);
            }
        }
    }
    assert_ne!(prev[to], usize::MAX, "no route from platform {from} to {to}");
    let mut path = vec![to];
    while *path.last().unwrap() != from {
        path.push(prev[*path.last().unwrap()]);
    }
    path.reverse();
    path
}

/// Point inside platform `p` (inset by `inset`) closest to (x, z).
fn closest_inside(p: &Platform, x: f32, z: f32, inset: f32) -> (f32, f32) {
    let cx = (p.bounds.min.x + p.bounds.max.x) / 2.0;
    let cz = (p.bounds.min.z + p.bounds.max.z) / 2.0;
    let hx = ((p.bounds.max.x - p.bounds.min.x) / 2.0 - inset).max(0.0);
    let hz = ((p.bounds.max.z - p.bounds.min.z) / 2.0 - inset).max(0.0);
    (x.clamp(cx - hx, cx + hx), z.clamp(cz - hz, cz + hz))
}

struct Bot {
    state: ControllerState,
    seq: u32,
    frames: u32,
    holding_jump: bool,
    /// Consecutive frames commanded to move but not moving horizontally (pressed against a wall).
    stall: u32,
}

impl Bot {
    fn input(&mut self, tx: f32, tz: f32, jump: bool) -> InputFrame {
        let (dx, dz) = (tx - self.state.pos.x, tz - self.state.pos.z);
        let d = (dx * dx + dz * dz).sqrt();
        let (mx, mz) = if d < 0.05 {
            (0, 0)
        } else {
            ((dx / d * 127.0) as i8, (dz / d * 127.0) as i8)
        };
        self.seq += 1;
        InputFrame {
            seq: self.seq,
            move_x: mx,
            move_z: mz,
            buttons: if jump { BTN_JUMP } else { 0 },
        }
    }

    /// Walk/jump to (gx, gz) on platform `goal`, following the planned route. `done` is checked every frame.
    fn travel(&mut self, lvl: &LevelData, world: &World, goal: usize, gx: f32, gz: f32, done: &dyn Fn(&Vec3) -> bool) {
        let mut on = standing_on(lvl, &self.state).expect("bot starts each leg grounded");
        let mut path = route(lvl, on, goal);
        let start = self.frames;
        let mut next_hop: Option<(usize, usize)> = None;
        let mut trace: VecDeque<String> = VecDeque::new();
        // Hysteresis: once at the takeoff point of a hop, commit to it until we change platform.
        let mut committed: Option<(usize, usize)> = None;
        while !done(&self.state.pos) {
            if self.frames - start >= MAX_FRAMES_PER_GOAL && std::env::var_os("BOT_TRACE").is_some() {
                for line in trace.iter().skip(40) {
                    eprintln!("{line}");
                }
            }
            assert!(
                self.frames - start < MAX_FRAMES_PER_GOAL,
                "bot stuck at {:?} heading to platform {goal}",
                self.state.pos
            );
            if self.state.pos.y <= world.kill_y {
                if std::env::var_os("BOT_TRACE").is_some() {
                    for line in &trace {
                        eprintln!("{line}");
                    }
                }
                let detail = next_hop.map(|(a, b)| {
                    let (pa, pb) = (&lvl.platforms[a], &lvl.platforms[b]);
                    format!(
                        "hop {a}->{b}: gap {:.2}, rise {:.2}, widths {:.1}x{:.1} -> {:.1}x{:.1}",
                        gap_xz(pa, pb),
                        pb.bounds.max.y - pa.bounds.max.y,
                        pa.bounds.max.x - pa.bounds.min.x,
                        pa.bounds.max.z - pa.bounds.min.z,
                        pb.bounds.max.x - pb.bounds.min.x,
                        pb.bounds.max.z - pb.bounds.min.z
                    )
                });
                panic!("bot fell into the void heading to platform {goal}: {detail:?}");
            }
            if let Some(p) = standing_on(lvl, &self.state)
                && p != on
            {
                on = p;
                path = route(lvl, on, goal);
            }
            let next = path
                .iter()
                .position(|&p| p == on)
                .and_then(|i| path.get(i + 1))
                .copied();
            if let Some(n) = next {
                next_hop = Some((on, n));
            }
            // Like a player: walk to the takeoff point (the spot on the current platform nearest the landing),
            // then run at the landing. Never jump from wherever we happen to leave the platform.
            let mut approaching = false;
            let (tx, tz) = match next {
                Some(n) => {
                    let landing = closest_inside(&lvl.platforms[n], self.state.pos.x, self.state.pos.z, 0.9);
                    let takeoff = closest_inside(&lvl.platforms[on], landing.0, landing.1, 0.45);
                    let to_takeoff =
                        ((takeoff.0 - self.state.pos.x).powi(2) + (takeoff.1 - self.state.pos.z).powi(2)).sqrt();
                    if committed != Some((on, n)) && to_takeoff <= 0.6 {
                        committed = Some((on, n));
                    }
                    if self.state.grounded
                        && committed != Some((on, n))
                        && gap_xz(&lvl.platforms[on], &lvl.platforms[n]) > 0.0
                    {
                        approaching = true;
                        takeoff
                    } else {
                        landing
                    }
                }
                None => (gx, gz),
            };
            let mut jump = false;
            // Like a player, the bot may use coyote time right after walking off an edge.
            if (self.state.grounded || self.state.coyote_frames > 0)
                && !self.holding_jump
                && let Some(n) = next
            {
                let cur = &lvl.platforms[on];
                let nxt = &lvl.platforms[n];
                // About to leave the current top, or the next one is higher and close, or blocked by a wall.
                let ahead = self.input_peek(tx, tz, 0.55);
                let leaving = !inside_xz(cur, ahead.0, ahead.1, -0.3);
                let higher_close = nxt.bounds.max.y > cur.bounds.max.y + 0.2 && gap_xz(cur, nxt) < 1.5 && {
                    let (ex, ez) = closest_inside(nxt, self.state.pos.x, self.state.pos.z, 0.0);
                    ((ex - self.state.pos.x).powi(2) + (ez - self.state.pos.z).powi(2)).sqrt() < 1.6
                };
                let blocked = self.stall >= 3;
                jump = (leaving && !approaching) || higher_close || blocked;
            }
            if jump {
                self.holding_jump = true;
            } else if self.holding_jump && self.state.vel.y <= 0.0 {
                self.holding_jump = false;
            }
            trace.push_back(format!(
                "f{} on={on} next={next:?} pos=({:.2},{:.2},{:.2}) vel=({:.1},{:.1},{:.1}) g={} coy={} tgt=({tx:.2},{tz:.2}) jump={} hold={}",
                self.frames, self.state.pos.x, self.state.pos.y, self.state.pos.z, self.state.vel.x, self.state.vel.y,
                self.state.vel.z, self.state.grounded, self.state.coyote_frames, jump, self.holding_jump
            ));
            if trace.len() > 70 {
                trace.pop_front();
            }
            let input = self.input(tx, tz, self.holding_jump);
            step(&mut self.state, &input, world);
            let commanded = input.move_x != 0 || input.move_z != 0;
            let speed = self.state.vel.x.abs() + self.state.vel.z.abs();
            self.stall = if commanded && speed < 0.1 { self.stall + 1 } else { 0 };
            self.frames += 1;
        }
    }

    /// Position `dist` ahead of the player toward (tx, tz).
    fn input_peek(&self, tx: f32, tz: f32, dist: f32) -> (f32, f32) {
        let (dx, dz) = (tx - self.state.pos.x, tz - self.state.pos.z);
        let d = (dx * dx + dz * dz).sqrt().max(1e-6);
        (self.state.pos.x + dx / d * dist, self.state.pos.z + dz / d * dist)
    }

    /// Stand still until grounded (after a landing).
    fn settle(&mut self, world: &World) {
        for _ in 0..30 {
            if self.state.grounded {
                return;
            }
            let input = InputFrame {
                seq: self.seq + 1,
                move_x: 0,
                move_z: 0,
                buttons: 0,
            };
            self.seq += 1;
            step(&mut self.state, &input, world);
            self.frames += 1;
        }
    }
}

fn play_level(n: u32) -> u32 {
    play(&level(n))
}

/// Plays a level to completion: the requested gifts (by index into `lvl.gifts`, or all of
/// them when `only` is `None`), then the beacon; returns frames used.
fn play_subset(lvl: &LevelData, only: Option<&[usize]>) -> u32 {
    let lvl = lvl.clone();
    let world = World::from_level(&lvl);
    let mut bot = Bot {
        state: spawn_state(lvl.spawn.clone()),
        seq: 0,
        frames: 0,
        holding_jump: false,
        stall: 0,
    };
    bot.settle(&world);
    let mut taken = vec![false; lvl.gifts.len()];
    let order: Vec<usize> = match only {
        Some(list) => list.to_vec(),
        None => (0..lvl.gifts.len()).collect(),
    };
    for &i in &order {
        if i >= lvl.gifts.len() || taken[i] {
            continue;
        }
        let g = lvl.gifts[i].clone();
        let goal = platform_under(&lvl, g.pos.x, g.pos.y, g.pos.z);
        bot.travel(&lvl, &world, goal, g.pos.x, g.pos.z, &|feet| gift_touched(feet, &g));
        for (j, gift) in lvl.gifts.iter().enumerate() {
            if gift_touched(&bot.state.pos, gift) {
                taken[j] = true;
            }
        }
        bot.settle(&world);
    }
    let b = lvl.beacon.clone();
    let goal = platform_under(&lvl, b.x, b.y, b.z);
    bot.travel(&lvl, &world, goal, b.x, b.z, &|feet| beacon_reached(feet, &b));
    bot.frames
}

/// Plays a level to completion (every gift, normal and golden, then the beacon); returns frames used.
fn play(lvl: &LevelData) -> u32 {
    play_subset(lvl, None)
}

#[test]
fn every_level_is_completable_with_the_real_controller() {
    let mut total = 0;
    for n in 1..=LEVEL_COUNT {
        let frames = play_level(n);
        println!("level {n}: all gifts + beacon in {:.1} s", frames as f32 / 60.0);
        total += frames;
    }
    println!("full 3-level run: {:.1} s", total as f32 / 60.0);
}

fn splitmix32(x: u32) -> u32 {
    let mut z = x.wrapping_add(0x9E37_79B9);
    z = (z ^ (z >> 16)).wrapping_mul(0x21F0_AAAD);
    z = (z ^ (z >> 15)).wrapping_mul(0x735A_2D97);
    z ^ (z >> 15)
}

/// Deterministic per-(seed, gift) rank; lower ranks are chosen first.
fn seeded_gift_rank(seed: u32, gift_index: usize) -> u32 {
    splitmix32(seed ^ (gift_index as u32).wrapping_mul(2_654_435_761))
}

/// The `count` gifts chosen by `seed` (lowest ranks first).
fn pick_seeded_gifts(lvl: &LevelData, seed: u32, count: usize) -> Vec<usize> {
    let mut order: Vec<usize> = (0..lvl.gifts.len()).collect();
    order.sort_by_key(|&i| seeded_gift_rank(seed, i));
    order.truncate(count.min(order.len()));
    order
}

/// Generated maps must be physically completable, not just graph-reachable: run the bot on
/// many of them. Per (seed, level) the bot must reach 8 seed-chosen gifts and then the beacon.
#[test]
fn generated_levels_are_completable_with_the_real_controller() {
    let mut played = 0;
    let mut failures = Vec::new();
    for level in 1..=6u32 {
        for seed in 0..10u32 {
            let actual_seed = seed.wrapping_mul(2_654_435_761);
            let lvl = levelgen::generate(actual_seed, level).data;
            let goals = pick_seeded_gifts(&lvl, actual_seed, 8);
            played += 1;
            let result = std::panic::catch_unwind(|| play_subset(&lvl, Some(&goals)));
            if result.is_err() {
                failures.push((seed, level));
            }
        }
    }
    println!("generated levels played: {played}, failed: {}", failures.len());
    assert!(
        failures.is_empty(),
        "bot could not complete generated levels (seed, level): {failures:?}"
    );
}

/// Debug helper: BOT_TRACE=1 BOT_SEED=<seed index> BOT_LEVEL=<level> cargo test --release --test playthrough trace_one -- --nocapture
#[test]
fn trace_one() {
    let (Some(seed), Some(level)) = (
        std::env::var("BOT_SEED").ok().and_then(|v| v.parse::<u32>().ok()),
        std::env::var("BOT_LEVEL").ok().and_then(|v| v.parse::<u32>().ok()),
    ) else {
        return;
    };
    let lvl = levelgen::generate(seed.wrapping_mul(2_654_435_761), level).data;
    for (i, p) in lvl.platforms.iter().enumerate() {
        eprintln!(
            "P{i}: x {:.2}..{:.2} z {:.2}..{:.2} top {:.2}",
            p.bounds.min.x, p.bounds.max.x, p.bounds.min.z, p.bounds.max.z, p.bounds.max.y
        );
    }
    play(&lvl);
}
