#!/usr/bin/env node
// Builds a ready-to-play folder: dist/VOIDWRAP_RUN/{voidwrap-server(.exe), web/, PLAY launchers, HOW-TO-PLAY}.
// Used by the release workflow and by `npm start`.
//   node tools/package.mjs [--skip-build]
import { spawnSync } from "node:child_process";
import { chmodSync, cpSync, existsSync, mkdirSync, rmSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const win = process.platform === "win32";
const run = (cmd, args, cwd = root, env = {}) => {
  console.log(`package: ${cmd} ${args.join(" ")}`);
  const r = spawnSync(cmd, args, { cwd, stdio: "inherit", shell: win, env: { ...process.env, ...env } });
  if (r.status !== 0) process.exit(r.status ?? 1);
};

if (!process.argv.includes("--skip-build")) {
  run("node", ["tools/build-wasm.mjs"]);
  run("pnpm", ["--dir", "web", "build"], root, { NODE_ENV: "production" });
  run("cargo", ["build", "--release", "-p", "server"], join(root, "server"));
}

const out = join(root, "dist/VOIDWRAP_RUN");
rmSync(out, { recursive: true, force: true });
mkdirSync(out, { recursive: true });
const targetDir = process.env.CARGO_TARGET_DIR ?? join(root, "server/target");
const exe = win ? "voidwrap-server.exe" : "voidwrap-server";
cpSync(join(targetDir, "release", exe), join(out, exe));
cpSync(join(root, "web/out"), join(out, "web"), { recursive: true });
if (win) {
  cpSync(join(root, "release/PLAY.cmd"), join(out, "PLAY.cmd"));
  cpSync(join(root, "release/PLAY-WITH-FRIENDS.cmd"), join(out, "PLAY-WITH-FRIENDS.cmd"));
} else {
  cpSync(join(root, "release/play.sh"), join(out, "play.sh"));
  chmodSync(join(out, "play.sh"), 0o755);
  chmodSync(join(out, exe), 0o755);
}
for (const doc of ["HOW-TO-PLAY.md", "LICENSE"]) {
  const src = join(root, doc);
  if (existsSync(src)) cpSync(src, join(out, doc));
}
console.log(`package: ready -> ${out}`);
