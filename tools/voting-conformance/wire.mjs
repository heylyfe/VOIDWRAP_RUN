#!/usr/bin/env node
// tools/voting-conformance/wire.mjs
//
// Dependency-free bincode 1.x codec (fixint encoding, little-endian) for the
// voting conformance kit. Node built-ins only: no network, no eval, no deps.
//
// Wire format (authoritative for this kit):
//   u8 / u16 / u32 / u64   fixed width, little-endian
//   f32                    little-endian IEEE-754
//   bool                   one byte, 0 or 1
//   string                 u64 LE byte length, then that many UTF-8 bytes
//   Vec<T>                 u64 LE element count, then each element
//   enum                   u32 LE variant index, then the variant's fields
//   struct                 fields in declaration order, no padding, no prefix
//
// Exports:
//   encodeClientMsg(msg)     -> Buffer
//   decodeServerMsg(buf)     -> plain object, throws RangeError when malformed
//   tryDecodeServerMsg(buf)  -> plain object, or null when malformed
//
// Self-test (no server, no network):
//   node tools/voting-conformance/wire.mjs --selftest

import { pathToFileURL } from 'node:url';

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

// Em dash used by the self-test report. Written as an escape so the output is
// byte-identical no matter how this file is checked out.
const DASH = '\u2014';

// ClientMsg variant indices (encode only).
// Wire sizes of the two Snapshot arrays nothing decodes, from the generated
// structs in server/crates/protocol/src/generated.rs. A mismatch here shows up
// as a decode failure on every Snapshot, which is loud rather than silent.
//   PlayerView: id u32, pos Vec3, vel Vec3, mode u8, class u8
//   EnemyView:  id u32, kind u8, pos Vec3, radius f32, state u8, target Vec3,
//               prog u8, flags u8
const PLAYER_VIEW_BYTES = 4 + 12 + 12 + 1 + 1;
const ENEMY_VIEW_BYTES = 4 + 1 + 12 + 4 + 1 + 12 + 1 + 1;

const CLIENT_VARIANTS = Object.freeze({
  Hello: 0,
  Inputs: 1,
  Ping: 2,
  Restart: 3,
  DraftPick: 4,
  Buy: 5,
  ChooseClass: 6,
  Vote: 7,
  // T-141: the debug command layer. Gated server-side (dev flag or dev code) and
  // singleplayer-only; the kit uses it to drive voting, the shop and altars without
  // needing a bot to survive a whole level.
  Debug: 8,
  // T-159: the pre-match theme/difficulty poll. A separate message family from the
  // pedestal vote; the vote's four messages are untouched.
  Poll: 9,
});

// ServerMsg variant indices (decode only).
const SERVER_VARIANTS = Object.freeze([
  'Welcome', // 0
  'Level', // 1
  'Snapshot', // 2
  'Pong', // 3
  'Error', // 4
  'RunOver', // 5
  'RunState', // 6
  'Draft', // 7
  'Shop', // 8
  'Notice', // 9
  'VoteOpen', // 10
  'VoteState', // 11
  'VoteResult', // 12
  'DebugResult', // 13
  'PollOpen', // 14
  'PollState', // 15
  'PollResult', // 16
]);

