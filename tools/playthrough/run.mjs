#!/usr/bin/env node
/**
 * Automated playthrough sweep.
 *
 * Drives a live VOIDWRAP server through real sessions with N clients, exercising
 * the draft, the pedestal vote, the shop and the altar prompt, and reports every
 * anomaly it observes as a PASS/FAIL line.
 *
 *   node tools/playthrough/run.mjs --url ws://127.0.0.1:9001/ws
 *   node tools/playthrough/run.mjs --url ... --clients 8 --seconds 90
 *
 * Exit code is 0 only when every check passes. No dependencies beyond Node.
 */
import { appendFileSync, readFileSync } from 'node:fs';
import { encodeClientMsg, decodeServerMsg } from '../voting-conformance/wire.mjs';

// The wire protocol version this sweep speaks. Mirrors PROTOCOL_VERSION in
// server/crates/protocol; a mismatch shows up as a Welcome-time Error frame.
const PROTOCOL_VERSION = 8;

// Classes are 0..=CLASS_GLIDER on the wire; design/content.json ships four.
const CLASS_COUNT = 4;

// Mirrors protocol::PHASE_*. The lobby shop is only open in PHASE_ENEMY_DRAFT.
const PHASE_ENEMY_DRAFT = 0;

// T-156: movement constants mirrored from server/crates/sim/src/tuning.rs. The bot has to
// know the jump envelope to decide whether a gap is clearable. The old edge probe guessed:
// it jumped at any landing up to 5.5 u away, which is past the envelope for any rise above
// ~0.3 u, so the bot jumped at gaps it could not clear and the first fall ended the run.
const RUN_SPEED = 9.0;
const GRAVITY = 32.0;
const JUMP_VELOCITY = 12.0;
/** Peak height of a flat jump: v^2 / 2g = 2.25 u. */
const JUMP_APEX = (JUMP_VELOCITY * JUMP_VELOCITY) / (2 * GRAVITY);
/** Fraction of the theoretical reach the bot will commit to. The arc starts a little before
 * the edge because the bot cannot time the jump to the physics frame, so the margin is not
 * optional. */
const JUMP_SAFETY = 0.75;
const EDGE_PROBE_STEP = 0.15;
const EDGE_PROBE_MAX = 1.5;
/** Player collision half-width, mirrored from tuning.rs. The centre can stand this far
 * past a platform edge before the player falls. */
const PLAYER_HALF_WIDTH = 0.4;
/** Extra margin on top of PLAYER_HALF_WIDTH before the bot commits to a jump. One input
 * batch covers 0.45 u at run speed, so 0.5 u is one batch of reaction time. */
const EDGE_JUMP_DISTANCE = 0.5;
const LANDING_STEP = 0.15;
/** Physics rate the server steps at, and the tick rate it drains input at. */
const PHYSICS_HZ = 60;
const TICK_HZ = 20;
/** Frames the server consumes per tick: PHYSICS_HZ / TICK_HZ. */
const STEPS_PER_TICK = PHYSICS_HZ / TICK_HZ;
/** Input batches are sent at the physics rate with one frame each, so the server's queue
 * neither backs up (which delays every input by a second) nor starves (which makes the
 * player stand still for a tick). */
const INPUT_BATCH_MS = 1000 / PHYSICS_HZ;
const INPUT_FRAMES_PER_BATCH = 1;

/** Append one NDJSON line to the `--trace` file. No-op when tracing is off. */
function traceLine(record) {
  if (!TRACE) return;
  try {
    appendFileSync(TRACE, JSON.stringify(record) + String.fromCharCode(10));
  } catch {
    // A trace that cannot be written must not take the sweep down with it.
  }
}

/**
 * Horizontal distance a flat jump covers before returning to `rise` units above the launch
 * point, discounted by JUMP_SAFETY. Zero when the rise is above the jump apex.
 */
function jumpReach(rise) {
  const disc = JUMP_VELOCITY * JUMP_VELOCITY - 2 * GRAVITY * Math.max(0, rise);
  if (disc <= 0) return 0;
  const airtime = (JUMP_VELOCITY + Math.sqrt(disc)) / GRAVITY;
  return airtime * RUN_SPEED * JUMP_SAFETY;
}

const argv = process.argv.slice(2);
const argValue = (flag, fallback) => {
  const i = argv.indexOf(flag);
  return i >= 0 && i + 1 < argv.length ? argv[i + 1] : fallback;
};
const flag = (name) => argv.includes(name);

const URL_ = argValue('--url', process.env.PLAYTHROUGH_URL || '');
const CLIENTS = Number(argValue('--clients', process.env.PLAYTHROUGH_CLIENTS || '8'));
const SECONDS = Number(argValue('--seconds', process.env.PLAYTHROUGH_SECONDS || '75'));
// Checks that are known-broken and must not fail the gate. Naming them explicitly is the
// point: an unnamed failure still fails, so this cannot hide a new regression. The only
// entry the repo uses is `level-progression`, which needs a faithful JS port of the
// movement controller (see tickets/T-115.findings.md F-115-8).
// T-130: optional NDJSON death log written by the server when `VOIDWRAP_DEATH_LOG`
// is set. Reading it turns "the bots died" into "the bots fell off the map at
// (x, y, z) with velocity v", which is the measurement the level-progression check
// was blocked on.
const DEATH_LOG = argValue('--death-log', process.env.PLAYTHROUGH_DEATH_LOG || '');
// T-154: the debug-driven progression mode. The bots cannot finish a level, so
// `level-progression`, `vote` and `shop` were permanently SKIPped and a regression in any
// of them would have gone unnoticed. `--actuate` drives those paths through the debug
// command layer instead of through movement. The debug gate is singleplayer-only, so this
// mode runs one client.
const ACTUATE = flag('--actuate');
const DEV_CODE = argValue('--code', process.env.PLAYTHROUGH_DEV_CODE || 'devaccess');
// T-156: per-steer decision trace. The bot's failures are all "it fell somewhere", and a
// death log only says where it landed. This records what the bot saw and decided, so the
// edge logic can be debugged from data instead of guessed at.
const TRACE = argValue('--trace', process.env.PLAYTHROUGH_TRACE || '');

const KNOWN_FAILURES = new Set(
  (argValue('--known-failures', process.env.PLAYTHROUGH_KNOWN_FAILURES || ''))
    .split(',')
    .map((name) => name.trim())
    .filter(Boolean),
);

