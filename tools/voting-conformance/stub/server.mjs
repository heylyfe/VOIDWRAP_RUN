#!/usr/bin/env node
// Faithful stub of the end-of-level voting system, used by the conformance kit's --selftest.
// It implements design/ENEMY_REGISTRY.md §9 (pool rules) against the same wire contract the kit
// speaks (see tools/voting-conformance/README.md) — plus one deliberately broken behaviour at a
// time, selected with VOTE_STUB_VIOLATION:
//
//   ""                    faithful
//   majority_inverted     the trailing option wins instead of the majority option
//   tie_ignored           a tied vote ignores the host's vote
//   cap_ignored           capped entries stay in the pool
//   permanence_dropped    winners are never added as permanents (or as pending temporaries)
//   rejoin_forgotten      a reconnecting client is treated as brand new
//   late_vote_accepted    votes cast after the countdown are accepted
//
// Usage: node stub/server.mjs [port]   (port 0 or omitted -> ephemeral)
// Prints "listening on ws://127.0.0.1:<port>" once ready.
import http from "node:http";
import crypto from "node:crypto";
import { ENEMIES, ENEMY_BY_ID, POOL_OFFER } from "../registry.mjs";

const WS_GUID = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11";
const VIOLATION = process.env.VOTE_STUB_VIOLATION ?? "";
const COLLAPSE_MS = Number(process.env.VOTE_STUB_COLLAPSE_MS ?? 250); // level start -> vote opens
const COUNTDOWN_MS = Number(process.env.VOTE_STUB_COUNTDOWN_MS ?? 900); // vote opens -> closes

// ---------------------------------------------------------------------------
// Rooms. A room is keyed by the ?room= query parameter (empty by default). The
// first (earliest-joined) connected client is the host; the role passes to the
// next earliest client when the host disconnects.
// ---------------------------------------------------------------------------
const rooms = new Map();

function roomOf(key) {
  let r = rooms.get(key);
  if (!r) {
    r = {
      key,
      clients: [], // ordered by join order; clients[0] is the host
      detached: new Map(), // token -> { clientId, name, joinSeq, pendingVote }
      nextClientId: 0,
      nextJoinSeq: 0,
      nextSessionId: 0,
      nextRunId: 0,
      run: null,
    };
    rooms.set(key, r);
  }
  return r;
}

function insertClient(r, conn) {
  const idx = r.clients.findIndex((c) => c.joinSeq > conn.joinSeq);
  if (idx === -1) r.clients.push(conn);
  else r.clients.splice(idx, 0, conn);
}

// ---------------------------------------------------------------------------
// Frames (server side: client->server frames are masked, server->client are not)
// ---------------------------------------------------------------------------
function decodeFrames(buffer) {
  const frames = [];
  let off = 0;
  for (;;) {
    if (buffer.length - off < 2) break;
    const opcode = buffer[off] & 0x0f;
    const masked = (buffer[off + 1] & 0x80) !== 0;
    let len = buffer[off + 1] & 0x7f;
    let pos = off + 2;
    if (len === 126) {
      if (buffer.length - pos < 2) break;
      len = buffer.readUInt16BE(pos);
      pos += 2;
    } else if (len === 127) {
      if (buffer.length - pos < 8) break;
      const big = buffer.readBigUInt64BE(pos);
      pos += 8;
      if (big > 0x4000000n) return { frames, rest: null, oversized: true };
      len = Number(big);
    }
    let mask = null;
    if (masked) {
      if (buffer.length - pos < 4) break;
      mask = buffer.subarray(pos, pos + 4);
      pos += 4;
    }
    if (buffer.length - pos < len) break;
    let payload = buffer.subarray(pos, pos + len);
    if (mask) {
      payload = Buffer.from(payload);
      for (let i = 0; i < payload.length; i++) payload[i] ^= mask[i & 3];
    }
    frames.push({ opcode, payload });
    off = pos + len;
  }
  return { frames, rest: buffer.subarray(off), oversized: false };
}

function writeFrame(conn, opcode, payload) {
  if (!conn.socket || conn.socket.destroyed) return;
  let header;
  if (payload.length < 126) {
    header = Buffer.from([0x80 | opcode, payload.length]);
  } else if (payload.length < 65536) {
    header = Buffer.alloc(4);
    header[0] = 0x80 | opcode;
    header[1] = 126;
    header.writeUInt16BE(payload.length, 2);
  } else {
    header = Buffer.alloc(10);
    header[0] = 0x80 | opcode;
    header[1] = 127;
    header.writeBigUInt64BE(BigInt(payload.length), 2);
  }
  try {
    conn.socket.write(Buffer.concat([header, payload]));
  } catch {
    // client went away; the close handler cleans up
  }
}