// Field tables for the four structs the kit has to decode in full.
//
// Each entry is [fieldName, kind] in Rust declaration order. `kind` is one of
// the scalar names, `vec:<kind>`, or the name of another struct in this table.
//
// NOTE: this table is the single place to correct a struct layout. It mirrors
// server/crates/protocol/src/generated.rs; see NOTES in the ticket reply for
// the exact assumption made per struct.
const STRUCT_FIELDS = Object.freeze({
  VoteOption: Object.freeze([
    ['index', 'u8'],
    ['id', 'string'],
    ['name', 'string'],
    ['kind', 'u8'],
  ]),
  PollOption: Object.freeze([
    ['id', 'string'],
    ['name', 'string'],
    ['kind', 'u8'],
  ]),
  DraftCard: Object.freeze([
    ['kind', 'u8'],
    ['id', 'string'],
  ]),
  ShopItem: Object.freeze([
    ['id', 'string'],
    ['cost', 'u32'],
    ['stock', 'u8'],
  ]),
  Tuning: Object.freeze([
    ['run_speed_mul', 'f32'],
    ['jump_mul', 'f32'],
    ['gravity_mul', 'f32'],
    ['dash_speed_mul', 'f32'],
    ['dash_cooldown_mul', 'f32'],
    ['air_jumps', 'u8'],
    ['friction_mul', 'f32'],
  ]),
  RunInfo: Object.freeze([
    ['phase', 'u8'],
    ['level', 'u32'],
    ['lives', 'u32'],
    ['currency', 'u32'],
    ['tuning', 'Tuning'],
    ['fog_density_mul', 'f32'],
    ['enemies', 'vec:string'],
    ['upgrades', 'vec:string'],
    ['curses', 'vec:string'],
  ]),
  Vec3: Object.freeze([
    ['x', 'f32'],
    ['y', 'f32'],
    ['z', 'f32'],
  ]),
  Aabb: Object.freeze([
    ['min', 'Vec3'],
    ['max', 'Vec3'],
  ]),
  Platform: Object.freeze([
    ['id', 'u16'],
    ['kind', 'u8'],
    ['bounds', 'Aabb'],
  ]),
  Gift: Object.freeze([
    ['id', 'u16'],
    ['pos', 'Vec3'],
    ['kind', 'u8'],
  ]),
  Altar: Object.freeze([
    ['id', 'u16'],
    ['pos', 'Vec3'],
  ]),
  LevelData: Object.freeze([
    ['level', 'u32'],
    ['seed', 'u32'],
    ['spawn', 'Vec3'],
    ['beacon', 'Vec3'],
    ['kill_y', 'f32'],
    ['platforms', 'vec:Platform'],
    ['gifts', 'vec:Gift'],
    ['event', 'u8'],
    ['altars', 'vec:Altar'],
  ]),
  ControllerState: Object.freeze([
    ['pos', 'Vec3'],
    ['vel', 'Vec3'],
    ['grounded', 'bool'],
    ['coyote_frames', 'u8'],
    ['jump_buffer_frames', 'u8'],
    ['prev_buttons', 'u8'],
    ['air_jumps_used', 'u8'],
    ['ability_cooldown_frames', 'u16'],
    ['ability_active_frames', 'u8'],
    ['ability_dir', 'Vec3'],
    ['class', 'u8'],
    ['ward_frames', 'u16'],
    ['stagger_frames', 'u8'],
  ]),
});

// ---------------------------------------------------------------------------
// Writer (encode side)
// ---------------------------------------------------------------------------

function assertUint(value, max, field) {
  if (typeof value !== 'number' || !Number.isInteger(value) || value < 0 || value > max) {
    throw new RangeError(
      `encodeClientMsg: field "${field}" must be an integer in [0, ${max}], got ${String(value)}`,
    );
  }
}

class Writer {
  constructor() {
    this.parts = [];
    this.size = 0;
  }

  _push(buf) {
    this.parts.push(buf);
    this.size += buf.length;
    return this;
  }

  u8(value, field) {
    assertUint(value, 0xff, field);
    const b = Buffer.allocUnsafe(1);
    b.writeUInt8(value, 0);
    return this._push(b);
  }

  u16(value, field) {
    assertUint(value, 0xffff, field);
    const b = Buffer.allocUnsafe(2);
    b.writeUInt16LE(value, 0);
    return this._push(b);
  }

  u32(value, field) {
    assertUint(value, 0xffffffff, field);
    const b = Buffer.allocUnsafe(4);
    b.writeUInt32LE(value, 0);
    return this._push(b);
  }

  u64(value, field) {
    let big;
    if (typeof value === 'bigint') {
      big = value;
    } else if (typeof value === 'number' && Number.isInteger(value) && value >= 0) {
      big = BigInt(value);
    } else {
      throw new RangeError(
        `encodeClientMsg: field "${field}" must be a non-negative integer, got ${String(value)}`,
      );
    }
    if (big < 0n || big > 0xffffffffffffffffn) {
      throw new RangeError(`encodeClientMsg: field "${field}" does not fit in u64: ${big}`);
    }
    const b = Buffer.allocUnsafe(8);
    b.writeBigUInt64LE(big, 0);
    return this._push(b);
  }

  string(value, field) {
    if (typeof value !== 'string') {
      throw new TypeError(
        `encodeClientMsg: field "${field}" must be a string, got ${typeof value}`,
      );
    }
    const bytes = Buffer.from(value, 'utf8');
    this.u64(BigInt(bytes.length), `${field}.len`);
    return this._push(bytes);
  }

  vecU8(values, field) {
    if (!Array.isArray(values) && !(values instanceof Uint8Array)) {
      throw new TypeError(`encodeClientMsg: field "${field}" must be an array of bytes`);
    }
    const bytes = Buffer.from(values);
    this.u64(BigInt(bytes.length), `${field}.len`);
    return this._push(bytes);
  }

  finish() {
    return Buffer.concat(this.parts, this.size);
  }
}

// ---------------------------------------------------------------------------
// Reader (decode side)
// ---------------------------------------------------------------------------

function toBuffer(buf) {
  if (Buffer.isBuffer(buf)) return buf;
  if (buf instanceof Uint8Array) return Buffer.from(buf.buffer, buf.byteOffset, buf.byteLength);
  throw new TypeError('decodeServerMsg expects a Buffer or Uint8Array');
}

class Reader {
  constructor(buf, variant) {
    this.buf = buf;
    this.pos = 0;
    this.variant = variant;
  }