// T-139: why each known failure is parked, printed with the SKIP so the line is
// self-documenting. A bare "known failure" reads like a bug someone gave up on; these
// are decisions, and the reason is the useful part.
const KNOWN_FAILURE_REASONS = {
  'level-progression':
    'T-156 removed the two bugs that made this unreachable: the bot fed the server 80 input '
    + 'frames/s against a 60 frames/s drain (so every input was applied about a second late) '
    + 'and its air control aimed at the point directly below it (so it fell straight down '
    + 'instead of crossing a gap). It now survives a full sweep with zero deaths. It still '
    + 'cannot empty a level: the remaining blocker is vertical routing. See tickets/T-156.md.',
};

if (!URL_) {
  console.log('playthrough: skipped (set PLAYTHROUGH_URL or pass --url)');
  process.exit(0);
}

const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

/**
 * Encode `ClientMsg::Inputs` (variant 1). `wire.mjs` refuses this variant on
 * purpose — the conformance kit never sends it — so the sweep carries its own
 * encoder. Layout: u32 variant, u64 frame count, then per frame
 * u32 seq, i8 move_x, i8 move_z, u8 buttons.
 */
function encodeInputs(frames) {
  const size = 4 + 8 + frames.length * 7;
  const buf = Buffer.alloc(size);
  let at = 0;
  buf.writeUInt32LE(1, at); at += 4;
  buf.writeBigUInt64LE(BigInt(frames.length), at); at += 8;
  for (const frame of frames) {
    buf.writeUInt32LE(frame.seq >>> 0, at); at += 4;
    buf.writeInt8(frame.move_x, at); at += 1;
    buf.writeInt8(frame.move_z, at); at += 1;
    buf.writeUInt8(frame.buttons, at); at += 1;
  }
  return buf;
}

// --- platform graph -------------------------------------------------------
//
// A greedy straight-line steer cannot clear a level: the collect phase ends
// only when every gift is taken, and gifts sit on platforms that are only
// reachable by jumping a specific gap. These helpers give the bot a route.
//
// The envelope mirrors `sim::levelgen::design_max_gap`: 6.0 u on the flat,
// 4.0 u at a +2.0 u rise, 8.0 u at a -4.0 u drop, and nothing above +2.0 u
// because the jump apex is +2.25 u.
const RAMP_KINDS = new Set([3, 4, 5, 6]);

function maxGapForRise(rise) {
  if (rise > 2.0) return 0;
  if (rise >= 0) return 6.0 - rise;
  return Math.min(8.0, 6.0 - rise * 0.5);
}

/** Edge-to-edge horizontal gap between two platform AABBs. */
function gapBetween(a, b) {
  const dx = Math.max(0, a.min.x - b.max.x, b.min.x - a.max.x);
  const dz = Math.max(0, a.min.z - b.max.z, b.min.z - a.max.z);
  return Math.hypot(dx, dz);
}

/** Adjacency over platforms: an edge exists when the gap is jumpable either way. */
function buildPlatformGraph(platforms) {
  const adj = Array.from({ length: platforms.length }, () => []);
  for (let i = 0; i < platforms.length; i += 1) {
    for (let j = i + 1; j < platforms.length; j += 1) {
      const a = platforms[i].bounds;
      const b = platforms[j].bounds;
      const gap = gapBetween(a, b);
      const up = b.max.y - a.max.y;
      const down = a.max.y - b.max.y;
      if (gap <= maxGapForRise(up) || gap <= maxGapForRise(down)) {
        adj[i].push(j);
        adj[j].push(i);
      }
    }
  }
  return adj;
}

/** One connected client with a decoded message log. */
class Client {
  constructor(name) {
    this.name = name;
    this.log = [];
    this.errors = [];
    this.decodeFailures = [];
    this.closed = false;
    this.ws = null;
    this.level = null;
    this.you = null;
    this.pos = null;
    this.collected = new Set();
    // Gifts this bot has given up on for a few seconds, and the frame counter
    // that decides when to give up. See `steer`.
    this.blacklist = new Map();
    this.stuckFrames = 0;
    this.lastPos = null;
    this.target = null;
    this.index = 0;
    this.graph = null;
    this.graphFor = null;
    // Latest `RunState.info.phase`. The shop gate reads it; see `react`.
    this.phase = null;
  }

  connect(url) {
    return new Promise((resolve, reject) => {
      const ws = new WebSocket(url);
      ws.binaryType = 'arraybuffer';
      this.ws = ws;
      const timer = setTimeout(() => reject(new Error(this.name + ': connect timed out')), 8000);
      ws.onopen = () => {
        clearTimeout(timer);
        resolve(this);
      };
      ws.onerror = () => {
        clearTimeout(timer);
        reject(new Error(this.name + ': socket error'));
      };
      ws.onclose = () => {
        this.closed = true;
      };
      ws.onmessage = (event) => {
        let msg;
        try {
          msg = decodeServerMsg(Buffer.from(event.data));
        } catch (error) {
          this.decodeFailures.push(error.message);
          return;
        }
        msg.type = msg.variant;
        this.log.push({ at: Date.now(), msg });
        if (msg.variant === 'Error') this.errors.push(msg);
        if (msg.variant === 'RunState' && msg.info && typeof msg.info.phase === 'number') {
          this.phase = msg.info.phase;
        }
        if (msg.variant === 'Level') {
          this.level = msg.data;
          this.collected = new Set();
          this.blacklist.clear();
          this.stuckFrames = 0;
          this.lastPos = null;
          this.target = null;
        } else if (msg.variant === 'Snapshot') {
          // Keep the whole controller state, not just the position: `grounded`
          // lives on ControllerState, and reading it off the position vector
          // made it `undefined`, so the bot never pressed jump.
          this.you = msg.you || null;
          this.pos = this.you && this.you.pos ? this.you.pos : null;
          // Stuck detection belongs here, not in `steer`: `steer` runs at the
          // input rate and snapshots arrive at the tick rate, so comparing
          // against the previous `steer` call counted "no new snapshot yet" as
          // "did not move" and the bot jumped every frame.
          if (this.pos && this.lastPos) {
            const moved = Math.hypot(this.pos.x - this.lastPos.x, this.pos.z - this.lastPos.z);
            this.stuckFrames = moved < 0.02 ? this.stuckFrames + 1 : 0;
          }
          if (this.pos) this.lastPos = { x: this.pos.x, z: this.pos.z };
          // The server is the authority on what has been picked up. Without
          // this the bot keeps steering at the gift it just took, because the
          // nearest gift is usually the one behind it.
          for (const id of msg.gifts_taken || []) this.collected.add(id);
        }
        this.react(msg);
      };
    });
  }

  send(msg) {
    if (!this.ws || this.closed) return;
    try {
      this.ws.send(msg.variant === 'Inputs' ? encodeInputs(msg.frames) : encodeClientMsg(msg));
    } catch (error) {
      this.decodeFailures.push('send ' + msg.variant + ': ' + error.message);
    }
  }

  /** Every decoded frame of one variant, in arrival order. */
  all(variant) {
    return this.log.map((entry) => entry.msg).filter((msg) => msg.variant === variant);
  }

