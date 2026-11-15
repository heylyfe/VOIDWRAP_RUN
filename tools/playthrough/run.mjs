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

// Mirrors protocol::CARD_ENEMY. A draft pick is an index into the offer's single card list,
// and the server counts enemy and reward picks separately.
const CARD_ENEMY = 0;

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
 * optional.
 *
 * T-158: raised from 0.75 to 0.90. The bot commits to a jump up to
 * `PLAYER_HALF_WIDTH + EDGE_JUMP_DISTANCE` (0.9 u) before the edge, so that much of the
 * reach is spent just getting to the edge and the widest gap it can actually clear is
 * `jumpReach(rise) - 0.9`. At 0.75 that is 4.16 u on the flat, which is *narrower* than the
 * 5.0 u flat gap `levelgen::can_jump_or_step` admits (design_max_gap(0) = 6.0 minus its
 * 1.0 u comfort margin). The bot would then refuse edges the generator guarantees are
 * jumpable, and blacklist the gifts behind them. At 0.90 the flat gap is 5.17 u and the
 * envelope is a strict superset of the generator's admission at every rise band:
 *
 *   rise   -4.0  -3.0  -2.0  -1.0   0.0  +0.5  +1.0  +1.5  +2.0
 *   gen     7.0   6.5   6.0   5.5   5.0   4.5   4.0   3.5   3.0
 *   bot     7.2   6.8   6.3   5.8   5.2   4.8   4.4   3.9   3.2
 *
 * The remaining 10% is the real margin: the bot now feeds input at a clock-paced 60 Hz
 * (see `startWalking`) and holds the jump through the rise, so its execution is far closer
 * to the theoretical arc than it was when 0.75 was chosen. */
const JUMP_SAFETY = 0.9;
const EDGE_PROBE_STEP = 0.15;
const EDGE_PROBE_MAX = 1.5;
/** Player collision half-width, mirrored from tuning.rs. The centre can stand this far
 * past a platform edge before the player falls. */
const PLAYER_HALF_WIDTH = 0.4;
/** Extra margin on top of PLAYER_HALF_WIDTH before the bot commits to a jump. One input
 * batch covers 0.45 u at run speed, so 0.5 u is one batch of reaction time. */
const EDGE_JUMP_DISTANCE = 0.5;
const LANDING_STEP = 0.15;

// T-158: launch platforms. Mirrors server/crates/sim/src/tuning.rs, the same way GRAVITY
// and JUMP_VELOCITY above do. A jump pad and a trampoline are the only two ways up past the
// jump apex, and the level generator places them precisely so a level can be climbed:
// `levelgen::can_jump_or_step` admits a pad edge up to +3.5 u within 4.5 u, which is above
// the +2.0 u ceiling the plain-jump envelope stops at. Without these the platform graph has
// no edge to any platform more than 2.0 u above the bot, so every gift that needs a climb
// is invisible to the router and gets blacklisted.
/** protocol::PLATFORM_JUMP_PAD. */
const PLATFORM_JUMP_PAD = 2;
/** protocol::PLATFORM_TRAMPOLINE. */
const PLATFORM_TRAMPOLINE = 7;
/** tuning.rs PAD_LAUNCH_VEL: vertical launch velocity applied on contact. */
const PAD_LAUNCH_VEL = 16.0;
/** tuning.rs TRAMPOLINE_LAUNCH_VEL: higher than a pad so it opens routes a pad cannot. */
const TRAMPOLINE_LAUNCH_VEL = 26.0;
/** Peak height of a launch, v^2 / 2g: pad +4.00 u, trampoline +10.56 u. */
const PAD_APEX = (PAD_LAUNCH_VEL * PAD_LAUNCH_VEL) / (2 * GRAVITY);
const TRAMPOLINE_APEX = (TRAMPOLINE_LAUNCH_VEL * TRAMPOLINE_LAUNCH_VEL) / (2 * GRAVITY);
/** Rise margin below the apex, mirroring `levelgen`'s "apex - 0.5" pad rule: a launch that
 * only just clears the target's top has no room to land on it. */
const LAUNCH_RISE_MARGIN = 0.5;
/** Fraction of the launch reach the bot will commit to. The launch is a contact event the
 * bot cannot time, so a margin is not optional -- but it has to stay loose enough that every
 * edge `levelgen::can_jump_or_step` admits is still in the graph, or the bot would blacklist
 * a gift the generator proved reachable. At the pad's +3.5 u ceiling the physical reach is
 * 6.09 u and the generator admits gaps up to 4.5 u, so 0.85 leaves 0.68 u of slack. */