  fail(field, detail) {
    const where = this.variant === null ? 'ServerMsg' : `ServerMsg::${this.variant}`;
    throw new RangeError(
      `${where}: ${detail} (field "${field}", offset ${this.pos}, buffer length ${this.buf.length})`,
    );
  }

  need(n, field) {
    if (n < 0 || this.pos + n > this.buf.length) {
      this.fail(
        field,
        `truncated buffer: need ${n} byte(s), ${this.buf.length - this.pos} remaining`,
      );
    }
  }

  u8(field) {
    this.need(1, field);
    const v = this.buf[this.pos];
    this.pos += 1;
    return v;
  }

  u16(field) {
    this.need(2, field);
    const v = this.buf.readUInt16LE(this.pos);
    this.pos += 2;
    return v;
  }

  u32(field) {
    this.need(4, field);
    const v = this.buf.readUInt32LE(this.pos);
    this.pos += 4;
    return v;
  }

  u64(field) {
    this.need(8, field);
    const v = this.buf.readBigUInt64LE(this.pos);
    this.pos += 8;
    return v;
  }

  f32(field) {
    this.need(4, field);
    const v = this.buf.readFloatLE(this.pos);
    this.pos += 4;
    return v;
  }

  bool(field) {
    const b = this.u8(field);
    if (b > 1) this.fail(field, `invalid bool byte 0x${b.toString(16).padStart(2, '0')}`);
    return b === 1;
  }

  // Advance past `n` bytes without decoding them. Used for the two large
  // Snapshot arrays nothing reads; the byte counts are asserted against the
  // generated structs in `PLAYER_VIEW_BYTES` / `ENEMY_VIEW_BYTES`.
  skip(n, field) {
    this.need(n, field);
    this.pos += n;
  }

  // u64 length prefix, validated against the bytes actually left in the buffer
  // *before* anything is allocated. Every element of every Vec/string we decode
  // occupies at least one byte, so a prefix larger than the remaining byte
  // count can never be satisfied and is rejected outright.
  count(field) {
    const raw = this.u64(field);
    const remaining = this.buf.length - this.pos;
    if (raw > BigInt(remaining)) {
      this.fail(field, `length prefix ${raw} exceeds the ${remaining} byte(s) remaining`);
    }
    return Number(raw);
  }

  string(field) {
    const n = this.count(field);
    this.need(n, field);
    const s = this.buf.toString('utf8', this.pos, this.pos + n);
    this.pos += n;
    return s;
  }

  vecU8(field) {
    const n = this.count(field);
    this.need(n, field);
    const out = new Array(n);
    for (let i = 0; i < n; i += 1) out[i] = this.buf[this.pos + i];
    this.pos += n;
    return out;
  }

  vecU32(field) {
    const n = this.count(field);
    this.need(n * 4, field);
    const out = new Array(n);
    for (let i = 0; i < n; i += 1) out[i] = this.buf.readUInt32LE(this.pos + i * 4);
    this.pos += n * 4;
    return out;
  }

  vec(field, read) {
    const n = this.count(field);
    const out = new Array(n);
    for (let i = 0; i < n; i += 1) out[i] = read(this, `${field}[${i}]`);
    return out;
  }
}

// ---------------------------------------------------------------------------
// Struct decoding driven by STRUCT_FIELDS
// ---------------------------------------------------------------------------

function readStruct(r, structName, field) {
  const fields = STRUCT_FIELDS[structName];
  if (fields === undefined) {
    throw new Error(`wire.mjs: no field table for struct "${structName}"`);
  }
  const out = {};
  for (const [name, kind] of fields) {
    out[name] = readKind(r, kind, `${field}.${name}`);
  }
  return out;
}