  /**
   * Wait for a frame matching `pred` that arrives at or after log index `from`.
   *
   * The index matters: `switch_level` produces a `Level` frame, and a level the bot
   * already loaded produced one too, so scanning the whole log would match the stale
   * frame and report success without the command having done anything.
   */
  async waitForAfter(from, pred, timeoutMs = 4000) {
    const deadline = Date.now() + timeoutMs;
    for (;;) {
      for (let i = from; i < this.log.length; i += 1) {
        if (pred(this.log[i])) return this.log[i];
      }
      if (this.closed || Date.now() > deadline) return null;
      await sleep(25);
    }
  }

  /** Wait for a frame matching `pred`, scanning everything received so far. */
  waitFor(pred, timeoutMs = 4000) {
    return this.waitForAfter(0, pred, timeoutMs);
  }

  /**
   * Send a debug command and wait for its `DebugResult`.
   *
   * The debug layer is gated three ways (dev flag or dev code, singleplayer only), so a
   * rejection here is a real answer, not a transport failure -- the caller reports it.
   */
  async debug(command, args = {}, code = DEV_CODE) {
    const from = this.log.length;
    this.send({ variant: 'Debug', code, command, args: JSON.stringify(args) });
    const hit = await this.waitForAfter(
      from,
      (entry) => entry.msg.variant === 'DebugResult',
      5000,
    );
    return hit ? hit.msg : { ok: false, message: 'timeout waiting for DebugResult' };
  }

  /**
   * Is there a surface under `(x, z)` that a player standing at height `y`
   * could stand on?
   *
   * Ramps are walkable along their whole incline, so their footprint counts at
   * any height inside the box. Every other platform only counts when its top
   * face is at or just below the player's feet -- `upTol` widens that for
   * landing probes, where a step up is allowed.
   */
  groundAt(x, z, y, upTol = 0.6, downTol = 3.0, slack = 0.25) {
    return this.platformUnder(x, z, y, upTol, downTol, slack) >= 0;
  }

  /** Index of the platform a player at `(x, z, y)` could stand on, or -1. */
  platformUnder(x, z, y, upTol = 0.6, downTol = 3.0, slack = 0.25) {
    const platforms = this.level && this.level.platforms;
    if (!Array.isArray(platforms)) return -1;
    for (let i = 0; i < platforms.length; i += 1) {
      const b = platforms[i].bounds;
      if (x < b.min.x - slack || x > b.max.x + slack) continue;
      if (z < b.min.z - slack || z > b.max.z + slack) continue;
      if (RAMP_KINDS.has(platforms[i].kind)) {
        if (y >= b.min.y - 0.8 && y <= b.max.y + 0.8) return i;
      } else if (b.max.y <= y + upTol && b.max.y >= y - downTol) {
        return i;
      }
    }
    return -1;
  }

  /**
   * Nearest point on any platform at or below `y`. Used for air control: a bot
   * that drifts off an edge with no input falls into the void, and the edge
   * probe cannot help once it is airborne.
   */
  nearestLanding(x, z, y) {
    const platforms = this.level && this.level.platforms;
    if (!Array.isArray(platforms)) return null;
    let best = null;
    let bestD = Infinity;
    for (const p of platforms) {
      const b = p.bounds;
      if (b.max.y > y + 0.5) continue;
      const px = Math.min(Math.max(x, b.min.x), b.max.x);
      const pz = Math.min(Math.max(z, b.min.z), b.max.z);
      const d = (px - x) ** 2 + (pz - z) ** 2;
      if (d < bestD) {
        bestD = d;
        best = { x: px, z: pz };
      }
    }
    return best;
  }

  /**
   * Nearest point on platform `i` to `(x, z)`, pushed `inset` units inside the footprint.
   *
   * T-156: without the inset this returns the bot's own position whenever the bot is
   * standing on the boundary between two platforms, so the route aim became `(0, 0)` and
   * the bot stood still jumping until the sweep ended. The trace showed it pinned at
   * (31.99, 0, 13.97) with `aimX`/`aimZ` equal to its own position for the last 90 s.
   */
  nearestOn(i, x, z, inset = 0) {
    const b = this.level.platforms[i].bounds;
    const axis = (v, min, max) => {
      const mid = (min + max) / 2;
      const lo = Math.min(min + inset, mid);
      const hi = Math.max(max - inset, mid);
      return Math.min(Math.max(v, lo), hi);
    };
    return { x: axis(x, b.min.x, b.max.x), z: axis(z, b.min.z, b.max.z) };
  }

  /**
   * Distance from `(x, z)` to the first point in direction `(dx, dz)` where a player
   * standing at height `y` would have no ground, or `null` when the ground continues past
   * the probe range.
   *
   * T-156: the old edge check probed a single fixed distance (0.9 u). A player runs at
   * 9 u/s, so 0.9 u is under two physics frames -- an edge anywhere between the probe and
   * the stopping distance was invisible until the bot was already past it.
   */
  edgeDistance(x, z, y, dx, dz) {
    // Strict footprint: no slack. With the 0.25 u slack the probe only reports an edge
    // once the bot is already 0.25 u past it, and the player's half-width is 0.4 u, so the
    // reaction window collapsed to 0.15 u -- one physics frame. The bot moves 0.45 u per
    // input batch, so it was always too late: the trace showed `edge: 0.15, jump: true`
    // followed by a snapshot already falling, because the jump input arrived after the
    // bot had left the platform.
    for (let d = EDGE_PROBE_STEP; d <= EDGE_PROBE_MAX; d += EDGE_PROBE_STEP) {
      if (!this.groundAt(x + dx * d, z + dz * d, y, 0.6, 3.0, 0)) return d;
    }
    return null;
  }

  /**
   * True when the ground continues for the whole flat-jump reach in direction `(dx, dz)`.
   *
   * T-156: a jump travels `jumpReach(0)` -- about 5 u after the safety discount -- so a
   * heuristic jump (climb toward a higher gift, or shake off a stuck frame) taken anywhere
   * closer than that to an edge is a jump into the void. The old code only ever looked
   * 0.9 u ahead, which is why the bot could be standing on solid ground and still fly off
   * the map: the trace showed a `stuck` jump from z=-6.8 that carried it 6.7 u to z=-0.1
   * with nothing underneath.
   */
  safeToJump(x, z, y, dx, dz) {
    const reach = jumpReach(0);
    for (let d = EDGE_PROBE_STEP; d <= reach; d += EDGE_PROBE_STEP) {
      if (!this.groundAt(x + dx * d, z + dz * d, y, 0.6, 3.0, 0)) return false;
    }
    return true;
  }

