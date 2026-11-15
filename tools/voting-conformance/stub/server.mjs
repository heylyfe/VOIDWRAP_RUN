#!/usr/bin/env node
/**
 * tools/voting-conformance/stub/server.mjs
 *
 * A self-driving contract double for the real Voidwrap server.
 *
 * It speaks the real wire format: bincode 1.x (fixint, little endian) over
 * binary WebSocket frames. It owns level progression exactly like the real
 * server does, so the conformance kit only has to connect, say `Hello`, and
 * observe.
 *
 * Lifecycle (per level):
 *
 *   RunState(phase = PHASE_ENEMY_DRAFT)
 *     -> after LEVEL_MS
 *   RunState(phase = PHASE_PLAYING) + Draft(3 cards)
 *     -> on DraftPick from every client, or after DRAFT_MS
 *   VoteOpen(level, 3 options, countdown_s)
 *     -> VoteState every 250 ms
 *     -> after countdown_s
 *   VoteResult(winner, id, name, kind, permanent)
 *     -> RunState(next level) and loop
 *
 * Environment:
 *   VOTE_STUB_PORT          listen port, 0 = ephemeral (default 0)
 *   VOTE_STUB_LEVEL_MS      draft-phase dwell time (default 150)
 *   VOTE_STUB_DRAFT_MS      draft pick window (default 120)
 *   VOTE_STUB_COUNTDOWN_S   vote countdown in seconds (default 2)
 *   VOTE_STUB_VIOLATION     one of the six violation switches, or empty
 *
 * The module is import-safe: it only starts listening when executed directly.
 * `loadPool()` is exported so the kit validates against the exact same pool
 * the stub draws from.
 */

import { createServer } from 'node:http';
import { createHash } from 'node:crypto';
import { fileURLToPath } from 'node:url';
import { resolve } from 'node:path';

import * as wire from '../wire.mjs';
import * as registry from '../registry.mjs';

/* ------------------------------------------------------------------ *
 * Protocol constants
 * ------------------------------------------------------------------ */

export const PROTOCOL_VERSION = 8;

export const PHASE_ENEMY_DRAFT = 0;
export const PHASE_PLAYING = 1;
export const PHASE_REWARD_DRAFT = 2;
export const PHASE_OVER = 3;

export const OPTIONS_PER_SESSION = 3;
export const CARDS_PER_DRAFT = 3;

export const ERR_NO_SESSION = 7;
export const ERR_BAD_OPTION = 8;
export const ERR_ALREADY_VOTED = 9;
export const ERR_PROTOCOL = 10;

const WS_GUID = '258EAFA5-E914-47DA-95CA-C5AB0DC85B11';

const VIOLATIONS = [
  'majority_inverted',
  'tie_ignored',
  'cap_ignored',
  'permanence_dropped',
  'rejoin_forgotten',
  'late_vote_accepted',
];

/* ------------------------------------------------------------------ *
 * bincode 1.x primitives (fixint, little endian)
 * ------------------------------------------------------------------ */

class Writer {
  constructor() {
    this.out = [];
  }

  u8(value) {
    this.out.push(value & 0xff);
    return this;
  }

  u16(value) {
    this.out.push(value & 0xff, (value >>> 8) & 0xff);
    return this;
  }

  u32(value) {
    this.out.push(value & 0xff, (value >>> 8) & 0xff, (value >>> 16) & 0xff, (value >>> 24) & 0xff);
    return this;
  }

  u64(value) {
    const big = BigInt(value);
    for (let i = 0n; i < 8n; i += 1n) {
      this.out.push(Number((big >> (8n * i)) & 0xffn));
    }
    return this;
  }

  f32(value) {
    const b = Buffer.alloc(4);
    b.writeFloatLE(Number(value), 0);
    for (const byte of b) this.out.push(byte);
    return this;
  }

  bool(value) {
    return this.u8(value ? 1 : 0);
  }

  str(value) {
    const bytes = Buffer.from(String(value), 'utf8');
    this.u64(bytes.length);
    for (const byte of bytes) this.out.push(byte);
    return this;
  }

  vecU8(values) {
    this.u64(values.length);
    for (const value of values) this.u8(value);
    return this;
  }

