#!/usr/bin/env node
/**
 * tools/voting-conformance/run.mjs
 *
 * Conformance kit for the Voidwrap voting protocol.
 *
 * The kit speaks the real wire format: every client message is produced by
 * `encodeClientMsg` and sent as a binary WebSocket frame, and every incoming
 * binary frame is decoded with `decodeServerMsg`. A text frame from the server
 * is a hard transport failure.
 *
 * Modes:
 *   (default)     run the ten stub checks against --url / VOTING_URL
 *   --selftest    spawn the stub on an ephemeral port, run the matrix, then
 *                 run it once per violation and assert the paired check fails
 *   --real        three-check smoke mode for a live server
 *
 * With no URL and no mode the kit prints `skipped` and exits 0, so `just
 * verify` stays green on machines without a server.
 */

import { spawn } from 'node:child_process';
import { createHash, randomBytes } from 'node:crypto';
import { connect as netConnect } from 'node:net';
import { connect as tlsConnect } from 'node:tls';
import { fileURLToPath } from 'node:url';
import { dirname, join } from 'node:path';

import { encodeClientMsg, decodeServerMsg } from './wire.mjs';
import {
  loadPool,
  stubPool,
  PROTOCOL_VERSION,
  PHASE_ENEMY_DRAFT,
  PHASE_PLAYING,
  PHASE_REWARD_DRAFT,
  PHASE_OVER,
  OPTIONS_PER_SESSION,
} from './stub/server.mjs';

const HERE = dirname(fileURLToPath(import.meta.url));
const STUB_PATH = join(HERE, 'stub', 'server.mjs');

const CLIENT_COUNT = 3;
const SESSION_COUNT = 3;

const CHECK_NAMES = [
  'session-at-every-level',
  'options-shape',
  'majority-wins',
  'tie-rules',
  'permanence',
  'cap-aware-pool',
  'min-level-gate',
  'overflow-temporary',
  'rejoin-safety',
  'countdown-late-vote',
];

const REAL_CHECK_NAMES = ['real-welcome', 'real-runstate', 'real-vote-outside-session'];

const KNOWN_PHASES = [PHASE_ENEMY_DRAFT, PHASE_PLAYING, PHASE_REWARD_DRAFT, PHASE_OVER];

const WS_GUID = '258EAFA5-E914-47DA-95CA-C5AB0DC85B11';

const sleep = (ms) => new Promise((done) => setTimeout(done, ms));

/* ------------------------------------------------------------------ *
 * WebSocket client (RFC 6455, no dependencies)
 * ------------------------------------------------------------------ */

class WsClient {
  constructor(socket) {
    this.socket = socket;
    this.buffer = Buffer.alloc(0);
    this.closed = false;
    this.fragments = [];
    this.fragmentOpcode = 0;
    this.handlers = { binary: [], text: [], close: [] };

    socket.setNoDelay(true);
    socket.on('data', (chunk) => this._onData(chunk));
    socket.on('close', () => this._onClose());
    socket.on('error', () => this._onClose());
  }

  on(event, handler) {
    if (this.handlers[event]) this.handlers[event].push(handler);
    return this;
  }

  _emit(event, value) {
    for (const handler of this.handlers[event] || []) handler(value);
  }

  _onClose() {
    if (this.closed) return;
    this.closed = true;
    this._emit('close', null);
  }

  sendBinary(bytes) {
    this._frame(0x2, Buffer.from(bytes));
  }

  close() {
    if (this.closed) return;
    this._frame(0x8, Buffer.alloc(0));
    this.closed = true;
    this.socket.destroy();
  }

  _frame(opcode, payload) {
    if (this.closed) return;
    const length = payload.length;
    let header;
    if (length < 126) {
      header = Buffer.alloc(2);
      header[1] = 0x80 | length;
    } else if (length < 65536) {
      header = Buffer.alloc(4);
      header[1] = 0x80 | 126;
      header.writeUInt16BE(length, 2);
    } else {
      header = Buffer.alloc(10);
      header[1] = 0x80 | 127;
      header.writeBigUInt64BE(BigInt(length), 2);
    }
    header[0] = 0x80 | opcode;
    const mask = randomBytes(4);
    const masked = Buffer.allocUnsafe(length);
    for (let i = 0; i < length; i += 1) masked[i] = payload[i] ^ mask[i & 3];
    this.socket.write(Buffer.concat([header, mask, masked]));
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
      this._onClose();
      this.socket.destroy();
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
    if (opcode === 0x2) this._emit('binary', Buffer.from(payload));
    else this._emit('text', payload.toString('utf8'));
  }
}

