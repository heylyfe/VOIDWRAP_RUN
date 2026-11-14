#!/usr/bin/env node
// M3 feature screenshot capture. Needs `just run` (or the server + web dev) already up.
// The level-4 draft/shop/hunter captures need a SECOND server started with
// VOIDWRAP_START_LEVEL=4; pass its URL via --url2 to enable them (otherwise those
// shots are skipped and reported as SKIP in the summary).
// Fails (exit 1) on page errors, console errors, or failed checks.
//
//   node tools/capture-m3.mjs [--url http://localhost:3000] [--url2 http://localhost:3001] [--out shots]
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
const url2 = arg("--url2", null);
const outDir = join(root, arg("--out", "shots"));
mkdirSync(outDir, { recursive: true });

const errors = [];
const saved = [];
const checks = [];

const browser = await chromium.launch({ args: ["--use-angle=swiftshader", "--enable-unsafe-swiftshader"] });
const page = await browser.newPage({ viewport: { width: 1280, height: 720 } });
page.on("pageerror", (e) => errors.push(`pageerror: ${e.message}`));
page.on("console", (m) => {
  if (m.type() === "error") errors.push(`console: ${m.text()}`);
});

const shot = async (name) => {
  const path = join(outDir, `${name}.png`);
  await page.screenshot({ path });
  saved.push(path);
  console.log(`capture: ${path}`);
};

const check = (ok, label, { fatal = false } = {}) => {
  checks.push(`${ok ? "PASS" : "FAIL"}: ${label}`);
  console.log(`check: ${ok ? "PASS" : "FAIL"} - ${label}`);
  if (!ok && fatal) errors.push(label);
};

const waitForConnectedTick = async () => {
  await page.waitForFunction(() => document.querySelector('[data-testid="conn-status"]')?.textContent === "connected", null, { timeout: 30_000 });
  await page.waitForFunction(() => Number(document.querySelector('[data-testid="server-tick"]')?.textContent) > 0, null, { timeout: 15_000 });
};

