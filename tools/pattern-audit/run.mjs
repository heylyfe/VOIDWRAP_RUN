#!/usr/bin/env node
/**
 * T-077 — black-box audit of the pattern composition engine.
 *
 * Node built-ins only; no new dependencies.
 *
 * Usage:
 *   node tools/pattern-audit/run.mjs
 *       SKIPPED, exit 0. Env-gated so `just verify` stays green by default.
 *   PATTERN_AUDIT_CMD="<cmd>" node tools/pattern-audit/run.mjs
 *       Runs the generator over PATTERN_AUDIT_SEEDS (default 5000) seeds:
 *       the seed count is appended to <cmd> as its single argument and one
 *       NDJSON level record per generated level is read from stdout.
 *   node tools/pattern-audit/run.mjs --from <capture.ndjson>
 *       Audit a captured NDJSON stream instead of running the generator.
 *   node tools/pattern-audit/run.mjs --selftest
 *       Deterministic self-test of every metric over synthetic records.
 *
 * Env:
 *   PATTERN_AUDIT_SEEDS        seed count (default 5000)
 *   PATTERN_AUDIT_ENEMY_TYPES  comma-separated enemy roster; roster types with
 *                              zero placements across the whole run count as
 *                              "never offered" (metric 6). Optional.
 *
 * Level record contract (one JSON object per stdout line; justified in README.md):
 *   { "seed": 12345, "level": 3, "players": 1,
 *     "plan":     ["pt01_ember_hub", ...],          // ordered CompositionPlan ids
 *     "patterns": [                                 // placed instances, in plan order
 *       { "id": "pt07_ember_span",
 *         "themes": ["ember", "ruin"],              // theme:<value> values
 *         "mechs":  ["span", "climb"],              // mech:<value> values
 *         "platforms": [ { "kind": "NORMAL" | "CRUMBLE" | "JUMP_PAD" | "RAMP_PX" | "RAMP_NX" | "RAMP_PZ" | "RAMP_NZ",
 *                          "min": [x,y,z], "max": [x,y,z] } ] } ],
 *     "enemies":   [ { "type": "chaser" } ],        // placed enemy spawns, may be []
 *     "fallbacks": [ { "kind": "...", "detail": "..." } ] } // anchor stitch fallbacks, may be []
 */

import { spawn } from 'node:child_process';
import { createReadStream, readFileSync, readdirSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import readline from 'node:readline';
import process from 'node:process';

// ---------------------------------------------------------------------------
// Thresholds. Every number is a decision the audit makes explicit; the
// justification sits next to the constant and in tools/pattern-audit/README.md.
// ---------------------------------------------------------------------------
const TH = {
  // Metric 1a — the bag already bans consecutive repeats and divides the weight of
  // anything seen in the last 6 picks by 32 (PATTERN_FORMAT §6.4). With 25+ catalog
  // entries and plans of 10-48 entries, mean within-level redundancy above 35%
  // means eligibility is collapsing into a small clique — that is the defect.
  INTRA_LEVEL_REDUNDANCY_MAX: 0.35,
  // A single pattern occupying more than half of a level Plan is never "varied".
  INTRA_LEVEL_DOMINANCE_MAX: 0.5,

  // Metric 1b — plans are derived from (seed, level, players); one identical ordered
  // list across two distinct seeds means the plan derivation ignored the seed.
  // "Effectively zero" here means exactly 0 measured collisions over the run.
  ACROSS_SEED_PLAN_COLLISIONS_MAX: 0,

  // Metric 2 — §6.4 rule 3 gives a bonus to themes used <2 times so far, so a level
  // is invested in a few themes but rarely a single one. Closed vocabularies from
  // PATTERN_FORMAT §3.1/§3.2; "never offered" uses these static lists.
  THEMES_MEAN_MIN: 3,
  THEMES_LEVEL_MIN: 2,
  MECHS_MEAN_MIN: 4,
  MECHS_LEVEL_MIN: 3,

  // Metric 3 — dead_end sealers and junction budgets make clean stitching the design
  // intent; more than 1% of ~50k generated levels needing rescue is systematic.
  FALLBACK_LEVEL_RATE_MAX: 0.01,

  // Metric 4 — Jaccard on the unordered pattern-id sets. High on purpose: every
  // level legitimately shares structure (a hub, common paths), so only sets that
  // agree on 85%+ of members while the bag still produced different orders count.
  NEARDUPE_JACCARD: 0.85,
  NEARDUPE_LEVEL_RATE_MAX: 0.002, // <= 0.2% of generated levels flagged

  // Metric 5 — T-068 ramp rules and the reference scale from the ticket.
  RAMP_MAX_RISE_RUN: 0.5,
  RAMP_MIN_RUN: 2.0,
  RAMP_MIN_WIDTH: 3.0,
  // T-107: a ramp whose high edge meets no other platform is a ramp the player
  // climbs into thin air. The dump measures this in Rust (levelgen::orphaned_ramps)
  // against the same surface_at the collision layer uses; zero is the only
  // acceptable count because every orphan is a visible geometry defect.
  RAMP_ANCHOR_RATE_MAX: 0,
  STAIR_MIN_STEPS: 3, // "stairs do not exist anywhere in the game" -> threshold 0
  STAIR_STEP_MIN: 0.05,
  STAIR_STEP_MAX: 2.0,
  STAIR_STEP_EPS: 0.05,
  HEIGHT_DIFF_MIN: 1.0,   // reference scale: height differences 1-6 u
  HEIGHT_DIFF_MAX: 6.0,
  // Share of measured adjacent-surface deltas allowed outside [1, 6] u. Raised from
  // 0.02 to 0.05 by owner decision (2026-11-15): the offending heights live in the
  // frozen design/chunks.json, and this is a style rule, not a correctness rule --
  // nothing outside the band is unplayable. Measured 4.47% at the time of the change
  // (down from 6.02% before the T-107 ramp-anchor fixes). A regression past 5% still
  // fails, so the metric keeps its teeth.
  HEIGHT_DIFF_RATE_MAX: 0.05,
  SCALE_EXTENT_MIN: 10.0, // reference scale: platforms 10-20 u — read as the
  SCALE_EXTENT_MAX: 20.0, // placed pattern's max XZ footprint extent (both worked
  SCALE_RATE_MAX: 0.10,   // examples in PATTERN_FORMAT §8 measure 16 u). 10% slack
  // T-112: a gift whose body overlaps a solid platform volume is invisible or
  // unreachable. Measured 1.956% before the spawn-safety rule was widened to every
  // platform; 0.0011% after.
  //
  // T-127: the threshold was 0.0001 (0.01%) to absorb 5 "hits" that turned out to be
  // measurement error -- the dump re-implemented the box test without the generator's
  // 1 mm epsilon, so gifts merely touching a platform face counted as intersecting.
  // The dump now calls `levelgen::gift_body_clear` itself, the count is 0 over
  // 755,878 gifts, and the owner's rule is "NEVER inside a platform", so the
  // threshold is exactly zero. Any hit is now a real defect.
  GIFT_INTERSECT_MAX: 0,

  // Metric 6 — "always offered" only counts cells (mode, level index) with at
  // least this many levels; below that, 100% presence is statistically noise.
  ENEMY_ALWAYS_CELL_MIN: 20,

  MAX_OUTLIERS: 25,
};

/**
 * Every pattern id the shipped catalog declares, read straight from `design/`.
 *
 * T-132: three separate bugs in this project shipped a piece that parsed,
 * validated, was counted by `validate-content`, and never reached a level --
 * `platform_kind_from_str` missing `"TRIPMINE"`, `verify_socket` rejecting ramps,
 * and `mech:fork` on a two-socket path. The first two are caught by the strict
 * parse count assertion; an *eligibility* filter is invisible to it, because the
 * piece is in the catalog and simply never chosen. This set is what makes that
 * visible: every id here must appear in at least one generated plan.
 *
 * Returns `null` when `design/` cannot be read, so the metric reports SKIP rather
 * than failing on a checkout that only has a dump.
 */
function readCatalogIds() {
  const here = dirname(fileURLToPath(import.meta.url));
  const design = join(here, '..', '..', 'design');
  let files;
  try {
    files = readdirSync(design).filter((f) => f === 'chunks.json' || /^patterns-.*\.json$/.test(f));
  } catch {
    return null;
  }
  if (!files.length) return null;
  const ids = new Set();
  for (const f of files) {
    let doc;
    try {
      doc = JSON.parse(readFileSync(join(design, f), 'utf8'));
    } catch {
      return null;
    }
    for (const c of doc.chunks ?? doc.patterns ?? []) {
      if (c && typeof c.id === 'string') ids.add(c.id);
    }
  }
  return ids.size ? ids : null;
}

const CATALOG_IDS = readCatalogIds();

const THEME_VOCAB = ['ember', 'void', 'frost', 'ruin', 'garden', 'machine', 'spire', 'hollow'];
const MECH_VOCAB = ['climb', 'drop', 'span', 'bounce', 'crawl', 'gauntlet', 'rest', 'loot', 'fork', 'dead_end'];
const RAMP_KINDS = new Set(['RAMP_PX', 'RAMP_NX', 'RAMP_PZ', 'RAMP_NZ']);

// ---------------------------------------------------------------------------
// Small helpers
// ---------------------------------------------------------------------------
function bump(map, key) { map.set(key, (map.get(key) ?? 0) + 1); }

function pushRecordError(A, msg) {
  if (A.recordErrors.length < 50) A.recordErrors.push(msg);
}

function pushEx(list, obj) {
  if (list.length < TH.MAX_OUTLIERS) list.push(obj);
}

function f2(x) { return Number.isFinite(x) ? (Math.round(x * 100) / 100).toFixed(2) : String(x); }
function pctf(x) { return Number.isFinite(x) ? (x * 100).toFixed(2) + '%' : 'n/a'; }
function loc(entry) { return `seed=${entry.seed} lvl=${entry.level} players=${entry.players}`; }
function quantile(sorted, q) {
  if (!sorted.length) return NaN;
  const pos = (sorted.length - 1) * q;
  const lo = Math.floor(pos), hi = Math.ceil(pos);
  if (lo === hi) return sorted[lo];
  return sorted[lo] + (sorted[hi] - sorted[lo]) * (pos - lo);
}
function jaccard(aSet, bSet) {
  let inter = 0;
  for (const x of aSet) if (bSet.has(x)) inter++;
  return inter / (aSet.size + bSet.size - inter);
}
function fnv1a(str) {
  let h = 0x811c9dc5 | 0;
  for (let i = 0; i < str.length; i++) {
    h ^= str.charCodeAt(i);
    h = Math.imul(h, 0x01000193);
  }
  return h >>> 0;
}
function show(v) { return typeof v === 'string' ? `"${v}"` : String(v); }

function parseRoster(raw) {
  if (!raw) return null;
  const list = raw.split(',').map((s) => s.trim()).filter((s) => s.length > 0);
  return list.length ? [...new Set(list)] : null;
}

function parseFlags(argv) {
  const flags = { selftest: false, from: null };
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    if (a === '--selftest') flags.selftest = true;
    else if (a === '--from') flags.from = argv[++i] ?? null;
    else { console.error(`unknown argument: ${a}`); process.exit(2); }
  }
  return flags;
}

