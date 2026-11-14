#!/usr/bin/env node
// Black-box conformance kit for the frozen VOIDWRAP matchmaker HTTP contract. Runs against any base
// URL (MATCHMAKER_URL, or --url <base>); prints "PASS — <check>" / "FAIL — <check>" per check and
// exits 0 only on a full pass. Without a URL it is a no-op (exit 0, "skipped") — that is how
// `just verify` includes it without requiring a live matchmaker.
//   MATCHMAKER_URL=http://127.0.0.1:8047 node tools/matchmaker-conformance/run.mjs
//   node tools/matchmaker-conformance/run.mjs --url http://127.0.0.1:8047
//   node tools/matchmaker-conformance/run.mjs --selftest     # ~3 min; see README.md
// --selftest proves the kit has teeth: the suite must fully pass against the canonical stub
// (tools/matchmaker-stub/server.mjs, spawned on an ephemeral port), and against the stub with each
// single deliberate violation, where the violation's paired check must be among the failures.
import { spawn } from "node:child_process";
import { fileURLToPath } from "node:url";

const STUB_PATH = fileURLToPath(new URL("../matchmaker-stub/server.mjs", import.meta.url));

const VERSION_MAIN = "1.2.3";
const VERSION_TRAP = "1.2.3.1"; // same prefix as VERSION_MAIN — catches non-exact filters
const VERSION_UNKNOWN = "9.9.9";
// [MATCHMAKER_STUB_VIOLATION value, the check that must fail because of it]
const VIOLATIONS = [
  ["health_missing", "health"],
  ["secret_weak", "register-list"],
  ["address_rewrite", "register-list"],
  ["list_field_drop", "register-list"],
  ["version_prefix", "version-filter"],
  ["no_cors", "cors-header"],
  ["heartbeat_no_fields", "heartbeat-refresh"],
  ["secret_lenient", "wrong-secret"],
  ["delete_ghost", "delete-removes"],
  ["heartbeat_stale", "ttl-expiry"],
  ["no_expiry", "ttl-expiry"],
];

function fail(message) {
  throw new Error(message);
}
const sleep = (ms) => new Promise((resolve) => {
  setTimeout(resolve, ms);
});

async function call(base, method, path, body) {
  const res = await fetch(base + path, {
    method,
    headers: body === undefined ? undefined : { "content-type": "application/json" },
    body: body === undefined ? undefined : JSON.stringify(body),
  });
  const text = await res.text();
  let json = null;
  if (text !== "") {
    try {
      json = JSON.parse(text);
    } catch {
      json = null;
    }
  }
  return { status: res.status, headers: res.headers, json, text };
}
function expectStatus(res, want, what) {
  if (res.status !== want) fail(`${what} -> ${res.status}, want ${want}`);
}
function expectFields(where, item, want) {
  if (item === null || typeof item !== "object") fail(`${where}: no entry for ${JSON.stringify(want.name)}`);
  for (const [key, value] of Object.entries(want)) {
    if (item[key] !== value) fail(`${where}: ${want.name}.${key} = ${JSON.stringify(item[key])}, want ${JSON.stringify(value)}`);
  }
}
const entryNamed = (list, name) => (Array.isArray(list) ? list : []).find((item) => item?.name === name) ?? null;