  vecU32(values) {
    this.u64(values.length);
    for (const value of values) this.u32(value);
    return this;
  }

  bytes() {
    return Buffer.from(this.out);
  }
}

class Reader {
  constructor(buffer) {
    this.buffer = buffer;
    this.pos = 0;
  }

  need(count) {
    if (this.pos + count > this.buffer.length) {
      throw new Error('bincode: unexpected end of buffer');
    }
  }

  u8() {
    this.need(1);
    const value = this.buffer[this.pos];
    this.pos += 1;
    return value;
  }

  u16() {
    this.need(2);
    const value = this.buffer.readUInt16LE(this.pos);
    this.pos += 2;
    return value;
  }

  u32() {
    this.need(4);
    const value = this.buffer.readUInt32LE(this.pos);
    this.pos += 4;
    return value;
  }

  u64() {
    this.need(8);
    const value = this.buffer.readBigUInt64LE(this.pos);
    this.pos += 8;
    return Number(value);
  }

  bool() {
    return this.u8() !== 0;
  }

  str() {
    const length = this.u64();
    this.need(length);
    const value = this.buffer.subarray(this.pos, this.pos + length).toString('utf8');
    this.pos += length;
    return value;
  }

  vecU8() {
    const length = this.u64();
    this.need(length);
    const values = Array.from(this.buffer.subarray(this.pos, this.pos + length));
    this.pos += length;
    return values;
  }

  vecU32() {
    const length = this.u64();
    const values = [];
    for (let i = 0; i < length; i += 1) values.push(this.u32());
    return values;
  }
}

/* ------------------------------------------------------------------ *
 * Mirror codec for the server -> client direction
 *
 * wire.mjs owns the client -> server encoder and the server -> client
 * decoder. The stub needs the two opposite directions. If a later revision
 * of wire.mjs exports them we use those; otherwise we use the mirror below,
 * which is kept honest by round-tripping every emitted frame through
 * wire.decodeServerMsg before it reaches the socket.
 * ------------------------------------------------------------------ */

// The real protocol's variant indices (server/crates/protocol/src/generated.rs).
// They are not contiguous: the stub only emits seven of the thirteen ServerMsg
// variants, so the numbers below are the real ones, not a compacted list.
const SERVER_VARIANTS = {
  Welcome: 0,
  Error: 4,
  RunState: 6,
  Draft: 7,
  VoteOpen: 10,
  VoteState: 11,
  VoteResult: 12,
};

// ClientMsg indices, likewise the real ones: Hello 0, Inputs 1, Ping 2,
// Restart 3, DraftPick 4, Buy 5, ChooseClass 6, Vote 7.
const CLIENT_VARIANTS = {
  0: 'Hello',
  2: 'Ping',
  4: 'DraftPick',
  6: 'ChooseClass',
  7: 'Vote',
};

function localEncodeServerMsg(msg) {
  const variant = SERVER_VARIANTS[msg.type];
  if (variant === undefined) {
    throw new Error('unknown server message type: ' + String(msg.type));
  }
  const w = new Writer();
  w.u32(variant);
  switch (msg.type) {
    case 'Welcome':
      w.u16(msg.protocol_version);
      w.u32(msg.player_id);
      w.u32(msg.tick);
      break;
    case 'RunState':
      // The real RunInfo: phase, level, lives, currency, Tuning, fog_density_mul,
      // then three string vectors.
      w.u8(msg.info.phase);
      w.u32(msg.info.level);
      w.u32(msg.info.lives);
      w.u32(msg.info.currency);
      w.f32(msg.info.tuning.run_speed_mul);
      w.f32(msg.info.tuning.jump_mul);
      w.f32(msg.info.tuning.gravity_mul);
      w.f32(msg.info.tuning.dash_speed_mul);
      w.f32(msg.info.tuning.dash_cooldown_mul);
      w.u8(msg.info.tuning.air_jumps);
      w.f32(msg.info.tuning.friction_mul);
      w.f32(msg.info.fog_density_mul);
      w.u64(msg.info.enemies.length);
      for (const id of msg.info.enemies) w.str(id);
      w.u64(msg.info.upgrades.length);
      for (const id of msg.info.upgrades) w.str(id);
      w.u64(msg.info.curses.length);
      for (const id of msg.info.curses) w.str(id);
      break;
    case 'Draft':
      w.vecU8(msg.enemy_picks);
      w.vecU8(msg.reward_picks);
      w.u64(msg.cards.length);
      for (const card of msg.cards) {
        w.str(card.id);
        w.u8(card.kind);
      }
      break;
    case 'VoteOpen':
      // VoteOption is { index: u8, id: string, name: string, kind: u8 }.
      w.u32(msg.level);
      w.u64(msg.options.length);
      for (const option of msg.options) {
        w.u8(option.index);
        w.str(option.id);
        w.str(option.name ?? option.id);
        w.u8(option.kind);
      }
      w.u16(msg.countdown_s);
      break;
    case 'VoteState':
      w.vecU32(msg.counts);
      w.vecU32(msg.voted);
      w.u32(msg.seconds_left);
      break;
    case 'VoteResult':
      w.u8(msg.winner);
      w.str(msg.id);
      w.str(msg.name);
      w.u8(msg.kind);
      w.bool(msg.permanent);
      break;
    case 'Error':
      w.u16(msg.code);
      w.str(msg.message);
      break;
    default:
      throw new Error('unhandled server message type: ' + String(msg.type));
  }
  return w.bytes();
}