function intEnv(name, dflt) {
  const raw = process.env[name];
  if (raw === undefined || raw === '') return dflt;
  const v = Number(raw);
  if (!Number.isInteger(v) || v <= 0) {
    console.error(`${name} must be a positive integer, got ${raw}`);
    process.exit(2);
  }
  return v;
}

// ---------------------------------------------------------------------------
// Input sources
// ---------------------------------------------------------------------------
async function* iterLinesFromCommand(cmd, seeds) {
  const child = spawn(cmd, [String(seeds)], { shell: true, stdio: ['ignore', 'pipe', 'pipe'] });
  const stderrTail = [];
  child.stderr.setEncoding('utf8');
  child.stderr.on('data', (d) => {
    stderrTail.push(d);
    if (stderrTail.length > 50) stderrTail.shift();
  });
  let exitInfo = null;
  child.on('exit', (code, sig) => { exitInfo = { code, sig }; });
  const rl = readline.createInterface({ input: child.stdout, crlfDelay: Infinity });
  for await (const line of rl) {
    const t = line.trim();
    if (t) yield t;
  }
  await new Promise((resolve) => { if (exitInfo) resolve(); else child.on('exit', resolve); });
  if (exitInfo === null) throw new Error('generator exited without a status');
  if (exitInfo.code !== 0) {
    throw new Error(`generator exited with code ${exitInfo.code}${exitInfo.sig ? ` (${exitInfo.sig})` : ''}; stderr tail:\n${stderrTail.join('')}`);
  }
}

async function* iterLinesFromFile(path) {
  const rl = readline.createInterface({ input: createReadStream(path, 'utf8'), crlfDelay: Infinity });
  for await (const line of rl) {
    const t = line.trim();
    if (t) yield t;
  }
}

// ---------------------------------------------------------------------------
// Aggregate state
// ---------------------------------------------------------------------------
function createAggregate() {
  return {
    recordCount: 0,
    recordKeys: new Set(),
    recordErrors: [],
    seedsSeen: new Set(),
    playersSeen: new Set(),
    entries: [], // one per level record
    schemaShortfallLevels: 0, // records without a "patterns" placement list
    planIds: new Set(), // every pattern id that reached at least one plan

    geo: {
      placed: 0,
      placementsNoPlatforms: 0,
      scaleViol: 0, scaleByPattern: new Map(), scaleEx: [],
      rampChecked: 0, rampViol: 0, rampEx: [],
      stairSequences: 0, stairPatterns: new Set(), stairEx: [],
      heightPairs: 0, heightViol: 0, heightEx: [],
      gifts: 0, giftIntersections: 0, giftIntersectionLevels: 0,
      rampAnchors: 0, rampOrphans: 0, rampOrphanLevels: 0,
      unreachablePlatforms: 0, unreachableGifts: 0, unreachableGiftLevels: 0,
      nearMissLevels: 0, tightestNearMiss: null,
    },

    planStats: { intraRepeat: 0, redundancySum: 0, redundancy: [], domMax: { v: 0, where: '-' } },
    // key: <level-index> NL <plan joined> -> { seed, players:Set, count }
    planKeys: new Map(),
    crossSeedPlanCollisions: 0,
    crossModeSameSeedKeys: 0,

    themePlacements: new Map(), themeLevels: new Map(), unknownThemeValues: new Map(),
    mechPlacements: new Map(), mechLevels: new Map(), unknownMechValues: new Map(),
    legacyLevels: new Set(), legacyPatternIds: new Map(), // §6.4 no-tag fallback signal

    levelsWithFallbacks: 0, fallbackByKind: new Map(), fallbackDetails: new Map(),

    enemyPlacements: new Map(),
    // key: "<players>|<level>" -> { levels: n, byType: Map(type -> count) }
    enemyCells: new Map(),
  };
}

