#!/usr/bin/env node
// Black-box conformance kit for the VOIDWRAP end-of-level vote session
// (design/ENEMY_REGISTRY.md §9 "Pool rules (the voting system)").
//
// The kit never reads the voting core: it opens real WebSocket connections (a minimal RFC 6455
// client over node:net / node:tls, so it runs on any Node >= 18 with zero dependencies), drives
// three clients through vote sessions and asserts the behaviour described in T-081. If the
// implementation disagrees with the ticket, the ticket wins — a FAIL here is a reportable mismatch.
//
//   VOTING_URL=ws://127.0.0.1:9001 node tools/voting-conformance/run.mjs
//   node tools/voting-conformance/run.mjs --url ws://127.0.0.1:9001
//   node tools/voting-conformance/run.mjs --selftest
//
// Without --url / VOTING_URL the kit prints "skipped" and exits 0 — that is how `just verify`
// includes it without requiring a live voting server.
// --selftest proves the kit has teeth: the suite must fully pass against the faithful stub
// (stub/server.mjs, spawned on an ephemeral port), and against that stub with each single
// deliberate violation, where the violation's paired check must be among the failures.
//
// The wire contract the kit speaks (and the server under test must implement) is documented in
// README.md next to this file.
import { spawn } from "node:child_process";
import { fileURLToPath } from "node:url";
import net from "node:net";
import tls from "node:tls";
import crypto from "node:crypto";
import { EventEmitter } from "node:events";
import { ENEMIES, capOf, isKnownEnemy, minLevelOf } from "./registry.mjs";

const STUB_PATH = fileURLToPath(new URL("./stub/server.mjs", import.meta.url));

const CONNECT_TIMEOUT_MS = 10_000;
const SPAWN_OR_OPEN_TIMEOUT_MS = 45_000; // a real server may need to run the level before collapsing
const CLOSE_TIMEOUT_MS = 30_000; // the countdown until the session closes itself
const REJECT_TIMEOUT_MS = 5_000;

// [VOTE_STUB_VIOLATION value in stub/server.mjs, the check that must fail because of it]
const VIOLATIONS = [
  ["majority_inverted", "majority-wins"],
  ["tie_ignored", "tie-rules"],
  ["cap_ignored", "cap-aware-pool"],
  ["permanence_dropped", "permanence"],
  ["rejoin_forgotten", "rejoin-safety"],
  ["late_vote_accepted", "countdown-late-vote"],
];

const CHECKS = [
  ["session-at-every-level", checkSessionAtEveryLevel],
  ["options-shape", checkOptionsShape],
  ["majority-wins", checkMajorityWins],
  ["tie-rules", checkTieRules],
  ["permanence", checkPermanence],
  ["cap-aware-pool", checkCapAwarePool],
  ["min-level-gate", checkMinLevelGate],
  ["overflow-temporary", checkOverflowTemporary],
  ["rejoin-safety", checkRejoinSafety],
  ["countdown-late-vote", checkCountdownLateVote],
];

function fail(message) {
  throw new Error(message);
}
const sleep = (ms) => new Promise((resolve) => {
  setTimeout(resolve, ms);
});

// ---------------------------------------------------------------------------
// Minimal RFC 6455 WebSocket client. Deliberately hand-rolled: the global
// WebSocket only exists on newer Node versions, and the kit must be dependency
// free and version independent.
// ---------------------------------------------------------------------------
const WS_GUID = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11";

class WsClient extends EventEmitter {
  constructor(url) {
    super();
    this.url = url instanceof URL ? url : new URL(url);
    this.messages = []; // every text frame, in arrival order: { t, msg }
    this.open = false;
    this.clientId = null;
    this.token = null;
    this.welcome = null;
    this.socket = null;
    this.buffer = Buffer.alloc(0);
  }

