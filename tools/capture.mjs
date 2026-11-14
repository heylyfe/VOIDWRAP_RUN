#!/usr/bin/env node
// Runtime smoke + screenshot capture. Needs `just run` (or the server + web dev) already up.
// Fails (exit 1) on page errors, console errors, or if the client never receives ticks.
//
//   node tools/capture.mjs [--url http://localhost:3000] [--out shots]
import { mkdirSync } from "node:fs";
import { createRequire } from "node:module";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const require = createRequire(join(root, "web/package.json"));
const { chromium } = require("@playwright/test");

const arg = (name, def) => {
  const i = process.argv.indexOf(name);
  return i > 0 ? process.argv[i + 1] : def;
};
const base = arg("--url", "http://localhost:3000");
const outDir = join(root, arg("--out", "shots"));
mkdirSync(outDir, { recursive: true });

const errors = [];
const browser = await chromium.launch({ args: ["--use-angle=swiftshader", "--enable-unsafe-swiftshader"] });
const page = await browser.newPage({ viewport: { width: 1280, height: 720 } });
page.on("pageerror", (e) => errors.push(`pageerror: ${e.message}`));
page.on("console", (m) => {
  if (m.type() === "error") errors.push(`console: ${m.text()}`);
});

const shot = async (name) => {
  const path = join(outDir, `${name}.png`);
  await page.screenshot({ path });
  console.log(`capture: ${path}`);
};

try {
  await page.goto(base, { waitUntil: "networkidle" });
  await shot("00-title");
  await page.getByTestId("play").click();
  await page.waitForFunction(() => document.querySelector('[data-testid="conn-status"]')?.textContent === "connected", null, { timeout: 30_000 });
  const tickOf = async () => Number(await page.getByTestId("server-tick").textContent());
  await page.waitForFunction(() => Number(document.querySelector('[data-testid="server-tick"]')?.textContent) > 0, null, { timeout: 15_000 });
  const t0 = await tickOf();
  await page.waitForTimeout(1000);
  const t1 = await tickOf();
  console.log(`capture: client saw ${t1 - t0} server ticks in ~1 s`);
  if (t1 - t0 < 15) errors.push(`tick rate too low at the client: ${t1 - t0}/s`);
  // Lobby: the draft screen over the spawn hub. Pick two hunters and one reward by hotkey (when offered), then Continue.
  await page.getByTestId("draft-screen").waitFor({ timeout: 15_000 });
  await page.waitForTimeout(600);
  await shot("01-lobby");
  // Level 1 is a plain round with no cards; later lobbies (or VOIDWRAP_START_LEVEL >= 2) offer them.
  if ((await page.getByTestId("draft-first-round").count()) === 0) {
    for (const key of ["Digit1", "Digit2", "Digit5"]) await page.keyboard.press(key);
    await page.waitForTimeout(200);
    await shot("01b-lobby-picked");
  }
  await page.getByTestId("draft-continue").click();
  await page.getByTestId("draft-screen").waitFor({ state: "detached", timeout: 15_000 });
  await page.getByTestId("gift-counter").waitFor({ timeout: 15_000 });
  await page.mouse.click(640, 360); // focus (pointer lock is not available headless)
  await page.waitForTimeout(600);
  await shot("01-spawn");
  const fps = await page.evaluate(() => new Promise((resolve) => {
    let n = 0;
    const t0 = performance.now();
    const tick = () => (performance.now() - t0 < 2000 ? (n++, requestAnimationFrame(tick)) : resolve(n / 2));
    requestAnimationFrame(tick);
  }));
  console.log(`capture: ~${Math.round(fps)} fps (headless software rendering; a lower bound, not a GPU measurement)`);
  const level = await page.getByTestId("level-number").textContent();
  const gifts = await page.getByTestId("gifts-remaining").textContent();
  const counter = await page.getByTestId("gift-counter").textContent();
  console.log(`capture: gift counter "${counter?.trim()}"`);
  console.log(`capture: HUD level=${level} gifts=${gifts}`);
  // M3: ~46+ small gifts per level, all still to collect at the start ("N / N" with N >= 40).
  const [left, total] = (gifts ?? "").split("/").map((s) => Number(s.trim()));
  if (level !== (process.env.CAPTURE_LEVEL ?? "1") || !(total >= 40 && left === total)) {
    errors.push(`unexpected HUD: level=${level} gifts=${gifts}`);
  }
  const currency = await page.getByTestId("currency").textContent();
  console.log(`capture: run status currency=${currency?.trim()}`);
  await page.keyboard.down("KeyW");
  await page.waitForTimeout(500);
  await page.keyboard.press("Space");
  await page.waitForTimeout(250);
  await shot("02-moving");
  await page.keyboard.up("KeyW");
  await page.waitForTimeout(400);
  await shot("03-stopped");
  // Run off the side of the 12x12 spawn platform into the void (it is never surrounded on every side).
  for (const key of ["KeyD", "KeyA", "KeyS"]) {
    if (await page.getByTestId("run-summary").isVisible().catch(() => false)) break;
    await page.keyboard.down(key);
    await page.waitForTimeout(2500);
    await page.keyboard.up(key);
  }
  await shot("04-falling");
  await page.getByTestId("run-summary").waitFor({ timeout: 10_000 });
  await shot("05-run-summary");
  await page.getByTestId("run-again").click();
  await page.getByTestId("run-summary").waitFor({ state: "detached", timeout: 10_000 });
  // A new run starts back in the lobby.
  await page.getByTestId("draft-screen").waitFor({ timeout: 10_000 });
  await page.waitForTimeout(500);
  await shot("06-new-run-lobby");
} catch (e) {
  errors.push(`flow: ${e.message}`);
  await shot("99-failure").catch(() => {});
} finally {
  await browser.close();
}

if (errors.length) {
  console.error(`capture: FAILED\n  ${errors.join("\n  ")}`);
  process.exit(1);
}
console.log("capture: ok, no console errors");