function localDecodeClientMsg(bytes) {
  const r = new Reader(Buffer.from(bytes));
  const variant = r.u32();
  const type = CLIENT_VARIANTS[variant];
  if (type === undefined) {
    throw new Error('unknown client message variant: ' + variant);
  }
  switch (type) {
    case 'Hello':
      return { type, protocol_version: r.u16(), name: r.str() };
    case 'ChooseClass':
      return { type, class: r.u8() };
    case 'DraftPick':
      return { type, picks: r.vecU8() };
    case 'Vote':
      return { type, option: r.u8() };
    case 'Ping':
      return { type, t: r.u32() };
    default:
      throw new Error('unhandled client message type: ' + type);
  }
}

const CODEC = {
  encodeServerMsg: typeof wire.encodeServerMsg === 'function' ? wire.encodeServerMsg : localEncodeServerMsg,
  decodeClientMsg: typeof wire.decodeClientMsg === 'function' ? wire.decodeClientMsg : localDecodeClientMsg,
};

function encodeServerMsg(msg) {
  const bytes = CODEC.encodeServerMsg(msg);
  try {
    wire.decodeServerMsg(bytes);
  } catch (error) {
    throw new Error(
      'stub emitted a ' + msg.type + ' frame that wire.mjs cannot decode: ' + error.message,
    );
  }
  return bytes;
}

/* ------------------------------------------------------------------ *
 * Registry pool
 * ------------------------------------------------------------------ */

const FALLBACK_POOL = [
  { id: 'ironhide', name: 'Ironhide', kind: 1, minLevel: 1, cap: 2, permanent: true },
  { id: 'overcharge', name: 'Overcharge', kind: 0, minLevel: 1, cap: 2, permanent: false },
  { id: 'quickstep', name: 'Quickstep', kind: 0, minLevel: 1, cap: 2, permanent: false },
  { id: 'bulwark', name: 'Bulwark', kind: 1, minLevel: 2, cap: 3, permanent: true },
  { id: 'ember', name: 'Ember', kind: 0, minLevel: 2, cap: 2, permanent: false },
  { id: 'lodestone', name: 'Lodestone', kind: 1, minLevel: 3, cap: 2, permanent: true },
  { id: 'fracture', name: 'Fracture', kind: 0, minLevel: 3, cap: 2, permanent: false },
  { id: 'wardstone', name: 'Wardstone', kind: 1, minLevel: 4, cap: 2, permanent: true },
  { id: 'cinder', name: 'Cinder', kind: 0, minLevel: 4, cap: 2, permanent: false },
  { id: 'nullfield', name: 'Nullfield', kind: 1, minLevel: 5, cap: 2, permanent: true },
  { id: 'gloam', name: 'Gloam', kind: 0, minLevel: 5, cap: 2, permanent: false },
];