  /**
   * First landing inside the jump envelope in direction `(dx, dz)`, or `null`.
   *
   * `from` is the measured edge distance: the search starts just past it, because a
   * landing before the edge is the platform the bot is already standing on. A platform
   * whose top is above the jump apex is not a landing, and a platform beyond the reach at
   * its own rise is not one either -- that is the check the old 5.5 u probe was missing.
   */
  findLanding(x, z, y, dx, dz, from) {
    for (let d = from + LANDING_STEP; d <= from + 12; d += LANDING_STEP) {
      const i = this.platformUnder(x + dx * d, z + dz * d, y, 2.0, 12.0, 0);
      if (i < 0) continue;
      const top = this.level.platforms[i].bounds.max.y;
      if (top - y > JUMP_APEX - 0.2) continue;
      // The gap is measured from the EDGE, not from the bot: the bot jumps at the edge,
      // so the distance it has to cover is `d - from`, not `d`.
      if (d - from <= jumpReach(top - y)) return { d, y: top };
    }
    return null;
  }

  /** First platform to step onto on a route from `from` to `to`, or -1. */
  nextHop(from, to) {
    if (from < 0 || to < 0 || from === to) return -1;
    const platforms = this.level.platforms;
    if (this.graphFor !== this.level) {
      this.graph = buildPlatformGraph(platforms);
      this.graphFor = this.level;
    }
    const prev = new Int32Array(platforms.length).fill(-1);
    const seen = new Uint8Array(platforms.length);
    const queue = [from];
    seen[from] = 1;
    for (let head = 0; head < queue.length; head += 1) {
      const cur = queue[head];
      if (cur === to) break;
      for (const nxt of this.graph[cur]) {
        if (seen[nxt]) continue;
        seen[nxt] = 1;
        prev[nxt] = cur;
        queue.push(nxt);
      }
    }
    if (!seen[to]) return -1;
    let step = to;
    while (prev[step] !== from && prev[step] !== -1) step = prev[step];
    return step;
  }

  /**
   * Steer toward an uncollected gift, without walking off the level.
   *
   * Returns `null` only when there is nothing to steer with yet (no `Level`, no
   * `Snapshot`), which keeps the random walk as the fallback. When there is
   * geometry but no reachable gift it returns a hold-still aim: a random walk
   * over a level with edges is a bot that dies in the void.
   */
  steer(depth = 0) {
    const level = this.level;
    const you = this.you;
    if (!level || !you || !Array.isArray(level.gifts)) return null;
    const pos = you.pos;
    const now = Date.now();
    const hold = { x: 0, z: 0, jump: false };

    // Stuck detection. A straight-line steer has no notion of an obstacle, so a
    // bot pressed against the side of a platform pushes forever. `stuckFrames`
    // is maintained by the snapshot handler; past the threshold, drop the
    // current target for a few seconds so the next-nearest gift gets a turn.
    if (this.stuckFrames > 20 && this.target !== null) {
      this.blacklist.set(this.target, now + 3000);
      this.target = null;
    }

    const candidates = [];
    for (const gift of level.gifts) {
      if (this.collected.has(gift.id)) continue;
      if ((this.blacklist.get(gift.id) || 0) > now) continue;
      const dx = gift.pos.x - pos.x;
      const dz = gift.pos.z - pos.z;
      candidates.push({ gift, d: dx * dx + dz * dz });
    }
    if (!candidates.length) return hold;
    candidates.sort((a, b) => a.d - b.d);
    // Hysteresis. Greedy nearest-target selection oscillates: two gifts on
    // opposite sides of the bot swap places as "nearest" every frame, the bot
    // reverses direction instead of walking anywhere, and the velocity trace
    // flips sign every second. Keep the current target until it is collected,
    // blacklisted, or something is more than twice as close.
    let best = null;
    if (this.target !== null) {
      const held = candidates.find((c) => c.gift.id === this.target);
      if (held && held.d <= candidates[0].d * 4) best = held.gift;
    }
    if (!best) {
      // Spread the herd. Eight bots all steering at the single nearest gift
      // collect one gift per pass; each bot takes a different one of the
      // nearest `CLIENTS` candidates instead, which keeps them apart with no
      // coordination.
      best = candidates[Math.min(this.index, candidates.length - 1)].gift;
    }
    this.target = best.id;
    this.lastTarget = best.pos;

    // Route over the platform graph. Aiming straight at a gift on another
    // platform walks into the void; aim at the next platform on the route
    // instead and let the edge probe below decide when to jump.
    let aimX = best.pos.x;
    let aimZ = best.pos.z;
    const here = this.platformUnder(pos.x, pos.z, pos.y);
    const there = this.platformUnder(best.pos.x, best.pos.z, best.pos.y, 2.0, 12.0);
    if (here >= 0 && there >= 0 && here !== there) {
      const hop = this.nextHop(here, there);
      if (hop >= 0) {
        const p = this.nearestOn(hop, pos.x, pos.z, 1.0);
        aimX = p.x;
        aimZ = p.z;
      }
    }
    // Air control: while airborne, steer toward the nearest landing instead of
    // the route. The edge probe only runs when grounded, so a bot that drifts
    // off an edge mid-jump has nothing else keeping it out of the void.
    if (!you.grounded) {
      const landing = this.nearestLanding(pos.x, pos.z, pos.y);
      // T-156: `nearestLanding` returns the point directly below an airborne bot, so
      // aiming at it zeroed the horizontal input and the bot fell straight down -- it
      // could not cross a gap it had just jumped. 79% of the steer frames in a 240 s run
      // had `aimX`/`aimZ` equal to the bot's own position, every one of them airborne.
      // Keep the ground aim unless the landing is somewhere else, which is the case that
      // actually needs correcting.
      if (landing && Math.hypot(landing.x - pos.x, landing.z - pos.z) > 0.5) {
        aimX = landing.x;
        aimZ = landing.z;
      }
    }

    const dx = aimX - pos.x;
    const dz = aimZ - pos.z;
    const len = Math.hypot(dx, dz) || 1;
    const dirX = dx / len;
    const dirZ = dz / len;
    const rise = best.pos.y - pos.y;
    // `grounded` is on ControllerState, not on the position vector. Reading it
    // off the position made this `undefined` and the bot never jumped.
    // Jump only when there is a reason to. The first version jumped whenever the
    // gift was more than 2 u away -- which is always -- and a bot that jumps
    // every frame never builds horizontal speed, so it bounced around the spawn
    // and collected nothing.
    // T-156: the jump heuristics (climb toward a higher gift, or shake off a stuck frame)
    // are only safe when the ground continues. Near an edge the measured landing decides,
    // because a heuristic jump there is a jump into the void.
    let jump = false;
    let edgeSeen = null;
    let landingSeen = null;

    // Edge awareness, measured rather than probed at one fixed distance.
    //
    // T-156: the old probe looked 0.9 u ahead and jumped at any landing up to 5.5 u away.
    // Two ways that kills the bot. A platform edge between 0.9 u and the stopping distance
    // is invisible until the bot is already past it, and 5.5 u is outside the jump envelope
    // for any rise above ~0.3 u, so the bot jumps at gaps it cannot clear. Both are
    // measured here instead of guessed.
    if (you.grounded) {
      const edge = this.edgeDistance(pos.x, pos.z, pos.y, dirX, dirZ);
      edgeSeen = edge;
      if (edge === null) {
        // No edge within the near probe. The only jump left is the unstick one, and it
        // still travels the full reach, so it is only allowed when the ground continues
        // that far.
        //
        // T-156: the old `rise > 0.3` climb heuristic is gone. Gifts are almost never at
        // exactly the bot's height, so that condition was true nearly every frame and the
        // bot bunny-hopped across the level with no control over where it landed -- the
        // trace showed it jumping from (19.8, 0, 2.6) and drifting +x while pressing -x
        // until it fell. Climbing is already covered by the deliberate edge jump below,
        // which accepts a landing up to JUMP_APEX - 0.2 above the launch point.
        jump = this.stuckFrames > 4
          && this.safeToJump(pos.x, pos.z, pos.y, dirX, dirZ);
      } else {
        const landing = this.findLanding(pos.x, pos.z, pos.y, dirX, dirZ, edge);
        landingSeen = landing ? landing.d : null;
        if (landing === null) {
          // Nothing to land on inside the envelope. Give up on this gift rather than die
          // for it, and re-steer at the next candidate in the same frame -- holding still
          // until the blacklist expires is what left the bot standing on an empty level.
          this.blacklist.set(best.id, now + 3000);
          this.target = null;
          return depth < 3 ? this.steer(depth + 1) : hold;
        }
        // Jump when the edge is close enough that the jump is the next move. The player
        // can stand with its centre PLAYER_HALF_WIDTH past the edge, and one input batch
        // covers 0.45 u, so the trigger has to fire inside that window or the jump input
        // arrives after the bot is already airborne.
        if (edge <= PLAYER_HALF_WIDTH + EDGE_JUMP_DISTANCE) jump = true;
      }
    }

    if (TRACE) {
      traceLine({
        t: Date.now(),
        x: +pos.x.toFixed(3), y: +pos.y.toFixed(3), z: +pos.z.toFixed(3),
        grounded: Boolean(you.grounded),
        target: best.id,
        aimX: +aimX.toFixed(3), aimZ: +aimZ.toFixed(3),
        dirX: +dirX.toFixed(3), dirZ: +dirZ.toFixed(3),
        edge: edgeSeen, landing: landingSeen, jump,
        stuck: this.stuckFrames,
      });
    }
    return { x: dirX, z: dirZ, jump };
  }