const send = (conn, message) => writeFrame(conn, 0x1, Buffer.from(JSON.stringify(message), "utf8"));
const broadcast = (r, message) => {
  for (const c of [...r.clients]) send(c, message);
};

// ---------------------------------------------------------------------------
// Pool construction — ENEMY_REGISTRY.md §9
// ---------------------------------------------------------------------------
function hooksSatisfied(entry, run) {
  return (entry.hooks ?? []).every((hook) => {
    if (!hook.requires) return true;
    let have = 0;
    for (const other of ENEMIES) {
      if (other.tags.includes(hook.requires.tag)) have += run.permanents.get(other.id) ?? 0;
    }
    return have >= hook.requires.count;
  });
}

function poolFor(run, level) {
  return ENEMIES
    .filter((e) => e.min_level <= level)
    .filter((e) => VIOLATION === "cap_ignored" || (run.permanents.get(e.id) ?? 0) < e.cap)
    .filter((e) => hooksSatisfied(e, run))
    .filter((e) => e.class_interactions?.[run.class] !== "ignores");
}

// §9 "Empty pool — overflow": temporary drafts, never offered to count against caps.
function overflowOptions(run, level) {
  const holders = ENEMIES
    .filter((e) => (run.permanents.get(e.id) ?? 0) > 0 && e.min_level <= level)
    .sort((a, b) => (b.min_level - a.min_level) || (ENEMIES.indexOf(a) - ENEMIES.indexOf(b)));
  const picks = holders.slice(0, POOL_OFFER).map((e) => ({ id: e.id, name: e.name, kind: "temporary" }));
  if (picks.length > 0) return picks;
  // §9 step 2: no permanents yet (level 1) -> lowest-min_level entry not excluded by class.
  const fallback = ENEMIES
    .filter((e) => e.class_interactions?.[run.class] !== "ignores")
    .sort((a, b) => (a.min_level - b.min_level) || (ENEMIES.indexOf(a) - ENEMIES.indexOf(b)))[0];
  return fallback ? [{ id: fallback.id, name: fallback.name, kind: "temporary" }] : [];
}

function expandPermanents(run) {
  const out = [];
  for (const entry of ENEMIES) {
    const n = run.permanents.get(entry.id) ?? 0;
    for (let i = 0; i < n; i++) out.push(entry.id);
  }
  return out;
}

function tallyOut(session) {
  const out = {};
  for (const option of session.options) out[option.id] = session.tally.get(option.id) ?? 0;
  return out;
}

// ---------------------------------------------------------------------------
// Vote session lifecycle
// ---------------------------------------------------------------------------
function openSession(r, run, level) {
  run.sessionTimer = null;
  const pool = poolFor(run, level);
  const options = pool.length === 0
    ? overflowOptions(run, level)
    : pool.slice(0, POOL_OFFER).map((e) => ({ id: e.id, name: e.name, kind: "permanent" }));
  const session = {
    id: ++r.nextSessionId,
    level,
    options,
    open: true,
    tally: new Map(),
    votes: new Map(), // clientId -> optionId
    closeTimer: null,
  };
  run.session = session;
  broadcast(r, { type: "vote_open", runId: run.id, sessionId: session.id, level, options, countdown_ms: COUNTDOWN_MS });
  session.closeTimer = setTimeout(() => closeSession(r, run, session), COUNTDOWN_MS);
}

function closeSession(r, run, session) {
  session.closeTimer = null;
  const entries = session.options.map((o) => ({ id: o.id, count: session.tally.get(o.id) ?? 0 }));
  const max = Math.max(...entries.map((e) => e.count));
  let leaders = entries.filter((e) => e.count === max);
  if (VIOLATION === "majority_inverted") {
    const min = Math.min(...entries.map((e) => e.count));
    leaders = entries.filter((e) => e.count === min);
  }
  const host = r.clients[0] ?? null;
  const hostOption = host ? session.votes.get(host.clientId) : undefined;
  let winner;
  if (VIOLATION !== "tie_ignored" && hostOption !== undefined && leaders.some((e) => e.id === hostOption)) {
    winner = hostOption; // a tie goes to the host's vote
  } else {
    winner = leaders.length > 0 ? leaders[leaders.length - 1].id : session.options[0]?.id ?? null;
  }
  session.open = false;
  const winnerOption = winner === null ? null : session.options.find((o) => o.id === winner) ?? null;
  const temporary = winnerOption ? winnerOption.kind === "temporary" : false;
  if (winnerOption) {
    if (temporary) {
      run.pendingTemp = { id: winner, level: session.level + 1 }; // next level only
    } else if (VIOLATION !== "permanence_dropped") {
      // §4: a grant that would exceed the cap grants nothing for that enemy.
      run.permanents.set(winner, Math.min((run.permanents.get(winner) ?? 0) + 1, winnerOption.cap ?? ENEMY_BY_ID.get(winner).cap));
    }
  }
  broadcast(r, {
    type: "vote_closed",
    runId: run.id,
    sessionId: session.id,
    level: session.level,
    winnerId: winner,
    winnerName: winnerOption ? winnerOption.name : null,
    temporary,
  });
}

