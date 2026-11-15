#!/usr/bin/env node
/**
 * tools/silhouette.mjs
 *
 * Measures whether the enemy roster is distinguishable by SHAPE, the way T-145
 * measured whether it is distinguishable by colour.
 *
 * Source reading cannot answer this: the models are animated JSX with conditional
 * rendering, refs and instanced meshes, so parsing their declarations measures what
 * was written, not what is drawn. This drives the dev-only gallery route
 * (`/dev/enemies`, T-152) and measures the rendered pixels.
 *
 * Method: render each enemy dormant, at its gameplay body radius, against a flat
 * background in a fixed camera frame. The pixels that differ from the background are
 * the silhouette. Every pair is compared with IoU in that fixed frame, so a size
 * difference counts as a difference -- two enemies of the same shape but different
 * body radius are distinguishable on screen, and normalising the bounding box away
 * would hide that. A normalised shape-only IoU is reported as a diagnostic.
 *
 *   node tools/silhouette.mjs --url http://localhost:3000 [--out shots] [--report]
 *
 * Needs `npm run dev` in web/ (the gallery route exists only outside production).
 * With no URL it prints `skipped` and exits 0, so `just verify` stays green.
 */
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
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
const BASE = arg("--url", process.env.SILHOUETTE_URL || null);
const OUT = arg("--out", null);
const REPORT = process.argv.includes("--report");

/** Roster ids in wire `kind` order. Mirrors GALLERY_IDS in DevEnemyGallery.tsx. */
const IDS = [
  "pallbearer", "snapshrike", "moteling", "rift_warden", "gale_wight",
  "lantern_leech", "turret", "mimic", "corrupter", "rewind",
  "warden", "peal", "kite", "static", "blight",
  "metronome", "overlook", "chorus", "coil", "lure",
];

/**
 * Body radius per enemy, straight from the content pipeline -- the same number the
 * server sends as `Enemy.radius` and the client scales the model by. Reading it here
 * rather than hardcoding it keeps one source of truth.
 */
function bodySizes() {
  const content = JSON.parse(readFileSync(join(root, "design/content.json"), "utf8"));
  const byId = new Map(content.enemies.map((e) => [e.id, e.params.size]));
  return IDS.map((id) => {
    const size = byId.get(id);
    if (typeof size !== "number" || !(size > 0)) {
      throw new Error(`design/content.json has no params.size for enemy "${id}"`);
    }
    return size;
  });
}

/**
 * Thresholds.
 *
 * MAX_IOU is a regression guard, not a perceptual constant. There is no standard for
 * "these two shapes are the same creature", so the number is anchored to the shipped
 * roster: the worst pair today is peal/coil at 0.780 and the p90 is 0.432, so 0.82
 * leaves the current roster passing with headroom while still failing any new model
 * that is more alike than anything already shipped. A pair above it is not
 * automatically wrong -- it is a pair that has to be justified.
 *
 * MIN_FILL is measured on the normalised shape, not the frame: it asks whether the
 * outline is a hairline that reads as nothing at gameplay distance, not how large the
 * enemy is. Size is already carried by the fixed-frame IoU.
 */
const MAX_IOU = 0.82;
const MIN_FILL = 0.08;

/** Grid used for the shape-only diagnostic. */
const SHAPE_GRID = 64;

/**
 * Runs in the page. Returns the exact silhouette mask (packed, base64) for the fixed
 * frame plus a normalised shape grid for the diagnostic. Doing this in the browser
 * avoids shipping a PNG decoder.
 */