function wsConnect(url, timeoutMs) {
  const limit = typeof timeoutMs === 'number' ? timeoutMs : 5000;
  return new Promise((resolvePromise, rejectPromise) => {
    let parsed;
    try {
      parsed = new URL(url);
    } catch (error) {
      rejectPromise(new Error('invalid websocket url "' + url + '": ' + error.message));
      return;
    }
    if (parsed.protocol !== 'ws:' && parsed.protocol !== 'wss:') {
      rejectPromise(new Error('unsupported websocket scheme "' + parsed.protocol + '"'));
      return;
    }
    const secure = parsed.protocol === 'wss:';
    const host = parsed.hostname;
    const port = Number(parsed.port || (secure ? 443 : 80));
    const path = (parsed.pathname || '/') + (parsed.search || '');
    const key = randomBytes(16).toString('base64');

    const socket = secure
      ? tlsConnect({ host, port, servername: host })
      : netConnect({ host, port });

    let settled = false;
    let head = Buffer.alloc(0);

    const timer = setTimeout(() => {
      if (settled) return;
      settled = true;
      socket.destroy();
      rejectPromise(new Error('websocket handshake to ' + url + ' timed out'));
    }, limit);

    const onData = (chunk) => {
      head = Buffer.concat([head, chunk]);
      const split = head.indexOf('\r\n\r\n');
      if (split < 0) return;
      socket.removeListener('data', onData);
      clearTimeout(timer);
      const text = head.subarray(0, split).toString('latin1');
      const rest = head.subarray(split + 4);
      const status = text.split('\r\n')[0] || '';
      if (!/^HTTP\/1\.[01] 101/.test(status)) {
        settled = true;
        socket.destroy();
        rejectPromise(new Error('websocket upgrade refused: ' + status));
        return;
      }
      const expected = createHash('sha1').update(key + WS_GUID).digest('base64');
      if (!text.toLowerCase().includes('sec-websocket-accept: ' + expected.toLowerCase())) {
        settled = true;
        socket.destroy();
        rejectPromise(new Error('websocket handshake: bad Sec-WebSocket-Accept'));
        return;
      }
      settled = true;
      const client = new WsClient(socket);
      if (rest.length > 0) client._onData(rest);
      resolvePromise(client);
    };

    socket.on('data', onData);
    socket.on('error', (error) => {
      if (settled) return;
      settled = true;
      clearTimeout(timer);
      rejectPromise(error);
    });

    const onReady = () => {
      socket.write(
        'GET ' + path + ' HTTP/1.1\r\n' +
          'Host: ' + host + ':' + port + '\r\n' +
          'Upgrade: websocket\r\n' +
          'Connection: Upgrade\r\n' +
          'Sec-WebSocket-Key: ' + key + '\r\n' +
          'Sec-WebSocket-Version: 13\r\n' +
          '\r\n',
      );
    };

    if (secure) socket.once('secureConnect', onReady);
    else socket.once('connect', onReady);
  });
}

/* ------------------------------------------------------------------ *
 * Harness
 * ------------------------------------------------------------------ */

class Harness {
  constructor(url) {
    this.url = url;
    this.clients = [];
    this.waiters = [];
    this.textFrames = [];
  }

  async connect(name) {
    const ws = await wsConnect(this.url);
    const client = { name, ws, log: [], cursor: 0, closed: false, onMessage: null };

    ws.on('binary', (bytes) => {
      let msg = null;
      let error = null;
      try {
        msg = decodeServerMsg(bytes);
        // wire.mjs names messages by `variant`; the checks below read `type`.
        if (msg && msg.type === undefined) msg.type = msg.variant;
      } catch (decodeError) {
        error = decodeError;
      }
      client.log.push({ at: Date.now(), msg, error, bytes });
      if (msg && typeof client.onMessage === 'function') client.onMessage(msg);
      this._wake();
    });

    ws.on('text', (text) => {
      this.textFrames.push({ from: name, text });
      client.log.push({ at: Date.now(), text });
      this._wake();
    });

    ws.on('close', () => {
      client.closed = true;
      this._wake();
    });

    this.clients.push(client);
    return client;
  }