function castVote(r, conn, session, optionId, { restored = false } = {}) {
  session.votes.set(conn.clientId, optionId);
  session.tally.set(optionId, (session.tally.get(optionId) ?? 0) + 1);
  conn.lastVoted = { sessionId: session.id, optionId };
  broadcast(r, {
    type: "vote_update",
    runId: r.run.id,
    sessionId: session.id,
    tally: tallyOut(session),
    voter: conn.clientId,
    ...(restored ? { restored } : {}),
  });
}

// ---------------------------------------------------------------------------
// Message handling
// ---------------------------------------------------------------------------
function handleMessage(roomKey, conn, msg) {
  const r = roomOf(roomKey);
  switch (msg?.type) {
    case "hello": return handleHello(r, conn, msg);
    case "start_run": return handleStartRun(r, conn, msg);
    case "start_level": return handleStartLevel(r, conn, msg);
    case "vote": return handleVote(r, conn, msg);
    default: return send(conn, { type: "error", reason: `unknown message type ${JSON.stringify(msg?.type)}` });
  }
}

function handleHello(r, conn, msg) {
  const prior = typeof msg.token === "string" && VIOLATION !== "rejoin_forgotten"
    ? r.detached.get(msg.token) ?? null
    : null;
  if (prior) {
    r.detached.delete(msg.token);
    conn.clientId = prior.clientId;
    conn.joinSeq = prior.joinSeq;
    conn.name = prior.name;
    conn.rejoin = true;
    insertClient(r, conn);
    const run = r.run;
    if (run && run.session && run.session.open && prior.pendingVote && prior.pendingVote.sessionId === run.session.id) {
      castVote(r, conn, run.session, prior.pendingVote.optionId, { restored: true });
    }
  } else {
    conn.clientId = ++r.nextClientId;
    conn.joinSeq = ++r.nextJoinSeq;
    conn.name = typeof msg.name === "string" && msg.name !== "" ? msg.name : `client-${conn.clientId}`;
    conn.rejoin = false;
    insertClient(r, conn);
  }
  conn.token = crypto.randomUUID();
  conn.lastVoted = null;
  send(conn, {
    type: "welcome",
    clientId: conn.clientId,
    token: conn.token,
    name: conn.name,
    host: r.clients[0] === conn,
    rejoin: conn.rejoin,
    runId: r.run ? r.run.id : null,
  });
}

function handleStartRun(r, conn, msg) {
  if (r.run?.sessionTimer) clearTimeout(r.run.sessionTimer);
  if (r.run?.session?.closeTimer) clearTimeout(r.run.session.closeTimer);
  if (r.run?.session) r.run.session.open = false;
  if (!Number.isFinite(Number(msg.seed))) {
    return send(conn, { type: "error", reason: `bad seed ${JSON.stringify(msg.seed)}` });
  }
  const run = {
    id: ++r.nextRunId,
    seed: Number(msg.seed),
    class: typeof msg.class === "string" && msg.class !== "" ? msg.class : "striker",
    permanents: new Map(),
    level: 0,
    session: null,
    sessionTimer: null,
    pendingTemp: null,
  };
  if (Array.isArray(msg.permanents)) {
    for (const raw of msg.permanents) {
      const entry = ENEMY_BY_ID.get(raw);
      if (!entry) return send(conn, { type: "error", reason: `unknown enemy id ${JSON.stringify(raw)}` });
      run.permanents.set(entry.id, (run.permanents.get(entry.id) ?? 0) + 1);
    }
    for (const [id, n] of run.permanents) {
      if (n > ENEMY_BY_ID.get(id).cap) {
        return send(conn, { type: "error", reason: `permanents seeding exceeds selection_cap for ${id} (${n} > ${ENEMY_BY_ID.get(id).cap})` });
      }
    }
  }
  r.run = run;
  broadcast(r, { type: "run_started", runId: run.id, seed: run.seed, class: run.class });
}

