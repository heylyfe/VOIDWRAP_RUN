//! Playthrough bot: proves a generated or hand-authored level is completable with the REAL
//! movement controller (same code the server and the WASM client run), frame by frame — gifts
//! collected, then the beacon reached — using only running and jumping (no dash).
//!
//! Generated maps are far larger than the hand-authored ones, so the generated-levels test
//! has the bot collect 8 seed-chosen gifts per (seed, level) and then the beacon; the
//! hand-authored levels are still played in full. The bot plans a route over the SAME
//! directed platform graph `levelgen::validate` uses (see `levelgen::platform_graph`): an
//! edge `i -> j` exists iff platform `j`'s top can be reached from platform `i`'s top — a
//! touching step (|rise| <= 0.55 u), a design-envelope jump edge-to-edge, or a jump pad
//! (+4.0 u within 5.0 u). Routes are BFS shortest paths. Navigation then walks the route
//! edge by edge — it steers around nothing else, because the edges of the route stay on
//! platform tops and therefore clear the level's obstacles:
//!
//! Generated levels are no longer gated on the route bot below (it is kept as an `#[ignore]`d
//! diagnostic, `cargo test --test playthrough -- --ignored`): the generated-map gate is now
//! `every_graph_edge_kind_is_physically_doable`, which proves every KIND of edge the platform
//! graph admits is performable with the real controller in a minimal two-platform world (plus
//! representative step-up / stair / drop cases).
//!
//! Standing on a platform reports the smallest matching footprint, so a platform set
//! flush into a larger same-height floor does not shadow it forever.
//!
//! - Walk/step edge (touching, rise <= 0.55 u): any touching flat or lower target — a long
//!   drop onto a lower touching platform (rise <= -0.55 u, incl. -4.5 u falls), a 0.5 u
//!   stair step or 1.0 u tread up or down onto a big platform, or a flat flush neighbour —
//!   steer at a point on the target's top at least player-half-width + 0.3 u beyond the
//!   current platform's footprint edge in the travel direction, and keep full input there
//!   until grounded on the target — never stop at the shared edge. Step-ups rely on the
//!   controller's 0.55 u step-up; a touching drop is never jumped (jumping would only
//!   widen the fall).
//! - Jump edge (gap > 0): run up along the line from the current platform's interior (its
//!   centre) through the takeoff point on its boundary toward the landing target's centre,
//!   take off in the last 0.3 u before the edge (coyote time honoured as a backstop, or when
//!   blocked by a wall), hold the jump to its full height while rising (in particular when
//!   the target is higher), and steer at the target's centre while airborne — steering is
//!   released only once grounded on the target. For jumps between platforms at different
//!   heights whose footprints touch or overlap (a high slab sitting on the current platform)
//!   there is no run-up: the bot stands at the closest point of the shared boundary and hops
//!   from there.
//! - Jump-pad edge: walk straight onto the pad's centre; once the pad has launched the bot
//!   (airborne) steer at the landing platform's centre.
//! - Oscillation: if the bot lands alternately on the same two platforms four times in a row
//!   (A, B, A, B, A) during one leg, the route for that leg is re-planned with that edge
//!   excluded.
//!
//! If the budget for one goal runs out, or the bot falls into the void, the failure message
//! prints the seed, the level, the current platform, the next platform, the edge kind and
//! the edge's geometry, so a real map defect (an edge the graph admits but physics cannot do)
//! can be reported instead of papered over. Enemies are not simulated: this test is about level
//! geometry and movement, not combat.

use protocol::{BTN_JUMP, ControllerState, InputFrame, LevelData, PLATFORM_JUMP_PAD, Platform, Vec3};
use sim::controller::{spawn_state, step};
use sim::levelgen;
use sim::levels::{LEVEL_COUNT, level};
use sim::rules::{beacon_reached, gift_touched};
use sim::world::World;
use std::collections::{BTreeMap, VecDeque};

