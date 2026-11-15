#!/usr/bin/env node
/**
 * tools/debug-conformance/run.mjs
 *
 * End-to-end conformance kit for the debug command layer (T-141).
 *
 * Why this exists: voting, the shop and altars all sit behind level 2, and the
 * playthrough bots cannot reach level 2 (T-139: a run starts with 0 spare lives, so
 * the first fall ends it). That left three shipped systems with no end-to-end
 * coverage at all. The debug command layer can drive them directly, in singleplayer,
 * without needing a bot to survive a level.
 *
 * The kit speaks the real wire format through `../voting-conformance/wire.mjs`, so a
 * protocol change breaks it the same way it breaks the voting kit.
 *
 * Usage:
 *   node tools/debug-conformance/run.mjs --url ws://127.0.0.1:7979/ws [--code devaccess]
 *
 * With no URL the kit prints `skipped` and exits 0, so `just verify` stays green on
 * machines without a server.
 */

import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { dirname, join } from 'node:path';

import { encodeClientMsg, decodeServerMsg } from '../voting-conformance/wire.mjs';

const HERE = dirname(fileURLToPath(import.meta.url));
const ROOT = join(HERE, '..', '..');

const argValue = (name, fallback) => {
  const i = process.argv.indexOf(name);
  if (i >= 0 && i + 1 < process.argv.length) return process.argv[i + 1];
  return fallback;
};

const URL_ = argValue('--url', process.env.DEBUG_URL || '');
const CODE = argValue('--code', process.env.VOIDWRAP_DEV_CODE || 'devaccess');
const PROTOCOL_VERSION = 8;
const PHASE_PLAYING = 1;
const PHASE_REWARD_DRAFT = 2;

const sleep = (ms) => new Promise((done) => setTimeout(done, ms));

if (!URL_) {
  console.log('debug-conformance: skipped (set DEBUG_URL or pass --url)');
  process.exit(0);
}

/** Content ids, read from the shipped catalog so the kit cannot drift. */
function readIds() {
  try {
    const content = JSON.parse(readFileSync(join(ROOT, 'design', 'content.json'), 'utf8'));
    return {
      enemy: content.enemies[0].id,
      upgrade: content.upgrades[0].id,
      curse: content.curses[0].id,
    };
  } catch {
    return { enemy: 'pallbearer', upgrade: 'zephyr_tread', curse: 'leadened_soles' };
  }
}

/** A minimal client over the global WebSocket. */
class Client {
  constructor(url, name) {
    this.name = name;
    this.frames = [];
    this.waiters = [];
    this.closed = false;
    this.ws = new WebSocket(url);
    this.ws.binaryType = 'arraybuffer';
    this.ready = new Promise((resolve, reject) => {
      this.ws.onopen = () => resolve();
      this.ws.onerror = () => reject(new Error(`${name}: socket error`));
    });
    this.ws.onmessage = (event) => {
      if (typeof event.data === 'string') {
        this.frames.push({ text: event.data });
        this._wake();
        return;
      }
      try {
        this.frames.push({ msg: decodeServerMsg(Buffer.from(event.data)) });
      } catch (err) {
        this.frames.push({ decodeError: String(err) });
      }
      this._wake();
    };
    this.ws.onclose = () => {
      this.closed = true;
      this._wake();
    };
  }

  _wake() {
    const pending = this.waiters;
    this.waiters = [];
    for (const w of pending) w();
  }

  send(msg) {
    this.ws.send(encodeClientMsg(msg));
  }

  /** Wait for the first frame matching `pred`, scanning frames already received. */
  async waitFor(pred, timeoutMs = 4000) {
    const deadline = Date.now() + timeoutMs;
    for (;;) {
      const hit = this.frames.find(pred);
      if (hit) return hit;
      if (this.closed) return null;
      const left = deadline - Date.now();
      if (left <= 0) return null;
      await new Promise((resolve) => {
        this.waiters.push(resolve);
        setTimeout(resolve, Math.min(left, 100));
      });
    }
  }