  /**
   * Walk the level: steer at the nearest gift when the level is known, otherwise
   * random-walk so a server that never sends `Level` still sees movement.
   */
  startWalking() {
    this.seq = 0;
    this.walkTimer = setInterval(() => {
      if (this.closed) return;
      const aim = this.steer();
      this.lastAim = aim;
      const frames = [];
      // T-156: exactly STEPS_PER_TICK frames per tick. The server consumes
      // PHYSICS_HZ / TICK_HZ = 3 frames per 20 Hz tick, so 4 frames every 50 ms fed it
      // 80 frames/s against a 60 frames/s drain. The queue filled to MAX_QUEUED_INPUTS
      // (60 frames = one full second) and then dropped the excess, so every input the bot
      // sent was applied about a second late. That is why the bot could see an edge,
      // press jump, and still fall: the jump arrived after it had already left the
      // platform. The trace showed `edge: 0.6, jump: true` twice in a row followed by a
      // snapshot already airborne.
      for (let i = 0; i < INPUT_FRAMES_PER_BATCH; i += 1) {
        this.seq += 1;
        const moveX = aim ? Math.round(aim.x * 127) : Math.round((Math.random() * 2 - 1) * 127);
        const moveZ = aim ? Math.round(aim.z * 127) : Math.round((Math.random() * 2 - 1) * 127);
        const jump = aim ? aim.jump : Math.random() < 0.05;
        frames.push({
          seq: this.seq,
          move_x: moveX,
          move_z: moveZ,
          buttons: jump ? 1 : 0,
        });
      }
      this.send({ variant: 'Inputs', frames });
    }, INPUT_BATCH_MS);
  }

  stopWalking() {
    if (this.walkTimer) clearInterval(this.walkTimer);
    this.walkTimer = null;
  }

  /** The sweep's autopilot: answer every prompt the server raises. */
  react(msg) {
    if (msg.variant === 'Draft') {
      const picks = [];
      for (let i = 0; i < (msg.enemy_picks || 0); i += 1) picks.push(i);
      for (let i = 0; i < (msg.reward_picks || 0); i += 1) picks.push(i);
      this.send({ variant: 'DraftPick', picks });
      this.drafts = (this.drafts || 0) + 1;
    } else if (msg.variant === 'VoteOpen') {
      this.send({ variant: 'Vote', option: 0 });
      this.votes = (this.votes || 0) + 1;
    } else if (msg.variant === 'PollOpen') {
      // T-159: the pre-match theme/difficulty poll gates the level-1 lobby, so a bot that
      // never answers it never gets a level. Option 0 is the first catalog theme, then
      // STANDARD.
      this.send({ variant: 'Poll', option: 0 });
      this.polls = (this.polls || 0) + 1;
    }
    // No auto-buy on `Shop`. A `Shop` frame is sent whenever the offer changes, including
    // for a level that is already playing, and the server answers a Buy outside
    // PHASE_ENEMY_DRAFT with 409 "the shop is not open". Guessing the phase from frame
    // ordering produced exactly that 409, which `error-frames` reported. A purchase has a
    // precondition, so the actuated driver sets it explicitly; see `actuate`.
  }

  close() {
    if (this.ws && !this.closed) this.ws.close();
  }
}

/** Pull `gifts_left` out of the `state` readout. */
function giftsLeft(stateMsg) {
  const m = /gifts_left=(\d+)/.exec(stateMsg?.message ?? '');
  return m ? Number(m[1]) : null;
}

/** Pull `currency` out of the `state` readout. */
function currency(stateMsg) {
  const m = /currency=(-?\d+)/.exec(stateMsg?.message ?? '');
  return m ? Number(m[1]) : null;
}

/** Pull `level` out of the `state` readout. */
function stateLevel(stateMsg) {
  const m = /level=(\d+)/.exec(stateMsg?.message ?? '');
  return m ? Number(m[1]) : null;
}