const POOL_KEYS = [
  'POOL',
  'VOTE_POOL',
  'OPTIONS',
  'VOTE_OPTIONS',
  'REGISTRY',
  'ENTRIES',
  'CARDS',
  'VOTE_CARDS',
  'pool',
  'votePool',
  'options',
  'voteOptions',
  'registry',
  'entries',
  'cards',
  'default',
];

function pickString(entry, keys) {
  for (const key of keys) {
    const value = entry[key];
    if (typeof value === 'string' && value.length > 0) return value;
  }
  return null;
}

function pickInt(entry, keys, fallback) {
  for (const key of keys) {
    const value = entry[key];
    if (typeof value === 'number' && Number.isFinite(value)) return Math.trunc(value);
  }
  return fallback;
}

function pickBool(entry, keys, fallback) {
  for (const key of keys) {
    const value = entry[key];
    if (typeof value === 'boolean') return value;
  }
  return fallback;
}

function normalizeEntry(entry) {
  if (!entry || typeof entry !== 'object' || Array.isArray(entry)) return null;
  const id = pickString(entry, ['id', 'key', 'slug', 'name']);
  if (!id) return null;
  const kind = pickInt(entry, ['kind', 'type', 'category'], 0);
  const minLevel = pickInt(entry, ['minLevel', 'min_level', 'min', 'level'], 1);
  const cap = pickInt(entry, ['cap', 'max', 'maxSelections', 'max_selections', 'limit', 'maxPicks'], 1);
  const permanent = pickBool(entry, ['permanent', 'isPermanent', 'is_permanent', 'persistent'], kind === 1);
  const name = pickString(entry, ['name', 'title', 'label']) || id;
  return { id, name, kind, minLevel, cap, permanent };
}

function toList(value) {
  if (Array.isArray(value)) return value;
  if (value instanceof Map) return Array.from(value.values());
  if (value && typeof value === 'object') {
    if (Array.isArray(value.options)) return value.options;
    if (Array.isArray(value.entries)) return value.entries;
    if (Array.isArray(value.pool)) return value.pool;
  }
  return null;
}

function extractEntries(mod) {
  if (!mod) return [];
  const candidates = [];
  for (const key of POOL_KEYS) {
    if (key in mod) candidates.push(mod[key]);
  }
  for (const key of Object.keys(mod)) {
    if (!POOL_KEYS.includes(key)) candidates.push(mod[key]);
  }
  for (const candidate of candidates) {
    const list = toList(candidate);
    if (!list) continue;
    const normalized = [];
    for (const item of list) {
      const entry = normalizeEntry(item);
      if (entry) normalized.push(entry);
    }
    if (normalized.length >= OPTIONS_PER_SESSION) return normalized;
  }
  return [];
}

let cachedPool = null;
let cachedStubPool = null;

/**
 * The vote pool the stub draws from. Prefers whatever `registry.mjs` exposes
 * in a recognisable shape and falls back to a built-in pool otherwise.
 */
/**
 * The stub's own option pool: the real registry with a deterministic subset
 * marked permanent-capable. Permanence is a run-state property, not a registry
 * field, so the stub models it here and the kit derives the same subset from
 * this function — both sides agree by construction.
 */
export function stubPool() {
  if (cachedStubPool) return cachedStubPool;
  cachedStubPool = loadPool().map((option, index) => ({
    ...option,
    permanent: index % 3 === 0 || index < 3,
  }));
  return cachedStubPool;
}

export function loadPool() {
  if (cachedPool) return cachedPool;
  const fromRegistry = extractEntries(registry);
  cachedPool = fromRegistry.length >= OPTIONS_PER_SESSION ? fromRegistry : FALLBACK_POOL;
  return cachedPool;
}

/* ------------------------------------------------------------------ *
 * WebSocket plumbing (RFC 6455, no dependencies)
 * ------------------------------------------------------------------ */

class Conn {
  constructor(socket) {
    this.socket = socket;
    this.buffer = Buffer.alloc(0);
    this.closed = false;
    this.fragments = [];
    this.fragmentOpcode = 0;
    this.playerId = 0;
    this.onMessage = null;

    socket.setNoDelay(true);
    socket.on('data', (chunk) => this._onData(chunk));
    socket.on('close', () => {
      this.closed = true;
    });
    socket.on('error', () => {
      this.closed = true;
    });
  }