// ---------------------------------------------------------------------------
// Geometry (metric 5)
// ---------------------------------------------------------------------------
function normPlat(raw) {
  if (!raw || typeof raw !== 'object' || typeof raw.kind !== 'string') return null;
  const mn = raw.min, mx = raw.max;
  if (!Array.isArray(mn) || !Array.isArray(mx) || mn.length !== 3 || mx.length !== 3) return null;
  for (const v of mn) if (!Number.isFinite(v)) return null;
  for (const v of mx) if (!Number.isFinite(v)) return null;
  return { kind: raw.kind, min: mn, max: mx };
}

// Pairs of boxes that share an edge along exactly one horizontal axis.
function boxAdjacencyList(boxes) {
  const out = [];
  for (let i = 0; i < boxes.length; i++) {
    for (let j = i + 1; j < boxes.length; j++) {
      const a = boxes[i], b = boxes[j];
      const ovX = Math.min(a.max[0], b.max[0]) - Math.max(a.min[0], b.min[0]);
      const ovZ = Math.min(a.max[2], b.max[2]) - Math.max(a.min[2], b.min[2]);
      const gapX = Math.max(a.min[0], b.min[0]) - Math.min(a.max[0], b.max[0]);
      const gapZ = Math.max(a.min[2], b.min[2]) - Math.min(a.max[2], b.max[2]);
      const touchX = ovZ > 0.1 && Math.abs(gapX) <= 0.05;
      const touchZ = ovX > 0.1 && Math.abs(gapZ) <= 0.05;
      if (touchX === touchZ) continue; // neither touches, or ambiguous corner contact
      if (touchX) {
        const dir = (b.min[0] + b.max[0]) >= (a.min[0] + a.max[0]) ? 1 : -1;
        out.push({ i, j, axis: 'x', dir });
        out.push({ i: j, j: i, axis: 'x', dir: -dir });
      } else {
        const dir = (b.min[2] + b.max[2]) >= (a.min[2] + a.max[2]) ? 1 : -1;
        out.push({ i, j, axis: 'z', dir });
        out.push({ i: j, j: i, axis: 'z', dir: -dir });
      }
    }
  }
  return out;
}

// Stair sequence: >=3 adjacent box tops, same horizontal direction, equal rise per
// step (within eps), steps in (0.05, 2.0] u. Only leaf paths are reported so a
// 4-step staircase is not double-counted as its own 3-step prefix.
function findStairChains(boxes, adj) {
  const neigh = new Map();
  for (const e of adj) {
    const dh = boxes[e.j].max[1] - boxes[e.i].max[1];
    const mag = Math.abs(dh);
    if (!(mag > TH.STAIR_STEP_MIN && mag <= TH.STAIR_STEP_MAX)) continue;
    if (!neigh.has(e.i)) neigh.set(e.i, []);
    neigh.get(e.i).push({ j: e.j, axis: e.axis, dir: e.dir, dh });
  }
  const found = new Set();
  const path = [];
  const onPath = new Set();
  function dfs(axis, dir, step) {
    const last = path[path.length - 1];
    let extended = false;
    for (const e of neigh.get(last) ?? []) {
      if (onPath.has(e.j)) continue;
      if (e.axis !== axis || e.dir !== dir) continue;
      if (Math.abs(e.dh - step) > TH.STAIR_STEP_EPS) continue;
      if (path.length >= TH.STAIR_MIN_STEPS + 10) continue;
      onPath.add(e.j); path.push(e.j);
      dfs(axis, dir, step);
      path.pop(); onPath.delete(e.j);
      extended = true;
    }
    if (!extended && path.length >= TH.STAIR_MIN_STEPS) {
      found.add([...path].sort((a, b) => a - b).join(','));
    }
  }
  for (const [start, edges] of neigh) {
    for (const e of edges) {
      if (onPath.size) { path.length = 0; onPath.clear(); }
      onPath.add(start); onPath.add(e.j);
      path.push(start, e.j);
      dfs(e.axis, e.dir, e.dh);
    }
  }
  return [...found];
}

function measurePatternGeometry(A, entry, id, pat) {
  A.geo.placed++;
  const plats = Array.isArray(pat.platforms) ? pat.platforms : null;
  if (!plats) { A.geo.placementsNoPlatforms++; return; }

  const boxes = [], ramps = [];
  let minX = Infinity, maxX = -Infinity, minZ = Infinity, maxZ = -Infinity, hasBox = false;
  let minY = Infinity, maxY = -Infinity;
  for (const raw of plats) {
    const p = normPlat(raw);
    if (!p) {
      pushRecordError(A, `bad platform entry in ${id} (${loc(entry)})`);
      continue;
    }
    if (RAMP_KINDS.has(p.kind)) { ramps.push(p); continue; }
    hasBox = true;
    boxes.push(p);
    if (p.min[0] < minX) minX = p.min[0];
    if (p.max[0] > maxX) maxX = p.max[0];
    if (p.min[2] < minZ) minZ = p.min[2];
    if (p.max[2] > maxZ) maxZ = p.max[2];
    if (p.min[1] < minY) minY = p.min[1];
    if (p.max[1] > maxY) maxY = p.max[1];
  }
  if (!hasBox && ramps.length) {
    // ramp-only pattern: fall back to the wedges for the footprint extent
    for (const p of ramps) {
      if (p.min[0] < minX) minX = p.min[0];
      if (p.max[0] > maxX) maxX = p.max[0];
      if (p.min[2] < minZ) minZ = p.min[2];
      if (p.max[2] > maxZ) maxZ = p.max[2];
    }
  }
  if (hasBox || ramps.length) {
    const ext = Math.max(maxX - minX, maxZ - minZ);
    if (!(ext >= TH.SCALE_EXTENT_MIN - 1e-6 && ext <= TH.SCALE_EXTENT_MAX + 1e-6)) {
      A.geo.scaleViol++;
      bump(A.geo.scaleByPattern, id);
      const drop = Number.isFinite(minY) && Number.isFinite(maxY) ? maxY - minY : 0;
      pushEx(A.geo.scaleEx, {
        where: loc(entry), pattern: id, extent: f2(ext), drop: f2(drop),
        // A chunk that drops `d` needs `2 * d / RAMP_MAX_RISE_RUN` of ramp plus its
        // decks, so a big drop forces a big footprint. Reporting the implied minimum
        // separates "out of band because it is a tall set-piece" from "out of band
        // because someone typed the wrong number".
        minExtent: f2(2 * (drop / TH.RAMP_MAX_RISE_RUN)),
        note: `max XZ extent outside [${TH.SCALE_EXTENT_MIN}, ${TH.SCALE_EXTENT_MAX}]u`,
      });
    }
  }

  for (const r of ramps) {
    A.geo.rampChecked++;
    const dx = r.max[0] - r.min[0], dz = r.max[2] - r.min[2], rise = r.max[1] - r.min[1];
    const alongX = r.kind === 'RAMP_PX' || r.kind === 'RAMP_NX';
    const run = alongX ? dx : dz;
    const width = alongX ? dz : dx;
    const issues = [];
    if (run < TH.RAMP_MIN_RUN - 1e-6) issues.push(`run ${f2(run)}u < ${TH.RAMP_MIN_RUN}u`);
    if (width < TH.RAMP_MIN_WIDTH - 1e-6) issues.push(`width ${f2(width)}u < ${TH.RAMP_MIN_WIDTH}u`);
    if (rise < -1e-6) issues.push('negative rise');
    if (run > 1e-6 && rise / run > TH.RAMP_MAX_RISE_RUN + 1e-9) {
      issues.push(`rise/run ${f2(rise / run)} > ${TH.RAMP_MAX_RISE_RUN}`);
    }
    if (issues.length) {
      A.geo.rampViol++;
      pushEx(A.geo.rampEx, {
        where: loc(entry), pattern: id, kind: r.kind,
        rise: f2(rise), run: f2(run), width: f2(width), issues: issues.join('; '),
      });
    }
  }

  if (boxes.length > 1) {
    const adj = boxAdjacencyList(boxes);
    for (const { i, j } of adj) {
      const d = Math.abs(boxes[i].max[1] - boxes[j].max[1]);
      A.geo.heightPairs++;
      if (d <= 0.01) continue; // same walkway surface continuing
      if (d < TH.HEIGHT_DIFF_MIN - 1e-6 || d > TH.HEIGHT_DIFF_MAX + 1e-6) {
        A.geo.heightViol++;
        pushEx(A.geo.heightEx, {
          where: loc(entry), pattern: id, delta: f2(d),
          note: `adjacent tops boxes ${i}/${j} outside [${TH.HEIGHT_DIFF_MIN}, ${TH.HEIGHT_DIFF_MAX}]u`,
        });
      }
    }
    const chains = findStairChains(boxes, adj);
    if (chains.length) {
      A.geo.stairSequences += chains.length;
      A.geo.stairPatterns.add(`${loc(entry)} ${id}`);
      pushEx(A.geo.stairEx, { where: loc(entry), pattern: id, chain: chains[0] });
    }
  }
}