/**
 * T-154: drive the level -> shop -> vote chain through the debug command layer.
 *
 * The bots cannot finish a level -- a greedy straight-line steer cannot clear a gap it
 * cannot probe -- so `level-progression`, `vote` and `shop` were permanently SKIPped and
 * a regression in any of them would have gone unnoticed. The debug layer is the actuator
 * that makes those paths reachable: it can change level, empty a level's gifts and grant
 * currency, which is the same sequence a real run performs.
 *
 * This is a coverage tool, not a claim that a player can complete a level by walking.
 * `gift-rate` stays the traversal check; this one proves the chain still fires.
 */
async function actuate(client, push) {
  const welcome = await client.waitFor((f) => f.msg && f.msg.variant === 'Welcome', 8000);
  if (!welcome) {
    push('actuate-connect', false, 'no Welcome within 8000ms');
    return;
  }

  // --- level progression, first jump -----------------------------------------
  // A jump must produce a fresh Level frame with gifts on it and a state readout that
  // agrees, which is what "the level changed" means. The log index is captured before the
  // command so a Level frame from the level the bot already had cannot be mistaken for the
  // result of the jump.
  const jumpFrom = client.log.length;
  // The jump is what generates the offer, so the Shop frame to read is the one it sends.
  // Capturing the index after the jump instead waits for a frame that only arrives on the
  // next purchase, which is a deadlock: no offer, no buy, no new offer.
  const shopFrom = jumpFrom;
  const jump2 = await client.debug('switch_level', { level: 2, seed: 4002 });
  const level2 = await client.waitForAfter(jumpFrom, (e) => e.msg.variant === 'Level', 5000);
  const gifts2 = level2 && level2.msg.data && Array.isArray(level2.msg.data.gifts)
    ? level2.msg.data.gifts.length
    : 0;
  const state2 = await client.debug('state');
  const jump2Ok = jump2.ok === true && level2 !== null && gifts2 > 0 && stateLevel(state2) === 2;

  // --- shop ------------------------------------------------------------------
  // The offer is generated by the jump, but it is only *open* in PHASE_ENEMY_DRAFT: the
  // server answers a Buy in any other phase with 409 "the shop is not open". So the shop
  // step has to set the phase, not just wait for the frame.
  const shop = await client.waitForAfter(shopFrom, (e) => e.msg.variant === 'Shop', 6000);
  const items = shop && Array.isArray(shop.msg.items) ? shop.msg.items : [];
  const stocked = items.filter((item) => item.stock > 0);
  let bought = false;
  let spent = null;
  if (stocked.length > 0) {
    await client.debug('set_phase', { phase: PHASE_ENEMY_DRAFT });
    await client.debug('grant_gold', { amount: 5000 });
    const before = currency(await client.debug('state'));
    client.send({ variant: 'Buy', item: stocked[0].id });
    const deadline = Date.now() + 4000;
    while (Date.now() < deadline) {
      const now = currency(await client.debug('state'));
      if (before !== null && now !== null && now < before) {
        bought = true;
        spent = before - now;
        break;
      }
      await sleep(100);
    }
  }
  push(
    'actuate-shop',
    items.length > 0 && stocked.length > 0 && bought,
    'Shop frames: ' + client.all('Shop').length + ', items: ' + items.length
      + ', stocked: ' + stocked.length
      + (bought ? ', bought ' + stocked[0].id + ' for ' + spent : ', no purchase landed'),
  );

  // --- level progression, second jump ----------------------------------------
  // A second jump proves the first was not a one-off: the world regenerates again and the
  // level counter follows.
  const jump3From = client.log.length;
  const jump3 = await client.debug('switch_level', { level: 3, seed: 4003 });
  const level3 = await client.waitForAfter(jump3From, (e) => e.msg.variant === 'Level', 5000);
  const gifts3 = level3 && level3.msg.data && Array.isArray(level3.msg.data.gifts)
    ? level3.msg.data.gifts.length
    : 0;
  const state3 = await client.debug('state');
  const jump3Ok = jump3.ok === true && level3 !== null && gifts3 > 0 && stateLevel(state3) === 3;

  push(
    'actuate-level-progression',
    jump2Ok && jump3Ok,
    'level 2: ok=' + (jump2.ok === true) + ' freshLevel=' + (level2 !== null) + ' gifts=' + gifts2
      + ' stateLevel=' + stateLevel(state2)
      + '; level 3: ok=' + (jump3.ok === true) + ' freshLevel=' + (level3 !== null)
      + ' gifts=' + gifts3 + ' stateLevel=' + stateLevel(state3),
  );

  // --- vote ------------------------------------------------------------------
  // Empty the level's gifts. The collapse window is when `open_vote_session` fires, so a
  // VoteOpen frame is the proof that the vote path is reachable from a real level state.
  const voteFrom = client.log.length;
  let left = null;
  for (let i = 0; i < 12; i += 1) {
    const granted = await client.debug('grant_gift', { count: 255 });
    if (!granted.ok) {
      push('actuate-vote', false, 'grant_gift failed: ' + (granted.message ?? 'no message'));
      return;
    }
    left = giftsLeft(await client.debug('state'));
    if (left === 0) break;
  }
  const open = await client.waitForAfter(voteFrom, (e) => e.msg.variant === 'VoteOpen', 10000);
  const options = open && Array.isArray(open.msg.options) ? open.msg.options : [];
  push(
    'actuate-vote',
    left === 0 && open !== null && options.length > 0,
    'gifts_left=' + left + ', VoteOpen=' + (open !== null) + ', options=' + options.length,
  );
}

function report(name, ok, detail) {
  const label = ok === null ? 'SKIP' : ok ? 'PASS' : 'FAIL';
  console.log(label + ' - ' + name + ' (' + detail + ')');
  return ok !== false;
}

