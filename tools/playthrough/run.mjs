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
import { readFileSync } from 'node:fs';
import { encodeClientMsg, decodeServerMsg } from '../voting-conformance/wire.mjs';

// The wire protocol version this sweep speaks. Mirrors PROTOCOL_VERSION in
// server/crates/protocol; a mismatch shows up as a Welcome-time Error frame.
const PROTOCOL_VERSION = 8;

// Classes are 0..=CLASS_GLIDER on the wire; design/content.json ships four.
const CLASS_COUNT = 4;

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
    'blocked by the starting-lives balance decision (a run starts with 0 spare lives, '
    + 'so the first fall ends it) and by the bot holding still when every remaining gift '
    + 'is behind a gap it cannot probe; see tickets/T-139.md',
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

  /**
   * Is there a surface under `(x, z)` that a player standing at height `y`
   * could stand on?
   *
   * Ramps are walkable along their whole incline, so their footprint counts at
   * any height inside the box. Every other platform only counts when its top
   * face is at or just below the player's feet -- `upTol` widens that for
   * landing probes, where a step up is allowed.
   */
  groundAt(x, z, y, upTol = 0.6, downTol = 3.0) {
    return this.platformUnder(x, z, y, upTol, downTol) >= 0;
  }

  /** Index of the platform a player at `(x, z, y)` could stand on, or -1. */
  platformUnder(x, z, y, upTol = 0.6, downTol = 3.0) {
    const platforms = this.level && this.level.platforms;
    if (!Array.isArray(platforms)) return -1;
    for (let i = 0; i < platforms.length; i += 1) {
      const b = platforms[i].bounds;
      if (x < b.min.x - 0.25 || x > b.max.x + 0.25) continue;
      if (z < b.min.z - 0.25 || z > b.max.z + 0.25) continue;
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

  /** Nearest point on platform `i` to `(x, z)`. */
  nearestOn(i, x, z) {
    const b = this.level.platforms[i].bounds;
    return {
      x: Math.min(Math.max(x, b.min.x), b.max.x),
      z: Math.min(Math.max(z, b.min.z), b.max.z),
    };
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
  steer() {
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
        const p = this.nearestOn(hop, pos.x, pos.z);
        aimX = p.x;
        aimZ = p.z;
      }
    }
    // Air control: while airborne, steer toward the nearest landing instead of
    // the route. The edge probe only runs when grounded, so a bot that drifts
    // off an edge mid-jump has nothing else keeping it out of the void.
    if (!you.grounded) {
      const landing = this.nearestLanding(pos.x, pos.z, pos.y);
      if (landing) {
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
    let jump = Boolean(you.grounded) && (rise > 0.3 || this.stuckFrames > 4);

    // Edge awareness. The first version of this bot steered in a straight line
    // and walked off the platform: the server logged "player out; spectating
    // cause=0" (DEATH_VOID) for every client. Probe one step ahead; if there is
    // no surface, look for one inside jump range and jump at it, otherwise give
    // up on this gift rather than die for it.
    if (you.grounded && !this.groundAt(pos.x + dirX * 0.9, pos.z + dirZ * 0.9, pos.y)) {
      let landing = null;
      for (let d = 1.5; d <= 5.5; d += 0.5) {
        if (this.groundAt(pos.x + dirX * d, pos.z + dirZ * d, pos.y, 2.0, 12.0)) {
          landing = d;
          break;
        }
      }
      if (landing === null) {
        this.blacklist.set(best.id, now + 3000);
        this.target = null;
        return hold;
      }
      jump = true;
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
      for (let i = 0; i < 4; i += 1) {
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
    }, 50);
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
    } else if (msg.variant === 'Shop') {
      const item = (msg.items || []).find((entry) => entry.stock > 0);
      if (item) {
        this.send({ variant: 'Buy', item: item.id });
        this.buys = (this.buys || 0) + 1;
      }
    }
  }

  close() {
    if (this.ws && !this.closed) this.ws.close();
  }
}

function report(name, ok, detail) {
  const label = ok === null ? 'SKIP' : ok ? 'PASS' : 'FAIL';
  console.log(label + ' - ' + name + ' (' + detail + ')');
  return ok !== false;
}

async function main() {
  const clients = [];
  for (let i = 0; i < CLIENTS; i += 1) {
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
  push('connect', connected === CLIENTS, connected + '/' + CLIENTS + ' clients connected');

  // --- let the run play out
  for (const client of clients) if (!client.closed) client.startWalking();
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

  const all = clients.flatMap((client) => client.log.map((entry) => entry.msg));
  const variants = new Set(all.map((msg) => msg.variant));
  const runStates = all.filter((msg) => msg.variant === 'RunState');
  const levels = runStates.map((msg) => msg.info && msg.info.level).filter((n) => typeof n === 'number');
  const phases = new Set(runStates.map((msg) => msg.info && msg.info.phase).filter((p) => p !== undefined));
  const maxLevel = levels.length ? Math.max(...levels) : 0;

  push('welcome', variants.has('Welcome'), 'Welcome frames: ' + all.filter((m) => m.variant === 'Welcome').length);
  push('runstate', runStates.length > 0, 'RunState frames: ' + runStates.length);
  // The sweep has no navigation bot, so a level only completes if the random
  // walk happens to collect every gift. Checks that need a completed level are
  // reported as SKIP rather than FAIL when that did not happen, so the sweep's
  // signal stays about the protocol and the lobby flow.
  const completed = maxLevel >= 2;
  const gate = (name, ok, detail) => push(name, completed ? ok : null, detail);
  // Gift progress is the number that explains a stalled level: a bot that
  // reaches level 1 and stops is either dying or not finding the gifts, and
  // the two look identical from the level counter alone.
  const totalGifts = clients.reduce(
    (n, c) => (c.level && Array.isArray(c.level.gifts) ? Math.max(n, c.level.gifts.length) : n),
    0,
  );
  const best = { taken: clients.reduce((n, c) => Math.max(n, c.collected.size), 0), total: totalGifts };
  push('level-progression', completed,
    'highest level reached: ' + maxLevel + ' (best client collected ' + best.taken + '/' + best.total + ' gifts)');
  // T-142: gift progress as its own check, not just a number inside the skip message.
  // `level-progression` needs a completed level, which the bots cannot reach (T-139),
  // so it is always skipped and a traversal regression would go unnoticed. This check
  // needs no completion: it asks whether the bots can still move around the level and
  // pick things up at a reasonable rate. Measured 28-50% across runs; the floor is 20%,
  // which leaves headroom for the run-to-run variance while still failing if a level
  // change makes traversal much worse.
  const GIFT_RATE_MIN = 0.2;
  const giftRate = best.total > 0 ? best.taken / best.total : 0;
  push('gift-rate', best.total > 0 && giftRate >= GIFT_RATE_MIN,
    'best client collected ' + best.taken + '/' + best.total + ' gifts ('
    + (giftRate * 100).toFixed(1) + '%) in ' + SECONDS + 's; threshold >= '
    + (GIFT_RATE_MIN * 100).toFixed(0) + '%');
  // `draft` does not need a completed level: the first draft fires before level 1
  // starts, so it is a live check even when the bot never finishes a level.
  const drafts = all.filter((m) => m.variant === 'Draft').length;
  const draftAnswers = clients.reduce((n, c) => n + (c.drafts || 0), 0);
  push('draft', drafts > 0 && draftAnswers > 0, 'Draft frames: ' + drafts + ', answered: ' + draftAnswers);
  gate('vote', variants.has('VoteOpen'), 'VoteOpen frames: ' + all.filter((m) => m.variant === 'VoteOpen').length);
  gate('shop', variants.has('Shop'), 'Shop frames: ' + all.filter((m) => m.variant === 'Shop').length);
  push('phase-coverage', phases.size >= 2, 'distinct phases: ' + phases.size + ' [' + [...phases].join(',') + ']');

  const decodeFailures = clients.flatMap((client) => client.decodeFailures);
  push('decode', decodeFailures.length === 0, decodeFailures.length + ' undecodable frames' + (decodeFailures.length ? ': ' + decodeFailures[0] : ''));

  const errorFrames = clients.flatMap((client) => client.errors).filter((e) => e.code !== undefined);
  const classRejects = errorFrames.filter((e) => /class can only be changed/i.test(e.message || ''));
  const otherErrors = errorFrames.filter((e) => !/class can only be changed/i.test(e.message || ''));
  push('class-select', classRejects.length === 0,
    classRejects.length + '/' + CLIENTS + ' clients had their class rejected' +
    (classRejects.length ? ' (server: "' + classRejects[0].message + '")' : ''));
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