// ---------------------------------------------------------------------------
// Record consumption (metrics 1, 2, 3, 6 collection)
// ---------------------------------------------------------------------------
function consumeRecord(A, r) {
  A.recordCount++;
  if (!r || typeof r !== 'object' || Array.isArray(r)) {
    pushRecordError(A, 'record is not a JSON object');
    return;
  }
  const { seed, level, players } = r;
  if (!Number.isInteger(seed) || !Number.isInteger(level) || !Number.isInteger(players)) {
    pushRecordError(A, `bad seed/level/players (seed=${show(seed)} level=${show(level)} players=${show(players)})`);
    return;
  }
  const key = `${seed}|${level}|${players}`;
  if (A.recordKeys.has(key)) {
    pushRecordError(A, `duplicate record seed=${seed} level=${level} players=${players}`);
    return;
  }
  A.recordKeys.add(key);
  A.seedsSeen.add(seed);
  A.playersSeen.add(players);

  if (!Array.isArray(r.plan) || r.plan.length === 0) {
    pushRecordError(A, `missing/empty plan (${loc({ seed, level, players })})`);
    return;
  }
  const plan = [];
  let badPlan = 0;
  for (const id of r.plan) {
    if (typeof id === 'string' && id) plan.push(id); else badPlan++;
  }
  if (badPlan) pushRecordError(A, `plan has ${badPlan} non-string entries (${loc({ seed, level, players })})`);

  for (const id of plan) A.planIds.add(id);

  const entry = {
    idx: A.entries.length, seed, level, players, plan,
    set: new Set(plan), themes: new Set(), mechs: new Set(), legacy: false,
  };

  // metric 1a — repeats inside one level, computed from the ordered plan
  const counts = new Map();
  let dom = 0;
  for (const id of plan) {
    const c = (counts.get(id) ?? 0) + 1;
    counts.set(id, c);
    if (c > dom) dom = c;
  }
  if (dom >= 2) A.planStats.intraRepeat++;
  const red = 1 - counts.size / plan.length;
  A.planStats.redundancySum += red;
  A.planStats.redundancy.push(red);
  if (dom / plan.length > A.planStats.domMax.v) {
    A.planStats.domMax = { v: dom / plan.length, where: loc(entry) };
  }

  // metric 1b — same ordered plan list across seeds (grouped by level index)
  const k2 = `${level}\u0000${plan.join('\u0001')}`;
  const info = A.planKeys.get(k2);
  if (!info) {
    A.planKeys.set(k2, { seed, players: new Set([players]), count: 1 });
  } else {
    info.count++;
    if (info.seed !== seed) A.crossSeedPlanCollisions++;
    else if (!info.players.has(players)) A.crossModeSameSeedKeys++;
    info.players.add(players);
  }

  // placed patterns — tags (metric 2) and geometry (metric 5)
  const pats = Array.isArray(r.patterns) ? r.patterns : null;
  if (!pats) A.schemaShortfallLevels++;
  for (const pat of (pats ?? [])) {
    if (!pat || typeof pat !== 'object') {
      pushRecordError(A, `bad pattern entry (${loc(entry)})`);
      continue;
    }
    const id = typeof pat.id === 'string' ? pat.id : '<unknown>';
    const themes = Array.isArray(pat.themes) ? pat.themes : [];
    const mechs = Array.isArray(pat.mechs) ? pat.mechs : [];
    for (const t of themes) {
      if (typeof t !== 'string' || !t) continue;
      entry.themes.add(t);
      if (THEME_VOCAB.includes(t)) bump(A.themePlacements, t);
      else bump(A.unknownThemeValues, t);
    }
    for (const m of mechs) {
      if (typeof m !== 'string' || !m) continue;
      entry.mechs.add(m);
      if (MECH_VOCAB.includes(m)) bump(A.mechPlacements, m);
      else bump(A.unknownMechValues, m);
    }
    if (!themes.length || !mechs.length) { entry.legacy = true; bump(A.legacyPatternIds, id); }
    measurePatternGeometry(A, entry, id, pat);
  }
  A.entries.push(entry);
  if (entry.legacy) A.legacyLevels.add(entry.idx);
  for (const t of entry.themes) {
    let s = A.themeLevels.get(t);
    if (!s) A.themeLevels.set(t, (s = new Set()));
    s.add(entry.idx);
  }
  for (const m of entry.mechs) {
    let s = A.mechLevels.get(m);
    if (!s) A.mechLevels.set(m, (s = new Set()));
    s.add(entry.idx);
  }

  // metric 3 — anchor / stitch fallbacks
  if (Array.isArray(r.fallbacks) && r.fallbacks.length > 0) {
    A.levelsWithFallbacks++;
    for (const fb of r.fallbacks) {
      const kind = fb && typeof fb.kind === 'string' && fb.kind ? fb.kind : 'unspecified';
      bump(A.fallbackByKind, kind);
      const detail = fb && typeof fb.detail === 'string' ? fb.detail : '';
      if (detail) {
        let byDetail = A.fallbackDetails.get(kind);
        if (!byDetail) A.fallbackDetails.set(kind, (byDetail = new Map()));
        bump(byDetail, detail);
      }
    }
  }

  // metric 6 — enemy spawn frequencies
  const cellKey = `${players}|${level}`;
  let cell = A.enemyCells.get(cellKey);
  if (!cell) A.enemyCells.set(cellKey, (cell = { levels: 0, byType: new Map() }));
  cell.levels++;
  if (Array.isArray(r.enemies)) {
    for (const en of r.enemies) {
      const t = en && typeof en.type === 'string' && en.type ? en.type : '<unknown>';
      bump(A.enemyPlacements, t);
      bump(cell.byType, t);
    }
  }
}