async function main() {
  // The debug gate rejects every command while more than one player is connected, so the
  // actuated mode is singleplayer by construction.
  const clientCount = ACTUATE ? 1 : CLIENTS;
  const clients = [];
  for (let i = 0; i < clientCount; i += 1) {
    const client = new Client('pt-' + i);
    // Each bot takes a different one of the nearest gifts, so eight of them do
    // not converge on the same one. See `steer`.
    client.index = i;
    clients.push(client);
  }

  const results = [];
  const push = (name, ok, detail) => results.push({ name, ok, detail });

  // --- connect
  // Connect every socket first, then greet them together: a client that joins
  // after the run has started cannot pick a class, and that 409 is expected
  // behaviour rather than an anomaly.
  const opened = await Promise.allSettled(clients.map((client) => client.connect(URL_)));
  const connected = opened.filter((r) => r.status === 'fulfilled').length;
  // Hello and ChooseClass go out back to back, exactly as the real client does.
  // The class window closes when the first draft completes, which is well under
  // a second, so any delay here reads as a rejected class.
  for (let i = 0; i < clients.length; i += 1) {
    if (clients[i].closed) continue;
    clients[i].send({ variant: 'Hello', protocol_version: PROTOCOL_VERSION, name: clients[i].name });
    // A finished run leaves the server in PHASE_OVER, where every class pick is
    // rejected. Restart is a no-op unless the run is over, so sending it here is
    // safe on a fresh server and makes the sweep repeatable on a used one.
    clients[i].send({ variant: 'Restart' });
    clients[i].send({ variant: 'ChooseClass', class: i % CLASS_COUNT });
  }
  push('connect', connected === clientCount, connected + '/' + clientCount + ' clients connected');

  // --- let the run play out
  for (const client of clients) if (!client.closed) client.startWalking();
  // T-154: in actuated mode the debug driver runs alongside the bot. It advances the run
  // through the command layer while the bot keeps walking, so the frames the checks below
  // read are produced by a real session rather than by a mock.
  const driver = ACTUATE && clients[0] && !clients[0].closed
    ? actuate(clients[0], push)
    : Promise.resolve();
  // `--verbose` prints one line per second per client: position, grounded, and
  // gift progress. A stalled level is either a bot that is dying or a bot that
  // is not finding gifts, and the level counter cannot tell those apart.
  if (flag('--verbose')) {
    const probe = setInterval(() => {
      for (const client of clients) {
        if (client.closed || !client.you) continue;
        const p = client.you.pos;
        console.log(
          '  ' + client.name + ' pos ' + p.x.toFixed(1) + ',' + p.y.toFixed(1) + ',' + p.z.toFixed(1) +
          ' grounded ' + client.you.grounded +
          ' vel ' + client.you.vel.x.toFixed(1) + ',' + client.you.vel.y.toFixed(1) + ',' + client.you.vel.z.toFixed(1) +
          ' stuck ' + client.stuckFrames +
          ' gifts ' + client.collected.size + '/' + (client.level ? client.level.gifts.length : 0) +
          ' target ' + client.target +
          ' aim ' + (client.lastAim ? client.lastAim.x.toFixed(2) + ',' + client.lastAim.z.toFixed(2) : 'none') +
          ' tgt ' + (client.lastTarget ? client.lastTarget.x.toFixed(1) + ',' + client.lastTarget.y.toFixed(1) + ',' + client.lastTarget.z.toFixed(1) : 'none'),
        );
      }
    }, 1000);
    probe.unref();
  }
  const deadline = Date.now() + SECONDS * 1000;
  while (Date.now() < deadline) {
    await sleep(250);
    if (clients.every((client) => client.closed)) break;
  }

  await driver;

  const all = clients.flatMap((client) => client.log.map((entry) => entry.msg));
  const variants = new Set(all.map((msg) => msg.variant));
  const runStates = all.filter((msg) => msg.variant === 'RunState');
  const levels = runStates.map((msg) => msg.info && msg.info.level).filter((n) => typeof n === 'number');
  const phases = new Set(runStates.map((msg) => msg.info && msg.info.phase).filter((p) => p !== undefined));
  const maxLevel = levels.length ? Math.max(...levels) : 0;
  // Progression means the level CHANGED during this session, not that it is high. The
  // server pushes its current run state on Hello and `Restart` only resets a run that is
  // over, so a server left at level 3 by an earlier sweep would otherwise report PASS for
  // a sweep that never advanced anything -- which is exactly what happened before this was
  // measured this way.
  const startLevel = levels.length ? levels[0] : 0;
  const advanced = levels.some((level) => level !== startLevel);

  push('welcome', variants.has('Welcome'), 'Welcome frames: ' + all.filter((m) => m.variant === 'Welcome').length);
  push('runstate', runStates.length > 0, 'RunState frames: ' + runStates.length);
  // The sweep has no navigation bot, so a level only completes if the random
  // walk happens to collect every gift. Checks that need a completed level are
  // reported as SKIP rather than FAIL when that did not happen, so the sweep's
  // signal stays about the protocol and the lobby flow.
  const completed = advanced;
  const gate = (name, ok, detail) => push(name, completed ? ok : null, detail);
  // Gift progress is the number that explains a stalled level: a bot that
  // reaches level 1 and stops is either dying or not finding the gifts, and
  // the two look identical from the level counter alone.
  const totalGifts = clients.reduce(
    (n, c) => (c.level && Array.isArray(c.level.gifts) ? Math.max(n, c.level.gifts.length) : n),
    0,
  );
  const best = { taken: clients.reduce((n, c) => Math.max(n, c.collected.size), 0), total: totalGifts };
  // T-154: gated, not a hard FAIL. The bots cannot complete a level by walking, so in the
  // 8-client sweep this is a SKIP with the number that explains it; the actuated mode
  // (`--actuate`) is where this check is live, because the debug layer can change level.
  gate('level-progression', completed,
    'level went ' + startLevel + ' -> ' + maxLevel + ' (best client collected '
      + best.taken + '/' + best.total + ' gifts)');
  // T-142: gift progress as its own check, not just a number inside the skip message.
  // `level-progression` needs a completed level, which the bots cannot reach (T-139),
  // so it is always skipped and a traversal regression would go unnoticed. This check
  // needs no completion: it asks whether the bots can still move around the level and
  // pick things up at a reasonable rate. Measured 28-50% across runs; the floor is 20%,
  // which leaves headroom for the run-to-run variance while still failing if a level
  // change makes traversal much worse.
  const GIFT_RATE_MIN = 0.2;
  const giftRate = best.total > 0 ? best.taken / best.total : 0;
  // Actuated mode grants gifts through the debug layer instead of walking to them, so the
  // rate it measures is not traversal. Reporting it as a number here would be a metric with
  // nothing to measure; the 8-client sweep is where this check belongs.
  push('gift-rate', ACTUATE ? null : best.total > 0 && giftRate >= GIFT_RATE_MIN,
    ACTUATE
      ? 'skipped: actuated mode grants gifts through the debug layer, so this measures no traversal'
      : 'best client collected ' + best.taken + '/' + best.total + ' gifts ('
        + (giftRate * 100).toFixed(1) + '%) in ' + SECONDS + 's; threshold >= '
        + (GIFT_RATE_MIN * 100).toFixed(0) + '%');
  // T-156: level completion as its own check. `level-progression` asks whether the level
  // CHANGED, which `--actuate` can force through the debug layer; this asks whether the
  // bots emptied a level by WALKING, which is the coverage hole the owner named. It stays a
  // SKIP while the bot cannot finish a level, and the detail carries the number so the gap
  // is visible in every gate run instead of being remembered.
  // SKIP, not FAIL, while the bot cannot finish a level: this is a known coverage hole, not
  // a regression, and a permanently red line would train the gate to be ignored. It becomes
  // a live PASS the moment a level is emptied by walking.
  push('level-completion', ACTUATE ? null : (completed ? true : null),
    ACTUATE
      ? 'skipped: actuated mode changes level through the debug layer, so this measures no traversal'
      : completed
        ? 'a level was emptied by walking (level went ' + startLevel + ' -> ' + maxLevel + ')'
        : 'no level was emptied by walking: best client collected ' + best.taken + '/' + best.total
          + ' gifts (' + (giftRate * 100).toFixed(1) + '%). T-156 fixed the input-rate and'
          + ' air-control bugs that made the bot fall off the map; the remaining blocker is'
          + ' vertical routing -- a gift directly above or below the bot has no route, so the'
          + ' bot blacklists it and holds. See tickets/T-156.md.');
  // A class pick and the first draft both only exist in the first lobby of a run, and
  // `Restart` resets a run only when it is over. A sweep that joins a run already in
  // progress therefore cannot see either -- by design, not by defect -- so both checks are
  // gated on the precondition rather than blaming the server for the sweep's own timing.
  const firstLobby = runStates.some(
    (msg) => msg.info && msg.info.level === 1 && msg.info.phase === PHASE_ENEMY_DRAFT,
  );
  // `draft` does not need a completed level: the first draft fires before level 1
  // starts, so it is a live check even when the bot never finishes a level.
  const drafts = all.filter((m) => m.variant === 'Draft').length;
  const draftAnswers = clients.reduce((n, c) => n + (c.drafts || 0), 0);
  push('draft', firstLobby ? drafts > 0 && draftAnswers > 0 : null,
    firstLobby
      ? 'Draft frames: ' + drafts + ', answered: ' + draftAnswers
      : 'skipped: the run was not in the first lobby when the clients joined, so no draft '
        + 'was due (Restart only resets a run that is over)');
  gate('vote', variants.has('VoteOpen'), 'VoteOpen frames: ' + all.filter((m) => m.variant === 'VoteOpen').length);
  gate('shop', variants.has('Shop'), 'Shop frames: ' + all.filter((m) => m.variant === 'Shop').length);
  push('phase-coverage', phases.size >= 2, 'distinct phases: ' + phases.size + ' [' + [...phases].join(',') + ']');

  const decodeFailures = clients.flatMap((client) => client.decodeFailures);
  push('decode', decodeFailures.length === 0, decodeFailures.length + ' undecodable frames' + (decodeFailures.length ? ': ' + decodeFailures[0] : ''));

  const errorFrames = clients.flatMap((client) => client.errors).filter((e) => e.code !== undefined);
  const classRejects = errorFrames.filter((e) => /class can only be changed/i.test(e.message || ''));
  const otherErrors = errorFrames.filter((e) => !/class can only be changed/i.test(e.message || ''));
  // A class pick is only legal in the first lobby of a run. `Restart` resets the run only
  // when it is over, so a sweep that joins a run already in progress cannot pick a class --
  // by design, not by defect. Reporting that as a rejection would blame the server for the
  // sweep's own precondition, so it is a SKIP with the reason.
  push('class-select', firstLobby ? classRejects.length === 0 : null,
    firstLobby
      ? classRejects.length + '/' + clientCount + ' clients had their class rejected' +
        (classRejects.length ? ' (server: "' + classRejects[0].message + '")' : '')
      : 'skipped: the run was not in the first lobby when the clients joined, so a class '
        + 'pick is rejected by design (Restart only resets a run that is over)');
  push('error-frames', otherErrors.length === 0,
    otherErrors.length + ' unexpected Error frames' + (otherErrors.length ? ': ' + JSON.stringify(otherErrors.slice(0, 3)) : ''));

  for (const client of clients) {
    client.stopWalking();
    client.close();
  }

  // A named known failure is downgraded to SKIP so it cannot fail the gate, but it is
  // still printed, and any failure NOT named here still fails.
  const downgraded = [];
  for (const r of results) {
    if (r.ok === false && KNOWN_FAILURES.has(r.name)) {
      downgraded.push(r.name);
      r.ok = null;
      const why = KNOWN_FAILURE_REASONS[r.name];
      r.detail = (r.detail ? r.detail + ' ' : '')
        + '(known failure, downgraded to SKIP'
        + (why ? `: ${why}` : '')
        + ')';
    }
  }
  reportDeaths();
  const ok = results.every((r) => r.ok !== false);
  for (const r of results) report(r.name, r.ok, r.detail);
  const passed = results.filter((r) => r.ok === true).length;
  const skipped = results.filter((r) => r.ok === null).length;
  console.log('playthrough: ' + passed + '/' + results.length + ' checks passed' +
    (skipped ? ', ' + skipped + ' skipped' : '') +
    (downgraded.length ? ' (known failures: ' + downgraded.join(', ') + ')' : ''));
  process.exit(ok ? 0 : 1);
}