  _scan(client, pred, fromStart) {
    const start = fromStart ? 0 : client.cursor;
    for (let i = start; i < client.log.length; i += 1) {
      if (pred(client.log[i])) {
        if (!fromStart) client.cursor = i + 1;
        return client.log[i];
      }
    }
    return null;
  }

  waitFor(client, pred, timeoutMs) {
    const hit = this._scan(client, pred, false);
    if (hit) return Promise.resolve(hit);
    return this._wait(client, pred, timeoutMs, false);
  }

  waitForAny(client, pred, timeoutMs) {
    const hit = this._scan(client, pred, true);
    if (hit) return Promise.resolve(hit);
    return this._wait(client, pred, timeoutMs, true);
  }

  _wait(client, pred, timeoutMs, fromStart) {
    return new Promise((resolvePromise) => {
      const waiter = { client, pred, fromStart, resolve: resolvePromise, timer: null };
      waiter.timer = setTimeout(() => {
        this.waiters = this.waiters.filter((item) => item !== waiter);
        resolvePromise(null);
      }, timeoutMs);
      this.waiters.push(waiter);
    });
  }

  _wake() {
    if (this.waiters.length === 0) return;
    const pending = this.waiters;
    this.waiters = [];
    for (const waiter of pending) {
      const hit = this._scan(waiter.client, waiter.pred, waiter.fromStart);
      if (hit) {
        clearTimeout(waiter.timer);
        waiter.resolve(hit);
      } else {
        this.waiters.push(waiter);
      }
    }
  }

  dispose() {
    for (const waiter of this.waiters) clearTimeout(waiter.timer);
    this.waiters = [];
    for (const client of this.clients) client.ws.close();
  }
}

/* ------------------------------------------------------------------ *
 * Client message helper
 * ------------------------------------------------------------------ */

// JSON.stringify that survives the BigInt values wire.mjs returns for u64 fields.
function safeJson(value) {
  return JSON.stringify(value, (_key, item) => (typeof item === 'bigint' ? item.toString() : item));
}

function sendClient(client, msg) {
  let bytes;
  // The kit names messages by `type`; wire.mjs names them by `variant`.
  const wire = { ...msg, variant: msg.variant ?? msg.type };
  delete wire.type;
  try {
    bytes = encodeClientMsg(wire);
  } catch (error) {
    throw new Error('encodeClientMsg(' + msg.type + ') failed: ' + error.message);
  }
  client.ws.sendBinary(bytes);
}

/* ------------------------------------------------------------------ *
 * Stub driver
 * ------------------------------------------------------------------ */

function planVotes(sessionIndex, options, pool) {
  const byId = new Map(pool.map((option) => [option.id, option]));
  if (sessionIndex === 0) {
    const index = options.findIndex((option) => {
      const entry = byId.get(option.id);
      return entry ? entry.permanent === true : false;
    });
    const target = index >= 0 ? index : 0;
    return [target, target, target];
  }
  if (sessionIndex === 1) {
    const index = options.findIndex((option) => {
      const entry = byId.get(option.id);
      return entry ? entry.permanent === false : false;
    });
    const target = index >= 0 ? index : 0;
    return [target, target, target];
  }
  return [0, 1, 2];
}

function lastVoteStateCounts(log, fromAt, toAt) {
  let counts = null;
  for (const entry of log) {
    if (!entry.msg || entry.msg.type !== 'VoteState') continue;
    if (entry.at < fromAt || entry.at > toAt) continue;
    counts = entry.msg.counts;
  }
  return counts;
}

function lastRunStateBefore(log, at) {
  let found = null;
  for (const entry of log) {
    if (!entry.msg || entry.msg.type !== 'RunState') continue;
    if (entry.at > at) break;
    found = entry;
  }
  return found;
}

