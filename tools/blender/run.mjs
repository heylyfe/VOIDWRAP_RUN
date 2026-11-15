#!/usr/bin/env node
/**
 * T-157 — headless Blender wrapper.
 *
 *   node tools/blender/run.mjs scripts/peal.py
 *   node tools/blender/run.mjs scripts/coil.py --no-render
 *
 * Blender is NOT a CI dependency. The scripts here are the source of truth; the `.glb`
 * they export is committed alongside them as a build artifact, and the gates validate the
 * result rather than authoring it. Run this locally after changing a script.
 *
 * Exit codes: 0 ok, 1 the script failed, 2 wrapper/setup problem.
 */
import { spawnSync } from 'node:child_process';
import { existsSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = dirname(fileURLToPath(import.meta.url));
const repo = resolve(root, '..', '..');

/** Where Blender lives. `BLENDER_EXE` overrides; the default is the 5.3 install. */
const BLENDER_EXE =
  process.env.BLENDER_EXE || 'C:/Program Files/Blender Foundation/Blender 5.3/blender.exe';

const [script, ...extra] = process.argv.slice(2);
if (!script) {
  console.error('usage: node tools/blender/run.mjs scripts/<name>.py [extra args...]');
  process.exit(2);
}
const scriptPath = resolve(root, script);
if (!existsSync(BLENDER_EXE)) {
  console.error(`Blender not found at ${BLENDER_EXE} (set BLENDER_EXE to override)`);
  process.exit(2);
}
if (!existsSync(scriptPath)) {
  console.error(`script not found: ${scriptPath}`);
  process.exit(2);
}

const args = [
  '-b',
  '--factory-startup',
  // Without this Blender exits 0 even when the script raises, which turns a broken model
  // into a silent success.
  '--python-exit-code',
  '1',
  '-P',
  scriptPath,
  '--',
  repo,
  ...extra,
];

// stdin closed: Blender can hang on an open stdin when dispatched from a script.
const r = spawnSync(BLENDER_EXE, args, { cwd: root, stdio: ['ignore', 'inherit', 'inherit'] });
process.exit(r.status ?? 1);
