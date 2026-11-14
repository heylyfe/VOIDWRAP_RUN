#!/usr/bin/env node
// Renders the icon SVGs into transparent PNGs and writes design/icon/icon.ico (all seven sizes,
// PNG-compressed entries, ICONDIR/ICONDIRENTRY written by hand), design/icon/icon-256.png,
// web/public/icon.png and design/icon/icon-64.rgba (raw RGBA, 64*64*4 = 16384 bytes).
// Binary outputs are committed; re-run this after changing the SVGs.
//   node tools/make-icon.mjs
import { createRequire } from "node:module";
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const require = createRequire(join(root, "web/package.json"));
const { chromium } = require("@playwright/test");

const iconDir = join(root, "design/icon");
const bigUrl = `data:image/svg+xml;base64,${readFileSync(join(iconDir, "icon.svg")).toString("base64")}`;
const smallUrl = `data:image/svg+xml;base64,${readFileSync(join(iconDir, "icon-small.svg")).toString("base64")}`;

// 16/24/32 come from the small SVG (it reads better at tiny sizes), the rest from the big one.
const jobs = [
  [16, smallUrl],
  [24, smallUrl],
  [32, smallUrl],
  [48, bigUrl],
  [64, bigUrl],
  [128, bigUrl],
  [256, bigUrl],
];

const browser = await chromium.launch();

/** Render one SVG data URL at a pixel size; returns the PNG buffer (transparent background). */
async function renderPng(size, url) {
  const page = await browser.newPage({ viewport: { width: size, height: size } });
  await page.setContent(`<!doctype html><html><body style="margin:0;background:transparent"><img src="${url}" width="${size}" height="${size}" style="display:block"></body></html>`);
  await page.evaluate(
    () =>
      new Promise((resolve, reject) => {
        const img = document.querySelector("img");
        if (img.complete) resolve(undefined);
        else {
          img.onload = () => resolve(undefined);
          img.onerror = () => reject(new Error("image failed to decode"));
        }
      }),
  );
  const png = await page.screenshot({ omitBackground: true, clip: { x: 0, y: 0, width: size, height: size } });
  await page.close();
  return png;
}

const pngs = new Map();
for (const [size, url] of jobs) {
  pngs.set(size, await renderPng(size, url));
}

// Raw 64x64 RGBA bytes via a canvas inside the page (premultiplied roundtrip; see NOTES).
const rgba64 = await (async () => {
  const page = await browser.newPage({ viewport: { width: 64, height: 64 } });
  await page.setContent(`<!doctype html><canvas id="c" width="64" height="64"></canvas>`);
  const base64 = await page.evaluate(async (pngDataUrl) => {
    const img = new Image();
    img.src = pngDataUrl;
    await img.decode();
    const ctx = document.getElementById("c").getContext("2d");
    ctx.clearRect(0, 0, 64, 64);
    ctx.drawImage(img, 0, 0);
    const d = ctx.getImageData(0, 0, 64, 64).data;
    let s = "";
    const CHUNK = 8192;
    for (let i = 0; i < d.length; i += CHUNK) s += String.fromCharCode(...d.subarray(i, i + CHUNK));
    return btoa(s);
  }, `data:image/png;base64,${pngs.get(64).toString("base64")}`);
  const buf = Buffer.from(base64, "base64");
  await page.close();
  return buf;
})();
if (rgba64.length !== 64 * 64 * 4) {
  throw new Error(`expected 16384 raw RGBA bytes, got ${rgba64.length}`);
}
await browser.close();

// --- ICOn (ICONDIR + ICONDIRENTRY + PNG blobs), written by hand. -------------------------
const SIZES = jobs.map(([size]) => size);
const offsets = [];
let offset = 6 + 16 * SIZES.length;
for (const size of SIZES) {
  offsets.push(offset);
  offset += pngs.get(size).length;
}
const icoHeader = Buffer.alloc(6 + 16 * SIZES.length);
icoHeader.writeUInt16LE(0, 0); // reserved
icoHeader.writeUInt16LE(1, 2); // type 1 = icon
icoHeader.writeUInt16LE(SIZES.length, 4);
let entryPos = 6;
for (let i = 0; i < SIZES.length; i += 1) {
  const size = SIZES[i];
  const png = pngs.get(size);
  const dim = size >= 256 ? 0 : size; // 0 means 256 in ICONDIRENTRY
  icoHeader.writeUInt8(dim, entryPos); // width
  icoHeader.writeUInt8(dim, entryPos + 1); // height
  icoHeader.writeUInt8(0, entryPos + 2); // palette colors (0 = true color)
  icoHeader.writeUInt8(0, entryPos + 3); // reserved
  icoHeader.writeUInt16LE(1, entryPos + 4); // planes
  icoHeader.writeUInt16LE(32, entryPos + 6); // bits per pixel
  icoHeader.writeUInt32LE(png.length, entryPos + 8); // bytes in resource
  icoHeader.writeUInt32LE(offsets[i], entryPos + 12); // image data offset
  entryPos += 16;
}
const ico = Buffer.concat([icoHeader, ...SIZES.map((size) => pngs.get(size))]);

// --- Write outputs. ------------------------------------------------------------------------
mkdirSync(iconDir, { recursive: true });
mkdirSync(join(root, "web/public"), { recursive: true });
writeFileSync(join(iconDir, "icon.ico"), ico);
writeFileSync(join(iconDir, "icon-256.png"), pngs.get(256));
writeFileSync(join(root, "web/public/icon.png"), pngs.get(256));
writeFileSync(join(iconDir, "icon-64.rgba"), rgba64);

for (const [size, png] of pngs) console.log(`make-icon: rendered ${size}x${size} PNG (${png.length} bytes)`);
console.log(`make-icon: wrote design/icon/icon.ico (${ico.length} bytes, ${SIZES.length} sizes: ${SIZES.join(", ")})`);
console.log(`make-icon: wrote design/icon/icon-256.png (${pngs.get(256).length} bytes)`);
console.log(`make-icon: wrote web/public/icon.png (${pngs.get(256).length} bytes)`);
console.log(`make-icon: wrote design/icon/icon-64.rgba (${rgba64.length} bytes)`);
console.log("make-icon: done");