async function driveStub(url, pool) {
  const harness = new Harness(url);
  const clients = [];

  for (let i = 0; i < CLIENT_COUNT; i += 1) {
    const client = await harness.connect('kit-' + i);
    client.onMessage = (msg) => {
      if (msg.type === 'Draft') sendClient(client, { type: 'DraftPick', picks: [0] });
    };
    clients.push(client);
    sendClient(client, { type: 'Hello', protocol_version: PROTOCOL_VERSION, name: 'kit-' + i });
    sendClient(client, { type: 'ChooseClass', class: i % 3 });
  }

  const observer = clients[0];
  const sessions = [];
  const rejoin = { attempted: false, ok: false, detail: 'rejoin was never attempted' };
  const lateVote = { sent: false, error: null, stateAfterResult: null };

  // The log is scanned linearly by `waitFor`, so a wait for a later message
  // consumes every earlier one. Session opens are therefore matched from the
  // start of the log against the last level already seen, not from the cursor.
  let lastLevel = 0;
  for (let index = 0; index < SESSION_COUNT; index += 1) {
    const openEntry = await harness.waitForAny(
      observer,
      (entry) =>
        Boolean(entry.msg && entry.msg.type === 'VoteOpen' && entry.msg.level > lastLevel),
      10000,
    );
    if (!openEntry) break;
    lastLevel = openEntry.msg.level;

    const open = openEntry.msg;
    const session = {
      level: open.level,
      options: Array.isArray(open.options) ? open.options : [],
      countdownS: open.countdown_s,
      openAt: openEntry.at,
      resultAt: 0,
      result: null,
      finalCounts: null,
    };
    sessions.push(session);
    if (index > 0) {
      const prev = sessions[index - 1];
      const rs = lastRunStateBefore(observer.log, session.openAt);
      prev.enemiesAfter =
        rs && rs.msg && rs.msg.info && Array.isArray(rs.msg.info.enemies)
          ? rs.msg.info.enemies.slice()
          : [];
    }

    const plan = planVotes(index, session.options, pool);
    for (let i = 0; i < clients.length; i += 1) {
      sendClient(clients[i], { type: 'Vote', option: plan[i] });
    }

    if (index === 1 && !rejoin.attempted) {
      rejoin.attempted = true;
      const victim = clients[2];
      victim.ws.close();
      await sleep(80);
      const back = await harness.connect('kit-rejoin');
      sendClient(back, { type: 'Hello', protocol_version: PROTOCOL_VERSION, name: 'kit-rejoin' });
      // Only a replay of the session that was open at rejoin time proves the
      // rejoin path ran; a later session's VoteOpen would arrive anyway.
      const openLevel = session.level;
      const seen = await harness.waitFor(
        back,
        (entry) =>
          Boolean(entry.msg && entry.msg.type === 'VoteOpen' && entry.msg.level === openLevel),
        2000,
      );
      if (!seen) {
        rejoin.ok = false;
        rejoin.detail = 'no VoteOpen was replayed to the rejoining client within 2000ms';
      } else if (seen.msg.type === 'VoteState') {
        const counts = Array.isArray(seen.msg.counts) ? seen.msg.counts : [];
        const total = counts.reduce((sum, value) => sum + value, 0);
        rejoin.ok = total >= CLIENT_COUNT;
        rejoin.detail = rejoin.ok
          ? 'rejoin replayed VoteState counts=' + JSON.stringify(counts)
          : 'rejoin replayed VoteState counts=' + JSON.stringify(counts) +
            ' but ' + CLIENT_COUNT + ' votes had already been cast';
      } else {
        rejoin.ok = true;
        rejoin.detail = 'rejoin replayed VoteOpen for level ' + seen.msg.level;
      }
      clients[2] = back;
    }

    const resultEntry = await harness.waitFor(
      observer,
      (entry) => Boolean(entry.msg && entry.msg.type === 'VoteResult'),
      10000,
    );
    if (!resultEntry) break;

    session.result = resultEntry.msg;
    session.resultAt = resultEntry.at;
    session.finalCounts = lastVoteStateCounts(observer.log, session.openAt, session.resultAt);

    if (!lateVote.sent) {
      lateVote.sent = true;
      sendClient(observer, { type: 'Vote', option: 0 });
      const errorEntry = await harness.waitFor(
        observer,
        (entry) => Boolean(entry.msg && entry.msg.type === 'Error'),
        800,
      );
      lateVote.error = errorEntry ? errorEntry.msg : null;
      const stray = observer.log.find(
        (entry) => entry.msg && entry.msg.type === 'VoteState' && entry.at > session.resultAt + 5,
      );
      lateVote.stateAfterResult = stray ? stray.msg : null;
    }
  }

  const observerLog = observer.log.slice();
  harness.dispose();
  return { sessions, rejoin, lateVote, observerLog, textFrames: harness.textFrames };
}

/* ------------------------------------------------------------------ *
 * The ten stub checks
 * ------------------------------------------------------------------ */

