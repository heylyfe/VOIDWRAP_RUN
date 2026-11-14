#!/usr/bin/env node
// Builds a ready-to-play folder: dist/VOIDWRAP_RUN/{voidwrap-server(.exe), web/, PLAY launchers, HOW-TO-PLAY}.
// On Windows (desktop build) it also builds dist/VOIDWRAP_RUN-<version>-windows-setup.exe when NSIS is available.
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

// Windows gets the desktop window (feature "desktop"); Linux packages stay browser-based for now
// (the webview there needs extra system libraries). VOIDWRAP_DESKTOP=0/1 overrides.
const desktop = (process.env.VOIDWRAP_DESKTOP ?? (win ? "1" : "0")) === "1";

if (!process.argv.includes("--skip-build")) {
  run("node", ["tools/build-wasm.mjs"]);
  run("pnpm", ["--dir", "web", "build"], root, { NODE_ENV: "production" });
  run("cargo", ["build", "--release", "-p", "server", ...(desktop ? ["--features", "desktop"] : [])], join(root, "server"));
}

const out = join(root, "dist/VOIDWRAP_RUN");
rmSync(out, { recursive: true, force: true });
mkdirSync(out, { recursive: true });
const targetDir = process.env.CARGO_TARGET_DIR ?? join(root, "server/target");
const exe = win ? "voidwrap-server.exe" : "voidwrap-server";
// The desktop build opens its own game window, so on Windows the program itself is what players double-click.
const shipped = win && desktop ? "VOIDWRAP RUN.exe" : exe;
cpSync(join(targetDir, "release", exe), join(out, shipped));
cpSync(join(root, "web/out"), join(out, "web"), { recursive: true });
if (win && desktop) {
  cpSync(join(root, "release/PLAY-IN-BROWSER.cmd"), join(out, "PLAY-IN-BROWSER.cmd"));
} else if (win) {
  cpSync(join(root, "release/PLAY.cmd"), join(out, "PLAY.cmd"));
  cpSync(join(root, "release/PLAY-WITH-FRIENDS.cmd"), join(out, "PLAY-WITH-FRIENDS.cmd"));
} else {
  cpSync(join(root, "release/play.sh"), join(out, "play.sh"));
  chmodSync(join(out, "play.sh"), 0o755);
  chmodSync(join(out, exe), 0o755);
}
for (const doc of ["HOW-TO-PLAY.md", "LICENSE", "ENEMIES.md"]) {
  const src = join(root, doc);
  if (existsSync(src)) cpSync(src, join(out, doc));
}

// On Windows with the desktop build, also produce a per-user installer when NSIS is available.
if (win && desktop) {
  const envVersion = process.env.VOIDWRAP_VERSION;
  const version = envVersion && envVersion.length > 0 ? envVersion : "dev";
  const nsisBundled = "C:\\Program Files (x86)\\NSIS\\makensis.exe";
  const where = spawnSync("where", ["makensis"], { encoding: "utf8" });
  let makensis = null;
  if (where.status === 0) {
    makensis = (where.stdout ?? "").split(/\r?\n/).map((line) => line.trim()).find((line) => line.length > 0) ?? null;
  } else if (existsSync(nsisBundled)) {
    makensis = nsisBundled;
  }
  if (makensis) {
    const setup = join(root, "dist", `VOIDWRAP_RUN-${version}-windows-setup.exe`);
    console.log(`package: building the Windows installer -> ${setup}`);
    // cwd = release\ so the icon path (..\design\icon\icon.ico) inside installer.nsi resolves.
    // "v0.3.0-alpha.1" -> "0.3.0.0" (the numeric file version Windows requires).
    const nums = (version.match(/\d+/g) ?? []).slice(0, 3);
    while (nums.length < 3) nums.push("0");
    const versionNum = `${nums.join(".")}.0`;
    const r = spawnSync(makensis, [`/DSRC=${out}`, `/DVERSION=${version}`, `/DVERSION_NUM=${versionNum}`, `/DOUTFILE=${setup}`, "installer.nsi"], {
      cwd: join(root, "release"),
      stdio: "inherit",
    });
    if (r.status !== 0) process.exit(r.status ?? 1);
  } else {
    console.log("package: makensis not found; skipping the Windows installer (install NSIS 3 to enable it)");
  }
}

console.log(`package: ready -> ${out}`);