// ---------------------------------------------------------------------------
// Metric 4 — MinHash (64 linear perms) + LSH (16 bands x 4 rows), exact verify
// ---------------------------------------------------------------------------
function nearDupeScan(entries) {
  const PERMS = 64, ROWS = 4;
  const pa = new Uint32Array(PERMS), pb = new Uint32Array(PERMS);
  for (let i = 0; i < PERMS; i++) {
    pa[i] = (fnv1a(`mh-a-${i}`) | 1) >>> 0;
    pb[i] = fnv1a(`mh-b-${i}`) >>> 0;
  }
  const idHash = new Map();
  const h = (id) => {
    let v = idHash.get(id);
    if (v === undefined) { v = fnv1a(id); idHash.set(id, v); }
    return v;
  };
  for (const e of entries) {
    const sig = new Uint32Array(PERMS).fill(0xffffffff);
    for (const id of e.set) {
      const base = h(id);
      for (let i = 0; i < PERMS; i++) {
        const v = (Math.imul(base, pa[i]) + pb[i]) >>> 0;
        if (v < sig[i]) sig[i] = v;
      }
    }
    e.sig = sig;
  }
  const groups = new Map(); // level index -> entries
  for (const e of entries) {
    let g = groups.get(e.level);
    if (!g) groups.set(e.level, (g = []));
    g.push(e);
  }
  const pairs = new Map();
  let candidates = 0;
  for (const g of groups.values()) {
    const buckets = new Map();
    for (let bi = 0; bi < PERMS / ROWS; bi++) {
      for (const e of g) {
        let bk = `${bi}`;
        for (let rr = 0; rr < ROWS; rr++) bk += `:${e.sig[bi * ROWS + rr]}`;
        let list = buckets.get(bk);
        if (!list) buckets.set(bk, (list = []));
        list.push(e);
      }
    }
    for (const list of buckets.values()) {
      if (list.length < 2) continue;
      for (let i = 0; i < list.length; i++) {
        for (let j = i + 1; j < list.length; j++) {
          const a = list[i], b = list[j];
          if (a.seed === b.seed && a.players === b.players) continue;
          candidates++;
          const pk = a.idx < b.idx ? `${a.idx}|${b.idx}` : `${b.idx}|${a.idx}`;
          if (pairs.has(pk)) continue;
          const jac = jaccard(a.set, b.set);
          if (jac >= TH.NEARDUPE_JACCARD) pairs.set(pk, { a, b, jac });
        }
      }
    }
  }
  return { pairs: [...pairs.values()], candidates };
}