function evaluate(data, pool) {
  const { sessions, rejoin, lateVote, observerLog } = data;
  const byId = new Map(pool.map((option) => [option.id, option]));
  const results = [];
  const push = (name, ok, detail) => results.push({ name, ok, detail });

  // 1. session-at-every-level
  {
    const levels = sessions.map((session) => session.level);
    const contiguous =
      levels.length === SESSION_COUNT && levels.every((level, index) => level === index + 1);
    let matched = true;
    let mismatch = '';
    for (const session of sessions) {
      const runState = lastRunStateBefore(observerLog, session.openAt);
      if (!runState || !runState.msg.info || runState.msg.info.level !== session.level) {
        matched = false;
        mismatch = 'level ' + session.level + ' was not preceded by a matching RunState';
        break;
      }
    }
    if (!contiguous) {
      push(
        'session-at-every-level',
        false,
        'expected levels 1..' + SESSION_COUNT + ', saw ' + JSON.stringify(levels),
      );
    } else {
      push(
        'session-at-every-level',
        matched,
        matched
          ? 'levels ' + levels.join(',') + ' each preceded by a matching RunState'
          : mismatch,
      );
    }
  }

  // 2. options-shape
  {
    let ok = true;
    let detail = 'every VoteOpen carried ' + OPTIONS_PER_SESSION + ' well-formed options';
    for (const session of sessions) {
      if (session.options.length !== OPTIONS_PER_SESSION) {
        ok = false;
        detail = 'level ' + session.level + ' offered ' + session.options.length + ' options';
        break;
      }
      const seen = new Set();
      for (let i = 0; i < session.options.length; i += 1) {
        const option = session.options[i];
        const wellFormed =
          option &&
          option.index === i &&
          typeof option.id === 'string' &&
          option.id.length > 0 &&
          Number.isInteger(option.kind) &&
          option.kind >= 0 &&
          option.kind <= 255;
        if (!wellFormed) {
          ok = false;
          detail = 'level ' + session.level + ' option ' + i + ' is malformed: ' + JSON.stringify(option);
          break;
        }
        if (seen.has(option.id)) {
          ok = false;
          detail = 'level ' + session.level + ' repeats option id ' + option.id;
          break;
        }
        seen.add(option.id);
      }
      if (!ok) break;
    }
    push('options-shape', ok, detail);
  }

  // 3. majority-wins
  {
    let ok = true;
    let detail = 'every unique-max session elected the top option';
    let checked = 0;
    for (const session of sessions) {
      const counts = session.finalCounts;
      if (!counts || !session.result) continue;
      const max = Math.max(...counts);
      const leaders = counts
        .map((count, index) => (count === max ? index : -1))
        .filter((index) => index >= 0);
      if (leaders.length !== 1) continue;
      checked += 1;
      if (session.result.winner !== leaders[0]) {
        ok = false;
        detail =
          'level ' + session.level + ' counts ' + JSON.stringify(counts) +
          ' but the winner was ' + session.result.winner;
        break;
      }
    }
    if (ok && checked === 0) {
      ok = false;
      detail = 'no session produced a unique majority';
    }
    push('majority-wins', ok, detail);
  }

  // 4. tie-rules
  {
    let ok = true;
    let detail = 'ties resolved to the lowest tied index';
    let checked = 0;
    for (const session of sessions) {
      const counts = session.finalCounts;
      if (!counts || !session.result) continue;
      const max = Math.max(...counts);
      const tied = counts
        .map((count, index) => (count === max ? index : -1))
        .filter((index) => index >= 0);
      if (tied.length < 2) continue;
      checked += 1;
      if (session.result.winner !== tied[0]) {
        ok = false;
        detail =
          'level ' + session.level + ' tied on ' + JSON.stringify(tied) +
          ' but the winner was ' + session.result.winner;
        break;
      }
    }
    if (ok && checked === 0) {
      ok = false;
      detail = 'no session produced a tie';
    }
    push('tie-rules', ok, detail);
  }

  // 5. permanence
  {
    let ok = true;
    let detail = "winners report the pool's permanence and persist across levels";
    let checked = 0;
    for (const session of sessions) {
      if (!session.result) continue;
      if (!Array.isArray(session.enemiesAfter)) continue;
      const entry = byId.get(session.result.id);
      if (!entry) continue;
      checked += 1;
      const carried = session.enemiesAfter.includes(session.result.id);
      if (session.result.permanent !== entry.permanent) {
        ok = false;
        detail =
          'level ' + session.level + ' winner ' + session.result.id +
          ' reported permanent=' + session.result.permanent +
          ' but the pool says ' + entry.permanent;
        break;
      }
      if (entry.permanent && !carried) {
        ok = false;
        detail =
          'level ' + session.level + ' winner ' + session.result.id +
          " is permanent but is absent from the next level's enemies";
        break;
      }
      if (!entry.permanent && carried) {
        ok = false;
        detail =
          'level ' + session.level + ' winner ' + session.result.id +
          " is temporary but persists into the next level's enemies";
        break;
      }
    }
    if (ok && checked === 0) {
      ok = false;
      detail = 'no VoteResult could be matched against the pool and a following RunState';
    }
    push('permanence', ok, detail);
  }

  // 6. cap-aware-pool
  {
    const offered = new Map();
    for (const session of sessions) {
      for (const option of session.options) {
        offered.set(option.id, (offered.get(option.id) || 0) + 1);
      }
    }
    let ok = true;
    let detail = 'no option exceeded its selection cap';
    for (const [id, count] of offered) {
      const entry = byId.get(id);
      if (!entry) continue;
      if (count > entry.cap) {
        ok = false;
        detail = id + ' was offered ' + count + ' times but its cap is ' + entry.cap;
        break;
      }
    }
    push('cap-aware-pool', ok, detail);
  }

  // 7. min-level-gate
  {
    let ok = true;
    let detail = 'every offered option met its minimum level';
    for (const session of sessions) {
      for (const option of session.options) {
        const entry = byId.get(option.id);
        if (!entry) continue;
        if (entry.minLevel > session.level) {
          ok = false;
          detail =
            option.id + ' (min level ' + entry.minLevel + ') was offered at level ' + session.level;
          break;
        }
      }
      if (!ok) break;
    }
    push('min-level-gate', ok, detail);
  }

  // 8. overflow-temporary
  {
    let ok = true;
    let detail = 'temporary winners never returned to the pool';
    let checked = 0;
    for (let i = 0; i < sessions.length; i += 1) {
      const session = sessions[i];
      if (!session.result) continue;
      const entry = byId.get(session.result.id);
      if (!entry || entry.permanent) continue;
      checked += 1;
      for (let j = i + 1; j < sessions.length; j += 1) {
        if (sessions[j].options.some((option) => option.id === session.result.id)) {
          ok = false;
          detail =
            'temporary winner ' + session.result.id +
            ' reappeared at level ' + sessions[j].level;
          break;
        }
      }
      if (!ok) break;
    }
    if (ok && checked === 0) {
      ok = false;
      detail = 'no temporary option ever won';
    }
    push('overflow-temporary', ok, detail);
  }

  // 9. rejoin-safety
  push('rejoin-safety', rejoin.ok, rejoin.detail);

  // 10. countdown-late-vote
  {
    let ok = false;
    let detail;
    if (!lateVote.sent) {
      detail = 'no late vote was sent';
    } else if (lateVote.stateAfterResult) {
      detail =
        'a vote sent after VoteResult was counted (VoteState counts=' +
        JSON.stringify(lateVote.stateAfterResult.counts) + ')';
    } else if (!lateVote.error) {
      detail = 'a vote sent after VoteResult produced no Error';
    } else {
      ok = true;
      detail = 'late vote rejected with Error code ' + lateVote.error.code;
    }
    push('countdown-late-vote', ok, detail);
  }

  return results;
}