const MEASURE = ({ grid }) => {
  const canvas = document.querySelector("canvas");
  if (!canvas) return { error: "no canvas" };
  const w = canvas.width;
  const h = canvas.height;
  const off = document.createElement("canvas");
  off.width = w;
  off.height = h;
  const ctx = off.getContext("2d", { willReadFrequently: true });
  if (!ctx) return { error: "no 2d context" };
  ctx.drawImage(canvas, 0, 0);
  const { data } = ctx.getImageData(0, 0, w, h);

  // The background is the gallery's flat clear colour. Anything that differs from
  // it by more than a small tolerance is the model.
  const bg = [0x10, 0x10, 0x14];
  const tol = 10;
  const mask = new Uint8Array(w * h);
  let minX = w, minY = h, maxX = -1, maxY = -1, count = 0;
  for (let y = 0; y < h; y += 1) {
    for (let x = 0; x < w; x += 1) {
      const i = (y * w + x) * 4;
      const d =
        Math.abs(data[i] - bg[0]) + Math.abs(data[i + 1] - bg[1]) + Math.abs(data[i + 2] - bg[2]);
      if (d <= tol) continue;
      mask[y * w + x] = 1;
      count += 1;
      if (x < minX) minX = x;
      if (x > maxX) maxX = x;
      if (y < minY) minY = y;
      if (y > maxY) maxY = y;
    }
  }
  if (count === 0) return { error: "empty silhouette", w, h };

  // Pack the fixed-frame mask one bit per pixel so it survives the JSON boundary.
  const packed = new Uint8Array(Math.ceil((w * h) / 8));
  for (let i = 0; i < w * h; i += 1) {
    if (mask[i]) packed[i >> 3] |= 1 << (i & 7);
  }
  let binary = "";
  const CHUNK = 0x8000;
  for (let i = 0; i < packed.length; i += CHUNK) {
    binary += String.fromCharCode.apply(null, packed.subarray(i, i + CHUNK));
  }

  const bw = maxX - minX + 1;
  const bh = maxY - minY + 1;

  // Shape-only diagnostic: crop to the bounding box and resample to a fixed grid, so
  // two enemies are compared by proportion with size factored out.
  const cells = new Uint8Array(grid * grid);
  for (let gy = 0; gy < grid; gy += 1) {
    for (let gx = 0; gx < grid; gx += 1) {
      const x0 = minX + Math.floor((gx * bw) / grid);
      const x1 = minX + Math.max(Math.floor(((gx + 1) * bw) / grid), Math.floor((gx * bw) / grid) + 1);
      const y0 = minY + Math.floor((gy * bh) / grid);
      const y1 = minY + Math.max(Math.floor(((gy + 1) * bh) / grid), Math.floor((gy * bh) / grid) + 1);
      let hit = 0;
      let total = 0;
      for (let y = y0; y < y1 && y < h; y += 1) {
        for (let x = x0; x < x1 && x < w; x += 1) {
          total += 1;
          if (mask[y * w + x]) hit += 1;
        }
      }
      cells[gy * grid + gx] = total > 0 && hit * 2 >= total ? 1 : 0;
    }
  }
  let filled = 0;
  for (const c of cells) filled += c;

  return {
    w, h,
    bbox: { x: minX, y: minY, w: bw, h: bh },
    pixels: count,
    frameFill: count / (w * h),
    shapeFill: filled / (grid * grid),
    aspect: bw / bh,
    // A silhouette touching the frame edge is a framing failure, not a shape.
    clipped: minX === 0 || minY === 0 || maxX === w - 1 || maxY === h - 1,
    shape: Array.from(cells).join(""),
    mask: btoa(binary),
  };
};

/** IoU of two equal-length bit strings ("0101..."). */
function iouBits(a, b) {
  let inter = 0;
  let union = 0;
  for (let i = 0; i < a.length; i += 1) {
    const x = a.charCodeAt(i) === 49;
    const y = b.charCodeAt(i) === 49;
    if (x && y) inter += 1;
    if (x || y) union += 1;
  }
  return union === 0 ? 1 : inter / union;
}

/** IoU of two packed base64 masks. */
function iouMasks(a, b) {
  const x = Buffer.from(a, "base64");
  const y = Buffer.from(b, "base64");
  let inter = 0;
  let union = 0;
  for (let i = 0; i < x.length; i += 1) {
    const p = x[i];
    const q = y[i];
    inter += POPCOUNT[p & q];
    union += POPCOUNT[p | q];
  }
  return union === 0 ? 1 : inter / union;
}

/** Bits set per byte, so mask IoU is a table lookup instead of eight tests. */
const POPCOUNT = new Uint8Array(256);
for (let i = 0; i < 256; i += 1) POPCOUNT[i] = (i & 1) + POPCOUNT[i >> 1];

/**
 * Proves the metric can fail. A threshold that has never been seen to reject anything
 * is not a check, so this feeds the same comparison functions known-answer masks and
 * asserts the gate fires on the ones it must.
 */
