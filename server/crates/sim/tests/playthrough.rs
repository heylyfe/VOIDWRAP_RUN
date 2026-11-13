//! Playthrough bot: proves every hand-authored level is completable with the REAL movement
//! controller (same code the server and the WASM client run), frame by frame — every gift
//! collected, then the beacon reached — using only running and jumping (no dash).
//!
//! The bot plans a route over platforms (edge = horizontal gap <= 4.0 and rise <= 1.5), then
//! steers toward the next platform and jumps when it is about to leave the current one, when the
//! next platform is higher, or when it is blocked by a wall. Enemies are not simulated: this
//! test is about level geometry and movement, not combat.

use protocol::{BTN_JUMP, ControllerState, InputFrame, LevelData, Platform, Vec3};
use sim::controller::{spawn_state, step};
use sim::levels::{LEVEL_COUNT, level};
use sim::rules::{beacon_reached, gift_touched};
use sim::world::World;
use std::collections::VecDeque;

const MAX_FRAMES_PER_GOAL: u32 = 60 * 25;

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
        while !done(&self.state.pos) {
            assert!(
                self.frames - start < MAX_FRAMES_PER_GOAL,
                "bot stuck at {:?} heading to platform {goal}",
                self.state.pos
            );
            assert!(
                self.state.pos.y > world.kill_y,
                "bot fell into the void heading to platform {goal}"
            );
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
            let (tx, tz) = match next {
                Some(n) => closest_inside(&lvl.platforms[n], self.state.pos.x, self.state.pos.z, 0.9),
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
                jump = leaving || higher_close || blocked;
            }
            if jump {
                self.holding_jump = true;
            } else if self.holding_jump && self.state.vel.y <= 0.0 {
                self.holding_jump = false;
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
    let lvl = level(n);
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
    while let Some(i) = taken.iter().position(|t| !t) {
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