/* ------------------------------------------------------------------ *
 * Reporting
 * ------------------------------------------------------------------ */

function report(title, results) {
  process.stdout.write('voting-conformance: ' + title + '\n');
  for (const result of results) {
    process.stdout.write(
      '  ' + (result.ok ? 'PASS' : 'FAIL') + '  ' +
        result.name.padEnd(24) + '  ' + result.detail + '\n',
    );
  }
  const passed = results.filter((result) => result.ok).length;
  process.stdout.write(
    'voting-conformance: ' + passed + '/' + results.length + ' checks passed\n',
  );
  return passed === results.length;
}

function reportTextFrames(textFrames) {
  if (textFrames.length === 0) return false;
  for (const frame of textFrames) {
    process.stdout.write(
      '  FAIL  binary-transport          text frame from ' + frame.from +
        ': ' + JSON.stringify(frame.text.slice(0, 200)) + '\n',
    );
  }
  return true;
}

/* ------------------------------------------------------------------ *
 * Modes
 * ------------------------------------------------------------------ */

async function stubMode(url) {
  const pool = stubPool();
  const data = await driveStub(url, pool);
  const results = evaluate(data, pool);
  const badTransport = reportTextFrames(data.textFrames);
  const ok = report('stub matrix against ' + url, results);
  return ok && !badTransport;
}