  /** Send a debug command and wait for its DebugResult. */
  async debug(command, args = {}, code = CODE) {
    const before = this.frames.length;
    this.send({ variant: 'Debug', code, command, args: JSON.stringify(args) });
    const deadline = Date.now() + 4000;
    for (;;) {
      const hit = this.frames.slice(before).find((f) => f.msg && f.msg.variant === 'DebugResult');
      if (hit) return hit.msg;
      if (Date.now() > deadline) return { ok: false, message: 'timeout waiting for DebugResult' };
      await sleep(20);
    }
  }

  /** The parsed `state` readout, or null. */
  async state() {
    const res = await this.debug('state');
    if (!res.ok) return null;
    const out = {};
    for (const part of res.message.split(/\s+/)) {
      const eq = part.indexOf('=');
      if (eq > 0) out[part.slice(0, eq)] = part.slice(eq + 1);
    }
    return out;
  }

  close() {
    try {
      this.ws.close();
    } catch {
      /* already gone */
    }
  }
}

async function connect(url, name) {
  const client = new Client(url, name);
  await client.ready;
  client.send({ variant: 'Hello', protocol_version: PROTOCOL_VERSION, name });
  const welcome = await client.waitFor((f) => f.msg && f.msg.variant === 'Welcome', 8000);
  if (!welcome) throw new Error(`${name}: no Welcome`);
  return client;
}

const results = [];
const push = (name, ok, detail) => results.push({ name, ok, detail });