  sendBinary(bytes) {
    this._frame(0x2, Buffer.from(bytes));
  }

  close() {
    if (this.closed) return;
    this._frame(0x8, Buffer.alloc(0));
    this.closed = true;
    this.socket.end();
  }

  _frame(opcode, payload) {
    if (this.closed) return;
    const length = payload.length;
    let header;
    if (length < 126) {
      header = Buffer.alloc(2);
      header[1] = length;
    } else if (length < 65536) {
      header = Buffer.alloc(4);
      header[1] = 126;
      header.writeUInt16BE(length, 2);
    } else {
      header = Buffer.alloc(10);
      header[1] = 127;
      header.writeBigUInt64BE(BigInt(length), 2);
    }
    header[0] = 0x80 | opcode;
    this.socket.write(Buffer.concat([header, payload]));
  }

  _onData(chunk) {
    this.buffer = Buffer.concat([this.buffer, chunk]);
    for (;;) {
      const buf = this.buffer;
      if (buf.length < 2) return;
      const b0 = buf[0];
      const b1 = buf[1];
      const fin = (b0 & 0x80) !== 0;
      const opcode = b0 & 0x0f;
      const masked = (b1 & 0x80) !== 0;
      let length = b1 & 0x7f;
      let offset = 2;
      if (length === 126) {
        if (buf.length < offset + 2) return;
        length = buf.readUInt16BE(offset);
        offset += 2;
      } else if (length === 127) {
        if (buf.length < offset + 8) return;
        const big = buf.readBigUInt64BE(offset);
        if (big > BigInt(Number.MAX_SAFE_INTEGER)) {
          this.close();
          return;
        }
        length = Number(big);
        offset += 8;
      }
      let mask = null;
      if (masked) {
        if (buf.length < offset + 4) return;
        mask = buf.subarray(offset, offset + 4);
        offset += 4;
      }
      if (buf.length < offset + length) return;
      let payload = buf.subarray(offset, offset + length);
      if (mask) {
        const unmasked = Buffer.allocUnsafe(length);
        for (let i = 0; i < length; i += 1) unmasked[i] = payload[i] ^ mask[i & 3];
        payload = unmasked;
      }
      this.buffer = buf.subarray(offset + length);
      this._frameIn(fin, opcode, payload);
    }
  }

  _frameIn(fin, opcode, payload) {
    if (opcode === 0x8) {
      this.close();
      return;
    }
    if (opcode === 0x9) {
      this._frame(0xa, payload);
      return;
    }
    if (opcode === 0xa) return;
    if (opcode === 0x0) {
      this.fragments.push(payload);
      if (fin) {
        const full = Buffer.concat(this.fragments);
        const op = this.fragmentOpcode;
        this.fragments = [];
        this.fragmentOpcode = 0;
        this._deliver(op, full);
      }
      return;
    }
    if (opcode === 0x1 || opcode === 0x2) {
      if (!fin) {
        this.fragmentOpcode = opcode;
        this.fragments = [payload];
        return;
      }
      this._deliver(opcode, payload);
    }
  }

  _deliver(opcode, payload) {
    if (opcode !== 0x2) return;
    let msg;
    try {
      msg = CODEC.decodeClientMsg(payload);
    } catch (error) {
      sendTo(this, { type: 'Error', code: ERR_PROTOCOL, message: 'undecodable frame: ' + error.message });
      return;
    }
    if (typeof this.onMessage === 'function') this.onMessage(this, msg);
  }
}

/* ------------------------------------------------------------------ *
 * Runtime configuration
 * ------------------------------------------------------------------ */

function intEnv(name, fallback) {
  const raw = process.env[name];
  if (raw === undefined || raw === '') return fallback;
  const parsed = Number.parseInt(raw, 10);
  return Number.isFinite(parsed) ? parsed : fallback;
}

