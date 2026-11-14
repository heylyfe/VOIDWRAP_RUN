#!/usr/bin/env node
// Canonical dev matchmaker for the frozen VOIDWRAP matchmaker HTTP contract. Plain Node built-ins, no
// dependencies; shared black-box dev target for the conformance kit (tools/matchmaker-conformance).
//   node tools/matchmaker-stub/server.mjs [port=8047]
// The default implementation is faithful to the contract. Setting MATCHMAKER_STUB_VIOLATION=<name>
// deliberately breaks exactly ONE behavior — the conformance kit's --selftest spawns it that way to
// prove each of its checks can actually fail. Names (see the kit's --selftest for the pairing):
// health_missing, secret_weak, address_rewrite, list_field_drop, version_prefix, no_cors,
// heartbeat_no_fields, secret_lenient, delete_ghost, heartbeat_stale, no_expiry.
//
// Contract recap: GET /health → 200 empty body. POST /matches → 200 {id, secret, address}; address is
// the supplied one verbatim, else the request source IP (trust X-Forwarded-For, fall back to the
// socket peer); id/secret random, secret unguessable; level/phase are opaque — store and echo.
// POST /matches/{id} body {secret, players, level, phase, joinable} → 200; 404 expired/unknown;
// 403 wrong secret. DELETE /matches/{id} body {secret} → 200 / 403 / 404. GET /matches?version=<str>
// → 200 array of {name, address, players, max_players, level, phase, joinable, version}, exact
// version filter, only entries with last_seen newer than 30 s, Access-Control-Allow-Origin: *.
// Entries expire 30 s after their last heartbeat; sweep every 10 s. The secret is the only authority.
import { createServer } from "node:http";
import { randomBytes } from "node:crypto";

const portArg = Number(process.argv[2]);
const PORT = process.argv[2] !== undefined && Number.isInteger(portArg) ? portArg : 8047;
const HOST = process.env.MATCHMAKER_STUB_HOST ?? "127.0.0.1";
const VIOLATION = process.env.MATCHMAKER_STUB_VIOLATION ?? "";
const TTL_MS = 30_000;
const SWEEP_MS = 10_000;

const entries = new Map(); // id -> { id, secret, last_seen, name, version, port, players, max_players, level, phase, joinable, address }

// "Expired" per the contract; the no_expiry violation pretends nothing ever ages out.
const isExpired = (entry) => VIOLATION !== "no_expiry" && Date.now() - entry.last_seen > TTL_MS;

setInterval(() => {
  if (VIOLATION === "no_expiry") return;
  for (const [id, entry] of entries) if (Date.now() - entry.last_seen > TTL_MS) entries.delete(id);
}, SWEEP_MS);

function readBody(req) {
  return new Promise((resolve) => {
    const chunks = [];
    req.on("data", (chunk) => chunks.push(chunk));
    req.on("end", () => resolve(Buffer.concat(chunks).toString("utf8")));
    req.on("error", () => resolve(""));
  });
}
async function readJson(req) {
  const text = await readBody(req);
  if (text === "") return {};
  try {
    return JSON.parse(text);
  } catch {
    return null;
  }
}
function send(res, code, body) {
  res.writeHead(code, { "content-type": "application/json" });
  res.end(JSON.stringify(body));
}
function sendList(res, items) {
  const headers = { "content-type": "application/json" };
  if (VIOLATION !== "no_cors") headers["access-control-allow-origin"] = "*";
  res.writeHead(200, headers);
  res.end(JSON.stringify(items));
}
function sourceAddress(req) {
  const forwarded = req.headers["x-forwarded-for"];
  if (typeof forwarded === "string" && forwarded.trim() !== "") return forwarded.split(",")[0].trim();
  return req.socket.remoteAddress ?? "";
}
const REGISTER_TYPES = {
  name: "string", version: "string", port: "number", players: "number",
  max_players: "number", level: "string", phase: "string", joinable: "boolean",
};
function validRegister(body) {
  if (body === null || typeof body !== "object") return false;
  for (const [key, type] of Object.entries(REGISTER_TYPES)) if (typeof body[key] !== type) return false;
  return body.address === null || body.address === undefined || typeof body.address === "string";
}

