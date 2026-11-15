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

const URL_ = argValue('--url', process.env.PLAYTHROUGH_URL || '');
const CLIENTS = Number(argValue('--clients', process.env.PLAYTHROUGH_CLIENTS || '8'));
const SECONDS = Number(argValue('--seconds', process.env.PLAYTHROUGH_SECONDS || '75'));

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
    this.pos = null;
    this.collected = new Set();
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
        } else if (msg.variant === 'Snapshot') {
          this.pos = msg.you && msg.you.pos ? msg.you.pos : null;
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
   * Steer toward the nearest uncollected gift.
   *
   * Returns `null` when there is nothing to steer to (no `Level` yet, no `Snapshot`
   * yet, or every gift taken), which keeps the random walk as the fallback.
   */
  steer() {
    const level = this.level;
    const pos = this.pos;
    if (!level || !pos || !Array.isArray(level.gifts)) return null;
    let best = null;
    let bestD = Infinity;
    for (const gift of level.gifts) {
      if (this.collected.has(gift.id)) continue;
      const dx = gift.pos.x - pos.x;
      const dz = gift.pos.z - pos.z;
      const d = dx * dx + dz * dz;
      if (d < bestD) {
        bestD = d;
        best = gift;
      }
    }
    if (!best) return null;
    if (bestD < 2.25) this.collected.add(best.id);
    const dx = best.pos.x - pos.x;
    const dz = best.pos.z - pos.z;
    const len = Math.hypot(dx, dz) || 1;
    const rise = best.pos.y - pos.y;
    return {
      x: dx / len,
      z: dz / len,
      jump: Boolean(pos.grounded) && (rise > 0.3 || len > 2.0),
    };
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
  for (let i = 0; i < CLIENTS; i += 1) clients.push(new Client('pt-' + i));

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
  push('level-progression', completed, 'highest level reached: ' + maxLevel);
  gate('draft', variants.has('Draft'), 'Draft frames: ' + all.filter((m) => m.variant === 'Draft').length);
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

  const ok = results.every((r) => r.ok !== false);
  for (const r of results) report(r.name, r.ok, r.detail);
  const passed = results.filter((r) => r.ok === true).length;
  const skipped = results.filter((r) => r.ok === null).length;
  console.log('playthrough: ' + passed + '/' + results.length + ' checks passed' +
    (skipped ? ', ' + skipped + ' skipped (level never completed)' : ''));
  process.exit(ok ? 0 : 1);
}

main().catch((error) => {
  console.error('playthrough: ' + error.message);
  process.exit(2);
});