const PORT = intEnv('VOTE_STUB_PORT', 0);
const LEVEL_MS = intEnv('VOTE_STUB_LEVEL_MS', 150);
const DRAFT_MS = intEnv('VOTE_STUB_DRAFT_MS', 120);
const COUNTDOWN_S = intEnv('VOTE_STUB_COUNTDOWN_S', 1);
const INTERMISSION_MS = intEnv('VOTE_STUB_INTERMISSION_MS', 250);
const VIOLATION = process.env.VOTE_STUB_VIOLATION || '';

if (VIOLATION && !VIOLATIONS.includes(VIOLATION)) {
  process.stderr.write('voting-conformance stub: unknown VOTE_STUB_VIOLATION "' + VIOLATION + '"\n');
}

/* ------------------------------------------------------------------ *
 * Run state machine
 * ------------------------------------------------------------------ */

const run = {
  started: false,
  level: 1,
  phase: PHASE_ENEMY_DRAFT,
  lives: 3,
  currency: 0,
  tick: 0,
  session: null,
  lastSession: null,
  offered: new Map(),
  wonTemporary: new Set(),
  permanents: [],
  nextPlayerId: 1,
  connections: new Set(),
};

let pendingTimer = null;
let draftWaiters = null;

function schedule(fn, ms) {
  if (pendingTimer) {
    clearTimeout(pendingTimer);
    pendingTimer = null;
  }
  pendingTimer = setTimeout(() => {
    pendingTimer = null;
    fn();
  }, ms);
}

function runInfo() {
  return {
    phase: run.phase,
    level: run.level,
    lives: run.lives,
    currency: run.currency,
    tuning: {
      run_speed_mul: 1,
      jump_mul: 1,
      gravity_mul: 1,
      dash_speed_mul: 1,
      dash_cooldown_mul: 1,
      air_jumps: 0,
      friction_mul: 1,
    },
    fog_density_mul: 1,
    enemies: run.permanents ? run.permanents.map((p) => p.id) : [],
    upgrades: [],
    curses: [],
  };
}

function sendTo(conn, msg) {
  if (!conn || conn.closed) return;
  conn.sendBinary(encodeServerMsg(msg));
}

function broadcastTo(targets, msg) {
  const bytes = encodeServerMsg(msg);
  for (const conn of targets) {
    if (conn && !conn.closed) conn.sendBinary(bytes);
  }
}

function broadcastAll(msg) {
  broadcastTo(run.connections, msg);
}

function buildPool(level) {
  const ignoreCaps = VIOLATION === 'cap_ignored';
  const byId = (a, b) => (a.id < b.id ? -1 : a.id > b.id ? 1 : 0);
  const eligible = stubPool().filter((option) => option.minLevel <= level);
  const fresh = eligible.filter((option) => option.permanent || !run.wonTemporary.has(option.id));
  const capped = fresh.filter(
    (option) => ignoreCaps || (run.offered.get(option.id) || 0) < option.cap,
  );
  const pick = (list) => {
    const sorted = list.slice().sort(byId);
    const chosen = sorted.slice(0, OPTIONS_PER_SESSION);
    // Every session offers at least one permanent-capable option, so the
    // permanence check always has something to exercise.
    if (!chosen.some((option) => option.permanent)) {
      const swap = sorted.find((option) => option.permanent && !chosen.includes(option));
      if (swap) chosen[chosen.length - 1] = swap;
    }
    return chosen;
  };
  if (capped.length >= OPTIONS_PER_SESSION) return pick(capped);
  if (fresh.length >= OPTIONS_PER_SESSION) return pick(fresh);
  return pick(eligible);
}

function advanceToPlaying() {
  run.phase = PHASE_PLAYING;
  broadcastAll({ type: 'RunState', info: runInfo() });
  broadcastAll({
    type: 'Draft',
    enemy_picks: [],
    reward_picks: [],
    cards: [
      { id: 'draft-a', kind: 0 },
      { id: 'draft-b', kind: 1 },
      { id: 'draft-c', kind: 2 },
    ],
  });
  draftWaiters = new Set(run.connections);
  schedule(openSession, DRAFT_MS);
}

