#!/usr/bin/env node
/**
 * tools/vote-integration/run.mjs
 *
 * Opens a REAL pedestal vote session on a live server and casts a ballot.
 *
 * Why this exists (T-150): the voting-conformance kit's `--real` mode only checks
 * that Welcome and RunState arrive and that a ballot outside a session is rejected.
 * Its stub mode tests the protocol against a stub, not the server. Nothing anywhere
 * opened a real session in a real run -- so when T-149 found that the client had a
 * complete vote UI that nothing mounted, no test could have caught it, and after the
 * fix no test proved the mechanic worked end to end.
 *
 * The kit drives the run with the frozen debug manifest (design/DEBUG_COMMANDS.md):
 * switch to a level with a vote pool, grant every gift so the level enters the
 * collapse window (which is when `open_vote_session` fires), then ballot.
 *
 *   node tools/vote-integration/run.mjs --url ws://127.0.0.1:7979/ws [--code devaccess]
 *
 * With no URL it prints `skipped` and exits 0, so `just verify` stays green on
 * machines without a server.
 */
import { encodeClientMsg, decodeServerMsg } from '../voting-conformance/wire.mjs';

const PROTOCOL_VERSION = 8;
const argValue = (name, def) => {
  const i = process.argv.indexOf(name);
  return i > 0 ? process.argv[i + 1] : def;
};
const URL_ARG = argValue('--url', process.env.VOTE_INTEGRATION_URL || null);
const CODE = argValue('--code', process.env.VOIDWRAP_DEV_CODE || 'devaccess');
// Which pedestal to ballot for. Exposed so the negative test can prove the ballot
// decides the outcome rather than the server's RNG fallback.
const OPTION = Number(argValue('--option', '1'));
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

const results = [];
const push = (name, ok, detail) => {
  results.push({ name, ok, detail });
  console.log(`${ok ? 'PASS' : 'FAIL'} - ${name} (${detail})`);
};

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

  /** Every decoded frame whose variant matches. */
  all(variant) {
    return this.frames.filter((f) => f.msg && f.msg.variant === variant).map((f) => f.msg);
  }

  /** Wait for the first frame matching `pred`, scanning frames already received. */
  async waitFor(pred, timeoutMs = 5000) {
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
  async debug(command, args = {}) {
    const before = this.frames.length;
    this.send({ variant: 'Debug', code: CODE, command, args: JSON.stringify(args) });
    const deadline = Date.now() + 5000;
    for (;;) {
      const hit = this.frames.slice(before).find((f) => f.msg && f.msg.variant === 'DebugResult');
      if (hit) return hit.msg;
      if (Date.now() > deadline) return { ok: false, message: 'timeout waiting for DebugResult' };
      await sleep(20);
    }
  }

  close() {
    try {
      this.ws.close();
    } catch {
      /* already gone */
    }
  }
}

/** Pull `gifts_left` out of the `state` readout. */
function giftsLeft(stateMsg) {
  const m = /gifts_left=(\d+)/.exec(stateMsg?.message ?? '');
  return m ? Number(m[1]) : null;
}

async function main() {
  if (!URL_ARG) {
    console.log('vote-integration: skipped (set VOTE_INTEGRATION_URL or pass --url)');
    return 0;
  }

  const client = new Client(URL_ARG, 'vote-integration');
  await client.ready;
  client.send({ variant: 'Hello', protocol_version: PROTOCOL_VERSION, name: 'vote-integration' });

  const welcome = await client.waitFor((f) => f.msg && f.msg.variant === 'Welcome', 8000);
  if (!welcome) {
    push('connect', false, 'no Welcome within 8000ms');
    client.close();
    return 1;
  }
  push('connect', true, `Welcome at protocol version ${PROTOCOL_VERSION}`);

  // Level 2 has a vote pool; level 1 does not (make_shop and the pool both start later).
  const switched = await client.debug('switch_level', { level: 2 });
  push('switch-level', switched.ok === true, switched.message ?? 'no message');
  if (!switched.ok) {
    client.close();
    return 1;
  }

  // Grant every normal gift. `count` is a u8, so a level with more than 255 gifts
  // needs several calls; loop until the readout says none are left.
  let left = null;
  for (let i = 0; i < 12; i += 1) {
    const granted = await client.debug('grant_gift', { count: 255 });
    if (!granted.ok) {
      push('grant-gifts', false, granted.message ?? 'grant_gift failed');
      client.close();
      return 1;
    }
    const state = await client.debug('state');
    left = giftsLeft(state);
    if (left === 0) break;
  }
  push('grant-gifts', left === 0, `gifts_left=${left} after granting every normal gift`);
  if (left !== 0) {
    client.close();
    return 1;
  }

  // The collapse window is when `open_vote_session` fires.
  const open = await client.waitFor((f) => f.msg && f.msg.variant === 'VoteOpen', 8000);
  if (!open) {
    push('vote-opens', false, 'no VoteOpen within 8000ms of the level emptying');
    client.close();
    return 1;
  }
  const options = open.msg.options ?? [];
  push(
    'vote-opens',
    options.length === 3 && open.msg.countdown_s === 20,
    `VoteOpen level=${open.msg.level} options=${options.length} countdown_s=${open.msg.countdown_s}`,
  );
  if (options.length !== 3) {
    client.close();
    return 1;
  }

  // Ballot for option 1 and prove the server counted it.
  const before = client.all('VoteState').length;
  client.send({ variant: 'Vote', option: OPTION });
  const deadline = Date.now() + 5000;
  let counted = null;
  while (Date.now() < deadline) {
    const states = client.all('VoteState');
    if (states.length > before) {
      const latest = states[states.length - 1];
      if ((latest.counts?.[OPTION] ?? 0) >= 1) {
        counted = latest;
        break;
      }
    }
    await sleep(50);
  }
  push(
    'ballot-counted',
    counted !== null,
    counted ? `VoteState counts=[${counted.counts.join(',')}] seconds_left=${counted.seconds_left}` : 'no VoteState showed the ballot',
  );

  // Every joined player has voted, so the session closes early and the result names
  // the option that was actually chosen -- not the RNG fallback.
  const result = await client.waitFor((f) => f.msg && f.msg.variant === 'VoteResult', 8000);
  push(
    'result-names-the-ballot',
    result !== null && result.msg.winner === OPTION,
    result ? `VoteResult winner=${result.msg.winner} (balloted ${OPTION}) id=${result.msg.id} permanent=${result.msg.permanent}` : 'no VoteResult within 8000ms',
  );

  client.close();
  const failed = results.filter((r) => !r.ok).length;
  console.log(`\nvote-integration: ${results.length - failed}/${results.length} checks passed`);
  return failed === 0 ? 0 : 1;
}

main()
  .then((code) => {
    process.exitCode = code;
  })
  .catch((err) => {
    console.error(`vote-integration: ${err?.stack ?? err}`);
    process.exitCode = 1;
  });
