#!/usr/bin/env node
// Builds the shared movement controller to WASM and copies it to web/public/sim.wasm.
import { spawnSync } from "node:child_process";
import { copyFileSync, mkdirSync, statSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const r = spawnSync("cargo", ["build", "-p", "sim-wasm", "--target", "wasm32-unknown-unknown", "--release"], {
  cwd: join(root, "server"),
  stdio: "inherit",
});
if (r.status !== 0) process.exit(r.status ?? 1);
const targetDir = process.env.CARGO_TARGET_DIR ?? join(root, "server/target");
const src = join(targetDir, "wasm32-unknown-unknown/release/sim_wasm.wasm");
const dst = join(root, "web/public/sim.wasm");
mkdirSync(dirname(dst), { recursive: true });
copyFileSync(src, dst);
console.log(`build-wasm: ${dst} (${statSync(dst).size} bytes)`);