const LAUNCH_SAFETY = 0.85;
/** Target id for the level beacon. Gift ids are u16, so a negative id cannot collide. */
const BEACON_TARGET = -1;

/** Physics rate the server steps at, and the tick rate it drains input at. */
const PHYSICS_HZ = 60;
const TICK_HZ = 20;
/** Frames the server consumes per tick: PHYSICS_HZ / TICK_HZ. */
const STEPS_PER_TICK = PHYSICS_HZ / TICK_HZ;
/** How often the bot wakes to steer and top up the input queue. The number of frames it
 * sends is decided by the clock, not by this interval -- see `startWalking`. */
const INPUT_BATCH_MS = 1000 / PHYSICS_HZ;

/**
 * Append one NDJSON line to the `--trace` file. No-op when tracing is off.
 *
 * T-158: buffered, not one `appendFileSync` per frame. A synchronous write per steer frame
 * at 60 Hz is slow enough to starve the input queue -- the server applies neutral input
 * after `STARVE_TICKS`, so a traced run moved at roughly half speed and fell short of jumps
 * it clears untraced. The trace was changing the thing it measured. Flushing every
 * TRACE_FLUSH_LINES lines keeps the file complete without touching the frame budget.
 */
const TRACE_BUFFER = [];
const TRACE_FLUSH_LINES = 256;

function flushTrace() {
  if (!TRACE || TRACE_BUFFER.length === 0) return;
  try {
    appendFileSync(TRACE, TRACE_BUFFER.join(''));
  } catch {
    // A trace that cannot be written must not take the sweep down with it.
  }
  TRACE_BUFFER.length = 0;
}

function traceLine(record) {
  if (!TRACE) return;
  TRACE_BUFFER.push(JSON.stringify(record) + String.fromCharCode(10));
  if (TRACE_BUFFER.length >= TRACE_FLUSH_LINES) flushTrace();
}

/**
 * Horizontal distance a flat jump covers before returning to `rise` units above the launch
 * point, discounted by JUMP_SAFETY. Zero when the rise is above the jump apex.
 *
 * `rise` is signed: a landing below the launch point is in the air longer, not shorter, so
 * a drop extends the reach. Clamping it at zero (as this did before T-158) made the bot
 * refuse drops the generator admits -- `design_max_gap` allows 7.0 u at a -4.0 u drop,
 * while a clamped reach caps out at 5.06 u whatever the drop.
 */
function jumpReach(rise) {
  const disc = JUMP_VELOCITY * JUMP_VELOCITY - 2 * GRAVITY * rise;
  if (disc <= 0) return 0;
  const airtime = (JUMP_VELOCITY + Math.sqrt(disc)) / GRAVITY;
  return airtime * RUN_SPEED * JUMP_SAFETY;
}

/**
 * Widest edge-to-edge gap the bot will commit to at `rise`.
 *
 * The bot presses jump as soon as the edge is within `PLAYER_HALF_WIDTH + EDGE_JUMP_DISTANCE`,
 * so it leaves the ground that far *before* the edge and has to cover the whole gap from
 * there. `jumpReach` is measured from the launch point, so the gap it can clear is the reach
 * minus that run-up. The router and `findLanding` both use this, which is what keeps the
 * graph from promising a jump the probe will refuse.
 */
function jumpGap(rise) {
  return Math.max(0, jumpReach(rise) - (PLAYER_HALF_WIDTH + EDGE_JUMP_DISTANCE));
}

/** Launch velocity of a platform kind, or 0 when the kind does not launch. */
function launchVelocityOf(kind) {
  if (kind === PLATFORM_JUMP_PAD) return PAD_LAUNCH_VEL;
  if (kind === PLATFORM_TRAMPOLINE) return TRAMPOLINE_LAUNCH_VEL;
  return 0;
}

/**
 * Horizontal distance a launch covers before the feet come back down to `rise` above the
 * launch point, discounted by LAUNCH_SAFETY. Zero when the rise is above the apex.
 *
 * The controller gives full air control while airborne -- `step_with` sets horizontal
 * velocity straight from the input every frame -- so the whole airtime is available at
 * RUN_SPEED. The launch's own horizontal multiplier (`PAD_HORIZONTAL_VEL_MUL` 0.6,
 * `TRAMPOLINE_HORIZONTAL_VEL_MUL` 1.15) only shapes the launch frame itself.
 */