async function runSuite(base, { ttlStage = true, log = console.log } = {}) {
  const results = [];
  const run = async (name, check) => {
    try {
      await check();
      results.push({ name, pass: true });
      log(`PASS — ${name}`);
    } catch (error) {
      const detail = error instanceof Error ? error.message : String(error);
      results.push({ name, pass: false, detail });
      log(`FAIL — ${name}: ${detail}`);
    }
  };
  const api = (method, path, body) => call(base, method, path, body);
  const namesForMainVersion = async () =>
    ((await api("GET", `/matches?version=${encodeURIComponent(VERSION_MAIN)}`)).json ?? [])
      .map((item) => item?.name);

  let alpha = null; // registration from register-list, reused by the later checks

  await run("health", async () => {
    const res = await api("GET", "/health");
    expectStatus(res, 200, "GET /health");
    if (res.text !== "") fail(`GET /health body ${JSON.stringify(res.text)}, want empty`);
  });

  await run("register-list", async () => {
    const aRes = await api("POST", "/matches", {
      name: "kit-alpha", version: VERSION_MAIN, port: 7777, players: 2, max_players: 4,
      level: "foundry", phase: "lobby", joinable: true, address: "203.0.113.9:1234",
    });
    expectStatus(aRes, 200, "POST /matches");
    alpha = aRes.json ?? {};
    if (typeof alpha.id !== "string" || alpha.id.length < 8) fail(`POST /matches id ${JSON.stringify(alpha.id)} unusable`);
    if (typeof alpha.secret !== "string" || !/^[A-Za-z0-9+/=_-]{32,}$/.test(alpha.secret)) {
      fail("POST /matches secret is not unguessable (want >= 32 chars of random material)");
    }
    if (alpha.address !== "203.0.113.9:1234") {
      fail(`POST /matches address ${JSON.stringify(alpha.address)}, want the supplied one verbatim`);
    }
    const bRes = await api("POST", "/matches", {
      name: "kit-bravo", version: VERSION_MAIN, port: 7788, players: 2, max_players: 8,
      level: "tidegrave", phase: "lobby", joinable: true, address: null,
    });
    expectStatus(bRes, 200, "POST /matches (no address supplied)");
    const bravoReg = bRes.json ?? {};
    if (typeof bravoReg.address !== "string" || bravoReg.address === "") {
      fail("POST /matches without address must fall back to the source IP, got none");
    }
    const list = (await api("GET", `/matches?version=${encodeURIComponent(VERSION_MAIN)}`)).json;
    expectFields("GET /matches", entryNamed(list, "kit-alpha"), {
      name: "kit-alpha", version: VERSION_MAIN, players: 2, max_players: 4,
      level: "foundry", phase: "lobby", joinable: true, address: "203.0.113.9:1234",
    });
    expectFields("GET /matches", entryNamed(list, "kit-bravo"), {
      name: "kit-bravo", version: VERSION_MAIN, players: 2, max_players: 8,
      level: "tidegrave", phase: "lobby", joinable: true, address: bravoReg.address,
    });
  });

  await run("version-filter", async () => {
    const trap = await api("POST", "/matches", {
      name: "kit-charlie", version: VERSION_TRAP, port: 7799, players: 1, max_players: 2,
      level: "ashfall", phase: "lobby", joinable: true, address: "198.51.100.3:4000",
    });
    expectStatus(trap, 200, "POST /matches");
    const main = (await api("GET", `/matches?version=${encodeURIComponent(VERSION_MAIN)}`)).json;
    if (entryNamed(main, "kit-charlie") !== null) {
      fail(`GET /matches?version=${VERSION_MAIN} also returned kit-charlie (version ${VERSION_TRAP}) — filter is not exact`);
    }
    const unknown = (await api("GET", `/matches?version=${encodeURIComponent(VERSION_UNKNOWN)}`)).json;
    if (!Array.isArray(unknown) || unknown.length !== 0) {
      fail(`GET /matches?version=${VERSION_UNKNOWN} -> ${JSON.stringify(unknown)}, want []`);
    }
  });

  await run("cors-header", async () => {
    const res = await api("GET", `/matches?version=${encodeURIComponent(VERSION_MAIN)}`);
    expectStatus(res, 200, "GET /matches");
    const origin = res.headers.get("access-control-allow-origin");
    if (origin !== "*") fail(`GET /matches carried Access-Control-Allow-Origin ${JSON.stringify(origin)}, want "*"`);
  });

  await run("heartbeat-refresh", async () => {
    const beat = await api("POST", `/matches/${alpha?.id}`, {
      secret: alpha?.secret, players: 3, level: "slagworks", phase: "in-run", joinable: false,
    });
    expectStatus(beat, 200, "POST /matches/{id} heartbeat");
    const list = (await api("GET", `/matches?version=${encodeURIComponent(VERSION_MAIN)}`)).json;
    expectFields("GET /matches", entryNamed(list, "kit-alpha"), {
      name: "kit-alpha", version: VERSION_MAIN, players: 3, level: "slagworks",
      phase: "in-run", joinable: false, address: "203.0.113.9:1234",
    });
    const bravo = entryNamed(list, "kit-bravo");
    if (bravo?.players !== 2 || bravo?.level !== "tidegrave") {
      fail("GET /matches: heartbeating kit-alpha disturbed kit-bravo's entry");
    }
  });

  await run("wrong-secret", async () => {
    const before = entryNamed((await api("GET", `/matches?version=${encodeURIComponent(VERSION_MAIN)}`)).json, "kit-alpha");
    const bad = "0".repeat(64);
    expectStatus(await api("POST", `/matches/${alpha?.id}`, {
      secret: bad, players: 99, level: "wrong", phase: "wrong", joinable: true,
    }), 403, "POST /matches/{id} with wrong secret");
    expectStatus(await api("DELETE", `/matches/${alpha?.id}`, { secret: bad }), 403, "DELETE /matches/{id} with wrong secret");
    const after = entryNamed((await api("GET", `/matches?version=${encodeURIComponent(VERSION_MAIN)}`)).json, "kit-alpha");
    expectFields("GET /matches (after rejected calls)", after, {
      players: before?.players, level: before?.level, phase: before?.phase, joinable: before?.joinable,
    });
  });

  await run("delete-removes", async () => {
    const fRes = await api("POST", "/matches", {
      name: "kit-delta", version: VERSION_MAIN, port: 7800, players: 1, max_players: 2,
      level: "vault", phase: "lobby", joinable: true, address: "198.51.100.4:4001",
    });
    expectStatus(fRes, 200, "POST /matches");
    const delta = fRes.json ?? {};
    expectStatus(await api("DELETE", `/matches/${delta.id}`, { secret: delta.secret }), 200, "DELETE /matches/{id}");
    const list = (await api("GET", `/matches?version=${encodeURIComponent(VERSION_MAIN)}`)).json;
    if (entryNamed(list, "kit-delta") !== null) fail("GET /matches: deleted entry is still listed");
    expectStatus(await api("POST", `/matches/${delta.id}`, {
      secret: delta.secret, players: 1, level: "vault", phase: "lobby", joinable: true,
    }), 404, "heartbeat of a deleted entry");
    expectStatus(await api("DELETE", `/matches/${delta.id}`, { secret: delta.secret }), 404, "second delete of the same entry");
    expectStatus(await api("DELETE", "/matches/does-not-exist", { secret: delta.secret }), 404, "delete of an unknown id");
  });

  if (ttlStage) {
    await run("ttl-expiry", async () => {
      const gRes = await api("POST", "/matches", {
        name: "kit-golf", version: VERSION_MAIN, port: 7811, players: 1, max_players: 2,
        level: "fluxbloom", phase: "lobby", joinable: true, address: "198.51.100.5:4002",
      });
      expectStatus(gRes, 200, "POST /matches");
      if (!(await namesForMainVersion()).includes("kit-golf")) fail("freshly registered entry is not listed");
      const until = Date.now() + 42_000;
      let midChecked = false;
      while (Date.now() < until) {
        await sleep(9_500);
        expectStatus(await api("POST", `/matches/${alpha?.id}`, {
          secret: alpha?.secret, players: 3, level: "slagworks", phase: "in-run", joinable: false,
        }), 200, "heartbeat on the kept-alive entry");
        const now = await namesForMainVersion();
        if (!midChecked && until - Date.now() < 24_000) {
          if (!now.includes("kit-golf")) fail("un-heartbeated entry vanished before the 30 s TTL (~20 s in)");
          midChecked = true;
        }
      }
      const finalNames = await namesForMainVersion();
      if (!finalNames.includes("kit-alpha")) fail("entry refreshed by heartbeats for ~45 s vanished — heartbeat does not push last_seen");
      if (finalNames.includes("kit-golf")) fail("un-heartbeated entry still listed after ~40 s — entries must expire 30 s after their last heartbeat");
      expectFields("final listing", entryNamed(finalNames.length ? [{ name: finalNames[0] }] : [], finalNames[0]), {});
      expect(finalNames, ["kit-alpha"], "final listing (expired and deleted entries must be gone)");
    });
  }

  return results;
}