function selftest() {
  const W = 64;
  const H = 64;
  const pack = (pred) => {
    const bytes = new Uint8Array((W * H) / 8);
    for (let y = 0; y < H; y += 1) {
      for (let x = 0; x < W; x += 1) {
        if (pred(x, y)) {
          const i = y * W + x;
          bytes[i >> 3] |= 1 << (i & 7);
        }
      }
    }
    return Buffer.from(bytes).toString("base64");
  };
  const box = (x0, y0, x1, y1) => (x, y) => x >= x0 && x < x1 && y >= y0 && y < y1;

  const cases = [];
  const check = (name, got, want) => {
    const ok = Math.abs(got - want) < 1e-9;
    cases.push({ name, ok, got, want });
  };

  // Identical shapes must score 1.0 and therefore trip the overlap gate.
  const a = pack(box(8, 8, 40, 40));
  check("identical masks score 1.0", iouMasks(a, a), 1);
  cases.push({ name: "identical masks exceed MAX_IOU", ok: iouMasks(a, a) > MAX_IOU, got: iouMasks(a, a), want: `> ${MAX_IOU}` });

  // Disjoint shapes must score 0.0 and pass.
  const b = pack(box(44, 44, 60, 60));
  check("disjoint masks score 0.0", iouMasks(a, b), 0);

  // Half-overlap: 32x32 boxes offset by 16 in x share 16x32 of a 48x32 union.
  const c = pack(box(24, 8, 56, 40));
  check("half-overlapping boxes score 1/3", iouMasks(a, c), 1 / 3);

  // A hairline must fall under MIN_FILL when normalised, which is what the fill gate
  // is for: a 1px-wide column is a shape that reads as nothing.
  const hairline = pack((x) => x === 32);
  let filled = 0;
  for (let y = 0; y < H; y += 1) if (hairline) filled += 1;
  const hairlineFill = filled / (W * H);
  cases.push({
    name: "hairline shape is under MIN_FILL",
    ok: hairlineFill < MIN_FILL,
    got: hairlineFill,
    want: `< ${MIN_FILL}`,
  });

  // Bit-string comparison must agree with the mask comparison on the same shapes.
  const bits = (pred) => {
    let out = "";
    for (let y = 0; y < H; y += 1) for (let x = 0; x < W; x += 1) out += pred(x, y) ? "1" : "0";
    return out;
  };
  check("bit and mask IoU agree", iouBits(bits(box(8, 8, 40, 40)), bits(box(24, 8, 56, 40))), 1 / 3);

  let failed = 0;
  for (const c2 of cases) {
    if (!c2.ok) failed += 1;
    console.log(`${c2.ok ? "PASS" : "FAIL"} - selftest.${c2.name} (got ${c2.got}, want ${c2.want})`);
  }
  console.log(`
silhouette selftest: ${failed === 0 ? "PASS" : "FAIL"} (${cases.length - failed}/${cases.length})`);
  return failed === 0 ? 0 : 1;
}