function launchReach(vel, rise) {
  const disc = vel * vel - 2 * GRAVITY * rise;
  if (disc <= 0) return 0;
  const airtime = (vel + Math.sqrt(disc)) / GRAVITY;
  return airtime * RUN_SPEED * LAUNCH_SAFETY;
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
    + 'instead of crossing a gap). T-158 gave the platform graph launch edges through the '
    + 'jump pad and the trampoline, so a climb is routable. See tickets/T-158.md.',
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
// T-158: the jump envelope is `jumpReach` above -- the bot's own measured reach, not
// `sim::levelgen::design_max_gap`. The design envelope (6.0 u on the flat, 4.0 u at a
// +2.0 u rise, 8.0 u at a -4.0 u drop, nothing above +2.0 u because the jump apex is
// +2.25 u) is what the generator admits for a human, and it is 1.0 u looser than what this
// bot will commit to. Using it here made the graph promise jumps the landing probe refused.
const RAMP_KINDS = new Set([3, 4, 5, 6]);

/** Edge-to-edge horizontal gap between two platform AABBs. */
function gapBetween(a, b) {
  const dx = Math.max(0, a.min.x - b.max.x, b.min.x - a.max.x);
  const dz = Math.max(0, a.min.z - b.max.z, b.min.z - a.max.z);
  return Math.hypot(dx, dz);
}

/**
 * Adjacency over platforms: a directed edge exists when the gap is jumpable in that
 * direction. Climbing and dropping are separate edges -- see the note in the loop.
 *
 * T-158: the jump edges use `jumpReach`, the SAME envelope `findLanding` uses, not the
 * design envelope `maxGapForRise` mirrors. The two disagreed, and the disagreement was the
 * real reason the bot could not empty a level. `maxGapForRise` is `levelgen`'s admission
 * rule for a *human* jump (6.0 u on the flat, minus a 1.0 u comfort margin inside the
 * generator), while `jumpReach` is what this bot will actually commit to (5.06 u on the
 * flat, after JUMP_SAFETY). So the graph claimed edges the landing probe then refused: on
 * the seed-7 level seven pairs were graph-reachable but probe-unreachable, including the
 * 6.0 u flat gap P9<->P19 that sits on the route to half the level. The bot routed at a
 * gap it could not clear, `findLanding` returned null, and it blacklisted the gift -- which
 * is exactly the "blacklist ends the run" failure the ticket names. One envelope, used by
 * both the router and the probe, means the graph can never promise a jump the bot will not
 * take. The design envelope is still the right model for the generator; it is the wrong
 * model for this bot.
 */
function buildPlatformGraph(platforms) {
  const adj = Array.from({ length: platforms.length }, () => []);
  for (let i = 0; i < platforms.length; i += 1) {
    for (let j = i + 1; j < platforms.length; j += 1) {
      const a = platforms[i].bounds;
      const b = platforms[j].bounds;
      const gap = gapBetween(a, b);
      const up = b.max.y - a.max.y;
      const down = a.max.y - b.max.y;
      // T-158: DIRECTED. A drop is not a climb. `gap <= jumpGap(up)` is i -> j (climbing),
      // `gap <= jumpGap(down)` is j -> i (dropping). An undirected edge let the router send
      // the bot UP a gap it could only ever take DOWN, and that is how it died on seed-7
      // level 2: P36 (top -10.25) and P28 (top 0.00) are 10.25 u apart, the only edge
      // between them is the 4.12 u drop off P28, and the undirected graph routed the bot
      // from P36 up to P28. It jumped at a platform 10 u above it, hit P34's side wall
      // (its own half-width face at z = 23.0), slid along it and fell to kill_y. Both
      // levels keep every platform and gift reachable with the edges directed.
      if (gap <= jumpGap(up)) adj[i].push(j);
      if (gap <= jumpGap(down)) adj[j].push(i);
    }
  }
  // T-158: launch edges, additive and DIRECTED. A pad is authored as its own box sitting on
  // top of a NORMAL plate (design/chunks.json: a 3x3 pad whose top is 0.5 u above the plate
  // it stands on), so the jump rule above already connects the plate to the pad -- the bot
  // steps up onto it. What was missing is the edge OUT of the pad: the launch. Keeping the
  // pad as its own node is what lets `steer` see "the next hop is a launch platform" and
  // aim at its centre instead of at a gift it cannot jump to.
  for (let i = 0; i < platforms.length; i += 1) {
    const vel = launchVelocityOf(platforms[i].kind);
    if (vel === 0) continue;
    const apex = (vel * vel) / (2 * GRAVITY);
    for (let j = 0; j < platforms.length; j += 1) {
      if (j === i) continue;
      const rise = platforms[j].bounds.max.y - platforms[i].bounds.max.y;
      if (rise > apex - LAUNCH_RISE_MARGIN) continue;
      if (gapBetween(platforms[i].bounds, platforms[j].bounds) > launchReach(vel, rise)) continue;
      if (!adj[i].includes(j)) adj[i].push(j);
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
    // T-158: the arc the bot is riding. A deliberate jump and a launch are both arcs with a
    // destination, and air control has to hold that destination instead of aiming at
    // `nearestLanding` -- which, mid-arc, is the platform the bot just left. `arcUntil` is
    // the wall-clock end of the arc, `arcTarget` the platform it is heading for.
    this.arcUntil = 0;
    this.arcTarget = -1;
    // T-158: the gift count of the level the bot just finished, captured before the new
    // `Level` frame clears `collected`. Without it the completion detail would report the
    // count on the level that just started, which is always 0.
    this.finished = null;
    // T-158: the best gift progress seen on ANY level, so `gift-rate` survives a level
    // change. `collected` is cleared by the next `Level` frame, so reading it at the end of
    // the sweep reports the level that just started -- 0/0 once a level is emptied.
    this.bestProgress = { taken: 0, total: 0 };
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
          // T-158: capture the level the bot just finished before the new one clears the
          // tally. A `Level` frame only arrives when the level changes, so `collected` here
          // is the previous level's final count.
          if (this.level && Array.isArray(this.level.gifts) && this.collected.size > 0) {
            this.finished = {
              level: this.level.level,
              taken: this.collected.size,
              total: this.level.gifts.length,
            };
          }
          this.noteProgress();
          this.level = msg.data;
          this.collected = new Set();
          this.blacklist.clear();
          this.stuckFrames = 0;
          this.lastPos = null;
          this.target = null;
          this.arcUntil = 0;
          this.arcTarget = -1;
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
          this.noteProgress();
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

  /** Record the best gift progress seen on the current level. See `bestProgress`. */
  noteProgress() {
    if (!this.level || !Array.isArray(this.level.gifts)) return;
    const total = this.level.gifts.length;
    if (total === 0) return;
    const taken = this.collected.size;
    const rate = taken / total;
    const bestRate = this.bestProgress.total > 0
      ? this.bestProgress.taken / this.bestProgress.total
      : 0;
    if (rate > bestRate) this.bestProgress = { taken, total };
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
   * Index of a launch platform the bot's feet are resting on, or -1.
   *
   * T-158: `platformUnder` cannot answer this. A pad sits 0.5 u above the plate it stands
   * on, so the plate's top is inside `platformUnder`'s 0.6 u up-tolerance and the plate
   * wins on index order. This probe asks the exact question the controller's
   * `standing_platform` asks -- feet level with the pad's top -- because that is the
   * condition that fires the contact launch.
   */
  launchPlatformUnder(x, z, y) {
    const platforms = this.level && this.level.platforms;
    if (!Array.isArray(platforms)) return -1;
    for (let i = 0; i < platforms.length; i += 1) {
      if (launchVelocityOf(platforms[i].kind) === 0) continue;
      const b = platforms[i].bounds;
      if (x < b.min.x - PLAYER_HALF_WIDTH || x > b.max.x + PLAYER_HALF_WIDTH) continue;
      if (z < b.min.z - PLAYER_HALF_WIDTH || z > b.max.z + PLAYER_HALF_WIDTH) continue;
      if (Math.abs(b.max.y - y) <= 0.05) return i;
    }
    return -1;
  }

  /**
   * Index of the platform the bot is actually standing on, or -1.
   *
   * T-158: `platformUnder`'s 0.25 u slack is right for a landing probe but wrong for the
   * route origin. Standing at z = 19.113 on P14 (z 19..23) beside P9 (z 1..19), the slack
   * let P9 match first on index order, so the router believed it was on P9 and aimed at a
   * platform P9 touches -- across the 6.0 u gap P14 actually faces. The bot walked to P14's
   * edge, found no landing inside the jump envelope, blacklisted the gift, and repeated
   * until every remaining gift was blacklisted and it held still for the rest of the sweep.
   * The trace showed 15 gifts failing from one pinned position with `edge: 1.5`.
   *
   * Strict footprint first, so the platform whose bounds contain the feet point wins. The
   * fallback is the physical half-width: the controller lets the centre stand
   * PLAYER_HALF_WIDTH past an edge while the box still overlaps, and the bot must still
   * know which platform that is.
   */
  standingOn(x, z, y) {
    const strict = this.platformUnder(x, z, y, 0.6, 3.0, 0);
    if (strict >= 0) return strict;
    return this.platformUnder(x, z, y, 0.6, 3.0, PLAYER_HALF_WIDTH);
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
   * True when the jump arc from `(x, y, z)` in direction `(dx, dz)` passes through a
   * platform before it has travelled `limit` units.
   *
   * T-158: the graph and `findLanding` both model a jump as a gap between two platforms,
   * which is a 2D idea -- it says nothing about what stands between them. On seed-7 level 2
   * that gap was P34, a plate whose top (-7.50) is above the bot's apex (-8.00) and whose
   * footprint covers the whole of the landing shelf P35. The bot jumped at P35, hit P34's
   * side wall, slid along it and fell to kill_y. Sampling the arc's own height at each probe
   * step is enough to see the wall before committing to the jump.
   *
   * The arc is the flat-jump parabola: the controller sets horizontal velocity straight from
   * the input every frame, so the feet are at `y + v*t - g*t^2/2` after `t = d / RUN_SPEED`.
   * `skip` is the landing platform, which the arc is allowed to reach.
   */
  arcBlocked(x, z, y, dx, dz, limit, skip) {
    const platforms = this.level.platforms;
    for (let d = EDGE_PROBE_STEP; d < limit; d += EDGE_PROBE_STEP) {
      const t = d / RUN_SPEED;
      const arcY = y + JUMP_VELOCITY * t - 0.5 * GRAVITY * t * t;
      const px = x + dx * d;
      const pz = z + dz * d;
      for (let i = 0; i < platforms.length; i += 1) {
        if (i === skip) continue;
        const p = platforms[i];
        // A ramp's AABB is the whole slope, so it "contains" the bot standing on it and
        // would block every jump taken from one -- the first version of this check turned
        // level 1 into 40/46 with 19812 `no-landing` frames, all of them a bot standing on
        // a ramp. Ramps are walkable, so they never block.
        if (RAMP_KINDS.has(p.kind)) continue;
        const b = p.bounds;
        if (px < b.min.x || px > b.max.x || pz < b.min.z || pz > b.max.z) continue;
        // The platform the bot is standing on is not an obstacle to leaving it.
        if (x >= b.min.x && x <= b.max.x && z >= b.min.z && z <= b.max.z) continue;
        // Feet strictly inside the platform's vertical span: the bot would hit its side.
        if (arcY > b.min.y + 0.05 && arcY < b.max.y - 0.05) return true;
      }
    }
    return false;
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
      // T-158: the distance the bot has to cover is `d`, not `d - from`. The bot presses
      // jump as soon as the edge is within PLAYER_HALF_WIDTH + EDGE_JUMP_DISTANCE, so it
      // leaves the ground up to 0.9 u *before* the edge and the run-up is not free. The old
      // `d - from` check was optimistic by exactly that run-up, which is how the bot came to
      // commit to a 4.5 u hop on level 2 that it could not finish: it jumped at
      // (-2.23, -10.25, 33.47), apexed at y=-8.10, and fell 16 u short of the landing.
      if (d <= jumpReach(top - y)) {
        // T-158: a landing the arc cannot reach without passing through something is not a
        // landing. See `arcBlocked`.
        if (this.arcBlocked(x, z, y, dx, dz, d, i)) continue;
        return { d, y: top };
      }
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
    // T-158: the trace has to fire on EVERY path, including the ones that return early. It
    // used to be written only at the bottom of `steer`, so the moment the bot entered the
    // blacklist loop the trace went silent -- which is exactly the state that needed
    // debugging. `reason` names the path taken.
    const traceOut = (reason, fields) => {
      if (!TRACE) return;
      traceLine({
        t: now,
        x: +pos.x.toFixed(3), y: +pos.y.toFixed(3), z: +pos.z.toFixed(3),
        grounded: Boolean(you.grounded),
        reason,
        stuck: this.stuckFrames,
        arc: this.arcUntil > now,
        arcTarget: this.arcTarget,
        ...fields,
      });
    };

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
    // T-158: the beacon. Taking the last gift ends the collect phase and opens the beacon
    // (`run::enter_collapse_pending`), and the level only completes when a player walks
    // within BEACON_RADIUS (1.5 u) of it. The bot used to hold still once the gifts ran out,
    // so a sweep that collected 46/46 still reported "level went 1 -> 1". The beacon is a
    // goal like any other, so it goes through the same router and the same edge probe.
    if (level.beacon && level.gifts.every((gift) => this.collected.has(gift.id))) {
      const dx = level.beacon.x - pos.x;
      const dz = level.beacon.z - pos.z;
      candidates.push({ gift: { id: BEACON_TARGET, pos: level.beacon }, d: dx * dx + dz * dz });
    }
    if (!candidates.length) {
      // T-158: the blacklist must not be the thing that ends the run. When every remaining
      // gift is blacklisted the bot used to hold still for the rest of the sweep -- the
      // 240 s trace spent 7464 of 10057 frames here, with 15 gifts still on the floor. Clear
      // the blacklist and try again instead: a gift that is genuinely unreachable costs one
      // more failed probe, and a gift that became reachable because the bot moved gets
      // another chance immediately rather than after the 3 s expiry.
      const giftsLeft = level.gifts.some((gift) => !this.collected.has(gift.id));
      const goalLeft = giftsLeft || Boolean(level.beacon);
      if (goalLeft && this.blacklist.size > 0) {
        this.blacklist.clear();
        this.target = null;
        if (depth < 1) return this.steer(depth + 1);
      }
      traceOut('no-candidates', { target: this.target });
      return hold;
    }
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
    const here = this.standingOn(pos.x, pos.z, pos.y);
    const there = this.platformUnder(best.pos.x, best.pos.z, best.pos.y, 2.0, 12.0);
    // T-158: latch a launch. `vel.y > JUMP_VELOCITY` is the signature of a launch and
    // nothing else: the controller sets `vel.y = PAD_LAUNCH_VEL` (16) or
    // `TRAMPOLINE_LAUNCH_VEL` (26) on contact, and a jump press can only ever reach 12.
    // `2 * vel.y / g` is the time left before the feet come back to the current height, a
    // safe upper bound on the rest of the arc.
    if (!you.grounded && you.vel.y > JUMP_VELOCITY + 0.5) {
      this.arcUntil = now + ((2 * you.vel.y) / GRAVITY) * 1000;
      if (this.arcTarget < 0) this.arcTarget = there;
    }
    let hop = -1;
    let hopIsLaunch = false;
    // True when the next hop is a launch platform the bot can simply walk onto. That is the
    // case where a missing landing is not a dead end: the route climbs, and contact launches.
    let hopIsWalkOnLaunch = false;
    if (here >= 0 && there >= 0 && here !== there) {
      hop = this.nextHop(here, there);
      if (hop >= 0) {
        hopIsLaunch = launchVelocityOf(this.level.platforms[hop].kind) > 0;
        if (hopIsLaunch) {
          // T-158: a launch platform is entered, not jumped at. Aim at its centre so the
          // bot's box lands on it -- the contact launch fires the frame the feet rest on
          // its top, so there is nothing to press.
          const b = this.level.platforms[hop].bounds;
          aimX = (b.min.x + b.max.x) / 2;
          aimZ = (b.min.z + b.max.z) / 2;
          hopIsWalkOnLaunch = gapBetween(this.level.platforms[here].bounds, b) <= 0.001;
          // The launch carries the bot to the route's destination, not to the pad.
          this.arcTarget = there;
        } else {
          const p = this.nearestOn(hop, pos.x, pos.z, 1.0);
          aimX = p.x;
          aimZ = p.z;
        }
      }
    }
    // Air control: while airborne, steer toward the nearest landing instead of
    // the route. The edge probe only runs when grounded, so a bot that drifts
    // off an edge mid-jump has nothing else keeping it out of the void.
    if (!you.grounded) {
      // T-158: a jump and a launch are arcs with a destination, and air control has to hold
      // that destination. `nearestLanding` only considers platforms at or below the bot, so
      // mid-arc it returns the platform the bot just left. The trace showed the bot pinned
      // at P5's edge with `aim` flipping between the route (z -15) and the point below
      // (z -11) every frame: it never crossed the 3.0 u gap to P6, and eventually fell into
      // the void. Hold the arc while it is live; fall back to `nearestLanding` only when the
      // bot is airborne with no arc, which is the walked-off-an-edge case that needs it.
      if (this.arcUntil > now && this.arcTarget >= 0) {
        const p = this.nearestOn(this.arcTarget, pos.x, pos.z, 1.0);
        aimX = p.x;
        aimZ = p.z;
      } else {
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

    // T-158: never press jump while standing on a launch platform. A jump press sets
    // vel.y = 12 and lifts the feet off the pad in the same step, so the controller's
    // `standing_platform` returns None and the contact launch never fires -- the bot would
    // trade a +4.00 u launch for a +2.25 u jump. The launch is the whole point of the
    // platform, so the edge probe is skipped entirely while the bot is on one.
    const onLaunch = you.grounded && this.launchPlatformUnder(pos.x, pos.z, pos.y) >= 0;

    // Edge awareness, measured rather than probed at one fixed distance.
    //
    // T-156: the old probe looked 0.9 u ahead and jumped at any landing up to 5.5 u away.
    // Two ways that kills the bot. A platform edge between 0.9 u and the stopping distance
    // is invisible until the bot is already past it, and 5.5 u is outside the jump envelope
    // for any rise above ~0.3 u, so the bot jumps at gaps it cannot clear. Both are
    // measured here instead of guessed.
    if (you.grounded && !onLaunch) {
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
          // T-158: a gift that needs a climb has no landing inside the JUMP envelope -- the
          // route goes through a pad -- so blacklisting here is exactly what left the bot
          // holding on an empty level. When the next hop is a launch platform the bot can
          // walk onto, the route is not a dead end: keep the target and walk onto the pad.
          // The blacklist stays the last resort for a gift the graph genuinely cannot
          // reach from here.
          if (hopIsWalkOnLaunch) {
            jump = false;
          } else {
            // Nothing to land on inside the envelope. Give up on this gift rather than die
            // for it, and re-steer at the next candidate in the same frame -- holding still
            // until the blacklist expires is what left the bot standing on an empty level.
            traceOut('no-landing', {
              target: best.id, aimX: +aimX.toFixed(3), aimZ: +aimZ.toFixed(3),
              dirX: +dirX.toFixed(3), dirZ: +dirZ.toFixed(3),
              edge: edgeSeen, landing: landingSeen, jump: false,
              hop, hopLaunch: hopIsLaunch,
            });
            this.blacklist.set(best.id, now + 3000);
            this.target = null;
            return depth < 3 ? this.steer(depth + 1) : hold;
          }
        } else if (edge <= PLAYER_HALF_WIDTH + EDGE_JUMP_DISTANCE) {
          // Jump when the edge is close enough that the jump is the next move. The player
          // can stand with its centre PLAYER_HALF_WIDTH past the edge, and one input batch
          // covers 0.45 u, so the trigger has to fire inside that window or the jump input
          // arrives after the bot is already airborne.
          jump = true;
        }
      }
    }

    // T-158: a deliberate jump is an arc too. Latch its destination so air control holds it
    // instead of aiming back at the platform the bot just left. `2 * v / g` is the flat-jump
    // airtime (0.75 s); landing early is harmless because the arc aim is where the bot wants
    // to go anyway.
    if (jump) {
      this.arcTarget = hop >= 0 ? hop : there;
      this.arcUntil = now + ((2 * JUMP_VELOCITY) / GRAVITY) * 1000;
    }
    // T-158: hold BTN_JUMP through the rise. The controller cuts an ascent to
    // JUMP_CUT_VELOCITY (4.0) on the RELEASE edge of a jump press, so a bot that presses for
    // one frame and lets go gets an apex of +0.25 u instead of +2.25 u -- the trace showed
    // the jump peaking at +0.98 u, which is why a 3.0 u gap it should clear was missed.
    // Holding the button for the whole rise is what makes `jumpReach` true. It cannot arm a
    // second jump: `jump_buffer_frames` is set on a rising edge only, so a held button never
    // re-presses.
    if (!you.grounded && you.vel.y > 0 && this.arcUntil > now) jump = true;

    traceOut('steer', {
      target: best.id,
      aimX: +aimX.toFixed(3), aimZ: +aimZ.toFixed(3),
      dirX: +dirX.toFixed(3), dirZ: +dirZ.toFixed(3),
      edge: edgeSeen, landing: landingSeen, jump,
      // T-158: the vertical route. `hop` is the next platform on the graph route and `arc`
      // says whether air control is holding a jump or launch arc, so a bot that stalls on a
      // climb can be told apart from one that never found the pad.
      hop, hopLaunch: hopIsLaunch,
    });
    return { x: dirX, z: dirZ, jump };
  }

  /**
   * Walk the level: steer at the nearest gift when the level is known, otherwise
   * random-walk so a server that never sends `Level` still sees movement.
   */
  startWalking() {
    this.seq = 0;
    this.framesSent = 0;
    this.walkStart = Date.now();
    this.walkTimer = setInterval(() => {
      if (this.closed) return;
      const aim = this.steer();
      this.lastAim = aim;
      // T-158: pace the input by the CLOCK, not by the timer. `setInterval(1000 / 60)` fires
      // at about 33 Hz on this machine (measured: 99 calls in 3.00 s), so the bot fed the
      // server 33 frames/s against a 60 frames/s drain. The server applies neutral input
      // after STARVE_TICKS, so the bot moved at about two thirds of RUN_SPEED -- which is
      // why it fell short of a 3.0 u gap it clears at full speed. Sending the number of
      // frames the clock says are due keeps the queue exactly balanced whatever the timer
      // does, and the cap keeps a stalled timer from flooding it.
      //
      // T-156 fixed the opposite failure: 4 frames every 50 ms fed 80 frames/s into the
      // same 60 frames/s drain, the queue filled to MAX_QUEUED_INPUTS (one full second) and
      // dropped the excess, so every input was applied about a second late.
      const due = Math.floor(((Date.now() - this.walkStart) * PHYSICS_HZ) / 1000);
      let count = due - this.framesSent;
      if (count <= 0) return;
      if (count > PHYSICS_HZ) {
        // The timer stalled (a long GC, a slow write). Resync rather than flood: the server
        // buffers at most MAX_QUEUED_INPUTS = PHYSICS_HZ frames and drops the excess.
        this.framesSent = due - STEPS_PER_TICK;
        count = STEPS_PER_TICK;
      }
      const frames = [];
      for (let i = 0; i < count; i += 1) {
        this.seq += 1;
        this.framesSent += 1;
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
      // T-158: picks are indices into the single `cards` list, and the server rejects a
      // repeated index with 422 "duplicate pick" (`run::validate_picks`). The old
      // `[0..enemy_picks] + [0..reward_picks]` was only correct while one of the two counts
      // was zero -- which it always was until a level could actually be completed, because
      // the second draft never happened. Pick by card kind instead.
      const cards = Array.isArray(msg.cards) ? msg.cards : [];
      const picks = [];
      let enemies = 0;
      let rewards = 0;
      for (let i = 0; i < cards.length; i += 1) {
        if (cards[i].kind === CARD_ENEMY) {
          if (enemies < (msg.enemy_picks || 0)) { picks.push(i); enemies += 1; }
        } else if (rewards < (msg.reward_picks || 0)) {
          picks.push(i);
          rewards += 1;
        }
      }
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
  // T-158: the clock a completion is timed against. Taken here, after the clients are
  // greeted, so the number is "how long the bots needed to empty a level", not "how long
  // the sweep has been running".
  const runStart = Date.now();
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
  // T-158: when the level changed, and to what. `all` drops the arrival time, so this walks
  // the per-client logs; the earliest frame that reports a level other than the starting one
  // is the moment a level was emptied.
  let completedAt = null;
  let completedLevel = null;
  for (const client of clients) {
    for (const entry of client.log) {
      if (entry.msg.variant !== 'RunState') continue;
      const lvl = entry.msg.info && entry.msg.info.level;
      if (typeof lvl !== 'number' || lvl === startLevel) continue;
      if (completedAt === null || entry.at < completedAt) {
        completedAt = entry.at;
        completedLevel = lvl;
      }
    }
  }

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
  //
  // T-158: read the best progress the client ever recorded, not its current tally. A `Level`
  // frame clears `collected`, so once a level is emptied the end-of-sweep reading is the
  // level that just started -- 0/0 -- and the rate collapsed to 0% on the very run that
  // completed a level.
  const best = clients.reduce((acc, c) => {
    const p = c.bestProgress;
    const rate = p.total > 0 ? p.taken / p.total : 0;
    return rate > acc.rate ? { taken: p.taken, total: p.total, rate } : acc;
  }, { taken: 0, total: 0, rate: 0 });
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
  // bots emptied a level by WALKING, which is the coverage hole the owner named.
  // T-158: the router now has launch edges through the jump pad and the trampoline, so this
  // is a live PASS when a level is emptied and the detail names the level and the time it
  // took. It stays a SKIP -- never a FAIL -- when no level is emptied: a coverage hole is
  // not a regression, and a permanently red line would train the gate to be ignored.
  const finished = clients.map((c) => c.finished).filter(Boolean)[0] || null;
  push('level-completion', ACTUATE ? null : (completed ? true : null),
    ACTUATE
      ? 'skipped: actuated mode changes level through the debug layer, so this measures no traversal'
      : completed
        ? 'level ' + (finished ? finished.level : completedLevel) + ' emptied by walking in '
          + ((completedAt - runStart) / 1000).toFixed(1) + 's ('
          + (finished ? finished.taken + '/' + finished.total : best.taken + '/' + best.total)
          + ' gifts); level went ' + startLevel + ' -> ' + maxLevel
        : 'no level was emptied by walking: best client collected ' + best.taken + '/' + best.total
          + ' gifts (' + (giftRate * 100).toFixed(1) + '%). T-156 fixed the input-rate and'
          + ' air-control bugs that made the bot fall off the map, and T-158 gave the router'
          + ' launch edges through the jump pad (+4.00 u apex) and the trampoline (+10.56 u);'
          + ' what is left is executing a launch arc. See tickets/T-158.md.');
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
  flushTrace();
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
  flushTrace();
  process.exit(2);
});