function handleStartLevel(r, conn, msg) {
  const run = r.run;
  if (!run) return send(conn, { type: "error", reason: "start_level before start_run" });
  const level = Math.floor(Number(msg.level));
  if (!Number.isFinite(level) || level < 1) {
    return send(conn, { type: "error", reason: `bad level ${JSON.stringify(msg.level)}` });
  }
  if (run.session?.closeTimer) clearTimeout(run.session.closeTimer);
  if (run.session) run.session.open = false;
  if (run.sessionTimer) clearTimeout(run.sessionTimer);
  run.level = level;
  if (run.pendingTemp && run.pendingTemp.level < level) run.pendingTemp = null; // its level ended
  const temporaries = run.pendingTemp && run.pendingTemp.level === level ? [run.pendingTemp.id] : [];
  const permanents = expandPermanents(run);
  broadcast(r, {
    type: "spawn_set",
    runId: run.id,
    level,
    permanents,
    temporaries,
    enemies: [...permanents, ...temporaries],
  });
  run.session = null;
  run.sessionTimer = setTimeout(() => openSession(r, run, level), COLLAPSE_MS);
}

function handleVote(r, conn, msg) {
  const run = r.run;
  const session = run ? run.session : null;
  const optionId = typeof msg.optionId === "string" ? msg.optionId : null;
  if (!session || !session.open) {
    if (VIOLATION === "late_vote_accepted" && session && !session.open
      && optionId && session.options.some((o) => o.id === optionId) && !session.votes.has(conn.clientId)) {
      session.votes.set(conn.clientId, optionId);
      session.tally.set(optionId, (session.tally.get(optionId) ?? 0) + 1);
      return broadcast(r, {
        type: "vote_update",
        runId: run.id,
        sessionId: session.id,
        tally: tallyOut(session),
        voter: conn.clientId,
        late: true,
      });
    }
    return send(conn, {
      type: "vote_rejected",
      reason: session ? "the vote session is closed" : "no vote session is open",
      runId: run ? run.id : null,
    });
  }
  if (!optionId || !session.options.some((o) => o.id === optionId)) {
    return send(conn, { type: "vote_rejected", reason: `unknown option ${JSON.stringify(msg.optionId)}`, runId: run.id });
  }
  if (session.votes.has(conn.clientId)) {
    return send(conn, { type: "vote_rejected", reason: "already voted in this session", runId: run.id });
  }
  castVote(r, conn, session, optionId);
}

function detach(roomKey, conn) {
  const r = roomOf(roomKey);
  const idx = r.clients.indexOf(conn);
  if (idx === -1) return;
  r.clients.splice(idx, 1);
  if (conn.token) {
    r.detached.set(conn.token, {
      clientId: conn.clientId,
      name: conn.name,
      joinSeq: conn.joinSeq,
      pendingVote: conn.lastVoted,
    });
  }
  if (r.clients.length === 0 && !r.run) rooms.delete(roomKey);
}

// ---------------------------------------------------------------------------
// Server
// ---------------------------------------------------------------------------
const server = http.createServer((req, res) => {
  res.writeHead(404, { "content-type": "text/plain" });
  res.end("voting-conformance stub speaks WebSocket only\n");
});

server.on("upgrade", (req, socket) => {
  const key = req.headers["sec-websocket-key"];
  if (!key) {
    socket.destroy();
    return;
  }
  const accept = crypto.createHash("sha1").update(key + WS_GUID).digest("base64");
  socket.write(
    "HTTP/1.1 101 Switching Protocols\r\n"
    + "Upgrade: websocket\r\n"
    + "Connection: Upgrade\r\n"
    + `Sec-WebSocket-Accept: ${accept}\r\n`
    + "\r\n",
  );
  socket.setNoDelay(true);
  const url = new URL(req.url ?? "/", "http://localhost");
  const roomKey = url.searchParams.get("room") ?? "";
  const conn = {
    socket,
    clientId: 0,
    name: "",
    token: null,
    joinSeq: 0,
    rejoin: false,
    lastVoted: null,
  };
  let buffer = Buffer.alloc(0);
  socket.on("data", (chunk) => {
    buffer = Buffer.concat([buffer, chunk]);
    const { frames, rest, oversized } = decodeFrames(buffer);
    if (oversized) {
      socket.destroy();
      return;
    }
    buffer = rest;
    for (const frame of frames) {
      if (frame.opcode === 0x1) {
        let msg = null;
        try {
          msg = JSON.parse(frame.payload.toString("utf8"));
        } catch {
          continue;
        }
        handleMessage(roomKey, conn, msg);
      } else if (frame.opcode === 0x8) {
        detach(roomKey, conn);
        socket.end();
      } else if (frame.opcode === 0x9) {
        writeFrame(conn, 0xA, Buffer.from(frame.payload)); // pong
      }
    }
  });
  const gone = () => detach(roomKey, conn);
  socket.on("close", gone);
  socket.on("error", gone);
});

const portArg = process.argv[2];
const port = portArg === undefined || portArg === "0" ? 0 : Number(portArg);
server.on("error", (error) => {
  console.error(String(error));
  process.exit(1);
});
server.listen(Number.isFinite(port) && port >= 0 ? port : 0, "127.0.0.1", () => {
  console.log(`listening on ws://127.0.0.1:${server.address().port}`);
});