async function main() {
  const ids = readIds();
  const client = await connect(URL_, 'solo');

  const st = await client.state();
  push(
    'debug-state',
    Boolean(st && st.level && st.phase !== undefined && st.currency !== undefined),
    st
      ? `level=${st.level} phase=${st.phase} currency=${st.currency} gifts_left=${st.gifts_left}`
      : 'state did not return a parseable readout',
  );

  const noCode = await client.debug('state', {}, '');
  push(
    'debug-no-code',
    noCode.ok === false && /not enabled/.test(noCode.message),
    `ok=${noCode.ok} message=${JSON.stringify(noCode.message)}`,
  );

  const wrong = await client.debug('state', {}, 'definitely-not-the-code');
  push(
    'debug-wrong-code',
    wrong.ok === false && /not enabled/.test(wrong.message),
    `ok=${wrong.ok} message=${JSON.stringify(wrong.message)}`,
  );

  // Wrong codes are rate-limited per connection, so this must be the last wrong-code
  // check on this connection: it locks the connection out.
  let rateLimited = null;
  for (let i = 0; i < 12; i += 1) {
    const res = await client.debug('state', {}, `wrong-${i}`);
    if (/too many wrong codes/.test(res.message)) {
      rateLimited = i + 1;
      break;
    }
  }
  push(
    'debug-rate-limit',
    rateLimited !== null,
    rateLimited === null
      ? '12 wrong codes were all answered with the generic rejection; the rate limit never fired'
      : `locked out after ${rateLimited} wrong codes`,
  );

  client.close();
  await sleep(200);
  const solo = await connect(URL_, 'solo2');

  const before = await solo.state();
  const grant = await solo.debug('grant_gold', { amount: 500 });
  const after = await solo.state();
  const delta = Number(after?.currency) - Number(before?.currency);
  push(
    'debug-grant-gold',
    grant.ok && delta === 500,
    `ok=${grant.ok} currency ${before?.currency} -> ${after?.currency} (delta ${delta}, expected 500)`,
  );

  const jump = await solo.debug('switch_level', { level: 5, seed: 12345 });
  const at5 = await solo.state();
  push(
    'debug-switch-level',
    jump.ok && at5?.level === '5' && Number(at5?.currency) === Number(after?.currency),
    `ok=${jump.ok} level=${at5?.level} currency=${at5?.currency} (kept from ${after?.currency})`,
  );

  const phase = await solo.debug('set_phase', { phase: PHASE_PLAYING });
  const playing = await solo.state();
  push(
    'debug-set-phase',
    phase.ok && playing?.phase === String(PHASE_PLAYING),
    `ok=${phase.ok} phase=${playing?.phase} (expected ${PHASE_PLAYING})`,
  );

  const up = await solo.debug('give_upgrade', { id: ids.upgrade });
  const withUp = await solo.state();
  push(
    'debug-give-upgrade',
    up.ok && String(withUp?.upgrades || '').includes(ids.upgrade),
    `ok=${up.ok} upgrades=[${withUp?.upgrades}] expected to contain ${ids.upgrade}`,
  );

  const cur = await solo.debug('give_curse', { id: ids.curse });
  const withCur = await solo.state();
  push(
    'debug-give-curse',
    cur.ok && String(withCur?.curses || '').includes(ids.curse),
    `ok=${cur.ok} curses=[${withCur?.curses}] expected to contain ${ids.curse}`,
  );

  // Altars are placed per level, so a given level may have none. Walk a few levels and
  // slot ids until one fires; the error string distinguishes "no altar here" from
  // "altar exists but is unusable", so a total miss is a real failure, not a skip.
  let altar = null;
  let altarWhere = '';
  for (const level of [5, 7, 9, 12, 15]) {
    await solo.debug('switch_level', { level, seed: 12345 });
    for (let id = 0; id < 4; id += 1) {
      const res = await solo.debug('trigger_altar', { id });
      if (res.ok) {
        altar = res;
        altarWhere = `level ${level} altar ${id}`;
        break;
      }
    }
    if (altar) break;
  }
  const afterAltar = await solo.state();
  push(
    'debug-trigger-altar',
    Boolean(altar) && Number(afterAltar?.altars_used) >= 1,
    altar
      ? `${altarWhere}: ${JSON.stringify(altar.message)} altars_used=${afterAltar?.altars_used}`
      : `no altar fired on levels 5/7/9/12/15 with ids 0-3; altars_used=${afterAltar?.altars_used}`,
  );

  const spawn = await solo.debug('spawn_enemy', { id: ids.enemy, count: 1 });
  const withEnemy = await solo.state();
  const despawn = await solo.debug('despawn_enemy', { id: ids.enemy });
  const withoutEnemy = await solo.state();
  push(
    'debug-spawn-despawn-enemy',
    spawn.ok && String(withEnemy?.enemies || '').includes(ids.enemy)
      && despawn.ok && !String(withoutEnemy?.enemies || '').includes(ids.enemy),
    `spawn ok=${spawn.ok} enemies=[${withEnemy?.enemies}] -> despawn ok=${despawn.ok} enemies=[${withoutEnemy?.enemies}]`,
  );

  // The shop frame is the coverage the playthrough could never reach. `make_shop`
  // returns None at level 1, so the shop only exists from level 2 on; the server sends
  // it on join, which is the deterministic path to observe it.
  await solo.debug('switch_level', { level: 6, seed: 12345 });
  const shopper = await connect(URL_, 'shopper');
  const shop = await shopper.waitFor((f) => f.msg && f.msg.variant === 'Shop', 5000);
  push(
    'debug-shop-frame',
    Boolean(shop),
    shop
      ? `Shop frame with ${shop.msg.items?.length ?? '?'} items on join at level 6`
      : 'no Shop frame within 5000ms of joining at level 6',
  );
  shopper.close();
  await sleep(200);

  const second = await connect(URL_, 'intruder');
  await sleep(300);
  const blocked = await solo.debug('grant_gold', { amount: 1 });
  push(
    'debug-singleplayer',
    blocked.ok === false && /singleplayer only/.test(blocked.message),
    `ok=${blocked.ok} message=${JSON.stringify(blocked.message)}`,
  );

  second.close();
  solo.close();
}

try {
  await main();
} catch (err) {
  push('debug-kit', false, `kit crashed: ${err && err.message ? err.message : String(err)}`);
}

const failed = results.filter((r) => r.ok === false);
for (const r of results) {
  console.log(`${r.ok ? 'PASS' : 'FAIL'} - ${r.name} (${r.detail})`);
}
console.log(
  failed.length === 0
    ? `debug-conformance: ${results.length}/${results.length} checks passed`
    : `debug-conformance: FAIL (${failed.length} of ${results.length} checks failed: ${failed.map((r) => r.name).join(', ')})`,
);
process.exit(failed.length === 0 ? 0 : 1);