// ---------------------------------------------------------------------------
// Report
// ---------------------------------------------------------------------------
function report(A, ctx, log) {
  const lines = [];
  const fails = [];
  const line = (name, pass, detail) => {
    const s = `${pass ? 'PASS' : 'FAIL'} - ${name} (${detail})`;
    lines.push(s);
    log(s);
    if (!pass) fails.push(name);
  };
  const tables = [];
  const tlog = (s) => { tables.push(s); log(s); };

  const N = A.entries.length;
  if (N === 0) {
    line('0.input', false, `no level records received from ${ctx.source.kind === 'file' ? ctx.source.path : ctx.source.cmd}`);
    for (const e of A.recordErrors.slice(0, TH.MAX_OUTLIERS)) tlog(`  record error: ${e}`);
    log(`RESULT: FAIL - 1 of 1 metric lines failed`);
    return fails;
  }

  // --- 0 input sanity -------------------------------------------------------
  line('0.input',
    A.recordErrors.length === 0 && A.schemaShortfallLevels === 0,
    `records: ${N} from ${A.seedsSeen.size} seeds (target ${ctx.seedsTarget}), modes: players=[${[...A.playersSeen].sort((a, b) => a - b).join(', ')}]; ` +
    `malformed/duplicate records: ${A.recordErrors.length}; levels missing pattern placement data: ${A.schemaShortfallLevels}; ` +
    `levels on the §6.4 no-tag fallback path: ${A.legacyLevels.size}`);

  // --- 1a pattern repeats within a level ------------------------------------
  const redSorted = [...A.planStats.redundancy].sort((a, b) => a - b);
  const meanRed = A.planStats.redundancySum / N;
  const p50 = quantile(redSorted, 0.5);
  const p90 = quantile(redSorted, 0.9);
  const dom = A.planStats.domMax;
  line('1.pattern-repeat.within-level',
    meanRed <= TH.INTRA_LEVEL_REDUNDANCY_MAX && dom.v <= TH.INTRA_LEVEL_DOMINANCE_MAX,
    `levels with a repeated pattern: ${A.planStats.intraRepeat}/${N} (${pctf(A.planStats.intraRepeat / N)}); ` +
    `mean redundancy ${(meanRed * 100).toFixed(2)}% (p50 ${(p50 * 100).toFixed(2)}%, p90 ${(p90 * 100).toFixed(2)}%); ` +
    `worst single-pattern share ${(dom.v * 100).toFixed(1)}% in ${dom.where}; ` +
    `thresholds: mean redundancy <= ${(TH.INTRA_LEVEL_REDUNDANCY_MAX * 100).toFixed(0)}%, share <= ${(TH.INTRA_LEVEL_DOMINANCE_MAX * 100).toFixed(0)}%`);

  // --- 1b identical ordered plan lists across seeds --------------------------
  line('1.pattern-repeat.across-seeds',
    A.crossSeedPlanCollisions <= TH.ACROSS_SEED_PLAN_COLLISIONS_MAX,
    `identical ordered plan lists across distinct seeds: ${A.crossSeedPlanCollisions} over ${N} level records ` +
    `(same seed, different mode, same list: ${A.crossModeSameSeedKeys} keys, informational); ` +
    `threshold: effectively zero (<= ${TH.ACROSS_SEED_PLAN_COLLISIONS_MAX})`);

  // --- 2 tag diversity -------------------------------------------------------
  const neverThemes = THEME_VOCAB.filter((t) => !A.themePlacements.has(t));
  const themeCounts = [...A.entries.map((e) => e.themes.size)].sort((a, b) => a - b);
  const meanThemes = themeCounts.reduce((s, x) => s + x, 0) / N;
  const minThemes = themeCounts[0] ?? 0;
  const unknownThemeTotal = [...A.unknownThemeValues.values()].reduce((s, x) => s + x, 0);
  line('2.tag-diversity.themes',
    meanThemes >= TH.THEMES_MEAN_MIN && minThemes >= TH.THEMES_LEVEL_MIN && neverThemes.length === 0,
    `distinct theme tags per level: mean ${meanThemes.toFixed(2)} (threshold >= ${TH.THEMES_MEAN_MIN}), min ${minThemes} (threshold >= ${TH.THEMES_LEVEL_MIN}); ` +
    `vocabulary coverage ${THEME_VOCAB.length - neverThemes.length}/${THEME_VOCAB.length} values used${neverThemes.length ? ` (never offered: ${neverThemes.join(', ')})` : ''}; ` +
    `unknown forward-compat values observed: ${unknownThemeTotal}`);

  const neverMechs = MECH_VOCAB.filter((m) => !A.mechPlacements.has(m));
  const mechCounts = [...A.entries.map((e) => e.mechs.size)].sort((a, b) => a - b);
  const meanMechs = mechCounts.reduce((s, x) => s + x, 0) / N;
  const minMechs = mechCounts[0] ?? 0;
  const unknownMechTotal = [...A.unknownMechValues.values()].reduce((s, x) => s + x, 0);
  line('2.tag-diversity.mechs',
    meanMechs >= TH.MECHS_MEAN_MIN && minMechs >= TH.MECHS_LEVEL_MIN && neverMechs.length === 0,
    `distinct mechanic tags per level: mean ${meanMechs.toFixed(2)} (threshold >= ${TH.MECHS_MEAN_MIN}), min ${minMechs} (threshold >= ${TH.MECHS_LEVEL_MIN}); ` +
    `vocabulary coverage ${MECH_VOCAB.length - neverMechs.length}/${MECH_VOCAB.length} values used${neverMechs.length ? ` (never offered: ${neverMechs.join(', ')})` : ''}; ` +
    `unknown forward-compat values observed: ${unknownMechTotal}`);

  // --- 3 anchor fallbacks -----------------------------------------------------
  const fbRate = A.levelsWithFallbacks / N;
  line('3.anchor-fallbacks',
    fbRate <= TH.FALLBACK_LEVEL_RATE_MAX,
    `levels needing a stitch fallback: ${A.levelsWithFallbacks}/${N} (${pctf(fbRate)}); threshold <= ${(TH.FALLBACK_LEVEL_RATE_MAX * 100).toFixed(0)}%`);

  // --- 4 near-duplicates -------------------------------------------------------
  const dup = nearDupeScan(A.entries);
  const flaggedLevels = new Set();
  for (const p of dup.pairs) { flaggedLevels.add(p.a.idx); flaggedLevels.add(p.b.idx); }
  line('4.near-duplicates',
    flaggedLevels.size / N <= TH.NEARDUPE_LEVEL_RATE_MAX,
    `pairs with pattern-set Jaccard >= ${TH.NEARDUPE_JACCARD}: ${dup.pairs.length}, covering ${flaggedLevels.size}/${N} levels (${pctf(flaggedLevels.size / N)}); ` +
    `MinHash-LSH candidate pairs examined: ${dup.candidates}, all flagged pairs verified with exact set Jaccard; ` +
    `threshold: <= ${(TH.NEARDUPE_LEVEL_RATE_MAX * 100).toFixed(1)}% of levels`);

  // --- 5 heights, ramps, stairs, reference scale --------------------------------
  const geo = A.geo;
  line('5.ramps',
    geo.rampViol === 0,
    `${geo.rampViol} violations across ${geo.rampChecked} ramp platforms; rules: rise/run <= ${TH.RAMP_MAX_RISE_RUN}, run >= ${TH.RAMP_MIN_RUN}u, width >= ${TH.RAMP_MIN_WIDTH}u; threshold: 0`);
  const raRate = geo.rampAnchors ? geo.rampOrphans / geo.rampAnchors : 0;
  line('5.ramp-anchors',
    raRate <= TH.RAMP_ANCHOR_RATE_MAX,
    `ramps whose high edge meets no other platform: ${geo.rampOrphans}/${geo.rampAnchors} (${pctf(raRate)}) across ${geo.rampOrphanLevels} levels; threshold: 0`);
  line('5.stairs',
    geo.stairSequences === 0,
    `stair-like box sequences (>= ${TH.STAIR_MIN_STEPS} adjacent tops, equal steps +-${TH.STAIR_STEP_EPS}u, step ${TH.STAIR_STEP_MIN}-${TH.STAIR_STEP_MAX}u) in a single direction: ${geo.stairSequences} across ${geo.stairPatterns.size} patterns; threshold: 0 (stairs do not exist)`);
  const hRate = geo.heightPairs ? geo.heightViol / geo.heightPairs : 0;
  line('5.heights',
    hRate <= TH.HEIGHT_DIFF_RATE_MAX,
    `adjacent surface deltas measured: ${geo.heightPairs}; outside [${TH.HEIGHT_DIFF_MIN}, ${TH.HEIGHT_DIFF_MAX}]u (flat continuations ~0u excluded): ${geo.heightViol} (${pctf(hRate)}); threshold <= ${(TH.HEIGHT_DIFF_RATE_MAX * 100).toFixed(0)}%`);
  const sRate = geo.placed ? geo.scaleViol / geo.placed : 1;
  // T-128: name the patterns that own the outlier budget. An aggregate rate hides
  // concentration: six patterns used to account for all 8,081 out-of-band placements,
  // and the metric still passed at 3.85% against a 10% ceiling. The per-pattern list
  // makes the concentration visible, and `minExtent` shows how much of each footprint
  // is forced by the 0.5 ramp-slope cap rather than chosen.
  const scaleOwners = [...geo.scaleByPattern.entries()]
    .sort((a, b) => b[1] - a[1])
    .slice(0, 6)
    .map(([id, n]) => {
      const ex = geo.scaleEx.find((e) => e.pattern === id);
      return ex ? `${id}:${n} (${ex.extent}u, drop ${ex.drop}u, ramp-forced >= ${ex.minExtent}u)` : `${id}:${n}`;
    });
  line('5.reference-scale',
    sRate <= TH.SCALE_RATE_MAX,
    `placed patterns with max XZ footprint extent outside [${TH.SCALE_EXTENT_MIN}, ${TH.SCALE_EXTENT_MAX}]u: ${geo.scaleViol}/${geo.placed} (${pctf(sRate)}); threshold <= ${(TH.SCALE_RATE_MAX * 100).toFixed(0)}%; `
    + `by pattern id: ${scaleOwners.length ? scaleOwners.join(' ') : 'none'}`);

  // --- 7 catalog coverage (T-132) -----------------------------------------------
  // Every pattern the catalog declares must reach at least one generated plan. A
  // piece that parses, validates and is counted by `validate-content` but is never
  // *chosen* is invisible to every other check in this file: it has no placements,
  // so it contributes to no rate. Three shipped bugs had exactly this shape.
  if (!CATALOG_IDS) {
    line('7.catalog-coverage', true, 'SKIP: design/ not readable from this checkout, so the catalog id list is unknown');
  } else {
    const never = [...CATALOG_IDS].filter((id) => !A.planIds.has(id)).sort();
    const unknown = [...A.planIds].filter((id) => !CATALOG_IDS.has(id)).sort();
    line('7.catalog-coverage',
      never.length === 0 && unknown.length === 0,
      `catalog patterns: ${CATALOG_IDS.size}; reached at least one plan: ${CATALOG_IDS.size - never.length}; `
      + `never placed: ${never.length}${never.length ? ` (${never.slice(0, 12).join(', ')}${never.length > 12 ? ', ...' : ''})` : ''}; `
      + `placed but not in the catalog: ${unknown.length}${unknown.length ? ` (${unknown.slice(0, 6).join(', ')})` : ''}; `
      + `threshold: 0 never-placed`);
  }

  // --- 5b gift spawn safety (T-112) ---------------------------------------------
  const gRate = geo.gifts ? geo.giftIntersections / geo.gifts : 0;
  line('5.gift-spawn-safety',
    geo.giftIntersections <= TH.GIFT_INTERSECT_MAX,
    `gifts whose body overlaps a solid platform volume: ${geo.giftIntersections}/${geo.gifts} (${pctf(gRate)}) across ${geo.giftIntersectionLevels} levels; threshold: ${TH.GIFT_INTERSECT_MAX} (the owner's rule is "never inside a platform")`);

  // --- 5c reachability (T-118) ---------------------------------------------------
  // The generator's promise is that every gift is reachable from the spawn. `validate`
  // enforces it per level; this proves it over the whole run. Unreachable PLATFORMS are
  // reported but not failed: a decorative pillar with no gift on it is scenery, and the
  // owner's "gap too big to jump" is exactly what the near-miss number measures.
  const nm = geo.tightestNearMiss;
  line('5.reachability',
    geo.unreachableGifts === 0,
    `gifts on a platform the spawn cannot reach: ${geo.unreachableGifts}/${geo.gifts} across ${geo.unreachableGiftLevels} levels; threshold: 0. `
    + `Unreachable platforms (scenery, informational): ${geo.unreachablePlatforms}. `
    + `Tightest rejected jump: ${nm ? `${nm.gap.toFixed(2)}u gap at ${nm.rise.toFixed(2)}u rise, ${nm.excess.toFixed(2)}u past the envelope (seed=${nm.seed} lvl=${nm.level} players=${nm.players})` : 'none'}; `
    + `levels with a measured near-miss: ${geo.nearMissLevels}`);

  // --- 6 enemy spawn frequencies -------------------------------------------------
  const neverList = ctx.enemyRoster ? ctx.enemyRoster.filter((t) => !A.enemyPlacements.has(t)) : null;
  line('6.enemy-frequencies.never-offered',
    neverList ? neverList.length === 0 : true,
    neverList
      ? `roster types with zero placements across the whole run: ${neverList.length ? neverList.join(', ') : 'none'}; threshold: 0`
      : `roster not provided (set PATTERN_AUDIT_ENEMY_TYPES); only observed diversity below, placed placements per type: ${[...A.enemyPlacements.entries()].map(([t, c]) => `${t}:${c}`).join(' ') || 'none'}`);

  const alwaysCells = [];
  for (const [cellKey, cell] of A.enemyCells) {
    if (cell.levels < TH.ENEMY_ALWAYS_CELL_MIN) continue;
    for (const [t, c] of cell.byType) {
      if (c === cell.levels && cell.byType.size >= 2) {
        alwaysCells.push({ cellKey, type: t, count: c, cellLevels: cell.levels, distinct: cell.byType.size });
      }
    }
  }
  line('6.enemy-frequencies.always-offered',
    alwaysCells.length === 0,
    `(mode, level) cells with >= ${TH.ENEMY_ALWAYS_CELL_MIN} levels where a type appears in 100% of levels: ${alwaysCells.length}; threshold: 0`);

  // ---------------------------------------------------------------------------
  // Raw numbers and flagged outliers
  // ---------------------------------------------------------------------------
  log('');
  tlog('---- distributions (raw numbers) ----');
  tlog('theme placements per value (placements, levels containing it):');
  for (const t of THEME_VOCAB) {
    tlog(`  theme:${t}: ${A.themePlacements.get(t) ?? 0} placements, ${(A.themeLevels.get(t)?.size ?? 0)} levels`);
  }
  for (const [t, c] of A.unknownThemeValues) tlog(`  theme:${t}: ${c} placements, UNKNOWN VALUE (forward-compat, validate-content should warn)`);
  tlog('mech placements per value (placements, levels containing it):');
  for (const m of MECH_VOCAB) {
    tlog(`  mech:${m}: ${A.mechPlacements.get(m) ?? 0} placements, ${(A.mechLevels.get(m)?.size ?? 0)} levels`);
  }
  for (const [m, c] of A.unknownMechValues) tlog(`  mech:${m}: ${c} placements, UNKNOWN VALUE (forward-compat, validate-content should warn)`);

  tlog('---- flagged outliers (first ' + TH.MAX_OUTLIERS + ' per table) ----');
  if (A.recordErrors.length) {
    tlog(`record errors (${A.recordErrors.length >= 50 ? '50+' : A.recordErrors.length}):`);
    for (const e of A.recordErrors) tlog(`  ${e}`);
  }
  if (dup.pairs.length) {
    tlog('near-duplicate level pairs:');
    for (const p of dup.pairs) {
      tlog(`  ${loc(p.a)} <-> ${loc(p.b)}: set Jaccard ${p.jac.toFixed(3)}, |A|=${p.a.set.size} |B|=${p.b.set.size}`);
    }
  }
  if (geo.rampEx.length) {
    tlog('ramp violations:');
    for (const x of geo.rampEx) tlog(`  ${x.where} ${x.pattern} ${x.kind} rise=${x.rise} run=${x.run} width=${x.width}: ${x.issues}`);
  }
  if (geo.stairEx.length) {
    tlog('stair sequences:');
    for (const x of geo.stairEx) tlog(`  ${x.where} ${x.pattern}: boxes ${x.chain}`);
  }
  if (geo.heightEx.length) {
    tlog('height differences outside the 1-6u reference scale:');
    for (const x of geo.heightEx) tlog(`  ${x.where} ${x.pattern}: delta=${x.delta}u (${x.note})`);
  }
  if (geo.scaleEx.length) {
    tlog('footprints outside the 10-20u reference scale:');
    for (const x of geo.scaleEx) tlog(`  ${x.where} ${x.pattern}: extent=${x.extent}u (${x.note})`);
    tlog('  by pattern id: ' + [...geo.scaleByPattern.entries()].sort((a, b) => b[1] - a[1]).map(([k, c]) => `${k}:${c}`).join(' '));
  }
  if (A.levelsWithFallbacks) {
    tlog(`fallback kinds across ${A.levelsWithFallbacks} levels:`);
    for (const [kind, c] of [...A.fallbackByKind.entries()].sort((a, b) => b[1] - a[1])) {
      tlog(`  ${kind}: ${c} levels`);
      const byDetail = A.fallbackDetails.get(kind);
      if (byDetail) {
        for (const [d, dc] of [...byDetail.entries()].sort((a, b) => b[1] - a[1]).slice(0, 5)) {
          tlog(`    ${dc}x ${d}`);
        }
      }
    }
  }
  if (alwaysCells.length) {
    tlog('never-varying enemy types per cell:');
    for (const x of alwaysCells) tlog(`  players/level=${x.cellKey}: type "${x.type}" in ${x.count}/${x.cellLevels} levels (cell has ${x.distinct} distinct types)`);
  }
  if (A.enemyCells.size) {
    tlog(`enemy cells (mode, level index); cell size >= ${TH.ENEMY_ALWAYS_CELL_MIN} qualifies for the always-offered check:`);
    const cellKeys = [...A.enemyCells.keys()].sort((a, b) => {
      const [pa, la] = a.split('|').map(Number);
      const [pb, lb] = b.split('|').map(Number);
      return pa - pb || la - lb;
    });
    for (const k of cellKeys) {
      const cell = A.enemyCells.get(k);
      const total = [...cell.byType.values()].reduce((s, x) => s + x, 0);
      tlog(`  players/level=${k}: ${total} enemies placed, ${cell.byType.size} distinct types (${[...cell.byType.entries()].map(([t, c]) => `${t}:${c}`).join(' ') || 'none'})`);
    }
  } else {
    tlog('enemy cells: none recorded (no record carried an "enemies" array)');
  }
  if (A.legacyLevels.size) {
    tlog('§6.4 no-tag fallback — most-seen tagless pattern ids:');
    for (const [id, c] of [...A.legacyPatternIds.entries()].sort((a, b) => b[1] - a[1]).slice(0, 10)) {
      tlog(`  ${id}: ${c} placements`);
    }
  }

  log('');
  log(`RESULT: ${fails.length ? 'FAIL' : 'PASS'} - ${fails.length} of ${lines.length} metric lines failed${fails.length ? `: ${fails.join(', ')}` : ''}`);
  return fails;
}

