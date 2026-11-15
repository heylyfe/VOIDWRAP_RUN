import { createRequire } from "node:module";
import { join } from "node:path";
const root = "C:/Users/Nathan/Documents/VOIDWRAP_RUN";
const require = createRequire(join(root, "web/package.json"));
const { chromium } = require("@playwright/test");
const browser = await chromium.launch({ args: ["--use-angle=swiftshader", "--enable-unsafe-swiftshader"] });
const page = await browser.newPage({ viewport: { width: 1280, height: 720 } });
await page.addInitScript(() => {
  window.__sent = [];
  const orig = WebSocket.prototype.send;
  WebSocket.prototype.send = function (data) {
    try { window.__sent.push(String(data).slice(0, 200)); } catch {}
    return orig.call(this, data);
  };
});
await page.goto("http://localhost:3000", { waitUntil: "networkidle" });
await page.getByTestId("play").click();
await page.waitForFunction(() => document.querySelector('[data-testid="conn-status"]')?.textContent === "connected", null, { timeout: 30000 });
await page.getByTestId("draft-screen").waitFor({ timeout: 15000 });
await page.getByTestId("draft-continue").click();
await page.getByTestId("draft-screen").waitFor({ state: "detached", timeout: 15000 });
await page.getByTestId("gift-counter").waitFor({ timeout: 15000 });
await page.mouse.click(640, 360);
await page.waitForTimeout(600);
await page.keyboard.press("Control+Shift+KeyD");
await page.getByTestId("debug-panel").waitFor({ timeout: 5000 });
await page.getByTestId("debug-code").fill("voidwrap-dev");
await page.waitForTimeout(300);
console.log("input value:", await page.getByTestId("debug-code").inputValue());
await page.getByTestId("debug-unlock").click();
await page.waitForTimeout(1500);
const sent = await page.evaluate(() => window.__sent.filter((s) => s.includes("Debug")));
console.log("Debug frames sent:", JSON.stringify(sent, null, 1));
console.log("lock:", await page.getByTestId("debug-lock").textContent());
console.log("log:", (await page.getByTestId("debug-log").textContent())?.trim());
await browser.close();