const server = createServer(async (req, res) => {
  const url = new URL(req.url ?? "/", "http://localhost");
  const path = url.pathname;
  try {
    if (req.method === "GET" && path === "/health") {
      if (VIOLATION === "health_missing") {
        res.writeHead(404);
        res.end();
        return;
      }
      res.writeHead(200);
      res.end(); // contract: 200, empty body
      return;
    }

    if (req.method === "POST" && path === "/matches") {
      const body = await readJson(req);
      if (!validRegister(body)) {
        send(res, 400, { error: "bad register body" });
        return;
      }
      const id = VIOLATION === "secret_weak" ? `m${entries.size + 1}` : randomBytes(8).toString("hex");
      const secret = VIOLATION === "secret_weak" ? "weak" : randomBytes(32).toString("hex");
      const supplied = typeof body.address === "string" && body.address !== "" ? body.address : null;
      const address = supplied !== null && VIOLATION === "address_rewrite"
        ? `${supplied} (rewritten)`
        : supplied ?? sourceAddress(req);
      entries.set(id, {
        id, secret, address, last_seen: Date.now(), name: body.name, version: body.version,
        port: body.port, players: body.players, max_players: body.max_players,
        level: body.level, phase: body.phase, joinable: body.joinable,
      });
      send(res, 200, { id, secret, address });
      return;
    }

    if (req.method === "GET" && path === "/matches") {
      const want = url.searchParams.get("version");
      const items = [];
      for (const entry of entries.values()) {
        if (isExpired(entry)) continue;
        if (want !== null) {
          const hit = VIOLATION === "version_prefix"
            ? String(entry.version).startsWith(want)
            : entry.version === want;
          if (!hit) continue;
        }
        const item = {
          name: entry.name, address: entry.address, players: entry.players,
          max_players: entry.max_players, level: entry.level, phase: entry.phase,
          joinable: entry.joinable, version: entry.version,
        };
        if (VIOLATION === "list_field_drop") delete item.max_players;
        items.push(item);
      }
      sendList(res, items);
      return;
    }

    if (path.startsWith("/matches/") && (req.method === "POST" || req.method === "DELETE")) {
      const id = decodeURIComponent(path.slice("/matches/".length));
      const body = await readJson(req);
      const entry = entries.get(id);
      if (entry === undefined || isExpired(entry)) {
        send(res, 404, { error: "not found" });
        return;
      }
      if (VIOLATION !== "secret_lenient" && body?.secret !== entry.secret) {
        send(res, 403, { error: "wrong secret" });
        return;
      }
      if (req.method === "POST") {
        if (VIOLATION !== "heartbeat_no_fields") {
          entry.players = body?.players ?? entry.players;
          entry.level = body?.level ?? entry.level;
          entry.phase = body?.phase ?? entry.phase;
          entry.joinable = body?.joinable ?? entry.joinable;
        }
        if (VIOLATION !== "heartbeat_stale") entry.last_seen = Date.now();
        send(res, 200, {});
        return;
      }
      if (VIOLATION !== "delete_ghost") entries.delete(id);
      send(res, 200, {});
      return;
    }

    send(res, 404, { error: "no route" });
  } catch {
    send(res, 500, { error: "internal error" });
  }
});

server.listen(PORT, HOST, () => {
  const shown = HOST === "0.0.0.0" || HOST === "::" ? "127.0.0.1" : HOST;
  const suffix = VIOLATION === "" ? "" : ` (violation: ${VIOLATION})`;
  console.log(`matchmaker-stub: listening on http://${shown}:${server.address().port}${suffix}`);
});