  async connect(timeoutMs = CONNECT_TIMEOUT_MS) {
    const secure = this.url.protocol === "wss:";
    const port = Number(this.url.port) || (secure ? 443 : 80);
    const hostname = this.url.hostname;
    const path = (this.url.pathname || "/") + this.url.search;
    const key = crypto.randomBytes(16).toString("base64");
    const expectedAccept = crypto.createHash("sha1").update(key + WS_GUID).digest("base64");
    await new Promise((resolve, reject) => {
      let settled = false;
      let upgraded = false;
      let head = Buffer.alloc(0);
      const socket = secure
        ? tls.connect({ port, host: hostname, servername: hostname })
        : net.connect({ port, host: hostname });
      this.socket = socket;
      socket.on("connect", () => {
        const crlf = String.fromCharCode(13, 10);
        socket.write(
          [`GET ${path} HTTP/1.1`,
            `Host: ${hostname}:${port}`,
            "Upgrade: websocket",
            "Connection: Upgrade",
            `Sec-WebSocket-Key: ${key}`,
            "Sec-WebSocket-Version: 13",
            "",
            ""].join(crlf),
        );
      });
      const timer = setTimeout(() => {
        socket.destroy();
        settle(new Error(`connect/handshake timed out after ${timeoutMs} ms (${this.url})`));
      }, timeoutMs);
      const settle = (error) => {
        if (settled) return;
        settled = true;
        clearTimeout(timer);
        if (error) {
          socket.destroy();
          reject(error);
        } else {
          resolve();
        }
      };
      socket.on("error", (error) => settle(error instanceof Error ? error : new Error(String(error))));
      socket.on("close", () => {
        if (!this.open) settle(new Error(`connection closed during handshake (${this.url})`));
        else this.handleClose();
      });
      socket.on("data", (chunk) => {
        if (this.open) {
          this.buffer = Buffer.concat([this.buffer, chunk]);
          this.pump();
          return;
        }
        if (upgraded) return;
        head = Buffer.concat([head, chunk]);
        const idx = head.indexOf("\r\n\r\n");
        if (idx === -1) return;
        upgraded = true;
        const rawHeader = head.subarray(0, idx).toString("latin1");
        const header = rawHeader.toLowerCase();
        const accept = /sec-websocket-accept:\s*(\S+)/i.exec(rawHeader);
        if (!/^http\/1\.1 101 /.test(header) || accept === null || accept[1] !== expectedAccept) {
          settle(new Error(`bad websocket handshake from ${this.url}`));
          return;
        }
        this.open = true;
        this.buffer = head.subarray(idx + 4);
        this.pump();
        settle();
      });
    });
  }

  pump() {
    for (;;) {
      const buf = this.buffer;
      if (buf.length < 2) return;
      const opcode = buf[0] & 0x0f;
      let len = buf[1] & 0x7f;
      let pos = 2;
      if (len === 126) {
        if (buf.length < 4) return;
        len = buf.readUInt16BE(2);
        pos = 4;
      } else if (len === 127) {
        if (buf.length < 10) return;
        const big = buf.readBigUInt64BE(2);
        if (big > 0x7fffffffn) {
          this.handleClose();
          return;
        }
        len = Number(big);
        pos = 10;
      }
      if (buf.length - pos < len) return;
      const payload = buf.subarray(pos, pos + len);
      this.buffer = buf.subarray(pos + len);
      if (opcode === 0x1) {
        let msg;
        try {
          msg = JSON.parse(payload.toString("utf8"));
        } catch {
          msg = { type: "__raw__", text: payload.toString("utf8") };
        }
        const rec = { t: Date.now(), msg };
        this.messages.push(rec);
        this.emit("message", rec);
      } else if (opcode === 0x9) {
        this.sendFrame(0xA, Buffer.from(payload)); // pong
      } else if (opcode === 0x8) {
        this.handleClose();
        return;
      }
    }
  }