function openSession() {
  if (run.session) return;
  if (pendingTimer) {
    clearTimeout(pendingTimer);
    pendingTimer = null;
  }
  draftWaiters = null;

  const options = buildPool(run.level);
  const session = {
    level: run.level,
    options,
    votes: new Map(),
    countdownS: COUNTDOWN_S,
    subscribers: new Set(run.connections),
    openedAt: Date.now(),
    closed: false,
    broadcastTimer: null,
    closeTimer: null,
  };
  run.session = session;

  for (const option of options) {
    run.offered.set(option.id, (run.offered.get(option.id) || 0) + 1);
  }

  broadcastTo(session.subscribers, {
    type: 'VoteOpen',
    level: session.level,
    options: options.map((option, index) => ({ index, id: option.id, kind: option.kind })),
    countdown_s: session.countdownS,
  });

  session.broadcastTimer = setInterval(() => broadcastVoteState(session), 250);
  session.closeTimer = setTimeout(() => closeSession(session), session.countdownS * 1000);
}

function tally(session) {
  const counts = session.options.map(() => 0);
  for (const index of session.votes.values()) {
    if (index >= 0 && index < counts.length) counts[index] += 1;
  }
  return counts;
}

function broadcastVoteState(session, force) {
  if (!force && session.closed) return;
  const counts = tally(session);
  const voted = Array.from(session.votes.keys());
  const elapsed = (Date.now() - session.openedAt) / 1000;
  const secondsLeft = Math.max(0, Math.ceil(session.countdownS - elapsed));
  broadcastTo(session.subscribers, {
    type: 'VoteState',
    counts,
    voted,
    seconds_left: secondsLeft,
  });
}

function pickWinner(counts) {
  if (counts.length === 0) return 0;
  let best = 0;
  for (let i = 1; i < counts.length; i += 1) {
    if (VIOLATION === 'majority_inverted') {
      if (counts[i] < counts[best]) best = i;
    } else if (VIOLATION === 'tie_ignored') {
      if (counts[i] >= counts[best]) best = i;
    } else if (counts[i] > counts[best]) {
      best = i;
    }
  }
  return best;
}

function closeSession(session) {
  if (session.closed) return;
  session.closed = true;
  if (session.broadcastTimer) clearInterval(session.broadcastTimer);
  if (session.closeTimer) clearTimeout(session.closeTimer);

  broadcastVoteState(session, true);

  const counts = tally(session);
  const winner = pickWinner(counts);
  const option = session.options[winner] || session.options[0] || {
    id: 'none',
    name: 'None',
    kind: 0,
    permanent: false,
  };
  const permanent = VIOLATION === 'permanence_dropped' ? false : option.permanent;

  broadcastTo(session.subscribers, {
    type: 'VoteResult',
    winner,
    id: option.id,
    name: option.name,
    kind: option.kind,
    permanent,
  });

  if (option.permanent) {
    if (VIOLATION !== 'permanence_dropped') run.permanents.push({ id: option.id, name: option.name, kind: option.kind });
  } else {
    run.wonTemporary.add(option.id);
  }

  run.lastSession = session;
  run.session = null;
  // A short intermission keeps the next session from opening on the same tick as
  // the result, so a vote sent just after VoteResult is genuinely late.
  schedule(nextLevel, INTERMISSION_MS);
}

function nextLevel() {
  run.level += 1;
  run.phase = PHASE_ENEMY_DRAFT;
  broadcastAll({ type: 'RunState', info: runInfo() });
  schedule(advanceToPlaying, LEVEL_MS);
}

/* ------------------------------------------------------------------ *
 * Client message handlers
 * ------------------------------------------------------------------ */

function onHello(conn, msg) {
  if (msg.protocol_version !== PROTOCOL_VERSION) {
    sendTo(conn, {
      type: 'Error',
      code: ERR_PROTOCOL,
      message: 'unsupported protocol version ' + msg.protocol_version,
    });
    return;
  }

  conn.playerId = run.nextPlayerId;
  run.nextPlayerId += 1;

  sendTo(conn, {
    type: 'Welcome',
    protocol_version: PROTOCOL_VERSION,
    player_id: conn.playerId,
    tick: run.tick,
  });
  sendTo(conn, { type: 'RunState', info: runInfo() });

  if (!run.started) {
    run.started = true;
    run.phase = PHASE_ENEMY_DRAFT;
    run.level = 1;
    schedule(advanceToPlaying, LEVEL_MS);
    return;
  }

  const session = run.session;
  if (session && !session.closed) {
    if (VIOLATION === 'rejoin_forgotten') return;
    session.subscribers.add(conn);
    sendTo(conn, {
      type: 'VoteOpen',
      level: session.level,
      options: session.options.map((option, index) => ({
        index,
        id: option.id,
        kind: option.kind,
      })),
      countdown_s: session.countdownS,
    });
    broadcastVoteState(session);
  }
}