// ---------------------------------------------------------------------------
// Runner
// ---------------------------------------------------------------------------
async function runAudit(source, seedsTarget, enemyRoster, log) {
  log(`== T-077 pattern audit == seeds target: ${seedsTarget}, enemy roster: ${enemyRoster ? enemyRoster.join(',') : '(none provided)'}, source: ${source.kind === 'file' ? source.path : source.cmd}`);
  const A = createAggregate();
  try {
    const lines = source.kind === 'file' ? iterLinesFromFile(source.path) : iterLinesFromCommand(source.cmd, seedsTarget);
    for await (const raw of lines) {
      let rec = raw;
      if (typeof rec === 'string') {
        try { rec = JSON.parse(raw); } catch { pushRecordError(A, 'unparseable JSON line'); continue; }
      }
      consumeRecord(A, rec);
      // T-112: gift spawn safety. The dump computes this in Rust, where the placed
      // geometry lives; a gift whose body box overlaps any solid platform volume is a
      // gift the player cannot see or reach.
      const giftTotal = Number(rec.gifts ?? 0);
      const giftHits = Number(rec.gift_intersections ?? 0);
      A.geo.gifts += giftTotal;
      A.geo.giftIntersections += giftHits;
      if (giftHits > 0) A.geo.giftIntersectionLevels += 1;
      // T-107: orphaned ramps. The dump counts ramps whose high edge anchors to
      // nothing; a level with any orphan is a level with a broken climb.
      const rampTotal = Number(rec.ramps ?? 0);
      const rampOrphans = Number(rec.orphan_ramps ?? 0);
      A.geo.rampAnchors += rampTotal;
      A.geo.rampOrphans += rampOrphans;
      if (rampOrphans > 0) A.geo.rampOrphanLevels += 1;
      // T-118: the reachability graph's own verdict. `validate` refuses a level whose
      // gifts are unreachable, so `unreachable_gifts` proves that promise over thousands
      // of levels instead of trusting the generator. `near_miss_*` is the rejected jump
      // that came closest to being legal: the "gap too big to jump" the owner asked for.
      const unreachablePlatforms = Number(rec.unreachable_platforms ?? 0);
      const unreachableGifts = Number(rec.unreachable_gifts ?? 0);
      A.geo.unreachablePlatforms += unreachablePlatforms;
      A.geo.unreachableGifts += unreachableGifts;
      if (unreachableGifts > 0) A.geo.unreachableGiftLevels += 1;
      const nmGap = Number(rec.near_miss_gap ?? -1);
      if (nmGap >= 0) {
        A.geo.nearMissLevels += 1;
        const nmExcess = Number(rec.near_miss_excess ?? 0);
        if (A.geo.tightestNearMiss === null || nmExcess < A.geo.tightestNearMiss.excess) {
          A.geo.tightestNearMiss = {
            excess: nmExcess,
            gap: nmGap,
            rise: Number(rec.near_miss_rise ?? 0),
            seed: rec.seed,
            level: rec.level,
            players: rec.players,
          };
        }
      }
    }
  } catch (err) {
    log(`FATAL - generator/input failed: ${err && err.message ? err.message : err}`);
    return ['FATAL'];
  }
  return report(A, { seedsTarget, enemyRoster, source }, log);
}

