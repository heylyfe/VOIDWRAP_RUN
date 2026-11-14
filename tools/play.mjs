#!/usr/bin/env node
// `npm start`: build everything once, then launch the game and open the browser. No extra tools needed
// beyond the prerequisites in HOW-TO-PLAY.md (Rust, Node.js, pnpm).
import { spawn, spawnSync } from "node:child_process";
import { existsSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const r = spawnSync("node", ["tools/package.mjs"], { cwd: root, stdio: "inherit" });
if (r.status !== 0) process.exit(r.status ?? 1);
const dist = join(root, "dist/VOIDWRAP_RUN");
const desktopExe = join(dist, "VOIDWRAP RUN.exe");
const exe = existsSync(desktopExe)
  ? desktopExe
  : join(dist, process.platform === "win32" ? "voidwrap-server.exe" : "voidwrap-server");
const friends = process.argv.includes("--friends");
const child = spawn(exe, [], {
  cwd: dist,
  stdio: "inherit",
  env: { ...process.env, VOIDWRAP_OPEN_BROWSER: "1", ...(friends ? { VOIDWRAP_HOST: "0.0.0.0" } : {}) },
});
child.on("exit", (code) => process.exit(code ?? 0));
process.on("SIGINT", () => child.kill("SIGINT"));