function readKind(r, kind, field) {
  switch (kind) {
    case 'u8':
      return r.u8(field);
    case 'u16':
      return r.u16(field);
    case 'u32':
      return r.u32(field);
    case 'u64':
      return r.u64(field);
    case 'f32':
      return r.f32(field);
    case 'bool':
      return r.bool(field);
    case 'string':
      return r.string(field);
    default:
      break;
  }
  if (kind.startsWith('vec:')) {
    const inner = kind.slice(4);
    if (inner === 'u8') return r.vecU8(field);
    if (inner === 'u32') return r.vecU32(field);
    return r.vec(field, (rr, f) => readKind(rr, inner, f));
  }
  if (Object.prototype.hasOwnProperty.call(STRUCT_FIELDS, kind)) {
    return readStruct(r, kind, field);
  }
  throw new Error(`wire.mjs: unknown field kind "${kind}" for "${field}"`);
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/**
 * Encode a ClientMsg to a bincode (fixint, LE) Buffer.
 *
 * @param {{variant: string} & Record<string, unknown>} msg
 * @returns {Buffer}
 */
export function encodeClientMsg(msg) {
  if (msg === null || typeof msg !== 'object') {
    throw new TypeError('encodeClientMsg expects an object with a "variant" property');
  }

  const w = new Writer();

  switch (msg.variant) {
    case 'Hello':
      w.u32(CLIENT_VARIANTS.Hello, 'variant');
      w.u16(msg.protocol_version, 'protocol_version');
      w.string(msg.name, 'name');
      break;

    case 'Inputs':
      // Variant 1 exists on the wire but the conformance kit never sends it.
      throw new Error('ClientMsg::Inputs (variant 1) is not supported by the conformance kit');

    case 'Ping':
      w.u32(CLIENT_VARIANTS.Ping, 'variant');
      w.u32(msg.t, 't');
      break;

    case 'Restart':
      w.u32(CLIENT_VARIANTS.Restart, 'variant');
      break;

    case 'DraftPick':
      w.u32(CLIENT_VARIANTS.DraftPick, 'variant');
      w.vecU8(msg.picks, 'picks');
      break;

    case 'Buy':
      w.u32(CLIENT_VARIANTS.Buy, 'variant');
      w.string(msg.item, 'item');
      break;

    case 'ChooseClass':
      w.u32(CLIENT_VARIANTS.ChooseClass, 'variant');
      w.u8(msg.class, 'class');
      break;

    case 'Vote':
      w.u32(CLIENT_VARIANTS.Vote, 'variant');
      w.u8(msg.option, 'option');
      break;

    case 'Debug':
      w.u32(CLIENT_VARIANTS.Debug, 'variant');
      w.string(msg.code, 'code');
      w.string(msg.command, 'command');
      w.string(msg.args, 'args');
      break;

    case 'Poll':
      w.u32(CLIENT_VARIANTS.Poll, 'variant');
      w.u8(msg.option, 'option');
      break;

    default:
      throw new RangeError(
        `ClientMsg: unknown variant ${JSON.stringify(msg.variant)} (known: ${Object.keys(
          CLIENT_VARIANTS,
        ).join(', ')})`,
      );
  }

  return w.finish();
}

/**
 * Decode a ServerMsg from a bincode (fixint, LE) Buffer.
 *
 * @param {Buffer | Uint8Array} buf
 * @returns {Record<string, unknown>}
 */
export function decodeServerMsg(buf) {
  const r = new Reader(toBuffer(buf), null);

  const index = r.u32('variant');
  const variantName = SERVER_VARIANTS[index];
  if (variantName === undefined) {
    throw new RangeError(
      `ServerMsg: unknown variant index ${index} (known indices 0..${SERVER_VARIANTS.length - 1})`,
    );
  }
  r.variant = variantName;

  switch (variantName) {
    case 'Welcome':
      return {
        variant: variantName,
        protocol_version: r.u16('protocol_version'),
        player_id: r.u32('player_id'),
        tick: r.u32('tick'),
      };

    // `Level` carries the whole level (platforms, gifts, altars) and `Snapshot`
    // carries the local controller state. The playthrough sweep navigates with
    // both, so they are decoded. `Snapshot` stops after `you`: the player and
    // enemy views are large and nothing reads them.
    case 'Level':
      return { variant: variantName, data: readStruct(r, 'LevelData', 'data') };

    case 'Snapshot': {
      const tick = r.u32('tick');
      const level = r.u32('level');
      const last_input_seq = r.u32('last_input_seq');
      const you = readStruct(r, 'ControllerState', 'you');
      // `players` and `enemies` are large and no kit reads them, so they are
      // skipped by size rather than decoded. `gifts_taken` is the one field past
      // them that matters: without it a bot cannot tell a gift it has already
      // picked up from one it has not, so it walks back to the same gift
      // forever. That is exactly what the playthrough sweep did.
      r.skip(r.count('players') * PLAYER_VIEW_BYTES, 'players');
      r.skip(r.count('enemies') * ENEMY_VIEW_BYTES, 'enemies');
      const gifts_taken = [];
      for (let i = 0, n = r.count('gifts_taken'); i < n; i += 1) gifts_taken.push(r.u16('gifts_taken'));
      return { variant: variantName, tick, level, last_input_seq, you, gifts_taken };
    }

    // RunOver carries a RunSummary the conformance kit never inspects. Decoding
    // it would be wasted work, so we stop right after the variant index and flag
    // the result as truncated. Callers that need the payload must extend this
    // file.
    case 'RunOver':
      return { variant: variantName, truncated: true };

    case 'Pong':
      return { variant: variantName, t: r.u32('t') };

    case 'DebugResult':
      return { variant: variantName, ok: r.bool('ok'), message: r.string('message') };

    case 'Error':
      return {
        variant: variantName,
        code: r.u16('code'),
        message: r.string('message'),
      };

    case 'RunState':
      return { variant: variantName, info: readStruct(r, 'RunInfo', 'info') };

    case 'Draft':
      return {
        variant: variantName,
        enemy_picks: r.u8('enemy_picks'),
        reward_picks: r.u8('reward_picks'),
        cards: r.vec('cards', (rr, f) => readStruct(rr, 'DraftCard', f)),
      };

    case 'Shop':
      return {
        variant: variantName,
        items: r.vec('items', (rr, f) => readStruct(rr, 'ShopItem', f)),
      };

    case 'Notice':
      return {
        variant: variantName,
        kind: r.u8('kind'),
        ids: r.vec('ids', (rr, f) => rr.string(f)),
      };

    case 'VoteOpen':
      return {
        variant: variantName,
        level: r.u32('level'),
        options: r.vec('options', (rr, f) => readStruct(rr, 'VoteOption', f)),
        countdown_s: r.u16('countdown_s'),
      };

    case 'VoteState':
      return {
        variant: variantName,
        counts: r.vecU32('counts'),
        voted: r.vecU32('voted'),
        seconds_left: r.u16('seconds_left'),
      };

    case 'VoteResult':
      return {
        variant: variantName,
        winner: r.u8('winner'),
        id: r.string('id'),
        name: r.string('name'),
        kind: r.u8('kind'),
        permanent: r.bool('permanent'),
      };

    case 'PollOpen':
      return {
        variant: variantName,
        options: r.vec('options', (rr, f) => readStruct(rr, 'PollOption', f)),
        countdown_s: r.u16('countdown_s'),
      };

    case 'PollState':
      return {
        variant: variantName,
        counts: r.vecU32('counts'),
        voted: r.vecU32('voted'),
        seconds_left: r.u16('seconds_left'),
      };

    case 'PollResult':
      return {
        variant: variantName,
        winner: r.u8('winner'),
        id: r.string('id'),
        name: r.string('name'),
        kind: r.u8('kind'),
      };

    default:
      // Unreachable: SERVER_VARIANTS is exhaustive above.
      throw new RangeError(`ServerMsg: unhandled variant ${variantName}`);
  }
}

/**
 * Like decodeServerMsg, but returns null instead of throwing.
 *
 * @param {Buffer | Uint8Array} buf
 * @returns {Record<string, unknown> | null}
 */
export function tryDecodeServerMsg(buf) {
  try {
    return decodeServerMsg(buf);
  } catch {
    return null;
  }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

// Independent byte builders. Deliberately not built on Writer, so a bug in the
// encoder cannot mask a bug in the decoder (and vice versa).
const b8 = (v) => {
  const b = Buffer.alloc(1);
  b.writeUInt8(v, 0);
  return b;
};
const b16 = (v) => {
  const b = Buffer.alloc(2);
  b.writeUInt16LE(v, 0);
  return b;
};
const b32 = (v) => {
  const b = Buffer.alloc(4);
  b.writeUInt32LE(v, 0);
  return b;
};
const b64 = (v) => {
  const b = Buffer.alloc(8);
  b.writeBigUInt64LE(typeof v === 'bigint' ? v : BigInt(v), 0);
  return b;
};
const bstr = (s) => {
  const u = Buffer.from(s, 'utf8');
  return Buffer.concat([b64(u.length), u]);
};
const cat = (...parts) => Buffer.concat(parts);
const bf32 = (v) => {
  const b = Buffer.alloc(4);
  b.writeFloatLE(v, 0);
  return b;
};

function hex(buf) {
  let s = '';
  for (const byte of buf) s += byte.toString(16).padStart(2, '0');
  return s;
}

function fmt(v) {
  if (typeof v === 'bigint') return `${v}n`;
  if (typeof v === 'string') return JSON.stringify(v);
  return String(v);
}

function eq(actual, expected, what) {
  if (actual !== expected) {
    throw new Error(`${what}: expected ${fmt(expected)}, got ${fmt(actual)}`);
  }
}

function expectRangeError(fn, what) {
  let caught = null;
  try {
    fn();
  } catch (err) {
    caught = err;
  }
  if (caught === null) throw new Error(`${what}: expected a RangeError, but nothing was thrown`);
  if (!(caught instanceof RangeError)) {
    throw new Error(`${what}: expected RangeError, got ${caught.constructor.name}: ${caught.message}`);
  }
  return caught;
}

function runSelfTest() {
  const results = [];

  const check = (name, fn) => {
    try {
      fn();
      results.push({ ok: true, name });
    } catch (err) {
      results.push({
        ok: false,
        name,
        detail: err && err.message ? err.message : String(err),
      });
    }
  };

  // -- 1. encode Hello ------------------------------------------------------
  check('encode Hello', () => {
    const buf = encodeClientMsg({ variant: 'Hello', protocol_version: 1, name: 'ab' });
    eq(hex(buf), '00000000010002000000000000006162', 'Hello bytes');
    eq(buf.length, 16, 'Hello byte length');
  });

  // -- 2. encode Vote -------------------------------------------------------
  check('encode Vote', () => {
    const buf = encodeClientMsg({ variant: 'Vote', option: 2 });
    eq(hex(buf), '0700000002', 'Vote bytes');
    eq(buf.length, 5, 'Vote byte length');
  });

  // -- 3. encode DraftPick --------------------------------------------------
  check('encode DraftPick', () => {
    const buf = encodeClientMsg({ variant: 'DraftPick', picks: [1, 2] });
    eq(hex(buf), '0400000002000000000000000102', 'DraftPick bytes');
    eq(buf.length, 14, 'DraftPick byte length');
  });

  // -- 4. encode Ping / Restart / ChooseClass / Buy -------------------------
  check('encode Ping', () => {
    const buf = encodeClientMsg({ variant: 'Ping', t: 0x01020304 });
    eq(hex(buf), '0200000004030201', 'Ping bytes');
    eq(buf.length, 8, 'Ping byte length');
  });

  check('encode Restart', () => {
    const buf = encodeClientMsg({ variant: 'Restart' });
    eq(hex(buf), '03000000', 'Restart bytes');
    eq(buf.length, 4, 'Restart byte length');
  });

  check('encode ChooseClass', () => {
    const buf = encodeClientMsg({ variant: 'ChooseClass', class: 3 });
    eq(hex(buf), '0600000003', 'ChooseClass bytes');
    eq(buf.length, 5, 'ChooseClass byte length');
  });

  check('encode Buy', () => {
    const buf = encodeClientMsg({ variant: 'Buy', item: 'x' });
    eq(hex(buf), '05000000010000000000000078', 'Buy bytes');
    eq(buf.length, 13, 'Buy byte length');
  });

  check('encode Inputs throws', () => {
    let caught = null;
    try {
      encodeClientMsg({ variant: 'Inputs', frames: [] });
    } catch (err) {
      caught = err;
    }
    if (caught === null) throw new Error('expected ClientMsg::Inputs to throw');
    if (!/Inputs/.test(caught.message)) {
      throw new Error(`error should name Inputs: ${caught.message}`);
    }
  });

  // -- 5. decode VoteOpen ---------------------------------------------------
  check('decode VoteOpen', () => {
    const buf = cat(
      b32(10), // variant VoteOpen
      b32(7), // level
      b64(2), // options.len
      b8(0),
      bstr('a'),
      bstr('Alpha'),
      b8(1),
      b8(1),
      bstr('bb'),
      bstr('B\u00e9ta'),
      b8(2),
      b16(15), // countdown_s
    );
    const msg = decodeServerMsg(buf);
    eq(msg.variant, 'VoteOpen', 'variant');
    eq(msg.level, 7, 'level');
    eq(msg.options.length, 2, 'options.length');
    eq(msg.options[0].index, 0, 'options[0].index');
    eq(msg.options[0].id, 'a', 'options[0].id');
    eq(msg.options[0].name, 'Alpha', 'options[0].name');
    eq(msg.options[0].kind, 1, 'options[0].kind');
    eq(msg.options[1].index, 1, 'options[1].index');
    eq(msg.options[1].id, 'bb', 'options[1].id');
    eq(msg.options[1].name, 'B\u00e9ta', 'options[1].name');
    eq(msg.options[1].kind, 2, 'options[1].kind');
    eq(msg.countdown_s, 15, 'countdown_s');
  });

  // -- 6. decode VoteState --------------------------------------------------
  check('decode VoteState', () => {
    const buf = cat(
      b32(11), // variant VoteState
      b64(3),
      b32(3),
      b32(0),
      b32(7), // counts = [3, 0, 7]
      b64(2),
      b32(1),
      b32(2), // voted = [1, 2]  (different length on purpose)
      b16(9), // seconds_left
    );
    const msg = decodeServerMsg(buf);
    eq(msg.variant, 'VoteState', 'variant');
    eq(msg.counts.length, 3, 'counts.length');
    eq(msg.counts[0], 3, 'counts[0]');
    eq(msg.counts[1], 0, 'counts[1]');
    eq(msg.counts[2], 7, 'counts[2]');
    eq(msg.voted.length, 2, 'voted.length');
    eq(msg.voted[0], 1, 'voted[0]');
    eq(msg.voted[1], 2, 'voted[1]');
    eq(msg.seconds_left, 9, 'seconds_left');
  });

  // -- 7. decode VoteResult -------------------------------------------------
  check('decode VoteResult (permanent true)', () => {
    const buf = cat(b32(12), b8(1), bstr('opt-b'), bstr('Beta'), b8(2), b8(1));
    const msg = decodeServerMsg(buf);
    eq(msg.variant, 'VoteResult', 'variant');
    eq(msg.winner, 1, 'winner');
    eq(msg.id, 'opt-b', 'id');
    eq(msg.name, 'Beta', 'name');
    eq(msg.kind, 2, 'kind');
    eq(msg.permanent, true, 'permanent');
  });

  check('decode VoteResult (permanent false)', () => {
    const buf = cat(b32(12), b8(0), bstr('opt-a'), bstr('Alpha'), b8(1), b8(0));
    const msg = decodeServerMsg(buf);
    eq(msg.variant, 'VoteResult', 'variant');
    eq(msg.winner, 0, 'winner');
    eq(msg.id, 'opt-a', 'id');
    eq(msg.name, 'Alpha', 'name');
    eq(msg.kind, 1, 'kind');
    eq(msg.permanent, false, 'permanent');
  });

  // -- 8. decode Error ------------------------------------------------------
  check('decode Error', () => {
    const text = '\u00e9chec \u2014 \u00fcn\u00efcode'; // multi-byte UTF-8
    const buf = cat(b32(4), b16(0x1234), bstr(text));
    const msg = decodeServerMsg(buf);
    eq(msg.variant, 'Error', 'variant');
    eq(msg.code, 0x1234, 'code');
    eq(msg.message, text, 'message');
    eq(Buffer.byteLength(text, 'utf8') > text.length, true, 'message really is multi-byte');
  });

  // -- 9. decode Welcome / Pong / RunState / Draft / Shop / Notice ----------
  check('decode Welcome', () => {
    const buf = cat(b32(0), b16(1), b32(42), b32(1234));
    const msg = decodeServerMsg(buf);
    eq(msg.variant, 'Welcome', 'variant');
    eq(msg.protocol_version, 1, 'protocol_version');
    eq(msg.player_id, 42, 'player_id');
    eq(msg.tick, 1234, 'tick');
  });

  check('decode Pong', () => {
    const buf = cat(b32(3), b32(0xdeadbeef));
    const msg = decodeServerMsg(buf);
    eq(msg.variant, 'Pong', 'variant');
    eq(msg.t, 0xdeadbeef, 't');
  });

  check('decode RunState', () => {
    const buf = cat(
      b32(6),
      b8(1), // phase
      b32(3), // level
      b32(4), // lives
      b32(250), // currency
      bf32(1.1), // tuning.run_speed_mul
      bf32(1.2), // tuning.jump_mul
      bf32(0.9), // tuning.gravity_mul
      bf32(1.3), // tuning.dash_speed_mul
      bf32(0.8), // tuning.dash_cooldown_mul
      b8(2), // tuning.air_jumps
      bf32(1.4), // tuning.friction_mul
      bf32(0.5), // fog_density_mul
      b64(1),
      bstr('rewind'), // enemies
      b64(1),
      bstr('zephyr_tread'), // upgrades
      b64(1),
      bstr('leadened_soles'), // curses
    );
    const msg = decodeServerMsg(buf);
    eq(msg.variant, 'RunState', 'variant');
    eq(msg.info.phase, 1, 'info.phase');
    eq(msg.info.level, 3, 'info.level');
    eq(msg.info.lives, 4, 'info.lives');
    eq(msg.info.currency, 250, 'info.currency');
    eq(msg.info.tuning.air_jumps, 2, 'info.tuning.air_jumps');
    eq(msg.info.fog_density_mul, 0.5, 'info.fog_density_mul');
    eq(msg.info.enemies[0], 'rewind', 'info.enemies[0]');
    eq(msg.info.upgrades[0], 'zephyr_tread', 'info.upgrades[0]');
    eq(msg.info.curses[0], 'leadened_soles', 'info.curses[0]');
  });

  check('decode Draft', () => {
    const buf = cat(
      b32(7),
      b8(2), // enemy_picks
      b8(1), // reward_picks
      b64(2), // cards.len
      b8(1),
      bstr('card-a'), // card 0: kind 1, id "card-a"
      b8(2),
      bstr('card-b'), // card 1: kind 2, id "card-b"
    );
    const msg = decodeServerMsg(buf);
    eq(msg.variant, 'Draft', 'variant');
    eq(msg.enemy_picks, 2, 'enemy_picks');
    eq(msg.reward_picks, 1, 'reward_picks');
    eq(msg.cards.length, 2, 'cards.length');
    eq(msg.cards[0].kind, 1, 'cards[0].kind');
    eq(msg.cards[0].id, 'card-a', 'cards[0].id');
    eq(msg.cards[1].kind, 2, 'cards[1].kind');
    eq(msg.cards[1].id, 'card-b', 'cards[1].id');
  });

  check('decode Shop', () => {
    const buf = cat(b32(8), b64(1), bstr('item-a'), b32(25), b8(4));
    const msg = decodeServerMsg(buf);
    eq(msg.variant, 'Shop', 'variant');
    eq(msg.items.length, 1, 'items.length');
    eq(msg.items[0].id, 'item-a', 'items[0].id');
    eq(msg.items[0].cost, 25, 'items[0].cost');
    eq(msg.items[0].stock, 4, 'items[0].stock');
  });

  check('decode Notice', () => {
    const buf = cat(b32(9), b8(3), b64(2), bstr('p1'), bstr('p2'));
    const msg = decodeServerMsg(buf);
    eq(msg.variant, 'Notice', 'variant');
    eq(msg.kind, 3, 'kind');
    eq(msg.ids.length, 2, 'ids.length');
    eq(msg.ids[0], 'p1', 'ids[0]');
    eq(msg.ids[1], 'p2', 'ids[1]');
  });

  check('decode Level / Snapshot / RunOver stop after the variant index', () => {
    const cases = [
      [1, 'Level'],
      [2, 'Snapshot'],
      [5, 'RunOver'],
    ];
    for (const [index, name] of cases) {
      const msg = decodeServerMsg(cat(b32(index), Buffer.from([0xde, 0xad, 0xbe, 0xef])));
      eq(msg.variant, name, `${name} variant`);
      eq(msg.truncated, true, `${name} truncated`);
    }
  });

  // -- 10. truncated buffer -------------------------------------------------
  check('truncated buffer throws RangeError', () => {
    const full = cat(
      b32(10),
      b32(7),
      b64(1),
      b8(0),
      bstr('a'),
      bstr('Alpha'),
      b8(1),
      b16(15),
    );
    const cut = full.subarray(0, full.length - 1); // one byte short of countdown_s
    const err = expectRangeError(() => decodeServerMsg(cut), 'VoteOpen cut short');
    if (!/VoteOpen/.test(err.message)) {
      throw new Error(`error should name the variant: ${err.message}`);
    }
    if (!/countdown_s/.test(err.message)) {
      throw new Error(`error should name the field: ${err.message}`);
    }

    // A buffer that cannot even hold the variant index must also throw.
    expectRangeError(() => decodeServerMsg(Buffer.alloc(2)), 'two-byte buffer');
    expectRangeError(() => decodeServerMsg(Buffer.alloc(0)), 'empty buffer');
  });

  // -- 11. absurd length prefix --------------------------------------------
  check('absurd length prefix throws RangeError', () => {
    const buf = cat(b32(10), b32(7), b64(2n ** 40n), b8(0));
    const err = expectRangeError(() => decodeServerMsg(buf), 'VoteOpen with 2^40 options');
    if (!/VoteOpen/.test(err.message)) {
      throw new Error(`error should name the variant: ${err.message}`);
    }
    if (!/options/.test(err.message)) {
      throw new Error(`error should name the field: ${err.message}`);
    }
  });

  // -- 12. unknown variant --------------------------------------------------
  check('unknown variant throws RangeError', () => {
    const err = expectRangeError(
      () => decodeServerMsg(cat(b32(99), b8(0), b8(0), b8(0))),
      'variant 99',
    );
    if (!/99/.test(err.message)) {
      throw new Error(`error should name the index: ${err.message}`);
    }
  });

  // -- extra: tryDecodeServerMsg never throws -------------------------------
  check('tryDecodeServerMsg returns null instead of throwing', () => {
    eq(tryDecodeServerMsg(Buffer.alloc(0)), null, 'empty buffer');
    eq(tryDecodeServerMsg(Buffer.from([0xff, 0xff, 0xff, 0xff])), null, 'unknown variant');
    eq(tryDecodeServerMsg(cat(b32(10), b32(7), b64(2n ** 40n))), null, 'absurd length');
    const ok = tryDecodeServerMsg(cat(b32(3), b32(7)));
    eq(ok === null ? 'null' : ok.variant, 'Pong', 'valid buffer');
    eq(ok === null ? null : ok.t, 7, 'valid buffer payload');
  });

  // -- report ---------------------------------------------------------------
  let failed = 0;
  for (const result of results) {
    if (result.ok) {
      process.stdout.write(`PASS ${DASH} ${result.name}\n`);
    } else {
      failed += 1;
      process.stdout.write(`FAIL ${DASH} ${result.name}\n`);
      process.stdout.write(`       ${result.detail}\n`);
    }
  }
  process.stdout.write(`\n${results.length - failed}/${results.length} checks passed\n`);

  return failed === 0;
}

// ---------------------------------------------------------------------------
// CLI entry point
// ---------------------------------------------------------------------------

const isMain = (() => {
  const entry = process.argv[1];
  if (typeof entry !== 'string' || entry.length === 0) return false;
  try {
    return import.meta.url === pathToFileURL(entry).href;
  } catch {
    return false;
  }
})();

if (isMain) {
  if (process.argv.includes('--selftest')) {
    process.exitCode = runSelfTest() ? 0 : 1;
  } else {
    process.stderr.write('usage: node tools/voting-conformance/wire.mjs --selftest\n');
    process.exitCode = 2;
  }
}
