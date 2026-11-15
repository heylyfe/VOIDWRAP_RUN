#!/usr/bin/env node
// E2E for the tester panel (T-111) and the ramp checker (T-106b).
// Requires `VOIDWRAP_DEV_CODE=<code> just run` (no VOIDWRAP_DEV) and the web dev server.
//   node tools/e2e-tester.mjs [--code voidwrap-dev] [--level 12]
import { mkdirSync } from "node:fs";
import { createRequire } from "node:module";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const require = createRequire(join(root, "web/package.json"));
const { chromium } = require("@playwright/test");
const arg = (name, def) => { const i = process.argv.indexOf(name); return i > 0 ? process.argv[i + 1] : def; };
const code = arg("--code", "voidwrap-dev");
const level = arg("--level", "12");
const outDir = join(root, "shots");
mkdirSync(outDir, { recursive: true });

const errors = [];
const browser = await chromium.launch({ args: ["--use-angle=swiftshader", "--enable-unsafe-swiftshader"] });
const page = await browser.newPage({ viewport: { width: 1280, height: 720 } });
page.on("pageerror", (e) => errors.push(`pageerror: ${e.message}`));
page.on("console", (m) => { if (m.type() === "error") errors.push(`console: ${m.text()}`); });
const shot = async (name) => { await page.screenshot({ path: join(outDir, `${name}.png`) }); console.log(`capture: ${name}.png`); };

try {
  await page.goto("http://localhost:3000", { waitUntil: "networkidle" });
  await page.getByTestId("play").click();
  await page.waitForFunction(() => document.querySelector('[data-testid="conn-status"]')?.textContent === "connected", null, { timeout: 30_000 });
  // A warm server may already be past the draft; only clear it when it shows.
  const draft = await page.getByTestId("draft-screen").waitFor({ timeout: 15_000 }).then(() => true).catch(() => false);
  if (draft) {
    await page.getByTestId("draft-continue").click();
    await page.getByTestId("draft-screen").waitFor({ state: "detached", timeout: 15_000 });
  }
  await page.getByTestId("gift-counter").waitFor({ timeout: 20_000 });
  await page.mouse.click(640, 360);
  await page.waitForTimeout(800);

  // A normal build shows no panel at all.
  if ((await page.getByTestId("debug-panel").count()) !== 0) errors.push("debug panel rendered without the hotkey");
  await page.keyboard.press("Control+Shift+KeyD");
  await page.getByTestId("debug-panel").waitFor({ timeout: 5_000 });
  await shot("10-tester-locked");
  const lock = (await page.getByTestId("debug-lock").textContent()) ?? "";
  if (lock.trim() !== "locked") errors.push(`panel did not start locked: "${lock}"`);

  // The server decides: the code unlocks the panel.
  await page.getByTestId("debug-code").fill(code);
  await page.waitForTimeout(250);
  await page.getByTestId("debug-unlock").dispatchEvent("click");
  await page.waitForTimeout(1500);
  const unlocked = (await page.getByTestId("debug-lock").textContent()) ?? "";
  if (unlocked.trim() !== "unlocked") {
    const log = (await page.getByTestId("debug-log").textContent().catch(() => "")) ?? "";
    errors.push(`correct code did not unlock: "${unlocked}" (log: ${log.trim()})`);
  }
  await shot("11-tester-unlocked");

  // Grant gold. A command row expands before its arguments exist.
  await page.getByTestId("debug-cmd-grant_gold").dispatchEvent("click");
  await page.getByTestId("debug-arg-grant_gold-amount").waitFor({ timeout: 10_000 });
  await page.getByTestId("debug-arg-grant_gold-amount").fill("500");
  await page.getByTestId("debug-run-grant_gold").dispatchEvent("click");
  await page.waitForTimeout(900);
  const dump = async (tag) => {
    const log = (await page.getByTestId("debug-log").textContent().catch(() => "<no log>")) ?? "";
    const err = (await page.getByTestId("debug-form-error").textContent().catch(() => "")) ?? "";
    console.log(`capture: ${tag} log = ${log.replace(/\s+/g, " ").trim()}`);
    if (err) console.log(`capture: ${tag} form-error = ${err.trim()}`);
  };
  await dump("after-grant");
  const currency = (await page.getByTestId("currency").textContent()) ?? "";
  console.log(`capture: currency after grant = ${currency.trim()}`);
  if (!/5\d\d/.test(currency)) errors.push(`grant_gold did not move the currency: "${currency}"`);

  // Jump to a ramp-heavy level.
  await page.getByTestId("debug-cmd-switch_level").dispatchEvent("click");
  await page.getByTestId("debug-arg-switch_level-level").waitFor({ timeout: 10_000 });
  await page.getByTestId("debug-arg-switch_level-level").fill(level);
  await page.getByTestId("debug-run-switch_level").dispatchEvent("click");
  await page.waitForTimeout(2500);
  await shot("12-level-switched");
  await dump("after-switch");
  const lvl = (await page.getByTestId("level-number").textContent()) ?? "";
  console.log(`capture: HUD level after switch = ${lvl.trim()}`);
  if (lvl.trim() !== level) errors.push(`switch_level did not change the level: "${lvl}"`);

  // The spawn deck is ringed by paths in some directions only, so sweep the camera a
  // quarter turn at a time and let the reviewer pick the frame that shows a ramp.
  await page.mouse.click(400, 300);
  await page.waitForTimeout(200);
  for (let i = 0; i < 4; i += 1) {
    await page.mouse.move(640, 360);
    await page.mouse.down();
    await page.mouse.move(240, 360, { steps: 12 });
    await page.mouse.up();
    await page.waitForTimeout(500);
    await shot(`16-look-${i}`);
  }

  // Hide the panel and look down at the deck to frame the checker on ramps.
  await page.keyboard.press("Control+Shift+KeyD");
  await page.waitForTimeout(300);
  await page.mouse.move(640, 360);
  await page.mouse.down();
  await page.mouse.move(640, 500, { steps: 12 });
  await page.mouse.up();
  await page.waitForTimeout(700);
  await shot("13-ramps");
} catch (e) {
  errors.push(`threw: ${e.message}`);
  await shot("99-e2e-failure").catch(() => {});
} finally {
  await browser.close();
}
if (errors.length) { console.log("FAILED"); for (const e of errors) console.log(`  ${e}`); process.exitCode = 1; }
else console.log("OK - tester panel E2E passed");