  sendFrame(opcode, payload) {
    if (!this.socket || this.socket.destroyed) return;
    const mask = crypto.randomBytes(4);
    const masked = Buffer.allocUnsafe(payload.length);
    for (let i = 0; i < payload.length; i++) masked[i] = payload[i] ^ mask[i & 3];
    let header;
    if (payload.length < 126) {
      header = Buffer.from([0x80 | opcode, 0x80 | payload.length]);
    } else if (payload.length < 65536) {
      header = Buffer.alloc(4);
      header[0] = 0x80 | opcode;
      header[1] = 0x80 | 126;
      header.writeUInt16BE(payload.length, 2);
    } else {
      header = Buffer.alloc(10);
      header[0] = 0x80 | opcode;
      header[1] = 0x80 | 127;
      header.writeBigUInt64BE(BigInt(payload.length), 2);
    }
    this.socket.write(Buffer.concat([header, mask, masked]));
  }

  send(message) {
    this.sendFrame(0x1, Buffer.from(JSON.stringify(message), "utf8"));
  }

  close() {
    try {
      this.sendFrame(0x8, Buffer.alloc(0));
    } catch {
      // socket already gone
    }
    this.open = false;
    const socket = this.socket;
    if (socket && !socket.destroyed) setTimeout(() => socket.destroy(), 50).unref?.();
  }

  handleClose() {
    if (!this.open) return;
    this.open = false;
    this.emit("close");
  }
}

// ---------------------------------------------------------------------------
// Waiting helpers. Every message ever received stays in ws.messages, so waits
// also see messages that arrived before the wait started.
// ---------------------------------------------------------------------------
function messageMatches(msg, type, match) {
  if (msg?.type !== type) return false;
  for (const [k, v] of Object.entries(match ?? {})) {
    if (msg[k] !== v) return false;
  }
  return true;
}

const isMsg = (type, match = {}) => (rec) => messageMatches(rec.msg, type, match);

function waitFor(clients, predicate, timeoutMs, what) {
  const list = Array.isArray(clients) ? clients : [clients];
  return new Promise((resolve, reject) => {
    let done = false;
    let timer = null;
    const onMessage = () => {
      scan();
    };
    const onClose = () => finish(null, new Error(`${what}: connection closed before the expected message arrived`));
    const cleanup = () => {
      clearTimeout(timer);
      for (const ws of list) {
        ws.off("message", onMessage);
        ws.off("close", onClose);
      }
    };
    const finish = (value, error) => {
      if (done) return;
      done = true;
      cleanup();
      if (error) reject(error);
      else resolve(value);
    };
    const scan = () => {
      for (const ws of list) {
        for (const rec of ws.messages) {
          if (predicate(rec)) {
            finish(rec);
            return true;
          }
        }
      }
      return false;
    };
    timer = setTimeout(() => finish(null, new Error(`${what}: timed out after ${timeoutMs} ms`)), timeoutMs);
    for (const ws of list) {
      ws.on("message", onMessage);
      ws.on("close", onClose);
    }
    scan();
  });
}