function expect(actual, want, what) {
  if (JSON.stringify([...actual].sort()) !== JSON.stringify([...want].sort())) {
    fail(`${what} = ${JSON.stringify(actual)}, want exactly ${JSON.stringify(want)}`);
  }
}

const argValue = (name) => {
  const i = process.argv.indexOf(name);
  return i !== -1 && i + 1 < process.argv.length ? process.argv[i + 1] : null;
};

async function startStub(violation) {
  const child = spawn(process.execPath, [STUB_PATH, "0"], {
    stdio: ["ignore", "pipe", "inherit"],
    env: { ...process.env, MATCHMAKER_STUB_VIOLATION: violation },
  });
  const base = await new Promise((resolve, reject) => {
    let out = "";
    child.stdout?.on("data", (chunk) => {
      out += chunk;
      const match = out.match(/listening on (http\S+)/);
      if (match) resolve(match[1]);
    });
    child.once("exit", (code) => reject(new Error(`stub exited early (code ${code})`)));
    setTimeout(() => reject(new Error("stub did not report its port within 5 s")), 5_000).unref?.();
  });
  return { child, base };
}

async function runSelftest() {
  let ok = true;
  {
    const stub = await startStub("");
    let failing = [];
    try {
      failing = (await runSuite(stub.base, { ttlStage: true })).filter((r) => !r.pass).map((r) => r.name);
    } catch (error) {
      failing = [error instanceof Error ? error.message : String(error)];
    } finally {
      stub.child.kill();
    }
    ok = ok && failing.length === 0;
    console.log(failing.length === 0
      ? "self-test ok: canonical stub passes the full check suite"
      : `self-test FAIL: canonical stub must pass everything, failing: ${failing.join(", ")}`);
  }
  for (const [violation, check] of VIOLATIONS) {
    const stub = await startStub(violation);
    let failing = [];
    try {
      failing = (await runSuite(stub.base, { ttlStage: check === "ttl-expiry" })).filter((r) => !r.pass).map((r) => r.name);
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

async function main() {
  if (process.argv.includes("--selftest")) return runSelftest();
  const base = (argValue("--url") ?? process.env.MATCHMAKER_URL ?? "").replace(/\/+$/, "");
  if (base === "") {
    console.log("matchmaker-conformance: skipped (no matchmaker to test; set MATCHMAKER_URL=<base url> or pass --url <base url>)");
    return 0;
  }
  const results = await runSuite(base);
  const failed = results.filter((r) => !r.pass);
  console.log(`matchmaker-conformance: ${results.length - failed.length}/${results.length} checks passed against ${base}`);
  return failed.length === 0 ? 0 : 1;
}

process.exit(await main());