const MAX_FRAMES_PER_GOAL: u32 = 60 * 60;
/// Player capsule half-width in u (mirrors the controller's collision radius).
const PLAYER_HALF_WIDTH: f32 = 0.3;

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
/// Overlapping same-height footprints (a platform set flush into a larger floor) make
/// several platforms match; the smallest footprint wins, so the route can advance to the
/// waypoint the bot actually crossed onto instead of being shadowed by the container.
fn standing_on(level: &LevelData, s: &ControllerState) -> Option<usize> {
    if !s.grounded {
        return None;
    }
    level
        .platforms
        .iter()
        .enumerate()
        .filter(|(_, p)| inside_xz(p, s.pos.x, s.pos.z, -0.4) && (p.bounds.max.y - s.pos.y).abs() < 1e-3)
        .min_by(|(ia, a), (ib, b)| {
            let aa = (a.bounds.max.x - a.bounds.min.x) * (a.bounds.max.z - a.bounds.min.z);
            let bb = (b.bounds.max.x - b.bounds.min.x) * (b.bounds.max.z - b.bounds.min.z);
            aa.total_cmp(&bb).then_with(|| ia.cmp(ib))
        })
        .map(|(i, _)| i)
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

/// The centre of a platform's footprint.
fn centre_xz(p: &Platform) -> (f32, f32) {
    (
        (p.bounds.min.x + p.bounds.max.x) * 0.5,
        (p.bounds.min.z + p.bounds.max.z) * 0.5,
    )
}

/// Unit direction of travel from `cur` toward `nxt` (centre to centre); falls back to the
/// direction toward the following waypoint, then to +x, for concentric footprints.
fn travel_dir(cur: &Platform, nxt: &Platform, wp: (f32, f32)) -> (f32, f32) {
    let (cx, cz) = centre_xz(cur);
    let (nx, nz) = centre_xz(nxt);
    let (mut dx, mut dz) = (nx - cx, nz - cz);
    if dx * dx + dz * dz <= 1e-6 {
        dx = wp.0 - cx;
        dz = wp.1 - cz;
    }
    let len = (dx * dx + dz * dz).sqrt();
    if len <= 1e-6 {
        return (1.0, 0.0);
    }
    (dx / len, dz / len)
}

/// Distance from (cx, cz) along the unit direction (ux, uz) to where the ray exits `p`'s XZ footprint.
fn ray_exit_t(p: &Platform, cx: f32, cz: f32, ux: f32, uz: f32) -> f32 {
    let mut t = f32::INFINITY;
    if ux > 1e-6 {
        t = t.min((p.bounds.max.x - cx) / ux);
    } else if ux < -1e-6 {
        t = t.min((p.bounds.min.x - cx) / ux);
    }
    if uz > 1e-6 {
        t = t.min((p.bounds.max.z - cz) / uz);
    } else if uz < -1e-6 {
        t = t.min((p.bounds.min.z - cz) / uz);
    }
    if t.is_finite() { t } else { 0.0 }
}

/// A point on `nxt`'s top at least `clearance` beyond `cur`'s footprint boundary in the
/// direction of travel: the exit point of the ray from `cur`'s centre along `dir`, pushed
/// `clearance` further out, clamped onto `nxt`'s top inset by `inset`. Used for touching
/// edges (flat, drops of any depth, and 0.5 u stair steps / 1.0 u treads up or down):
/// steering there walks the bot off the edge; the bot keeps steering there until it is
/// grounded on the lower platform (the route only advances once it is).
fn beyond_edge_point(cur: &Platform, nxt: &Platform, dir: (f32, f32), clearance: f32, inset: f32) -> (f32, f32) {
    let (cx, cz) = centre_xz(cur);
    let (dx, dz) = dir;
    let t = ray_exit_t(cur, cx, cz, dx, dz);
    let ex = cx + dx * t;
    let ez = cz + dz * t;
    closest_inside(nxt, ex + dx * clearance, ez + dz * clearance, inset)
}

/// One-line geometry summary of an edge, for failure messages.
fn edge_geometry(lvl: &LevelData, on: usize, next: Option<usize>, edge: Option<EdgeKind>) -> String {
    match next {
        Some(n) => {
            let (pa, pb) = (&lvl.platforms[on], &lvl.platforms[n]);
            format!(
                "edge {on}->{n} ({:?}): gap {:.2}, rise {:.2}, widths {:.1}x{:.1} -> {:.1}x{:.1}",
                edge,
                gap_xz(pa, pb),
                pb.bounds.max.y - pa.bounds.max.y,
                pa.bounds.max.x - pa.bounds.min.x,
                pa.bounds.max.z - pa.bounds.min.z,
                pb.bounds.max.x - pb.bounds.min.x,
                pb.bounds.max.z - pb.bounds.min.z
            )
        }
        None => format!("edge {on}-><none>"),
    }
}

/// The kind of a route edge between two adjacent platforms on the planned route.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EdgeKind {
    /// Touching platforms (no horizontal gap) with |rise| at most the 0.55 u step height,
    /// or a drop onto a lower touching platform of any depth.
    Walk,
    /// From a jump-pad platform: launch reaching up to +4.0 u within 5.0 u.
    JumpPad,
    /// Any other edge: a design-envelope jump (edge-to-edge).
    Jump,
}

/// Classifies the edge from `from` to `to`. Mirrors the admission rules of
/// `levelgen::can_jump_or_step`: touching with |rise| <= 0.55 u is a plain step; a jump pad
/// reaches +4.0 u within 5.0 u; everything else admitted by the graph is a design-envelope
/// jump. Order matters: a touching edge is walked even off a pad, so the bot is not
/// launched across a platform it could just step onto.
fn edge_kind(from: &Platform, to: &Platform) -> EdgeKind {
    let rise = to.bounds.max.y - from.bounds.max.y;
    let gap = gap_xz(from, to);
    // Touching neighbours: a step of up to 0.55 u, or any DROP onto a lower touching platform,
    // is walked (walking off the edge drops the player onto it; jumping would overshoot it).
    if gap <= 0.001 && rise <= 0.55 + 0.001 {
        return EdgeKind::Walk;
    }
    if from.kind == PLATFORM_JUMP_PAD && rise <= 4.0 + 0.001 && gap <= 5.0 + 0.001 {
        return EdgeKind::JumpPad;
    }
    EdgeKind::Jump
}

