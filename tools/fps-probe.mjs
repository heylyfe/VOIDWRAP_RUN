#!/usr/bin/env node
// Client frame-time probe (T-113). Needs `just run` (server + web dev) already up.
// Headless Chromium renders with SwiftShader, so the absolute number is a software
// rasteriser floor, not a GPU measurement. It is still a fair A/B: the same scene,
// the same rasteriser, one variable changed.
//   node tools/fps-probe.mjs [--url http://localhost:3000] [--seconds 4]
import { createRequire } from "node:module";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const require = createRequire(join(root, "web/package.json"));
const { chromium } = require("@playwright/test");
const arg = (name, def) => { const i = process.argv.indexOf(name); return i > 0 ? process.argv[i + 1] : def; };
const base = arg("--url", "http://localhost:3000");
const seconds = Number(arg("--seconds", "4"));

const browser = await chromium.launch({ args: ["--use-angle=swiftshader", "--enable-unsafe-swiftshader"] });
const width = Number(arg("--width", "1280"));
const height = Number(arg("--height", "720"));
const page = await browser.newPage({ viewport: { width, height } });
await page.goto(base, { waitUntil: "networkidle" });
await page.getByTestId("play").click();
await page.waitForFunction(() => document.querySelector('[data-testid="conn-status"]')?.textContent === "connected", null, { timeout: 30_000 });
const draft = await page.getByTestId("draft-screen").waitFor({ timeout: 15_000 }).then(() => true).catch(() => false);
if (draft) {
  await page.getByTestId("draft-continue").click();
  await page.getByTestId("draft-screen").waitFor({ state: "detached", timeout: 15_000 });
}
// A warm server may still hold the previous run's end state; start a fresh one.
if (await page.getByTestId("run-summary").isVisible().catch(() => false)) {
  await page.getByTestId("run-again").click();
  await page.getByTestId("run-summary").waitFor({ state: "detached", timeout: 15_000 });
  const again = await page.getByTestId("draft-screen").waitFor({ timeout: 15_000 }).then(() => true).catch(() => false);
  if (again) {
    await page.getByTestId("draft-continue").click();
    await page.getByTestId("draft-screen").waitFor({ state: "detached", timeout: 15_000 });
  }
}
await page.getByTestId("gift-counter").waitFor({ timeout: 20_000 });
await page.mouse.click(640, 360);
await page.waitForTimeout(1500);
const stats = await page.evaluate((ms) => new Promise((resolve) => {
  const frames = [];
  let last = performance.now();
  const t0 = last;
  const tick = () => {
    const now = performance.now();
    frames.push(now - last);
    last = now;
    if (now - t0 < ms) requestAnimationFrame(tick);
    else {
      frames.sort((a, b) => a - b);
      const mean = frames.reduce((a, b) => a + b, 0) / frames.length;
      resolve({
        frames: frames.length,
        fps: 1000 / mean,
        meanMs: mean,
        p50: frames[Math.floor(frames.length * 0.5)],
        p95: frames[Math.floor(frames.length * 0.95)],
        worst: frames[frames.length - 1],
        dpr: window.devicePixelRatio,
        canvas: (() => { const c = document.querySelector("canvas"); return c ? `${c.width}x${c.height}` : "none"; })(),
      });
    }
  };
  requestAnimationFrame(tick);
}), seconds * 1000);
console.log(`fps-probe: ${stats.frames} frames in ${seconds}s | ${stats.fps.toFixed(1)} fps | mean ${stats.meanMs.toFixed(1)} ms | p50 ${stats.p50.toFixed(1)} | p95 ${stats.p95.toFixed(1)} | worst ${stats.worst.toFixed(1)} | dpr ${stats.dpr} | canvas ${stats.canvas}`);
await browser.close();