async function main() {
  if (process.argv.includes("--selftest")) return selftest();
  if (!BASE) {
    console.log("silhouette: skipped (set SILHOUETTE_URL or pass --url)");
    return 0;
  }
  const sizes = bodySizes();
  if (OUT) mkdirSync(join(root, OUT), { recursive: true });

  const browser = await chromium.launch({
    args: ["--use-angle=swiftshader", "--enable-unsafe-swiftshader"],
  });
  const page = await browser.newPage({ viewport: { width: 512, height: 512 } });
  const errors = [];
  page.on("pageerror", (e) => errors.push(`pageerror: ${e.message}`));
  page.on("console", (m) => {
    if (m.type() === "error") errors.push(`console: ${m.text()}`);
  });

  const measured = [];
  for (let i = 0; i < IDS.length; i += 1) {
    const size = sizes[i];
    await page.goto(`${BASE}/dev/enemies/?enemy=${i}&size=${size}`, { waitUntil: "networkidle" });
    // The label only exists once the component mounted; the canvas needs a frame on top.
    await page.getByTestId("gallery-label").waitFor({ timeout: 15000 });
    await page.waitForTimeout(350);
    if (OUT) {
      await page.screenshot({ path: join(root, OUT, `${String(i).padStart(2, "0")}-${IDS[i]}.png`) });
    }
    const m = await page.evaluate(MEASURE, { grid: SHAPE_GRID });
    if (m.error) {
      console.log(`FAIL - render ${IDS[i]} (${m.error})`);
      await browser.close();
      return 1;
    }
    measured.push({ id: IDS[i], size, ...m });
  }
  await browser.close();

  // --- per-enemy numbers -----------------------------------------------------
  console.log(`silhouette: ${measured.length} enemies at gameplay body radius, fixed frame`);
  for (const m of measured) {
    console.log(
      `  ${m.id.padEnd(14)} size ${m.size.toFixed(1)} bbox ${String(m.bbox.w).padStart(3)}x${String(m.bbox.h).padStart(3)} ` +
        `aspect ${m.aspect.toFixed(2)} frameFill ${(m.frameFill * 100).toFixed(1)}% ` +
        `shapeFill ${(m.shapeFill * 100).toFixed(1)}%` +
        (m.clipped ? "  CLIPPED" : ""),
    );
  }

  // --- pairwise overlap ------------------------------------------------------
  const pairs = [];
  for (let i = 0; i < measured.length; i += 1) {
    for (let j = i + 1; j < measured.length; j += 1) {
      pairs.push({
        a: measured[i].id,
        b: measured[j].id,
        iou: iouMasks(measured[i].mask, measured[j].mask),
        shapeIou: iouBits(measured[i].shape, measured[j].shape),
      });
    }
  }
  pairs.sort((x, y) => y.iou - x.iou);

  if (REPORT) {
    console.log("\n  closest pairs by fixed-frame IoU (shape-only IoU in brackets):");
    for (const p of pairs.slice(0, 12)) {
      console.log(`    ${p.a} / ${p.b}: ${p.iou.toFixed(3)}  [${p.shapeIou.toFixed(3)}]`);
    }
    const sorted = pairs.map((p) => p.iou).sort((x, y) => x - y);
    const at = (q) => sorted[Math.min(sorted.length - 1, Math.floor(q * sorted.length))];
    console.log(
      `\n  distribution over ${sorted.length} pairs: min ${sorted[0].toFixed(3)} ` +
        `p25 ${at(0.25).toFixed(3)} median ${at(0.5).toFixed(3)} p75 ${at(0.75).toFixed(3)} ` +
        `p90 ${at(0.9).toFixed(3)} max ${sorted[sorted.length - 1].toFixed(3)}`,
    );
  }

  const worst = pairs[0];
  const clipped = measured.filter((m) => m.clipped).map((m) => m.id);
  const thin = measured.filter((m) => m.shapeFill < MIN_FILL).map((m) => m.id);
  const thinnest = Math.min(...measured.map((m) => m.shapeFill));

  const okOverlap = worst.iou <= MAX_IOU;
  const okFill = thin.length === 0;
  const okFrame = clipped.length === 0;
  console.log(
    `\n${okOverlap ? "PASS" : "FAIL"} - silhouette-overlap (closest pair ${worst.a}/${worst.b} at ` +
      `${worst.iou.toFixed(3)}; threshold <= ${MAX_IOU})`,
  );
  console.log(
    `${okFill ? "PASS" : "FAIL"} - silhouette-fill (thinnest ${thinnest.toFixed(3)}; ` +
      `threshold >= ${MIN_FILL}${thin.length ? `; too thin: ${thin.join(", ")}` : ""})`,
  );
  console.log(
    `${okFrame ? "PASS" : "FAIL"} - silhouette-framing (${clipped.length} clipped` +
      `${clipped.length ? `: ${clipped.join(", ")}` : ""})`,
  );
  for (const e of errors.slice(0, 5)) console.log(`  ${e}`);

  if (OUT) {
    writeFileSync(
      join(root, OUT, "silhouette.json"),
      JSON.stringify(
        {
          maxIou: MAX_IOU,
          minFill: MIN_FILL,
          measured: measured.map(({ mask, shape, ...rest }) => rest),
          pairs: pairs.slice(0, 40),
        },
        null,
        2,
      ) + "\n",
    );
  }

  const failed = !okOverlap || !okFill || !okFrame || errors.length > 0;
  console.log(`\nsilhouette: ${failed ? "FAIL" : "PASS"}`);
  return failed ? 1 : 0;
}

main()
  .then((code) => {
    process.exitCode = code;
  })
  .catch((err) => {
    console.error(`silhouette: ${err && err.stack ? err.stack : err}`);
    process.exitCode = 1;
  });