function onChooseClass(conn, msg) {
  conn.classId = msg.class;
}

function onDraftPick(conn) {
  if (!draftWaiters) return;
  draftWaiters.delete(conn);
  if (draftWaiters.size === 0) openSession();
}

function onVote(conn, msg) {
  const session = run.session;

  if (!session || session.closed) {
    if (VIOLATION === 'late_vote_accepted') {
      const last = run.lastSession;
      if (last) {
        last.votes.set(conn.playerId, msg.option);
        broadcastVoteState(last, true);
      }
      return;
    }
    sendTo(conn, {
      type: 'Error',
      code: ERR_NO_SESSION,
      message: 'no vote session is open',
    });
    return;
  }

  if (msg.option >= session.options.length) {
    sendTo(conn, {
      type: 'Error',
      code: ERR_BAD_OPTION,
      message: 'vote option ' + msg.option + ' is out of range',
    });
    return;
  }

  if (session.votes.has(conn.playerId)) {
    sendTo(conn, {
      type: 'Error',
      code: ERR_ALREADY_VOTED,
      message: 'already voted in this session',
    });
    return;
  }

  session.votes.set(conn.playerId, msg.option);
  broadcastVoteState(session);
}

function onClientMessage(conn, msg) {
  switch (msg.type) {
    case 'Hello':
      onHello(conn, msg);
      break;
    case 'ChooseClass':
      onChooseClass(conn, msg);
      break;
    case 'DraftPick':
      onDraftPick(conn, msg);
      break;
    case 'Vote':
      onVote(conn, msg);
      break;
    case 'Ping':
      break;
    default:
      break;
  }
}

/* ------------------------------------------------------------------ *
 * Server bootstrap
 * ------------------------------------------------------------------ */

function start() {
  const httpServer = createServer((req, res) => {
    res.writeHead(426, { 'content-type': 'text/plain' });
    res.end('websocket upgrade required\n');
  });

  httpServer.on('upgrade', (req, socket) => {
    const key = req.headers['sec-websocket-key'];
    if (typeof key !== 'string' || key.length === 0) {
      socket.end('HTTP/1.1 400 Bad Request\r\nConnection: close\r\n\r\n');
      return;
    }
    const accept = createHash('sha1').update(key + WS_GUID).digest('base64');
    socket.write(
      'HTTP/1.1 101 Switching Protocols\r\n' +
        'Upgrade: websocket\r\n' +
        'Connection: Upgrade\r\n' +
        'Sec-WebSocket-Accept: ' + accept + '\r\n' +
        '\r\n',
    );

    const conn = new Conn(socket);
    conn.onMessage = onClientMessage;
    run.connections.add(conn);
    socket.on('close', () => {
      run.connections.delete(conn);
      if (draftWaiters) draftWaiters.delete(conn);
    });
  });

  httpServer.listen(PORT, '127.0.0.1', () => {
    const address = httpServer.address();
    const port = address && typeof address === 'object' ? address.port : PORT;
    process.stdout.write('voting-conformance stub listening on ws://127.0.0.1:' + port + '\n');
  });

  const shutdown = () => {
    if (pendingTimer) clearTimeout(pendingTimer);
    const session = run.session;
    if (session) {
      if (session.broadcastTimer) clearInterval(session.broadcastTimer);
      if (session.closeTimer) clearTimeout(session.closeTimer);
    }
    for (const conn of run.connections) conn.close();
    httpServer.close(() => process.exit(0));
    setTimeout(() => process.exit(0), 200);
  };

  process.on('SIGTERM', shutdown);
  process.on('SIGINT', shutdown);
}

const isMain =
  typeof process.argv[1] === 'string' &&
  resolve(process.argv[1]) === fileURLToPath(import.meta.url);

if (isMain) start();

