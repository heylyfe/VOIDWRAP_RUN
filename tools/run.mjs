#!/usr/bin/env node
// `just run`: starts the game server and the web client together; Ctrl+C stops both.
import { spawn } from "node:child_process";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const procs = [
  { name: "server", color: 35, cmd: "cargo", args: ["run", "--release", "-p", "server"], cwd: join(root, "server") },
  { name: "web", color: 36, cmd: "pnpm", args: ["--dir", "web", "dev"], cwd: root },
];
const children = procs.map((p) => {
  const child = spawn(p.cmd, p.args, { cwd: p.cwd, shell: process.platform === "win32", stdio: ["ignore", "pipe", "pipe"] });
  const prefix = `\x1b[${p.color}m[${p.name}]\x1b[0m `;
  const pipe = (stream, out) => {
    let buf = "";
    stream.on("data", (d) => {
      buf += d;
      const lines = buf.split(/\r?\n/);
      buf = lines.pop() ?? "";
      for (const l of lines) out.write(prefix + l + "\n");
    });
  };
  pipe(child.stdout, process.stdout);
  pipe(child.stderr, process.stderr);
  child.on("exit", (code) => {
    console.log(`${prefix}exited with ${code}; stopping everything`);
    shutdown(code ?? 1);
  });
  return child;
});
console.log("VOIDWRAP RUN: open http://localhost:3000 once both are up (Ctrl+C to stop)");
let stopping = false;
function shutdown(code) {
  if (stopping) return;
  stopping = true;
  for (const c of children) {
    if (c.exitCode !== null) continue;
    if (process.platform === "win32") spawn("taskkill", ["/pid", String(c.pid), "/t", "/f"], { stdio: "ignore" });
    else c.kill("SIGINT");
  }
  setTimeout(() => process.exit(code), 500);
}
process.on("SIGINT", () => shutdown(0));
process.on("SIGTERM", () => shutdown(0));
