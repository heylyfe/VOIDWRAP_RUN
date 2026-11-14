#!/usr/bin/env node
/* tools/validate-chunks.mjs
   Validates design/chunks.json against design/CHUNK_FORMAT.md (+ binding M3 amendments:
   walkable 0.5 u risers, >= 3 u walkways, 4 u stacking clearance, design hop envelope with
   a 1.0 u comfort margin and a 2.0 u minimum landing extent, internal reachability;
   plain jumps climb at most +2.0 u, the +2.25 u apex being unreachable in practice;
   jump pads climb at most +3.5 u, the +4.0 u pad apex being unreachable in practice).
   Node 24+, ESM, no packages.
   Exit 0 = ok; exit 1 = errors (printed, one per line). Always prints a per-chunk summary. */
import { readFile } from "node:fs/promises";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";

const EPS = 1e-6;
const ROOT = join(dirname(fileURLToPath(import.meta.url)), "..");
const KINDS = new Set(["NORMAL", "CRUMBLE", "JUMP_PAD"]);
const ROLES = new Set(["hub", "path", "junction", "dead_end"]);
const FACINGS = new Set(["N", "E", "S", "W"]);
// Design hop envelope, design max gap (u), rows ascending in deltaY. Conservative:
// next-higher row wins => gap never exceeds the published value for the true deltaY.
// Plain (non-pad) jumps climb at most +2.0 u: the jump apex is exactly +2.25 u
// (12^2 / (2*32)), so a +2.25 u rise can never be cleared in practice. Touching drops
// (gap ~0, rise < -0.55) are always fine. Jump edges additionally require a >= 2.0 u
// smaller landing extent and a 1.0 u comfort margin (gap <= row value - 1.0).
// Jump pads climb at most +3.5 u: the pad apex is exactly +4.0 u (16^2 / (2*32)),
// 3.87 u at 60 Hz, so a +4.0 u rise can never be cleared in practice.
const ENV = [[-4, 8], [-3, 7.5], [-2, 7], [-1, 6.5], [0, 6], [0.5, 5.5], [1, 5], [1.5, 4.5], [2, 4]];
function envelopeGap(dy) {
  if (dy < -4.0 - EPS) return 8;        // deeper drop: same fall budget as the Δ-4.0 row
  for (const [row, gap] of ENV) if (row + EPS >= dy) return gap;
  return null;                           // Δ > +2.0: not jumpable (needs pads/stairs)
}
const isNum = v => typeof v === "number" && Number.isFinite(v);
const v3 = v => Array.isArray(v) && v.length === 3 && v.every(isNum);
const rectOf = p => ({ x0: p.min[0], z0: p.min[2], x1: p.max[0], z1: p.max[2] });
const gapXZ = (a, b) => Math.hypot(
  Math.max(0, Math.max(a.x0 - b.x1, b.x0 - a.x1)),
  Math.max(0, Math.max(a.z0 - b.z1, b.z0 - a.z1)));
const overlapAreaXZ = (a, b) => {
  const ox = Math.min(a.x1, b.x1) - Math.max(a.x0, b.x0);
  const oz = Math.min(a.z1, b.z1) - Math.max(a.z0, b.z0);
  return ox > 1e-9 && oz > 1e-9 ? ox * oz : 0;
};
const f2 = n => +n.toFixed(2);

let raw;
try { raw = await readFile(join(ROOT, "design", "chunks.json"), "utf8"); }
catch (e) { console.error(`FATAL: cannot read design/chunks.json: ${e.message}`); process.exit(1); }
let doc;
try { doc = JSON.parse(raw); }
catch (e) { console.error(`FATAL: chunks.json is not valid JSON: ${e.message}`); process.exit(1); }

const errors = [];
const fail = (id, msg) => errors.push(`[${id ?? "root"}] ${msg}`);
const summaries = [];