// ---------------------------------------------------------------------------
// Flow helpers
// ---------------------------------------------------------------------------
function normalizeWsUrl(raw) {
  let s = String(raw).trim().replace(/\/+$/, "");
  if (/^https:\/\//.test(s)) s = "wss://" + s.slice("https://".length);
  else if (/^http:\/\//.test(s)) s = "ws://" + s.slice("http://".length);
  let url;
  try {
    url = new URL(s);
  } catch {
    throw new Error(`invalid WebSocket URL: ${raw}`);
  }
  if (url.protocol !== "ws:" && url.protocol !== "wss:") {
    throw new Error(`unsupported URL ${raw}: want ws:// or wss://`);
  }
  return url.toString();
}

async function connectClient(base, name, token = undefined) {
  const ws = new WsClient(base);
  await ws.connect();
  ws.send({ type: "hello", name, ...(token ? { token } : {}) });
  const rec = await waitFor(ws, (r) => r.msg.type === "welcome" || r.msg.type === "error", CONNECT_TIMEOUT_MS, `hello ${name}`);
  if (rec.msg.type === "error") fail(`hello ${name} rejected: ${rec.msg.reason ?? "(no reason given)"}`);
  ws.clientId = rec.msg.clientId;
  ws.token = rec.msg.token;
  ws.welcome = rec.msg;
  return ws;
}

async function openRoom(base, size = 3) {
  const clients = [];
  try {
    for (let i = 0; i < size; i++) clients.push(await connectClient(base, i === 0 ? "kit-host" : `kit-${i}`));
    if (clients[0].welcome.host !== true) {
      fail(`the first connected client is not the host (welcome.host = ${JSON.stringify(clients[0].welcome.host)})`);
    }
    for (let i = 1; i < size; i++) {
      if (clients[i].welcome.host !== false) fail(`late joiner kit-${i} unexpectedly reported host = true`);
    }
    return clients;
  } catch (error) {
    for (const c of clients) c.close();
    throw error;
  }
}

function closeAll(clients) {
  for (const c of clients) if (c) c.close();
}

async function startRun(host, { seed, permanents = null, klass = "striker" } = {}) {
  host.send({ type: "start_run", seed, class: klass, ...(permanents ? { permanents } : {}) });
  const rec = await waitFor(host, (r) => r.msg.type === "run_started" || r.msg.type === "error", CONNECT_TIMEOUT_MS, "start_run");
  if (rec.msg.type === "error") fail(`start_run rejected: ${rec.msg.reason ?? "(no reason given)"}`);
  return rec.msg.runId;
}

async function runLevel(host, runId, level) {
  host.send({ type: "start_level", level });
  const spawnRec = await waitFor(host, isMsg("spawn_set", { runId, level }), SPAWN_OR_OPEN_TIMEOUT_MS, `spawn_set for level ${level}`);
  const openRec = await waitFor(host, isMsg("vote_open", { runId, level }), SPAWN_OR_OPEN_TIMEOUT_MS, `vote_open for level ${level}`);
  if (spawnRec.t > openRec.t) {
    fail(`level ${level}: vote_open arrived before spawn_set — the session must open during the collapse window, after the level starts`);
  }
  return { spawn: spawnRec.msg, open: openRec.msg };
}

async function awaitClose(host, runId, sessionId) {
  const rec = await waitFor(host, isMsg("vote_closed", { runId, sessionId }), CLOSE_TIMEOUT_MS, `vote_closed for session ${sessionId}`);
  return rec.msg;
}

async function voteAll(entries, runId, sessionId) {
  for (const [client, optionId] of entries) {
    client.send({ type: "vote", optionId });
    await waitFor(client, isMsg("vote_update", { runId, sessionId, voter: client.clientId }), CONNECT_TIMEOUT_MS, "ack of a vote");
  }
}

const countIn = (list, id) => list.filter((x) => x === id).length;

function multisetEqual(a, b) {
  if (a.length !== b.length) return false;
  const s = [...a].sort();
  const t = [...b].sort();
  return s.every((v, i) => v === t[i]);
}

function fullCapSeeding() {
  const out = [];
  for (const entry of ENEMIES) for (let i = 0; i < entry.cap; i++) out.push(entry.id);
  return out;
}

// ---------------------------------------------------------------------------
// Checks
// ---------------------------------------------------------------------------
async function checkSessionAtEveryLevel(base) {
  const [host, two, three] = await openRoom(base);
  try {
    const runId = await startRun(host, { seed: 1101 });
    for (const level of [1, 2, 3, 4]) {
      const { open } = await runLevel(host, runId, level);
      await awaitClose(host, runId, open.sessionId);
    }
  } finally {
    closeAll([host, two, three]);
  }
}

async function checkOptionsShape(base) {
  const [host, two, three] = await openRoom(base);
  try {
    const runId = await startRun(host, { seed: 1201 });
    for (const level of [1, 2]) {
      const { open } = await runLevel(host, runId, level);
      const options = open.options;
      if (!Array.isArray(options) || options.length !== 3) {
        fail(`level ${level}: offered ${JSON.stringify(options?.length)} options, want exactly 3`);
      }
      const ids = new Set();
      for (const option of options) {
        if (typeof option?.id !== "string" || option.id === "") fail(`level ${level}: option without an id: ${JSON.stringify(option)}`);
        if (!isKnownEnemy(option.id)) fail(`level ${level}: option id ${JSON.stringify(option.id)} is not an enemy from design/ENEMY_REGISTRY.md`);
        if (typeof option?.name !== "string" || option.name === "") fail(`level ${level}: option ${option.id} has no name`);
        if (option?.kind !== "permanent" && option?.kind !== "temporary") {
          fail(`level ${level}: option ${option.id} kind ${JSON.stringify(option?.kind)} is neither "permanent" nor "temporary"`);
        }
        if (ids.has(option.id)) fail(`level ${level}: duplicate option ${option.id}`);
        ids.add(option.id);
      }
      await awaitClose(host, runId, open.sessionId);
    }
  } finally {
    closeAll([host, two, three]);
  }
}

async function checkMajorityWins(base) {
  const [host, two, three] = await openRoom(base);
  try {
    const runId = await startRun(host, { seed: 1301 });
    const { open } = await runLevel(host, runId, 1);
    const want = open.options[0].id;
    const other = open.options[1].id;
    await voteAll([[host, want], [two, want], [three, other]], runId, open.sessionId);
    const closed = await awaitClose(host, runId, open.sessionId);
    if (closed.winnerId !== want) {
      fail(`three clients voted 2-1 for ${JSON.stringify(want)}, but the winner was ${JSON.stringify(closed.winnerId)} — the majority option must win`);
    }
  } finally {
    closeAll([host, two, three]);
  }
}

async function checkTieRules(base) {
  // (a) 1-1 tie between two options: the host's vote breaks the tie.
  {
    const [host, two, three] = await openRoom(base);
    try {
      const runId = await startRun(host, { seed: 1401 });
      const { open } = await runLevel(host, runId, 1);
      const hostPick = open.options[0].id;
      const otherPick = open.options[1].id;
      await voteAll([[host, hostPick], [two, otherPick]], runId, open.sessionId);
      const closed = await awaitClose(host, runId, open.sessionId);
      if (closed.winnerId !== hostPick) {
        fail(`1-1 tie with the host voting ${JSON.stringify(hostPick)} resolved to ${JSON.stringify(closed.winnerId)} — a tie must go to the host's vote`);
      }
    } finally {
      closeAll([host, two, three]);
    }
  }
  // (b) tie with no host vote: the same seed and level must pick the same winner on a re-run.
  {
    const winners = [];
    for (let attempt = 0; attempt < 2; attempt++) {
      const [host, two, three] = await openRoom(base);
      try {
        const runId = await startRun(host, { seed: 1402 });
        const { open } = await runLevel(host, runId, 1);
        await voteAll([[two, open.options[0].id], [three, open.options[1].id]], runId, open.sessionId);
        const closed = await awaitClose(host, runId, open.sessionId);
        winners.push(closed.winnerId);
      } finally {
        closeAll([host, two, three]);
      }
    }
    if (winners[0] !== winners[1]) {
      fail(`no-host-vote ties resolved differently on identical re-runs (seed 1402, level 1): ${JSON.stringify(winners)} — the server pick must be deterministic`);
    }
  }
}

async function checkPermanence(base) {
  const [host, two, three] = await openRoom(base);
  try {
    const runId = await startRun(host, { seed: 1501 });
    const first = await runLevel(host, runId, 1);
    const winner = first.open.options[0].id;
    await voteAll([[host, winner], [two, winner], [three, winner]], runId, first.open.sessionId);
    const closed = await awaitClose(host, runId, first.open.sessionId);
    if (closed.winnerId !== winner) fail(`unanimous vote for ${JSON.stringify(winner)} resolved to ${JSON.stringify(closed.winnerId)}`);
    const second = await runLevel(host, runId, 2);
    if (!second.spawn.enemies.includes(winner)) {
      fail(`winner ${winner} is missing from level 2's spawn set — the vote winner must become permanent`);
    }
    if (countIn(second.spawn.permanents, winner) !== 1) {
      fail(`level 2 spawn set holds ${JSON.stringify(second.spawn.permanents)} — want exactly one permanent ${winner}`);
    }
    await awaitClose(host, runId, second.open.sessionId);
    const third = await runLevel(host, runId, 3);
    if (countIn(third.spawn.permanents, winner) < 1) {
      fail(`winner ${winner} vanished by level 3 — permanence must hold for later levels`);
    }
  } finally {
    closeAll([host, two, three]);
  }
}

async function checkCapAwarePool(base) {
  const [host, two, three] = await openRoom(base);
  try {
    const seeded = ["pallbearer", "pallbearer"]; // selection_cap 2 -> already reached
    const runId = await startRun(host, { seed: 1601, permanents: seeded });
    const { open } = await runLevel(host, runId, 1);
    for (const option of open.options) {
      if (!isKnownEnemy(option.id)) fail(`offered option ${JSON.stringify(option.id)} is not in the frozen registry`);
      const held = countIn(seeded, option.id);
      if (held >= capOf(option.id)) {
        fail(`${option.id} offered although ${held} permanents already reach its selection_cap (${capOf(option.id)})`);
      }
      if (option.kind !== "permanent") {
        fail(`option ${option.id} offered as ${JSON.stringify(option.kind)} — a normal-pool option must be permanent`);
      }
    }
    await awaitClose(host, runId, open.sessionId);
  } finally {
    closeAll([host, two, three]);
  }
}

async function checkMinLevelGate(base) {
  const [host, two, three] = await openRoom(base);
  try {
    const runId = await startRun(host, { seed: 1701 });
    const { open } = await runLevel(host, runId, 1);
    for (const option of open.options) {
      const min = minLevelOf(option.id);
      if (min === undefined) fail(`offered option ${JSON.stringify(option.id)} is not in the frozen registry`);
      if (min > 1) fail(`level 1 offered ${option.id} whose min_level is ${min} — nothing above the current level may be offered`);
    }
    await awaitClose(host, runId, open.sessionId);
  } finally {
    closeAll([host, two, three]);
  }
}

async function checkOverflowTemporary(base) {
  const [host, two, three] = await openRoom(base);
  try {
    const seeded = fullCapSeeding(); // every entry at its selection_cap -> empty pool
    const runId = await startRun(host, { seed: 1801, permanents: seeded });
    const first = await runLevel(host, runId, 1);
    if (!multisetEqual(first.spawn.permanents, seeded)) {
      fail(`level 1 spawn set permanents ${JSON.stringify(first.spawn.permanents)} do not match the ${seeded.length} seeded permanents`);
    }
    if (first.spawn.temporaries.length !== 0) {
      fail(`level 1 already reports temporaries ${JSON.stringify(first.spawn.temporaries)}`);
    }
    if (!Array.isArray(first.open.options) || first.open.options.length < 1) {
      fail("overflow session offered no options — the run must not skip the enemy slot");
    }
    for (const option of first.open.options) {
      if (!isKnownEnemy(option.id)) fail(`overflow option ${JSON.stringify(option.id)} is not a known enemy`);
      if (option.kind !== "temporary") {
        fail(`with every entry capped, option ${option.id} is offered as ${JSON.stringify(option.kind)} — overflow options must be temporary`);
      }
    }
    const winner = first.open.options[0].id;
    await voteAll([[host, winner], [two, winner], [three, winner]], runId, first.open.sessionId);
    const closed = await awaitClose(host, runId, first.open.sessionId);
    if (closed.temporary !== true) {
      fail(`vote_closed reports temporary = ${JSON.stringify(closed.temporary)} — an overflow winner must be temporary`);
    }
    const second = await runLevel(host, runId, 2);
    if (countIn(second.spawn.permanents, winner) !== countIn(seeded, winner)) {
      fail(`overflow winner ${winner} added a permanent copy (level 2 permanents ${JSON.stringify(second.spawn.permanents)}) — overflow must never add a permanent`);
    }
    if (!second.spawn.temporaries.includes(winner)) {
      fail(`overflow winner ${winner} missing from level 2's temporaries (${JSON.stringify(second.spawn.temporaries)})`);
    }
    await awaitClose(host, runId, second.open.sessionId);
    const third = await runLevel(host, runId, 3);
    if (third.spawn.temporaries.includes(winner)) {
      fail(`overflow winner ${winner} still present at level 3 — temporary copies are removed when their level ends`);
    }
    if (!multisetEqual(third.spawn.permanents, seeded)) {
      fail(`level 3 permanents drifted from the seeded set: ${JSON.stringify(third.spawn.permanents)}`);
    }
  } finally {
    closeAll([host, two, three]);
  }
}

async function checkRejoinSafety(base) {
  const [host, two, three] = await openRoom(base);
  const extras = [];
  try {
    const runId = await startRun(host, { seed: 1901 });
    const { open } = await runLevel(host, runId, 1);
    const preserved = open.options[0].id; // the rejoiner's pre-disconnect pick
    const majorityPick = open.options[1].id;
    two.send({ type: "vote", optionId: preserved });
    await waitFor(two, isMsg("vote_update", { runId, sessionId: open.sessionId, voter: two.clientId }), CONNECT_TIMEOUT_MS, "ack of the first vote");
    two.close();
    const rejoiner = await connectClient(base, "kit-1", two.token);
    extras.push(rejoiner);
    if (rejoiner.welcome.rejoin !== true) {
      fail(`server treated the reconnecting client as new (welcome.rejoin = ${JSON.stringify(rejoiner.welcome.rejoin)})`);
    }
    if (rejoiner.welcome.clientId !== two.clientId) {
      fail(`rejoined client kept identity ${JSON.stringify(two.clientId)} but was assigned ${JSON.stringify(rejoiner.welcome.clientId)}`);
    }
    if (rejoiner.welcome.host !== false) fail("rejoined client unexpectedly became the host");
    // host votes the other option; the third client votes the rejoiner's option. If the
    // pre-disconnect vote was kept: 2-1 for it. If it was forgotten: 1-1 tie that the host
    // tie-breaks toward the other option. The winner tells the two cases apart.
    await voteAll([[host, majorityPick], [three, preserved]], runId, open.sessionId);
    const closed = await awaitClose(host, runId, open.sessionId);
    if (closed.winnerId !== preserved) {
      fail(`a client that disconnects and rejoins keeps its vote: expected ${JSON.stringify(preserved)} (its vote + client 2 = 2-1), got ${JSON.stringify(closed.winnerId)}`);
    }
  } finally {
    closeAll([host, two, three, ...extras]);
  }
}

async function checkCountdownLateVote(base) {
  const [host, two, three] = await openRoom(base);
  try {
    const runId = await startRun(host, { seed: 2001 });
    host.send({ type: "start_level", level: 1 });
    const openRec = await waitFor(host, isMsg("vote_open", { runId, level: 1 }), SPAWN_OR_OPEN_TIMEOUT_MS, "vote_open for level 1");
    // nobody votes: the countdown must close the session on its own.
    const closed = await awaitClose(host, runId, openRec.msg.sessionId);
    if (closed.winnerId == null) fail("countdown closed the session without reporting a winner");
    for (const client of [host, two, three]) client.send({ type: "vote", optionId: openRec.msg.options[0].id });
    const rejected = await waitFor(
      [host, two, three],
      (r) => r.msg.type === "vote_rejected" || r.msg.type === "error",
      REJECT_TIMEOUT_MS,
      "rejection of a late vote after the countdown closed the session",
    );
    if (rejected.msg.type === "error" && typeof rejected.msg.reason !== "string") {
      fail("late vote produced an error message without a reason");
    }
  } finally {
    closeAll([host, two, three]);
  }
}

// ---------------------------------------------------------------------------
// Suite driver
// ---------------------------------------------------------------------------
async function runSuite(base, { log = console.log, only = null } = {}) {
  const results = [];
  for (const [name, check] of CHECKS) {
    if (only && !only.has(name)) continue;
    try {
      await check(base);
      results.push({ name, pass: true });
      log(`PASS - ${name}`);
    } catch (error) {
      const detail = error instanceof Error ? error.message : String(error);
      results.push({ name, pass: false, detail });
      log(`FAIL - ${name}: ${detail}`);
    }
    await sleep(150); // let the server reap the closed client sockets between checks
  }
  return results;
}

async function startStub(violation) {
  const child = spawn(process.execPath, [STUB_PATH, "0"], {
    stdio: ["ignore", "pipe", "inherit"],
    env: { ...process.env, VOTE_STUB_VIOLATION: violation },
  });
  const url = await new Promise((resolve, reject) => {
    let out = "";
    child.stdout?.on("data", (chunk) => {
      out += chunk;
      const match = out.match(/listening on (ws?:\/\/\S+)/);
      if (match) resolve(match[1]);
    });
    child.once("exit", (code) => reject(new Error(`stub exited early (code ${code})`)));
  });
  return { child, url };
}

async function runSelftest() {
  let ok = true;
  {
    const stub = await startStub("");
    let failing = [];
    try {
      failing = (await runSuite(stub.url)).filter((r) => !r.pass).map((r) => r.name);
    } catch (error) {
      failing = [error instanceof Error ? error.message : String(error)];
    } finally {
      stub.child.kill();
    }
    ok = ok && failing.length === 0;
    console.log(failing.length === 0
      ? "self-test ok: the faithful stub passes the full check suite"
      : `self-test FAIL: faithful stub must pass everything, failing: ${failing.join(", ")}`);
  }
  for (const [violation, check] of VIOLATIONS) {
    const stub = await startStub(violation);
    let failing = [];
    try {
      failing = (await runSuite(stub.url, { only: new Set([check]) })).filter((r) => !r.pass).map((r) => r.name);
    } catch (error) {
      failing = [error instanceof Error ? error.message : String(error)];
    } finally {
      stub.child.kill();
    }
    const caught = failing.includes(check);
    ok = ok && caught;
    console.log(caught
      ? `self-test ok: violation "${violation}" trips "${check}"`
      : `self-test FAIL: violation "${violation}" did not trip "${check}" (failing: ${failing.join(", ") || "nothing"})`);
  }
  console.log(ok ? "self-test: PASSED" : "self-test: FAILED");
  return ok ? 0 : 1;
}

const argValue = (name) => {
  const i = process.argv.indexOf(name);
  return i !== -1 && i + 1 < process.argv.length ? process.argv[i + 1] : null;
};

async function main() {
  if (process.argv.includes("--selftest")) return runSelftest();
  const raw = argValue("--url") ?? process.env.VOTING_URL ?? "";
  if (raw.trim() === "") {
    console.log('voting-conformance: skipped (no server to test; set VOTING_URL=<ws url> or pass --url <ws url>)');
    return 0;
  }
  let base;
  try {
    base = normalizeWsUrl(raw);
  } catch (error) {
    console.error(`voting-conformance: aborted - ${error instanceof Error ? error.message : String(error)}`);
    return 2;
  }
  const results = await runSuite(base);
  const failed = results.filter((r) => !r.pass);
  console.log(`voting-conformance: ${results.length - failed.length}/${results.length} checks passed against ${base}`);
  return failed.length === 0 ? 0 : 1;
}

process.exit(await main());
