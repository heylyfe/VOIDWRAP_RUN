#!/usr/bin/env node
// The live half of the merge gate.
//
// `just verify` is hermetic: it runs the unit suites and the static checks, and it invokes
// the four black-box kits with no arguments, where each prints "skipped" and exits 0. That
// means a green `just verify` has never proved the game runs. This script closes that hole
// without making `just verify` depend on a live process: it builds, starts a server, runs
// the kits against it, and stops it.
//
//   node tools/verify-live.mjs                 # build, dump, serve, run every kit
//   node tools/verify-live.mjs --skip-build    # reuse the existing binaries
//   node tools/verify-live.mjs --seeds 100     # smaller dump (default 400 -> 6000 levels)
//   node tools/verify-live.mjs --port 7979     # serve on another port (default 7878)
//
// The matchmaker kit needs a deployed matchmaker, which this repo does not ship, so it runs
// only when MATCHMAKER_URL is set. Everything else always runs.
import { spawn, spawnSync } from 'node:child_process';
import { existsSync, mkdtempSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import net from 'node:net';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const win = process.platform === 'win32';
const argv = process.argv.slice(2);
const flag = (name) => argv.includes(name);
const value = (name, fallback) => {
  const i = argv.indexOf(name);
  return i >= 0 && i + 1 < argv.length ? argv[i + 1] : fallback;
};

const SEEDS = Number(value('--seeds', '400'));
const SKIP_BUILD = flag('--skip-build');
const PORT = Number(value('--port', '7878'));
const WS_URL = `ws://127.0.0.1:${PORT}/ws`;
const SERVER_BIN = join(root, 'server', 'target', 'release', win ? 'voidwrap-server.exe' : 'voidwrap-server');

const results = [];
let server = null;

function log(line) {
  process.stdout.write(line + '\n');
}

function run(label, cmd, args, opts = {}) {
  log(`\n=== ${label} ===`);
  const r = spawnSync(cmd, args, {
    cwd: opts.cwd ?? root,
    stdio: 'inherit',
    shell: win,
    env: { ...process.env, ...(opts.env ?? {}) },
  });
  const ok = r.status === 0;
  results.push({ label, ok, status: r.status });
  return ok;
}

function waitForPort(port, timeoutMs) {
  const deadline = Date.now() + timeoutMs;
  return new Promise((resolve, reject) => {
    const attempt = () => {
      const socket = net.connect({ port, host: '127.0.0.1' });
      socket.once('connect', () => {
        socket.destroy();
        resolve();
      });
      socket.once('error', () => {
        socket.destroy();
        if (Date.now() > deadline) {
          reject(new Error(`nothing listening on 127.0.0.1:${port} after ${timeoutMs}ms`));
          return;
        }
        setTimeout(attempt, 250);
      });
    };
    attempt();
  });
}

function portInUse(port) {
  return new Promise((resolve) => {
    const socket = net.connect({ port, host: '127.0.0.1' });
    socket.once('connect', () => {
      socket.destroy();
      resolve(true);
    });
    socket.once('error', () => {
      socket.destroy();
      resolve(false);
    });
  });
}

function stopServer() {
  if (!server) return;
  const child = server;
  server = null;
  if (win) {
    spawnSync('taskkill', ['/pid', String(child.pid), '/t', '/f'], { stdio: 'ignore' });
  } else {
    child.kill('SIGTERM');
  }
}

async function main() {
  if (!SKIP_BUILD) {
    if (!run('build wasm', 'node', ['tools/build-wasm.mjs'])) return finish();
    if (!run('build server', 'cargo', ['build', '--release', '-p', 'server'], { cwd: join(root, 'server') })) {
      return finish();
    }
  }
  if (!existsSync(SERVER_BIN)) {
    log(`verify-live: missing ${SERVER_BIN}; run without --skip-build`);
    results.push({ label: 'server binary', ok: false, status: null });
    return finish();
  }

  // The dump is the input for both data kits. It is generated from the current tree, so a
  // stale dump cannot mask a generation regression.
  const dumpDir = mkdtempSync(join(tmpdir(), 'voidwrap-verify-live-'));
  const dumpPath = join(dumpDir, 'dump.ndjson');
  log(`\n=== level dump (${SEEDS} seeds) ===`);
  const dump = spawnSync(
    'cargo',
    ['run', '--release', '-p', 'sim', '--example', 'pattern_dump', '--', String(SEEDS)],
    { cwd: join(root, 'server'), shell: win, encoding: 'buffer', maxBuffer: 1 << 30 },
  );
  if (dump.status !== 0) {
    log(`verify-live: pattern_dump failed (${dump.status})`);
    results.push({ label: 'level dump', ok: false, status: dump.status });
    return finish();
  }
  const { writeFileSync } = await import('node:fs');
  writeFileSync(dumpPath, dump.stdout);
  const lines = dump.stdout.toString('utf8').split('\n').filter((l) => l.trim()).length;
  log(`verify-live: ${lines} level records -> ${dumpPath}`);
  results.push({ label: 'level dump', ok: lines > 0, status: 0 });

  // Refuse to run against a server this script did not start. A desktop build or a
  // `just run` server already holding the port answers the kits with its own state -- a run
  // in progress, players already joined -- and every result would describe that server, not
  // this tree. That is the exact class of silent wrongness this script exists to remove, so
  // it is a hard failure rather than a warning.
  if (await portInUse(PORT)) {
    log(`\nverify-live: something is already listening on 127.0.0.1:${PORT}.`);
    log('verify-live: close it (the desktop app, or a `just run` server) or pass --port <free port>.');
    log('verify-live: refusing to test a server this script did not start.');
    results.push({ label: `port ${PORT} free`, ok: false, status: null });
    return finish();
  }

  // A fresh server: a stale player from an earlier run leaves the run in a finished state
  // and the playthrough kit then sees no level progression at all.
  log('\n=== start server ===');
  // T-130: the death log makes the playthrough's death causes visible on every gate
  // run. Without it the kit can only say "the bots died"; with it, "8 deaths (void:8),
  // 8 ended a run" -- which is the difference between a bot bug and a level bug.
  const deathLog = join(tmpdir(), `voidwrap-deaths-${process.pid}.ndjson`);
  server = spawn(SERVER_BIN, [], {
    cwd: join(root, 'server'),
    stdio: ['ignore', 'pipe', 'pipe'],
    env: {
      ...process.env,
      VOIDWRAP_DEV: '',
      VOIDWRAP_DEV_CODE: '',
      VOIDWRAP_PORT: String(PORT),
      VOIDWRAP_DEATH_LOG: deathLog,
    },
  });
  let serverLog = '';
  server.stdout.on('data', (c) => {
    serverLog += c.toString('utf8');
  });
  server.stderr.on('data', (c) => {
    serverLog += c.toString('utf8');
  });
  try {
    await waitForPort(PORT, 30000);
  } catch (err) {
    log(`verify-live: ${err.message}`);
    log(serverLog.slice(-2000));
    results.push({ label: 'server start', ok: false, status: null });
    return finish();
  }
  if (server.exitCode !== null) {
    log(`verify-live: the server exited with code ${server.exitCode} right after starting`);
    log(serverLog.slice(-2000));
    results.push({ label: 'server start', ok: false, status: server.exitCode });
    return finish();
  }
  log(`verify-live: server is listening on 127.0.0.1:${PORT}`);

  run('pattern audit', 'node', ['tools/pattern-audit/run.mjs', '--from', dumpPath]);
  run('economy audit', 'node', ['tools/economy-audit/run.mjs', '--from', dumpPath]);

  // Playthrough runs BEFORE the voting kit: it opens 8 clients, and the server caps the
  // lobby, so a connection the voting kit has not finished tearing down makes the eighth
  // client fail with 503 "server full". That is a test-ordering artefact, not a game bug.
  //
  // `level-progression` is a named known failure (tickets/T-115.findings.md F-115-8: the
  // bot cannot navigate ramps). Naming it downgrades exactly that check to SKIP; any other
  // playthrough failure still fails the gate.
  run('playthrough', 'node', [
    'tools/playthrough/run.mjs',
    '--url', WS_URL,
    '--clients', '8',
    '--seconds', '60',
    '--known-failures', 'level-progression',
    '--death-log', deathLog,
  ]);
  run('voting conformance (real)', 'node', ['tools/voting-conformance/run.mjs', '--real', '--url', WS_URL]);

  const matchmaker = process.env.MATCHMAKER_URL;
  if (matchmaker) {
    run('matchmaker conformance', 'node', ['tools/matchmaker-conformance/run.mjs', '--url', matchmaker]);
  } else {
    log('\n=== matchmaker conformance ===\nskipped: set MATCHMAKER_URL to a deployed matchmaker to run it');
  }

  return finish();
}

function finish() {
  stopServer();
  const failed = results.filter((r) => !r.ok);
  log('\n=== verify-live summary ===');
  for (const r of results) {
    log(`  ${r.ok ? 'PASS' : 'FAIL'}  ${r.label}`);
  }
  log(failed.length === 0 ? '\nverify-live: PASS' : `\nverify-live: FAIL (${failed.length} step(s))`);
  process.exitCode = failed.length === 0 ? 0 : 1;
}

process.on('SIGINT', () => {
  stopServer();
  process.exit(130);
});

main().catch((err) => {
  log(`verify-live: ${err && err.stack ? err.stack : String(err)}`);
  stopServer();
  process.exitCode = 1;
});