// ===== one chunk =====
function validateChunk(c) {
  const id = c && typeof c.id === "string" ? c.id : "?";
  if (!c || typeof c !== "object") { fail(id, "chunk must be an object"); return; }
  if (typeof c.id !== "string" || !/^[a-z0-9_]+$/.test(c.id)) fail(id, "id must match [a-z0-9_]+");
  if (typeof c.name !== "string" || !c.name.trim()) fail(id, "name must be a non-empty string");
  if (!ROLES.has(c.role)) fail(id, `role must be one of ${[...ROLES].join("|")}`);
  if (!Number.isInteger(c.min_level) || c.min_level < 1) fail(id, "min_level must be an integer >= 1");
  if (!Number.isInteger(c.difficulty) || c.difficulty < 1 || c.difficulty > 3) fail(id, "difficulty must be an integer 1..3");
  if (!Array.isArray(c.tags) || c.tags.some(t => typeof t !== "string" || !t.trim())) fail(id, "tags must be an array of non-empty strings");

  if (!Array.isArray(c.platforms) || c.platforms.length === 0) { fail(id, "platforms must be a non-empty array"); summaries.push(`${id}: INVALID`); return; }
  const plats = [];
  c.platforms.forEach((p, i) => {
    const ok = p && KINDS.has(p.kind) && v3(p.min) && v3(p.max)
      && p.min[0] < p.max[0] && p.min[1] < p.max[1] && p.min[2] < p.max[2];
    if (!ok) { fail(id, `platform #${i}: needs kind NORMAL|CRUMBLE|JUMP_PAD and min < max on all axes`); return; }
    const eX = p.max[0] - p.min[0], eZ = p.max[2] - p.min[2];
    const small = Math.min(eX, eZ), large = Math.max(eX, eZ);
    if (small + EPS < 1.0) fail(id, `platform #${i}: smaller horizontal extent ${f2(small)} u < 1.0 u`);
    if (large + EPS < 3.0) fail(id, `platform #${i}: larger horizontal extent ${f2(large)} u < 3.0 u`);
    plats.push({ kind: p.kind, min: p.min.slice(), max: p.max.slice(), rect: rectOf(p), top: p.max[1] });
  });
  if (!plats.length) { summaries.push(`${id}: INVALID`); return; }

  let minX = Infinity, maxX = -Infinity, minZ = Infinity, maxZ = -Infinity, lo = Infinity, hi = -Infinity;
  for (const p of plats) {
    minX = Math.min(minX, p.min[0]); maxX = Math.max(maxX, p.max[0]);
    minZ = Math.min(minZ, p.min[2]); maxZ = Math.max(maxZ, p.max[2]);
    lo = Math.min(lo, p.min[1]); hi = Math.max(hi, p.max[1]);
  }

  if (c.role === "hub" || c.role === "junction") {
    if (!plats.some(p => (p.max[0] - p.min[0]) >= 6 - EPS && (p.max[2] - p.min[2]) >= 6 - EPS))
      fail(id, `${c.role} chunk needs at least one platform >= 6 x 6 u`);
  }

  // ===== sockets =====
  if (!Array.isArray(c.sockets)) fail(id, "sockets must be an array");
  const socketsRaw = Array.isArray(c.sockets) ? c.sockets : [];
  const sockIds = new Set();
  const support = socketsRaw.map(() => []);  // socket index -> supporting platform indices
  socketsRaw.forEach((s, i) => {
    if (!s || typeof s !== "object") { fail(id, `socket #${i}: must be an object`); return; }
    if (typeof s.id !== "string" || !s.id) fail(id, `socket #${i}: id required`);
    else if (sockIds.has(s.id)) fail(id, `socket '${s.id}': duplicate id`); else sockIds.add(s.id);
    if (!v3(s.pos)) { fail(id, `socket '${s.id}': pos must be [x,y,z]`); return; }
    if (!FACINGS.has(s.facing)) fail(id, `socket '${s.id}': facing must be N|E|S|W`);
    if (!isNum(s.width) || s.width + EPS < 3.0) fail(id, `socket '${s.id}': width must be >= 3 u`);
    if (!FACINGS.has(s.facing)) return;
    const y = s.pos[1], w = isNum(s.width) ? s.width : 0;
    const ns = s.facing === "N" || s.facing === "S";
    const onEdge = s.facing === "N" ? Math.abs(s.pos[2] - maxZ) <= EPS :
                   s.facing === "S" ? Math.abs(s.pos[2] - minZ) <= EPS :
                   s.facing === "E" ? Math.abs(s.pos[0] - maxX) <= EPS :
                                      Math.abs(s.pos[0] - minX) <= EPS;
    if (!onEdge) fail(id, `socket '${s.id}': pos is not on the ${s.facing} footprint edge`);
    const t = ns ? s.pos[0] : s.pos[2], tLo = ns ? minX : minZ, tHi = ns ? maxX : maxZ;
    if (t < tLo - EPS || t > tHi + EPS) fail(id, `socket '${s.id}': pos outside footprint tangentially`);
    plats.forEach((p, pi) => {
      if (Math.abs(p.top - y) > 1e-6) return;
      const flush = s.facing === "N" ? Math.abs(p.max[2] - maxZ) <= EPS :
                    s.facing === "S" ? Math.abs(p.min[2] - minZ) <= EPS :
                    s.facing === "E" ? Math.abs(p.max[0] - maxX) <= EPS :
                                       Math.abs(p.min[0] - minX) <= EPS;
      if (!flush) return;
      const c0 = ns ? p.min[0] : p.min[2], c1 = ns ? p.max[0] : p.max[2];
      if (c0 <= t - w / 2 + EPS && c1 >= t + w / 2 - EPS) support[i].push(pi);
    });
    if (!support[i].length) fail(id, `socket '${s.id}': no platform touches the ${s.facing} edge across the full ${w} u width at y=${y}`);
  });

  // ===== beacon / enemy slots =====
  const hasBeacon = v3(c.beacon);
  if (c.beacon !== undefined && !hasBeacon) fail(id, "beacon must be [x,y,z]");
  if (hasBeacon && c.role !== "hub") fail(id, "beacon only allowed on hub chunks");
  const enemies = Array.isArray(c.enemy_slots) ? c.enemy_slots : (fail(id, "enemy_slots must be an array"), []);
  if (enemies.length) {
    if (c.role !== "hub") fail(id, "enemy slots only allowed on hub chunks");
    if (!hasBeacon) fail(id, "enemy slots require a beacon");
    if (enemies.length !== 8) fail(id, `exactly 8 enemy slots on hubs, got ${enemies.length}`);
    enemies.forEach((e, i) => {
      if (!v3(e)) { fail(id, `enemy slot #${i}: must be [x,y,z]`); return; }
      const onTop = plats.some(p => Math.abs(p.top - e[1]) <= 1e-6 &&
        e[0] >= p.min[0] - EPS && e[0] <= p.max[0] + EPS && e[2] >= p.min[2] - EPS && e[2] <= p.max[2] + EPS);
      if (!onTop) fail(id, `enemy slot #${i}: not on a platform top`);
      if (hasBeacon) {
        const d = Math.hypot(e[0] - c.beacon[0], e[2] - c.beacon[2]);
        if (d < 6 - EPS || d > 8 + EPS) fail(id, `enemy slot #${i}: ring radius ${f2(d)} u outside 6..8 u of beacon`);
      }
    });
  }

  // ===== stacking: XZ overlaps need >= 4.0 u clearance or contact =====
  for (let i = 0; i < plats.length; i++) for (let j = i + 1; j < plats.length; j++) {
    const a = plats[i], b = plats[j];
    if (overlapAreaXZ(a.rect, b.rect) <= 0) continue;          // zero-area touch: fine
    const vertOverlap = a.min[1] < b.max[1] - EPS && b.min[1] < a.max[1] - EPS;
    if (vertOverlap) { fail(id, `platforms #${i} and #${j} interpenetrate (XZ + vertical overlap)`); continue; }
    const [loP, hiP] = a.min[1] <= b.min[1] ? [a, b] : [b, a];
    const gap = hiP.min[1] - loP.max[1];                        // > 0 (no vertical overlap)
    if (gap > EPS && gap + EPS < 4.0) fail(id, `platforms #${i} and #${j}: only ${f2(gap)} u vertical clearance (< 4.0 u)`);
    // gap <= EPS: contact (one's top == other's bottom): fine
  }

  // ===== gift slots =====
  const gifts = Array.isArray(c.gift_slots) ? c.gift_slots : (fail(id, "gift_slots must be an array"), []);
  const lowestSocketY = socketsRaw.length && socketsRaw.every(s => s && v3(s.pos))
    ? Math.min(...socketsRaw.map(s => s.pos[1])) : 0;
  const giftPlats = [];
  gifts.forEach((g, i) => {
    if (!g || !v3(g.pos)) { fail(id, `gift slot #${i}: pos must be [x,y,z]`); return; }
    if (typeof g.elevated !== "boolean") fail(id, `gift slot #${i}: elevated must be boolean`);
    const p = plats.find(p => p.kind === "NORMAL" && Math.abs(p.top - (g.pos[1] - 0.3)) <= 1e-6 &&
      g.pos[0] >= p.min[0] + 0.5 - EPS && g.pos[0] <= p.max[0] - 0.5 + EPS &&
      g.pos[2] >= p.min[2] + 0.5 - EPS && g.pos[2] <= p.max[2] - 0.5 + EPS);
    if (!p) { fail(id, `gift slot #${i}: not on a NORMAL top at pos.y == top + 0.3 u, or within 0.5 u of an edge (no CRUMBLE/JUMP_PAD gifts)`); return; }
    giftPlats.push(plats.indexOf(p));
    if (typeof g.elevated === "boolean") {
      const want = p.top >= lowestSocketY + 2.0 - EPS;
      if (g.elevated !== want) fail(id, `gift slot #${i}: elevated=${g.elevated} but top ${p.top} vs lowest socket ${lowestSocketY} + 2.0`);
    }
    if (hasBeacon) {
      const d = Math.hypot(g.pos[0] - c.beacon[0], g.pos[2] - c.beacon[2]);
      if (d < 2.5 - EPS) fail(id, `gift slot #${i}: ${f2(d)} u from beacon (< 2.5 u)`);
    }
    for (let k = 0; k < i; k++) {
      const o = gifts[k]; if (!o || !v3(o.pos)) continue;
      const d = Math.hypot(o.pos[0] - g.pos[0], o.pos[2] - g.pos[2]);
      if (d < 2.0 - EPS) fail(id, `gift slots #${k} and #${i}: ${f2(d)} u apart (< 2.0 u)`);
    }
  });

  // ===== altar slots: on NORMAL tops, y == top, >= 1.5 u from edges; max 2; never on a beacon hub =====
  const altars = Array.isArray(c.altar_slots) ? c.altar_slots : (fail(id, "altar_slots must be an array"), []);
  if (altars.length > 2) fail(id, `at most 2 altar slots per chunk, got ${altars.length}`);
  if (hasBeacon && altars.length) fail(id, "altar slots never on a beacon/spawn hub");
  const altarPlats = [];
  altars.forEach((a, i) => {
    if (!v3(a)) { fail(id, `altar slot #${i}: must be [x,y,z]`); return; }
    const p = plats.find(p => p.kind === "NORMAL" && Math.abs(p.top - a[1]) <= 1e-6 &&
      a[0] >= p.min[0] + 1.5 - EPS && a[0] <= p.max[0] - 1.5 + EPS &&
      a[2] >= p.min[2] + 1.5 - EPS && a[2] <= p.max[2] - 1.5 + EPS);
    if (!p) fail(id, `altar slot #${i}: not on a NORMAL top (y == top) or within 1.5 u of an edge`);
    else altarPlats.push(plats.indexOf(p));
  });

  // ===== internal reachability (directed) =====
  const adj = plats.map(() => []);
  for (let i = 0; i < plats.length; i++) for (let j = 0; j < plats.length; j++) {
    if (i === j) continue;
    const u = plats[i], v = plats[j];
    const d = gapXZ(u.rect, v.rect), dy = v.top - u.top;
    if (d <= 0.05 + EPS && Math.abs(dy) <= 0.55 + EPS) { adj[i].push(j); continue; }   // touch + step-up
    if (d <= 0.05 + EPS && dy < -0.55 - EPS) { adj[i].push(j); continue; }             // touching drop: always fine
    // JUMP edge: landing needs >= 2.0 u smaller horizontal extent, and the gap must be
    // <= design max gap for the rise minus a 1.0 u comfort margin.
    const landingSmall = Math.min(v.rect.x1 - v.rect.x0, v.rect.z1 - v.rect.z0);
    if (landingSmall + EPS < 2.0) continue;
    if (u.kind === "JUMP_PAD" && dy >= -0.5 - EPS && dy <= 3.5 + EPS && d <= 4.5 + EPS) { adj[i].push(j); continue; }  // pad launch (apex - 0.5)
    const mg = envelopeGap(dy);                                                        // design envelope, 1.0 u comfort
    if (mg !== null && d <= (mg - 1.0) + EPS) { adj[i].push(j); continue; }
  }
  const bfs = seeds => {
    const seen = new Set(seeds), q = [...seeds];
    while (q.length) { const n = q.pop(); for (const m of adj[n]) if (!seen.has(m)) { seen.add(m); q.push(m); } }
    return seen;
  };
  const allSocketPlats = support.flat();
  const targets = [...new Set([...allSocketPlats, ...giftPlats, ...altarPlats])];
  socketsRaw.forEach((s, i) => {
    if (!s || !v3(s.pos) || !support[i].length) return;
    const seen = bfs(support[i]);
    for (const t of targets) if (!seen.has(t))
      fail(id, `socket '${s.id ?? i}': platform #${t} (socket/gift/altar platform) unreachable`);
  });

  summaries.push(`${id}: size=${f2(maxX - minX)}x${f2(maxZ - minZ)}u, sockets=${socketsRaw.length}, gift_slots=${gifts.length}, height=${f2(lo)}..${f2(hi)}u`);
}

// ===== run =====
if (doc === null || typeof doc !== "object") fail("root", "top level must be an object");
else {
  if (doc.version !== 1) fail("root", `version must be 1, got ${JSON.stringify(doc.version)}`);
  if (!Array.isArray(doc.chunks)) fail("root", "chunks must be an array");
  else {
    if (doc.chunks.length !== 25) fail("root", `exactly 25 chunks required, got ${doc.chunks.length}`);
    const seen = new Set();
    for (const c of doc.chunks) { const id = c && c.id; if (typeof id === "string") { if (seen.has(id)) fail(id, "duplicate chunk id"); seen.add(id); } }
    for (const c of doc.chunks) validateChunk(c);
    if (!doc.chunks.some(c => c && c.role === "hub" && Array.isArray(c.enemy_slots) && c.enemy_slots.length === 8 && v3(c.beacon)))
      fail("root", "at least one hub must have a beacon and exactly 8 enemy slots");
  }
}

console.log(summaries.join("\n"));
if (errors.length) {
  console.error(`\nVALIDATION FAILED: ${errors.length} error(s)`);
  for (const e of errors) console.error("  - " + e);
  process.exit(1);
}
console.log(`\nOK: ${doc.chunks.length} chunks valid`);