async function realMode(url) {
  const harness = new Harness(url);
  const results = [];
  const push = (name, ok, detail) => results.push({ name, ok, detail });

  try {
    const client = await harness.connect('kit-real');
    sendClient(client, { type: 'Hello', protocol_version: PROTOCOL_VERSION, name: 'kit-real' });
    sendClient(client, { type: 'Vote', option: 0 });

    const welcome = await harness.waitForAny(
      client,
      (entry) => Boolean(entry.msg && entry.msg.type === 'Welcome'),
      8000,
    );
    if (!welcome) {
      push('real-welcome', false, 'no Welcome arrived within 8000ms');
    } else if (welcome.msg.protocol_version !== PROTOCOL_VERSION) {
      push(
        'real-welcome',
        false,
        'Welcome carried protocol_version ' + welcome.msg.protocol_version +
          ', expected ' + PROTOCOL_VERSION,
      );
    } else {
      push('real-welcome', true, 'Welcome decoded at protocol version ' + PROTOCOL_VERSION);
    }

    const runState = await harness.waitForAny(
      client,
      (entry) => Boolean(entry.msg && entry.msg.type === 'RunState'),
      8000,
    );
    if (!runState) {
      push('real-runstate', false, 'no RunState arrived within 8000ms');
    } else if (!runState.msg.info || !KNOWN_PHASES.includes(runState.msg.info.phase)) {
      push(
        'real-runstate',
        false,
        'RunState carried an unknown phase: ' + safeJson(runState.msg.info),
      );
    } else {
      push(
        'real-runstate',
        true,
        'RunState phase ' + runState.msg.info.phase + ' at level ' + runState.msg.info.level,
      );
    }

    const error = await harness.waitForAny(
      client,
      (entry) => Boolean(entry.msg && entry.msg.type === 'Error'),
      4000,
    );
    if (!error) {
      push(
        'real-vote-outside-session',
        false,
        'a Vote sent with no session open produced no Error',
      );
    } else if (typeof error.msg.code !== 'number' || typeof error.msg.message !== 'string') {
      push(
        'real-vote-outside-session',
        false,
        'Error frame is malformed: ' + JSON.stringify(error.msg),
      );
    } else {
      push(
        'real-vote-outside-session',
        true,
        'Vote outside a session rejected with code ' + error.msg.code,
      );
    }
  } finally {
    harness.dispose();
  }

  const badTransport = reportTextFrames(harness.textFrames);
  const ok = report('real smoke against ' + url, results);
  return ok && !badTransport;
}

function spawnStub(violation) {
  return new Promise((resolvePromise, rejectPromise) => {
    const child = spawn(process.execPath, [STUB_PATH], {
      env: {
        ...process.env,
        VOTE_STUB_PORT: '0',
        VOTE_STUB_LEVEL_MS: '150',
        VOTE_STUB_DRAFT_MS: '100',
        VOTE_STUB_COUNTDOWN_S: '1',
        VOTE_STUB_VIOLATION: violation,
      },
      stdio: ['ignore', 'pipe', 'pipe'],
    });

    const stderrRef = { text: '' };
    let stdout = '';
    let settled = false;

    const timer = setTimeout(() => {
      if (settled) return;
      settled = true;
      child.kill('SIGKILL');
      rejectPromise(
        new Error('stub did not report a port within 10000ms\n' + stderrRef.text),
      );
    }, 10000);

    child.stdout.on('data', (chunk) => {
      stdout += chunk.toString('utf8');
      const match = stdout.match(/ws:\/\/127\.0\.0\.1:(\d+)/);
      if (match && !settled) {
        settled = true;
        clearTimeout(timer);
        resolvePromise({ url: 'ws://127.0.0.1:' + match[1], child, stderrRef });
      }
    });

    child.stderr.on('data', (chunk) => {
      stderrRef.text += chunk.toString('utf8');
    });

    child.on('exit', (code) => {
      if (settled) return;
      settled = true;
      clearTimeout(timer);
      rejectPromise(
        new Error('stub exited with code ' + code + ' before reporting a port\n' + stderrRef.text),
      );
    });
  });
}