try {
  // ---- Phase A: level 1 on --url ----
  await page.goto(base, { waitUntil: "networkidle" });
  await page.getByTestId("play").click();
  await waitForConnectedTick();
  // Lobby: pick the Bulwark class (ClassPicker hotkeys 1-4; Bulwark = 3), screenshot.
  await page.getByTestId("draft-screen").waitFor({ timeout: 15_000 });
  await page.waitForTimeout(600);
  await page.keyboard.press("Digit3"); // Bulwark
  await page.waitForTimeout(200);
  await shot("m3-00-lobby");
  await page.getByTestId("draft-continue").click();
  await page.getByTestId("draft-screen").waitFor({ state: "detached", timeout: 15_000 });
  await page.getByTestId("gift-counter").waitFor({ timeout: 15_000 });
  await page.mouse.click(640, 360); // focus (pointer lock is not available headless)
  await page.waitForTimeout(1500); // let the world render (software renderer)
  // WIDE overview: drag the mouse to pan/orbit the camera if the game supports it.
  await page.mouse.move(640, 360);
  await page.mouse.down();
  await page.mouse.move(900, 260, { steps: 16 });
  await page.mouse.up();
  await page.waitForTimeout(300);
  await shot("m3-01-level");
  // Hold W for 2 s and jump, then screenshot.
  await page.keyboard.down("KeyW");
  await page.waitForTimeout(2000);
  await page.keyboard.press("Space");
  await page.waitForTimeout(250);
  await shot("m3-02-moving");
  await page.keyboard.up("KeyW");
  // Ctrl mouse-lock toggle: log document.pointerLockElement before/between/after (headless may not support pointer lock; log, non-fatal).
  const lockState = () => page.evaluate(() => (document.pointerLockElement ? document.pointerLockElement.tagName : null));
  const lockBefore = await lockState();
  await page.keyboard.press("ControlLeft");
  await page.waitForTimeout(400);
  const lockMid = await lockState();
  await page.keyboard.press("ControlLeft");
  await page.waitForTimeout(400);
  const lockAfter = await lockState();
  console.log(`capture: pointerLockElement before=${lockBefore ?? "null"} after 1st Ctrl=${lockMid ?? "null"} after 2nd Ctrl=${lockAfter ?? "null"}`);
  check(lockMid !== lockBefore || lockAfter !== lockMid, "Ctrl toggles pointer lock");
  // Gift counter: read the HUD text ("x / total") and assert total >= 40 on level 1.
  const counterText = (await page.getByTestId("gift-counter").textContent())?.trim() ?? "";
  const level = (await page.getByTestId("level-number").textContent())?.trim();
  const gifts = (await page.getByTestId("gifts-remaining").textContent())?.trim();
  console.log(`capture: HUD level=${level} gifts=${gifts}`);
  console.log(`capture: gift counter "${counterText}"`);
  const m = counterText.match(/(\d+)\s*\/\s*(\d+)/);
  if (!m) {
    errors.push(`could not parse gift counter "${counterText}" (expected "x / total")`);
  } else {
    const total = Number(m[2]);
    check(total >= 40, `gift counter total ${total} >= 40 (level 1)`, { fatal: true });
  }
  // ---- Phase B: level 4 on --url2 (VOIDWRAP_START_LEVEL=4 server) ----
  if (url2) {
    console.log(`capture: second run (VOIDWRAP_START_LEVEL=4) at ${url2}`);
    await page.goto(url2, { waitUntil: "networkidle" });
    await page.getByTestId("play").click();
    await waitForConnectedTick();
    await page.getByTestId("draft-screen").waitFor({ timeout: 15_000 });
    await page.waitForTimeout(600);
    // Level 4 offers cards: pick 2 hunters + 1 reward by hotkeys (1, 2, 5).
    if ((await page.getByTestId("draft-first-round").count()) === 0) {
      for (const key of ["Digit1", "Digit2", "Digit5"]) await page.keyboard.press(key);
      await page.waitForTimeout(300);
    } else {
      console.log("capture: draft-first-round present (unexpected for a level-4 start); skipping hunter/reward picks");
    }
    await shot("m3-10-draft");
    // Scroll down to reveal the shop section.
    await page.mouse.move(640, 360);
    for (let i = 0; i < 6; i += 1) {
      await page.mouse.wheel(0, 600);
      await page.waitForTimeout(100);
    }
    await page.waitForTimeout(300);
    await shot("m3-11-shop");
    await page.getByTestId("draft-continue").click();
    await page.getByTestId("draft-screen").waitFor({ state: "detached", timeout: 15_000 });
    await page.getByTestId("gift-counter").waitFor({ timeout: 15_000 });
    await page.mouse.click(640, 360); // focus
    // Wait 6 s for the hunters to wake, then shoot 1 s apart so telegraphs/models may show.
    await page.waitForTimeout(6000);
    for (const n of [12, 13, 14]) {
      await shot(`m3-${n}-hunters`);
      if (n < 14) await page.waitForTimeout(1000);
    }
  } else {
    console.log("capture: --url2 not provided; skipping level-4 captures (m3-10-draft, m3-11-shop, m3-12..14-hunters)");
    checks.push("SKIP: --url2 not provided; level-4 draft/shop/hunter shots not captured");
  }
} catch (e) {
  errors.push(`flow: ${e.message}`);
  await shot("99-failure").catch(() => {});
} finally {
  await browser.close();
}

// Summary of saved files and checks (printed before exiting).
console.log("capture-m3: summary");
console.log("  saved files:");
if (saved.length === 0) console.log("    (none)");
for (const f of saved) console.log(`    ${f}`);
console.log("  checks:");
if (checks.length === 0) console.log("    (none)");
for (const c of checks) console.log(`    ${c}`);

if (errors.length) {
  console.error(`capture-m3: FAILED\n  ${errors.join("\n  ")}`);
  process.exit(1);
}
console.log("capture-m3: ok, no console errors");