/// BFS from `from` to `to` over a directed platform graph; the returned path includes both
/// endpoints. Panics when no route exists (the maps under test are validated by levelgen,
/// so every goal is reachable and this should never trigger).
fn bfs_route(adj: &[Vec<usize>], from: usize, to: usize) -> Vec<usize> {
    let n = adj.len();
    let mut prev = vec![usize::MAX; n];
    let mut queue = VecDeque::from([from]);
    prev[from] = from;
    while let Some(i) = queue.pop_front() {
        if i == to {
            break;
        }
        for &j in &adj[i] {
            if prev[j] == usize::MAX {
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

/// The point in the XZ region shared by `a` and `b` closest to (x, z). For touching
/// platforms this is the closest point on the shared edge (a point of the boundary segment
/// both platforms have); for XZ-overlapping platforms (a high slab sitting on the current
/// platform) it is the closest point inside the overlapping region.
fn closest_shared_point(a: &Platform, b: &Platform, x: f32, z: f32) -> (f32, f32) {
    let lo_x = a.bounds.min.x.max(b.bounds.min.x);
    let hi_x = a.bounds.max.x.min(b.bounds.max.x);
    let lo_z = a.bounds.min.z.max(b.bounds.min.z);
    let hi_z = a.bounds.max.z.min(b.bounds.max.z);
    (x.clamp(lo_x, hi_x), z.clamp(lo_z, hi_z))
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

    /// Walk/jump to (gx, gz) on platform `goal`, following the planned route edge by edge.
    /// `done` is checked every frame.
    fn travel(&mut self, lvl: &LevelData, world: &World, goal: usize, gx: f32, gz: f32, done: &dyn Fn(&Vec3) -> bool) {
        // One graph (the same one `levelgen::validate` uses), shared by every BFS below.
        // The local copy is mutated if the bot oscillates between two platforms, so the
        // route can be re-planned without that edge for the rest of this leg.
        let mut adj = levelgen::platform_graph(lvl);
        let mut on = standing_on(lvl, &self.state).expect("bot starts each leg grounded");
        let mut path = bfs_route(&adj, on, goal);
        // Grounded platform visits on this leg, oldest first; used to detect ping-ponging.
        let mut visits: Vec<usize> = vec![on];
        let start = self.frames;
        let mut trace: VecDeque<String> = VecDeque::new();
        // Hysteresis: once at the takeoff point of a hop, commit to it until we change platform.
        let mut committed: Option<(usize, usize)> = None;
        while !done(&self.state.pos) {
            // Refresh the route bookkeeping first: the failure message needs current values.
            if let Some(p) = standing_on(lvl, &self.state)
                && p != on
            {
                on = p;
                visits.push(p);
                // Ping-ponged between the same two platforms (A, B, A, B, A — four
                // alternations)? Re-plan without that edge for the rest of this leg.
                if visits.len() >= 5 {
                    let m = visits.len();
                    let (a, b) = (visits[m - 5], visits[m - 4]);
                    if a != b && visits[m - 3] == a && visits[m - 2] == b && visits[m - 1] == a {
                        adj[a].retain(|&q| q != b);
                        adj[b].retain(|&q| q != a);
                        visits.clear();
                        visits.push(on);
                        committed = None;
                    }
                }
                path = bfs_route(&adj, on, goal);
            }
            let next = path
                .iter()
                .position(|&p| p == on)
                .and_then(|i| path.get(i + 1))
                .copied();
            let edge = next.map(|n| edge_kind(&lvl.platforms[on], &lvl.platforms[n]));
            if self.frames - start >= MAX_FRAMES_PER_GOAL && std::env::var_os("BOT_TRACE").is_some() {
                for line in trace.iter().skip(40) {
                    eprintln!("{line}");
                }
            }
            assert!(
                self.frames - start < MAX_FRAMES_PER_GOAL,
                "bot stuck at {:?} (seed {}, level {}) on platform {on}, next platform {next:?}, edge {edge:?}, heading to platform {goal}: {}",
                self.state.pos,
                lvl.seed,
                lvl.level,
                edge_geometry(lvl, on, next, edge)
            );
            if self.state.pos.y <= world.kill_y {
                if std::env::var_os("BOT_TRACE").is_some() {
                    for line in &trace {
                        eprintln!("{line}");
                    }
                }
                panic!(
                    "bot fell into the void at {:?} (seed {}, level {}) on platform {on}, next platform {next:?}, edge {edge:?}, heading to platform {goal}: {}",
                    self.state.pos,
                    lvl.seed,
                    lvl.level,
                    edge_geometry(lvl, on, next, edge)
                );
            }
            // Where to steer this frame. Everything routes at the next platform on the
            // planned path (or at the goal itself once the route is exhausted).
            let mut approaching = false;
            let mut takeoff_dist = f32::INFINITY;
            let (tx, tz) = match next {
                Some(n) => match edge.expect("edge kind exists whenever next exists") {
                    EdgeKind::Walk => {
                        let cur = &lvl.platforms[on];
                        let nxt = &lvl.platforms[n];
                        // The waypoint after the target on this route (the next platform's
                        // centre, or the final goal); used to prefer the side facing it.
                        let following = match path.iter().position(|&p| p == on).and_then(|i| path.get(i + 2)) {
                            Some(&f) => {
                                let fp = &lvl.platforms[f];
                                (
                                    (fp.bounds.min.x + fp.bounds.max.x) * 0.5,
                                    (fp.bounds.min.z + fp.bounds.max.z) * 0.5,
                                )
                            }
                            None => (gx, gz),
                        };
                        // Touching edge (flat, a drop of any depth incl. long falls, or a
                        // 0.5 u stair step / 1.0 u tread up or down onto a big platform):
                        // steer at a point on the target's top at least half-width + 0.3 u
                        // beyond the current platform's footprint edge in the travel
                        // direction, and keep full input there until grounded on the
                        // target — never stop at the shared edge. Step-ups rely on the
                        // controller's 0.55 u step-up; a touching drop is never jumped.
                        let dir = travel_dir(cur, nxt, following);
                        beyond_edge_point(cur, nxt, dir, PLAYER_HALF_WIDTH + 0.3, 0.3)
                    }
                    EdgeKind::JumpPad => {
                        // Walk straight onto the pad's centre; once launched (airborne)
                        // steer at the landing platform's centre.
                        if self.state.grounded {
                            centre_xz(&lvl.platforms[on])
                        } else {
                            centre_xz(&lvl.platforms[n])
                        }
                    }
                    EdgeKind::Jump => {
                        let cur = &lvl.platforms[on];
                        let nxt = &lvl.platforms[n];
                        if gap_xz(cur, nxt) <= 0.001 {
                            // Touching/overlapping footprints at a rise the controller
                            // cannot step (a high slab sitting on the current platform):
                            // no run-up exists, so stand at the closest point of the
                            // shared boundary and hop from there.
                            closest_shared_point(cur, nxt, self.state.pos.x, self.state.pos.z)
                        } else {
                            // Run-up line: from the current platform's interior (its centre)
                            // through the takeoff point on its boundary, toward the landing
                            // target's centre.
                            let centre = centre_xz(nxt);
                            let (cx, cz) = centre_xz(cur);
                            let mut ux = centre.0 - cx;
                            let mut uz = centre.1 - cz;
                            let len = (ux * ux + uz * uz).sqrt();
                            if len <= 1e-6 {
                                let (dx, dz) = travel_dir(cur, nxt, (gx, gz));
                                ux = dx;
                                uz = dz;
                            } else {
                                ux /= len;
                                uz /= len;
                            }
                            let t = ray_exit_t(cur, cx, cz, ux, uz);
                            let takeoff = (cx + ux * t, cz + uz * t);
                            let to_takeoff = ((takeoff.0 - self.state.pos.x).powi(2)
                                + (takeoff.1 - self.state.pos.z).powi(2))
                            .sqrt();
                            takeoff_dist = to_takeoff;
                            if committed != Some((on, n)) && to_takeoff <= 0.6 {
                                committed = Some((on, n));
                            }
                            if self.state.grounded && committed != Some((on, n)) {
                                // Run up along the line: steer at a point on the line from
                                // the current platform's interior through the takeoff point,
                                // just ahead of the bot (clamped to the segment ending at
                                // the takeoff point). The bot is lined up and runs straight
                                // through the edge square-on to the target's centre line.
                                approaching = true;
                                let proj = (self.state.pos.x - cx) * ux + (self.state.pos.z - cz) * uz;
                                let ahead = (proj + 1.0).clamp(0.0, t);
                                (cx + ux * ahead, cz + uz * ahead)
                            } else {
                                // Committed at the edge, or airborne: steer at the target's
                                // centre. Steering is released only once grounded on the
                                // target (the route advances on standing_on).
                                centre
                            }
                        }
                    }
                },
                None => (gx, gz),
            };
            let mut jump = false;
            // Like a player, the bot may use coyote time right after walking off an edge.
            if (self.state.grounded || self.state.coyote_frames > 0)
                && !self.holding_jump
                && matches!(edge, Some(EdgeKind::Jump))
                && let Some(n) = next
            {
                let cur = &lvl.platforms[on];
                let nxt = &lvl.platforms[n];
                // In the last 0.3 u before the takeoff edge, or about to leave the current
                // top, or the next one is higher and close, or blocked by a wall.
                let ahead = self.input_peek(tx, tz, 0.55);
                let leaving = !inside_xz(cur, ahead.0, ahead.1, -0.3);
                let at_takeoff = takeoff_dist <= 0.3 + 0.001;
                let higher_close = nxt.bounds.max.y > cur.bounds.max.y + 0.2
                    && gap_xz(cur, nxt) < 1.5
                    && {
                        let (ex, ez) = closest_inside(nxt, self.state.pos.x, self.state.pos.z, 0.0);
                        ((ex - self.state.pos.x).powi(2) + (ez - self.state.pos.z).powi(2)).sqrt() < 1.0
                    }
                    // Only once at the takeoff (or committed to it), or with no gap to
                    // clear: never hop from too far out onto a higher platform.
                    && (gap_xz(cur, nxt) <= 0.001 || committed == Some((on, n)) || takeoff_dist <= 0.75);
                let blocked = self.stall >= 3;
                jump = (leaving && !approaching) || at_takeoff || higher_close || blocked;
            }
            if jump {
                self.holding_jump = true;
            } else if self.holding_jump && self.state.vel.y <= 0.0 {
                // Full-height jump: hold BTN_JUMP through the whole rise, release on the way down.
                self.holding_jump = false;
            }
            trace.push_back(format!(
                "f{} on={on} next={next:?} edge={edge:?} pos=({:.2},{:.2},{:.2}) vel=({:.1},{:.1},{:.1}) g={} coy={} tgt=({tx:.2},{tz:.2}) jump={} hold={}",
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
        state: spawn_state(lvl.spawn),
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
    let b = lvl.beacon;
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

/// Best-effort extraction of a panic payload's message (for reporting; never used for control flow).
fn panic_message(payload: Box<dyn std::any::Any + Send>) -> String {
    if let Some(s) = payload.downcast_ref::<&str>() {
        (*s).to_owned()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "unknown panic".to_string()
    }
}

/// Diagnostic only, NOT a gate: generated maps are pinned by edge feasibility in
/// `every_graph_edge_kind_is_physically_doable`. Kept for `cargo test -- --ignored`:
/// runs the route bot on 10 seeds x levels 1..=6 and prints every failure with the
/// seed, the level, the current platform, the next platform, the edge kind and the edge's
/// geometry (all harvested from the panicking leg), so a real map defect can be reported,
/// not papered over.
#[test]
#[ignore]
fn generated_levels_are_completable_with_the_real_controller() {
    let mut played = 0;
    let mut failures: Vec<String> = Vec::new();
    for level in 1..=6u32 {
        for seed in 0..10u32 {
            let actual_seed = seed.wrapping_mul(2_654_435_761);
            let lvl = levelgen::generate(actual_seed, level).data;
            let goals = pick_seeded_gifts(&lvl, actual_seed, 8);
            played += 1;
            let result = std::panic::catch_unwind(|| play_subset(&lvl, Some(&goals)));
            if let Err(payload) = result {
                failures.push(format!(
                    "seed {seed} (levelgen seed {actual_seed}), level {level}: {}",
                    panic_message(payload)
                ));
            }
        }
    }
    println!("generated levels played: {played}, failed: {}", failures.len());
    assert!(
        failures.is_empty(),
        "bot could not complete generated levels (seed, level, current platform, next platform, edge kind and geometry):\n{}",
        failures.join("\n")
    );
}

// ---------------------------------------------------------------------------
// Edge feasibility: prove that every KIND of edge `platform_graph` admits is
// physically doable with the real controller, instead of routing whole maps.
// ---------------------------------------------------------------------------

/// Quantized bucket of a jump/pad edge: gap rounded UP to 0.25 u, rise rounded
/// to the nearest 0.25 u, the landing's smaller horizontal extent rounded DOWN
/// to 0.5 u, and whether the launcher is a jump pad. Ordered, so the printed
/// table is deterministic.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct LandingBucket {
    gap_q: i64,
    rise_q: i64,
    width_q: i64,
    pad: bool,
}

impl LandingBucket {
    fn quantize(gap: f32, rise: f32, landing_smaller_extent: f32, pad: bool) -> Self {
        // Small epsilons keep exact quarter/half-u values from landing on the
        // wrong side of a float boundary.
        let gap_q = ((gap / 0.25) - 1e-3).ceil().max(0.0) as i64;
        let rise_q = (rise / 0.25).round() as i64;
        let width_q = ((landing_smaller_extent / 0.5) - 1e-3).floor().max(0.0) as i64;
        Self {
            gap_q,
            rise_q,
            width_q,
            pad,
        }
    }

    fn gap(&self) -> f32 {
        self.gap_q as f32 * 0.25
    }

    fn rise(&self) -> f32 {
        self.rise_q as f32 * 0.25
    }

    fn width(&self) -> f32 {
        self.width_q as f32 * 0.5
    }
}

/// Outcome of simulating one bucket or representative case.
struct EdgeSim {
    ok: bool,
    /// Closest horizontal distance from the feet to the goal footprint (0 when over it),
    /// tracked only while at or below goal-top + 0.5 u (above that we are flying over it).
    closest: f32,
}

/// Whether the simulated run may jump (a plain jump), is launched by the pad it
/// stands on (never presses BTN_JUMP), or never jumps at all (walk/step/drop).
#[derive(Clone, Copy, PartialEq, Eq)]
enum SimMode {
    Walk,
    Jump,
    Pad,
}

/// A non-pad platform from a generated level, used as a clone-source for the test
/// platforms: bounds and (when required) kind are overwritten, every other field is
/// irrelevant to movement. Never constructed from scratch, so no assumption is made
/// about the full protocol shape of `Platform`.
fn platform_proto() -> Platform {
    let base = levelgen::generate(0, 1).data;
    base.platforms
        .iter()
        .find(|p| p.kind != PLATFORM_JUMP_PAD)
        .or_else(|| base.platforms.first())
        .cloned()
        .expect("a generated level has at least one platform")
}

/// Clone of `proto` with its bounds replaced, kind untouched.
fn rect_platform(proto: &Platform, min_x: f32, max_x: f32, min_y: f32, max_y: f32, min_z: f32, max_z: f32) -> Platform {
    let mut p = proto.clone();
    p.bounds.min.x = min_x;
    p.bounds.max.x = max_x;
    p.bounds.min.y = min_y;
    p.bounds.max.y = max_y;
    p.bounds.min.z = min_z;
    p.bounds.max.z = max_z;
    p
}

/// Builds a LevelData containing exactly `platforms`, gifts emptied, with the player
/// spawn moved onto platform A at (sx, sy, sz). The level is a donor clone of a
/// generated level: platforms and gifts are overwritten, the spawn fields are moved —
/// every other field is copied, never constructed. `kill_y` is set to the lowest
/// platform top in the world minus 15 u, so a player falling toward a deep-drop
/// landing (or riding out a high pad arc) is never killed before the height of B;
/// the donor's own kill_y is arbitrary and may sit above B.
fn tiny_level(platforms: Vec<Platform>, sx: f32, sy: f32, sz: f32) -> LevelData {
    let mut lvl = levelgen::generate(0, 1).data;
    let min_top = platforms.iter().map(|p| p.bounds.max.y).fold(f32::INFINITY, f32::min);
    lvl.platforms = platforms;
    lvl.gifts.clear();
    lvl.spawn.x = sx;
    lvl.spawn.y = sy;
    lvl.spawn.z = sz;
    lvl.kill_y = min_top - 15.0;
    lvl
}

/// Builds the two-platform world for one bucket: A is a 12 u long, 4 u wide run-up
/// along +x with its top at y = 0 (a 3x3 PLATFORM_JUMP_PAD for pad buckets, i.e. A is
/// the pad itself), B sits edge-to-edge `bucket.gap` beyond A's (the pad's) edge, its
/// top at `bucket.rise`, its depth 3 u along the jump direction and its width
/// `bucket.width` across it. The bucket geometry is therefore measured from the pad's
/// edge for pad buckets.
fn bucket_platforms(bucket: LandingBucket) -> Vec<Platform> {
    let proto = platform_proto();
    let (gap, rise, width) = (bucket.gap(), bucket.rise(), bucket.width());
    let a_min_x = if bucket.pad { -3.0 } else { -12.0 };
    let a_half_z = if bucket.pad { 1.5 } else { 2.0 };
    let mut a = rect_platform(&proto, a_min_x, 0.0, -1.0, 0.0, -a_half_z, a_half_z);
    if bucket.pad {
        a.kind = PLATFORM_JUMP_PAD;
    }
    let b = rect_platform(&proto, gap, gap + 3.0, rise - 1.0, rise, -width * 0.5, width * 0.5);
    vec![a, b]
}

/// Runs the real controller from the far end of A straight at B's centre.
/// Full input the whole time; steering is always toward B's centre. In `Jump`
/// mode BTN_JUMP is sent as a fresh rising-edge press — the controller jumps on
/// a rising edge / jump buffer, so the frame before the press never holds
/// BTN_JUMP — timed so the player is still grounded (or within coyote frames)
/// at A's edge:
/// - gap 0 with a B above step height (> 0.55 u): take off ~1.4 u before A's
///   edge and keep running, so the ascent happens before B's face is reached
///   (the front may still clip B's face while rising; the controller unblocks
///   the horizontal move once the feet clear B's top, and the player then
///   rides up onto B);
/// - otherwise: take off in the last ~0.3 u before the edge;
/// - if the player has already left the edge (coyote backstop), hop on the
///   first coyote frame still holding the press.
///
/// The press is held through the whole rise and released only once the jump has
/// left the ground and `vel.y <= 0.0`, so the variable-jump rule never cuts the
/// ascent. `jump_launched` exists so the release check can never swallow the
/// press frame itself (where `vel.y` is still 0 before the step runs).
/// `Pad` mode never presses BTN_JUMP: the player is spawned standing on the pad's
/// centre, the pad launches on contact/landing (vel.y = PAD_LAUNCH_VEL, horizontal
/// velocity x0.6, once per contact), and the player then steers at B's centre while
/// airborne with full input — never a jump press.
/// `goal_top` / `goal` describe the landing platform for the success check
/// (grounded on its top) and the closest-approach metric. The tiny world's kill_y
/// sits below every platform (see `tiny_level`), so a falling player reaches B
/// before hitting the kill plane. 180 frames = 3 s.
fn simulate_controller(
    platforms: Vec<Platform>,
    spawn: (f32, f32, f32),
    target: (f32, f32),
    mode: SimMode,
    goal_top: f32,
    goal: (f32, f32, f32, f32), // min_x, max_x, min_z, max_z
) -> EdgeSim {
    // A's jump edge is always at x = 0 in the constructed worlds.
    const A_EDGE_X: f32 = 0.0;
    // Mirrors sim::tuning::STEP_HEIGHT (max ledge the controller steps up).
    const STEP_HEIGHT: f32 = 0.55;
    // Gap 0 with a higher B: take off ~1.4 u before A's edge (inside the
    // ~0.5-1.5 u window) and keep running, so the ascent starts before B's face.
    const FLUSH_TAKEOFF_X: f32 = A_EDGE_X - 1.4;
    // General case: take off in the last ~0.3 u still grounded on A.
    const EDGE_TAKEOFF_X: f32 = A_EDGE_X - 0.3;
    let lvl = tiny_level(platforms, spawn.0, spawn.1, spawn.2);
    let world = World::from_level(&lvl);
    let mut state = spawn_state(lvl.spawn);
    let mut seq = 0u32;
    let mut frames = 0u32;
    let mut holding_jump = false;
    let mut jumped = false;
    // True once the pressed jump has actually left the ground (vel.y > 0 seen
    // after the press). The release check below only fires once this is set, so
    // the press frame — where vel.y is still 0 — can never release the hold
    // before the press has been sent.
    let mut jump_launched = false;
    let mut closest = f32::INFINITY;
    while frames < 180 {
        frames += 1;
        seq += 1;
        let dx = target.0 - state.pos.x;
        let dz = target.1 - state.pos.z;
        let d = (dx * dx + dz * dz).sqrt();
        // Full input straight at the goal's centre.
        let (mx, mz) = if d < 0.05 {
            (0, 0)
        } else {
            ((dx / d * 127.0) as i8, (dz / d * 127.0) as i8)
        };
        // Closest approach of the feet to the goal footprint; only meaningful at/below
        // goal-top + 0.5 u (higher up, we are still flying over it).
        let clamp_x = state.pos.x.clamp(goal.0, goal.1);
        let clamp_z = state.pos.z.clamp(goal.2, goal.3);
        if state.pos.y <= goal_top + 0.5 {
            let dist = ((state.pos.x - clamp_x).powi(2) + (state.pos.z - clamp_z).powi(2)).sqrt();
            closest = closest.min(dist);
        }
        // Release BTN_JUMP on the way down, never before the jump has launched.
        // Runs BEFORE the press decision below: the frame of the press itself
        // still has vel.y == 0, so a hold started this frame survives the check,
        // and the previous frame never held BTN_JUMP — the press is a fresh
        // rising edge (never a re-arm of an already-held button).
        if holding_jump && state.vel.y > 0.0 {
            jump_launched = true;
        }
        if holding_jump && jump_launched && state.vel.y <= 0.0 {
            // Release on the way down; BTN_JUMP was held for the full rise.
            holding_jump = false;
        }
        // Takeoff. Gap 0 with a flush B above step height: jump ~1.4 u before
        // the edge and keep running. Otherwise: jump in the last ~0.3 u still
        // grounded on A; coyote frames are a backstop if the edge was walked
        // off first.
        if mode == SimMode::Jump && !jumped && !holding_jump {
            let flush_high = goal.0 <= 0.001 && goal_top > STEP_HEIGHT;
            let takeoff_x = if flush_high { FLUSH_TAKEOFF_X } else { EDGE_TAKEOFF_X };
            let at_takeoff = state.grounded && state.pos.x >= takeoff_x;
            let coyote_hop = !state.grounded && state.coyote_frames > 0;
            if at_takeoff || coyote_hop {
                holding_jump = true;
                jump_launched = false;
                jumped = true;
            }
        }
        let input = InputFrame {
            seq,
            move_x: mx,
            move_z: mz,
            buttons: if holding_jump { BTN_JUMP } else { 0 },
        };
        step(&mut state, &input, &world);
        let in_goal = state.pos.x >= goal.0 - 0.4
            && state.pos.x <= goal.1 + 0.4
            && state.pos.z >= goal.2 - 0.4
            && state.pos.z <= goal.3 + 0.4;
        if state.grounded && (state.pos.y - goal_top).abs() < 1e-3 && in_goal {
            return EdgeSim { ok: true, closest: 0.0 };
        }
        if state.pos.y <= world.kill_y {
            break;
        }
    }
    EdgeSim { ok: false, closest }
}

/// Prints one representative (non-bucket) case and records it as a failure when it fails.
fn report_case(label: &str, out: &EdgeSim, failures: &mut Vec<String>) {
    println!("case {label}: ok={} closest={:.2} u", out.ok, out.closest);
    if !out.ok {
        failures.push(format!(
            "representative case {label}: closest landing {:.2} u",
            out.closest
        ));
    }
}

/// Every KIND of edge the platform graph admits must be physically doable with the
/// real controller. For 10 seeds x levels 1..=10 this walks every directed edge of
/// `levelgen::platform_graph`, classifies it as walk / step-up / drop / jump / pad,
/// buckets the jump and pad edges by (gap rounded up to 0.25 u, rise rounded to 0.25 u,
/// landing smaller extent rounded down to 0.5 u, launcher is pad) and simulates each
/// bucket in a two-platform world (full input straight at B's centre — steering is
/// always toward B's centre; for pad buckets the player starts standing on the pad's
/// centre and rides the contact launch, for jump buckets it starts at the far end of A;
/// BTN_JUMP is sent as a fresh rising-edge press, held through the full rise and
/// released on the way down, timed so the player is grounded or within coyote frames
/// at A's edge — ~1.4 u before the edge for a flush B above step height, else the
/// last ~0.3 u; pad buckets never press BTN_JUMP; success = grounded on B's top
/// within 3 s). Representative 0.5 u step-up, 4-step (0.5 u rise, 1 u tread) stair and
/// touching -6.0 u drop cases are simulated the same way, plus three hand cases that
/// fix the physics by construction: rise +1.0 gap 0.0 (jump ~1.4 u before the face and
/// keep running), rise +2.0 gap 2.0 (held running jump at the edge; ride up B's face
/// once the feet clear its top) and rise -4.0 gap 7.0 (a running jump lands ~1.0 s
/// later, ~9 u out). Prints a per-bucket table (count, gap, rise, width, pad, ok); on
/// failure prints the failing buckets with the closest landing distance. Fails if any
/// bucket or representative case cannot be performed.
#[test]
fn every_graph_edge_kind_is_physically_doable() {
    let mut counts: BTreeMap<LandingBucket, u32> = BTreeMap::new();
    let mut walk_n = 0u32;
    let mut step_up_n = 0u32;
    let mut drop_n = 0u32;
    for level_n in 1..=10u32 {
        for seed_i in 0..10u32 {
            let seed = seed_i.wrapping_mul(2_654_435_761);
            let lvl = levelgen::generate(seed, level_n).data;
            let adj = levelgen::platform_graph(&lvl);
            for (from_i, edges) in adj.iter().enumerate() {
                for &to_i in edges {
                    let from = &lvl.platforms[from_i];
                    let to = &lvl.platforms[to_i];
                    let rise = levelgen::edge_rise(from, to);
                    let gap = gap_xz(from, to);
                    // Touching and within step height: a plain step; touching and lower:
                    // a drop; touching and otherwise level: a walk. (Mirrored from
                    // `edge_kind`: a touching edge is never jumped.)
                    if gap <= 0.001 && rise <= 0.55 + 0.001 {
                        if rise > 0.001 {
                            step_up_n += 1;
                        } else if rise < -0.001 {
                            drop_n += 1;
                        } else {
                            walk_n += 1;
                        }
                        continue;
                    }
                    let pad = from.kind == PLATFORM_JUMP_PAD && rise <= 4.0 + 0.001 && gap <= 5.0 + 0.001;
                    let smaller_extent = (to.bounds.max.x - to.bounds.min.x).min(to.bounds.max.z - to.bounds.min.z);
                    *counts
                        .entry(LandingBucket::quantize(gap, rise, smaller_extent, pad))
                        .or_insert(0) += 1;
                }
            }
        }
    }
    println!(
        "edge kinds walked: walk={walk_n} step_up={step_up_n} drop={drop_n} jump/pad buckets={}",
        counts.len()
    );
    println!(
        "{:>6} {:>6} {:>7} {:>6} {:>4} {:>4}",
        "count", "gap", "rise", "width", "pad", "ok"
    );

    let mut failures: Vec<String> = Vec::new();

    for (bucket, &count) in &counts {
        let gap = bucket.gap();
        let rise = bucket.rise();
        let width = bucket.width();
        // Pad buckets: spawn standing on the pad's centre (A = the 3 u pad, its
        // centre at x = -1.5, top at y = 0) and let the contact launch do the
        // work — the pad launches on landing/contact with vel.y = PAD_LAUNCH_VEL
        // (horizontal velocity x0.6, once per contact). Do not jump; while
        // airborne the player steers at B's centre with full input, exactly like
        // a jump bucket but without ever pressing BTN_JUMP.
        let (spawn, mode) = if bucket.pad {
            ((-1.5, 0.0, 0.0), SimMode::Pad)
        } else {
            ((-11.0, 1.0, 0.0), SimMode::Jump)
        };
        let out = simulate_controller(
            bucket_platforms(*bucket),
            spawn,
            (gap + 1.5, 0.0),
            mode,
            rise,
            (gap, gap + 3.0, -width * 0.5, width * 0.5),
        );
        println!(
            "{:>6} {:>6.2} {:>+7.2} {:>6.1} {:>4} {:>4}",
            count,
            gap,
            rise,
            width,
            if bucket.pad { "pad" } else { "-" },
            if out.ok { "ok" } else { "FAIL" }
        );
        if !out.ok {
            failures.push(format!(
                "gap {gap:.2} rise {rise:+.2} width {width:.1} pad {} ({count} graph edges): closest landing {:.2} u",
                bucket.pad, out.closest
            ));
        }
    }

    let proto = platform_proto();
    // Representative non-jump edges.
    // 1) One 0.5 u step-up onto a 4 x 4 platform (walked, never jumped).
    {
        let a = rect_platform(&proto, -12.0, 0.0, -1.0, 0.0, -2.0, 2.0);
        let b = rect_platform(&proto, 0.0, 4.0, -0.5, 0.5, -2.0, 2.0);
        let out = simulate_controller(
            vec![a, b],
            (-11.0, 1.0, 0.0),
            (2.0, 0.0),
            SimMode::Walk,
            0.5,
            (0.0, 4.0, -2.0, 2.0),
        );
        report_case("step-up-0.5", &out, &mut failures);
    }
    // 2) Stair: 4 risers of 0.5 u with 1.0 u treads (walked, never jumped).
    {
        let a = rect_platform(&proto, -12.0, 0.0, -1.0, 0.0, -2.0, 2.0);
        let mut plats = vec![a];
        for k in 0..4u32 {
            let top = (k as f32 + 1.0) * 0.5;
            let min_x = k as f32 * 1.0;
            plats.push(rect_platform(&proto, min_x, min_x + 1.0, top - 1.0, top, -2.0, 2.0));
        }
        let out = simulate_controller(
            plats,
            (-11.0, 1.0, 0.0),
            (3.5, 0.0),
            SimMode::Walk,
            2.0,
            (3.0, 4.0, -2.0, 2.0),
        );
        report_case("stair 4x(0.5 up, 1.0 tread)", &out, &mut failures);
    }
    // 3) Touching -6.0 u drop (walked off the edge, never jumped).
    {
        let a = rect_platform(&proto, -12.0, 0.0, -1.0, 0.0, -2.0, 2.0);
        let b = rect_platform(&proto, 0.0, 4.0, -7.0, -6.0, -2.0, 2.0);
        let out = simulate_controller(
            vec![a, b],
            (-11.0, 1.0, 0.0),
            (2.0, 0.0),
            SimMode::Walk,
            -6.0,
            (0.0, 4.0, -2.0, 2.0),
        );
        report_case("drop -6.0 off the edge", &out, &mut failures);
    }
    // Hand cases (Jump) with takeoffs and arcs verified against the tuning
    // constants (DT = 1/60, GRAVITY = 32, JUMP_VELOCITY = 12, RUN_SPEED = 9).
    // 4) Rise +1.0, gap 0.0 (B flush against A): the jump starts ~1.4 u before
    //    A's edge; by the time the box reaches B's face the feet are above its
    //    top and the player lands on B.
    {
        let a = rect_platform(&proto, -12.0, 0.0, -1.0, 0.0, -2.0, 2.0);
        let b = rect_platform(&proto, 0.0, 3.0, 0.0, 1.0, -2.0, 2.0);
        let out = simulate_controller(
            vec![a, b],
            (-11.0, 1.0, 0.0),
            (1.5, 0.0),
            SimMode::Jump,
            1.0,
            (0.0, 3.0, -2.0, 2.0),
        );
        report_case("rise +1.0 gap 0.0", &out, &mut failures);
    }
    // 5) Rise +2.0, gap 2.0: the running jump leaves the edge with the boost of
    //    full height; even if the box meets B's face below its top, the
    //    controller unblocks once the feet rise above it, and the player rides
    //    up onto B's top (apex ~2.15 u > 2.0 u).
    {
        let a = rect_platform(&proto, -12.0, 0.0, -1.0, 0.0, -2.0, 2.0);
        let b = rect_platform(&proto, 2.0, 5.0, 1.0, 2.0, -2.0, 2.0);
        let out = simulate_controller(
            vec![a, b],
            (-11.0, 1.0, 0.0),
            (3.5, 0.0),
            SimMode::Jump,
            2.0,
            (2.0, 5.0, -2.0, 2.0),
        );
        report_case("rise +2.0 gap 2.0", &out, &mut failures);
    }
    // 6) Rise -4.0, gap 7.0: a jump that lands 4 u lower stays airborne
    //    (12 + sqrt(144 + 2*32*4)) / 32 = 1.0 s, covering ~9 u at 9 u/s.
    {
        let a = rect_platform(&proto, -12.0, 0.0, -1.0, 0.0, -2.0, 2.0);
        let b = rect_platform(&proto, 7.0, 10.0, -5.0, -4.0, -2.0, 2.0);
        let out = simulate_controller(
            vec![a, b],
            (-11.0, 1.0, 0.0),
            (8.5, 0.0),
            SimMode::Jump,
            -4.0,
            (7.0, 10.0, -2.0, 2.0),
        );
        report_case("rise -4.0 gap 7.0", &out, &mut failures);
    }

    assert!(
        failures.is_empty(),
        "graph admits edges the real controller cannot perform (gap, rise, width, pad):\n{}",
        failures.join("\n")
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