async function selftest() {
  const cases = [
    { label: 'faithful', violation: '', expect: [] },
    { label: 'majority_inverted', violation: 'majority_inverted', expect: ['majority-wins'] },
    { label: 'tie_ignored', violation: 'tie_ignored', expect: ['tie-rules'] },
    { label: 'cap_ignored', violation: 'cap_ignored', expect: ['cap-aware-pool'] },
    { label: 'permanence_dropped', violation: 'permanence_dropped', expect: ['permanence'] },
    { label: 'rejoin_forgotten', violation: 'rejoin_forgotten', expect: ['rejoin-safety'] },
    { label: 'late_vote_accepted', violation: 'late_vote_accepted', expect: ['countdown-late-vote'] },
  ];

  const pool = stubPool();
  let failures = 0;

  for (const testCase of cases) {
    let stub = null;
    let results = [];
    let textFrames = [];
    let thrown = null;

    try {
      stub = await spawnStub(testCase.violation);
      const data = await driveStub(stub.url, pool);
      results = evaluate(data, pool);
      textFrames = data.textFrames;
    } catch (error) {
      thrown = error;
    } finally {
      if (stub) stub.child.kill('SIGKILL');
    }

    const failed = results.filter((result) => !result.ok).map((result) => result.name);
    const label = testCase.violation === '' ? 'faithful' : testCase.violation;

    if (thrown) {
      failures += 1;
      process.stdout.write('voting-conformance: selftest ' + label + ' FAILED to run\n');
      process.stdout.write('  ' + String(thrown.message).split('\n').join('\n  ') + '\n');
      if (stub && stub.stderrRef.text) {
        process.stdout.write('  stub stderr: ' + stub.stderrRef.text.trim() + '\n');
      }
      continue;
    }

    if (textFrames.length > 0) {
      failures += 1;
      process.stdout.write('voting-conformance: selftest ' + label + ' FAILED (text frame)\n');
      reportTextFrames(textFrames);
      continue;
    }

    if (testCase.expect.length === 0) {
      const ok = report('selftest ' + label, results);
      if (!ok) failures += 1;
      continue;
    }

    const missing = testCase.expect.filter((name) => !failed.includes(name));
    const ok = missing.length === 0;
    process.stdout.write(
      'voting-conformance: selftest ' + label + ' ' + (ok ? 'PASS' : 'FAIL') + '\n',
    );
    process.stdout.write('  expected failures: ' + testCase.expect.join(', ') + '\n');
    process.stdout.write(
      '  observed failures: ' + (failed.length > 0 ? failed.join(', ') : '(none)') + '\n',
    );
    if (!ok) {
      failures += 1;
      process.stdout.write('  missing: ' + missing.join(', ') + '\n');
    }
  }

  process.stdout.write(
    'voting-conformance: selftest ' + (failures === 0 ? 'green' : failures + ' case(s) failed') + '\n',
  );
  return failures === 0;
}

/* ------------------------------------------------------------------ *
 * Entry point
 * ------------------------------------------------------------------ */

function argValue(argv, flag) {
  const index = argv.indexOf(flag);
  if (index >= 0 && index + 1 < argv.length) return argv[index + 1];
  const prefix = flag + '=';
  const hit = argv.find((arg) => arg.startsWith(prefix));
  return hit ? hit.slice(prefix.length) : null;
}

async function main() {
  const argv = process.argv.slice(2);
  const url = argValue(argv, '--url') || process.env.VOTING_URL || '';

  if (argv.includes('--selftest')) {
    const ok = await selftest();
    process.exitCode = ok ? 0 : 1;
    return;
  }

  if (argv.includes('--real')) {
    if (!url) {
      process.stderr.write('voting-conformance: --real needs --url or VOTING_URL\n');
      process.exitCode = 2;
      return;
    }
    const ok = await realMode(url);
    process.exitCode = ok ? 0 : 1;
    return;
  }

  if (!url) {
    process.stdout.write('voting-conformance: skipped (set VOTING_URL or pass --url)\n');
    process.exitCode = 0;
    return;
  }

  const ok = await stubMode(url);
  process.exitCode = ok ? 0 : 1;
}

main().catch((error) => {
  process.stderr.write('voting-conformance: ' + (error && error.stack ? error.stack : String(error)) + '\n');
  process.exitCode = 1;
});