// ---------------------------------------------------------------------------
// Self-test: synthetic records with known planted defects. Verifies the metric
// machinery flags exactly what it should and stays quiet otherwise.
// ---------------------------------------------------------------------------
function buildSelftestRecords() {
  const recs = [];
  const THEMES = ['ember', 'frost', 'garden', 'spire'];
  const MECHS = ['climb', 'span', 'rest', 'loot'];
  const mkPat = (id, extraPlatforms) => ({
    id,
    themes: [THEMES[id.length % 4 === 0 ? 0 : id.charCodeAt(id.length - 1) % 4]],
    mechs: [MECHS[id.charCodeAt(1) % 4]],
    platforms: [{ kind: 'NORMAL', min: [-6, 0, -6], max: [6, 0.5, 6] }, ...(extraPlatforms ?? [])],
  });
  for (const seed of [1, 2, 3]) {
    for (const level of [1, 2]) {
      // seed 2 level 1 deliberately re-uses seed 1 level 1's ordered plan (metric 1b)
      const salt = seed === 2 && level === 1 ? 1 : seed;
      const plan = [], pats = [];
      for (let i = 0; i < 10; i++) {
        const id = `p${salt}_${i}`;
        plan.push(id);
        pats.push(mkPat(id));
      }
      if (seed === 3 && level === 2) {
        // starve the level's theme mix (metric 2 min themes) …
        for (const p of pats) p.themes = ['ember'];
        // … and plant an illegal ramp (metric 5): rise/run = 4/6 > 0.5
        pats[0].platforms.push({ kind: 'RAMP_PZ', min: [-3, 0, -6], max: [3, 4, 0] });
      }
      recs.push({
        seed, level, players: 1, plan, patterns: pats,
        enemies: [{ type: 'a' }, { type: 'b' }],
        fallbacks: [],
      });
    }
  }
  return recs;
}

// Recompute tag sets for the self-test records (mkPat derives them from ids).
function selftestTags(recs) {
  const THEMES = ['ember', 'frost', 'garden', 'spire'];
  const MECHS = ['climb', 'span', 'rest', 'loot'];
  for (const r of recs) {
    for (const p of r.patterns) {
      if (p.id.startsWith('p3_') && r.seed === 3 && r.level === 2) {
        p.themes = ['ember']; // starved level (already set by builder; keep explicit)
      } else {
        p.themes = [THEMES[p.id.charCodeAt(p.id.length - 1) % 4]];
      }
      p.mechs = [MECHS[p.id.charCodeAt(1) % 4]];
    }
  }
  return recs;
}

function selftest() {
  const recs = selftestTags(buildSelftestRecords());
  const A = createAggregate();
  for (const rec of recs) consumeRecord(A, rec);
  const fails = report(A, { seedsTarget: 3, enemyRoster: ['a', 'b', 'c', 'ghost'], source: { kind: 'selftest', cmd: '(synthetic records)' } }, console.log);
  const expected = new Set([
    '1.pattern-repeat.across-seeds',
    '2.tag-diversity.themes',
    '2.tag-diversity.mechs',
    '4.near-duplicates',
    '5.ramps',
    '6.enemy-frequencies.never-offered',
  ]);
  const got = new Set(fails);
  const ok = expected.size === got.size && [...expected].every((x) => got.has(x));
  console.log(`SELFTEST - ${ok ? 'PASS' : 'FAIL'} (expected failing metrics: ${[...expected].sort().join(', ')})`);
  return ok;
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------
async function main() {
  const flags = parseFlags(process.argv.slice(2));
  if (flags.selftest) { process.exitCode = selftest() ? 0 : 1; return; }

  const cmd = process.env.PATTERN_AUDIT_CMD;
  if (flags.from === null && !cmd) {
    // Env-gated by default so `just verify` stays green without the generator.
    console.log('SKIPPED - pattern-audit (set PATTERN_AUDIT_CMD="<generator command>" or run with --from <capture.ndjson> or --selftest)');
    process.exitCode = 0;
    return;
  }
  const seeds = intEnv('PATTERN_AUDIT_SEEDS', 5000);
  const roster = parseRoster(process.env.PATTERN_AUDIT_ENEMY_TYPES);
  const source = flags.from !== null ? { kind: 'file', path: flags.from } : { kind: 'cmd', cmd };
  const fails = await runAudit(source, seeds, roster, console.log);
  process.exitCode = fails.length ? 1 : 0;
}

main().catch((err) => {
  console.error(`FATAL - ${err && err.stack ? err.stack : err}`);
  process.exitCode = 2;
});