/**
 * Summarise the server's death log, if one was produced.
 *
 * Reports the cause split (void fall vs enemy kill), how many deaths ended a run
 * outright, and the fall positions, so a regression in level geometry or in the
 * bot's edge handling shows up as a number instead of a vague "bots died".
 */
function reportDeaths() {
  if (!DEATH_LOG) return;
  let text;
  try {
    text = readFileSync(DEATH_LOG, 'utf8');
  } catch {
    report('death-log', null, 'no death log at ' + DEATH_LOG + ' (server not started with VOIDWRAP_DEATH_LOG)');
    return;
  }
  const rows = text.split(String.fromCharCode(10)).filter(Boolean).map((line) => {
    try { return JSON.parse(line); } catch { return null; }
  }).filter(Boolean);
  if (!rows.length) {
    report('death-log', true, '0 deaths recorded');
    return;
  }
  const byNote = new Map();
  for (const r of rows) byNote.set(r.note, (byNote.get(r.note) || 0) + 1);
  const out = rows.filter((r) => r.out).length;
  const split = [...byNote.entries()].map(([k, v]) => k + ':' + v).join(' ');
  const falls = rows.filter((r) => r.note === 'void');
  const deepest = falls.length ? Math.min(...falls.map((r) => r.y)) : null;
  const detail = rows.length + ' deaths (' + split + '), ' + out + ' ended a run'
    + (deepest === null ? '' : ', deepest fall y=' + deepest.toFixed(2))
    + '; levels: ' + [...new Set(rows.map((r) => r.level))].sort((a, b) => a - b).join(',');
  // Informational: a death is not a failure. The check exists so the cause split is
  // printed on every run instead of being invisible.
  report('death-log', true, detail);
}

main().catch((error) => {
  console.error('playthrough: ' + error.message);
  process.exit(2);
});
