#!/usr/bin/env node
/**
 * T-089 — black-box economy and flow audit.
 *
 * Node built-ins only; no new dependencies. Reports numbers, not judgement: every
 * metric prints `PASS — name` or `FAIL — name` plus the figures behind it, so the
 * owner's later balancing pass starts from evidence instead of guesses.
 *
 * Usage:
 *   node tools/economy-audit/run.mjs
 *       SKIPPED, exit 0. Env-gated so `just verify` stays green by default.
 *   node tools/economy-audit/run.mjs --from <levels.ndjson>
 *       Audit a captured generator dump (see `sim/examples/pattern_dump.rs`).
 *   ECONOMY_AUDIT_CMD="<cmd>" node tools/economy-audit/run.mjs
 *       Run the generator yourself; the seed count is appended as the single argument.
 *   node tools/economy-audit/run.mjs --selftest
 *       Deterministic self-test of the metric maths over synthetic records.
 *
 * Exit code is 0 only when every metric passes.
 */

import { spawn } from 'node:child_process';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = join(dirname(fileURLToPath(import.meta.url)), '..', '..');
const read = (rel) => JSON.parse(readFileSync(join(root, rel), 'utf8'));

const content = read('design/content.json');
const catalogs = ['design/chunks.json', 'design/patterns-a.json', 'design/patterns-b.json'].map(read);

/** Every catalog entry by id, with its gift-slot count and tags. */
const byId = new Map();
for (const cat of catalogs) {
  for (const p of cat.chunks ?? cat.patterns ?? []) {
    byId.set(p.id, {
      gifts: (p.gift_slots ?? []).length,
      tags: p.tags ?? [],
      role: p.role,
      kinds: (p.platforms ?? []).map((pl) => pl.kind),
    });
  }
}

// --- the constants the server actually uses (server/crates/server/src/run.rs) -----------------
const GREATER_EVERY = 10;
const SHOP_OFFER = 4;
const SHOP_UPGRADE_COST = 300;
const SHOP_UPGRADE_DRAWS = 2;

/** Mode knobs, mirroring `sim::mode::DifficultyMode`. */
const MODES = {
  standard: { capBonus: 0, minLevelShift: 0, shopEveryLevel: false },
  extreme: { capBonus: 1, minLevelShift: -1, shopEveryLevel: false },
  chaos: { capBonus: 0, minLevelShift: 0, shopEveryLevel: true },
  random: { capBonus: 0, minLevelShift: 0, shopEveryLevel: false },
};

const results = [];
function metric(ok, name, detail) {
  results.push({ ok, name, detail });
  console.log(`${ok ? 'PASS' : 'FAIL'} — ${name}`);
  for (const line of detail) console.log(`       ${line}`);
}

const median = (xs) => {
  if (xs.length === 0) return 0;
  const s = [...xs].sort((a, b) => a - b);
  const m = s.length >> 1;
  return s.length % 2 ? s[m] : (s[m - 1] + s[m]) / 2;
};
const mean = (xs) => (xs.length === 0 ? 0 : xs.reduce((a, b) => a + b, 0) / xs.length);
const pct = (n, d) => (d === 0 ? '0.0' : ((100 * n) / d).toFixed(1));

function loadLevels() {
  const fromIdx = process.argv.indexOf('--from');
  if (fromIdx !== -1) {
    const path = process.argv[fromIdx + 1];
    if (!path) throw new Error('--from needs a path');
    return readFileSync(path, 'utf8').split('\n').filter(Boolean).map((l) => JSON.parse(l));
  }
  const cmd = process.env.ECONOMY_AUDIT_CMD;
  if (!cmd) return null;
  const seeds = process.env.ECONOMY_AUDIT_SEEDS ?? '200';
  const r = spawn(cmd, [seeds], { cwd: root, encoding: 'utf8', shell: true });
  let out = '';
  r.stdout.on('data', (d) => { out += d; });
  r.stderr.on('data', () => {});
  return new Promise((resolve, reject) => {
    r.on('close', (code) => {
      if (code !== 0) reject(new Error(`generator exited ${code}`));
      else resolve(out.split('\n').filter(Boolean).map((l) => JSON.parse(l)));
    });
  });
}

// --- metrics ----------------------------------------------------------------------------------
function auditGiftIncome(levels) {
  const perLevel = levels.map((lv) =>
    (lv.plan ?? []).reduce((sum, id) => sum + (byId.get(id)?.gifts ?? 0), 0),
  );
  const nonZero = perLevel.filter((n) => n > 0);
  const cheapest = Math.min(...content.shop.map((s) => s.cost));
  const med = median(perLevel);
  const levelsPerCheapest = med === 0 ? Infinity : cheapest / med;
  metric(levelsPerCheapest <= 4 && levelsPerCheapest >= 0.5, '1.gift_income', [
    `levels sampled: ${levels.length}`,
    `gifts per level: median ${med}, mean ${mean(perLevel).toFixed(2)}, min ${Math.min(...perLevel)}, max ${Math.max(...perLevel)}`,
    `levels with zero gifts: ${perLevel.length - nonZero.length} (${pct(perLevel.length - nonZero.length, perLevel.length)}%)`,
    `cheapest shop item: ${cheapest} currency`,
    `levels of income to afford the cheapest item: ${med === 0 ? 'never' : levelsPerCheapest.toFixed(2)}`,
  ]);
}

function auditShopMix() {
  const statics = content.shop.filter((s) => ['life', 'reroll', 'mods'].includes(s.kind));
  const perks = content.shop.filter((s) => s.kind === 'perk');
  const radar = content.shop.filter((s) => s.kind === 'radar');
  const counters = content.shop.filter((s) => s.kind === 'counter');
  const perShop = SHOP_OFFER + SHOP_UPGRADE_DRAWS + perks.length + radar.length;
  metric(perShop >= 4 && perShop <= 16, '2.shop_offer_mix', [
    `static pool: ${statics.length} (life/reroll/mods), offered ${Math.min(SHOP_OFFER, statics.length)} per shop`,
    `upgrade draws: ${SHOP_UPGRADE_DRAWS} at ${SHOP_UPGRADE_COST} each`,
    `perks: ${perks.length}, radar: ${radar.length}, counters: ${counters.length} (counters only for owned permanents)`,
    `items per shop before counters: ${perShop}`,
    `shop opens on level 1: standard/extreme/random no, chaos yes`,
  ]);
}

function auditPedestalPool() {
  const lines = [];
  let allOk = true;
  for (const [modeId, mode] of Object.entries(MODES)) {
    const capacity = content.enemies.reduce((sum, e) => sum + e.selection_cap + mode.capBonus, 0);
    const firstAvailable = content.enemies
      .map((e) => Math.max(1, e.min_level + mode.minLevelShift))
      .sort((a, b) => a - b);
    const atLevel = (lv) => firstAvailable.filter((m) => m <= lv).length;
    lines.push(
      `${modeId}: capacity ${capacity} permanents, overflow from level ${capacity + 1}; ` +
        `pool at level 1 = ${atLevel(1)}, 10 = ${atLevel(10)}, 20 = ${atLevel(20)}`,
    );
    if (atLevel(1) < 3) allOk = false;
  }
  lines.push(`enemies in the registry: ${content.enemies.length}`);
  metric(allOk, '3.pedestal_pool', lines);
}

function auditGreaterCurses() {
  const greater = content.curses.filter((c) => c.greater);
  const plain = content.curses.filter((c) => !c.greater);
  metric(greater.length >= 3, '4.greater_curse_rate', [
    `greater curses: ${greater.length} (${greater.map((c) => c.id).join(', ')})`,
    `plain curses: ${plain.length}`,
    `greater-curse draft every ${GREATER_EVERY} levels`,
    `over 50 levels: ${Math.floor(50 / GREATER_EVERY)} greater drafts`,
  ]);
}

function auditHazardTags(levels) {
  const counts = new Map();
  let total = 0;
  for (const lv of levels) {
    for (const id of lv.plan ?? []) {
      for (const tag of byId.get(id)?.tags ?? []) {
        if (!tag.startsWith('hazard:')) continue;
        counts.set(tag, (counts.get(tag) ?? 0) + 1);
        total += 1;
      }
    }
  }
  const detail = [`hazard-tagged placements: ${total}`];
  for (const [tag, n] of [...counts.entries()].sort((a, b) => b[1] - a[1])) {
    detail.push(`${tag}: ${n} (${pct(n, total)}%)`);
  }
  if (total === 0) detail.push('no hazard tags placed — trampolines/tripmines are not in the engine yet');
  metric(total > 0, '5.hazard_tags', detail);
}

function auditRadar() {
  const radar = content.shop.filter((s) => s.kind === 'radar');
  metric(radar.length === 3, '6.radar_offering', [
    `radar modules: ${radar.length} (${radar.map((r) => `${r.id}@${r.cost}`).join(', ')})`,
    `offered every shop (make_shop appends all of them)`,
    `total radar spend to own all: ${radar.reduce((a, r) => a + r.cost, 0)}`,
  ]);
}

function auditPatternsPerLevel(levels) {
  const lens = levels.map((lv) => (lv.plan ?? []).length);
  metric(median(lens) >= 3, '7.patterns_per_level', [
    `patterns per level: median ${median(lens)}, min ${Math.min(...lens)}, max ${Math.max(...lens)}`,
    `catalog size: ${byId.size}`,
    `fallback levels: ${levels.filter((lv) => (lv.fallbacks ?? []).length > 0).length} of ${levels.length}`,
  ]);
}

/**
 * Tag presence is not the same as a placed entity: a pattern can carry
 * `hazard:trampoline` and still contain no TRAMPOLINE platform. This metric is the
 * one that actually proves the hazard work landed.
 */
function auditHazardEntities() {
  const want = [
    ['hazard:trampoline', 'TRAMPOLINE'],
    ['hazard:tripmine', 'TRIPMINE'],
  ];
  const lines = [];
  let allOk = true;
  for (const [tag, kind] of want) {
    const tagged = [...byId.entries()].filter(([, v]) => v.tags.includes(tag));
    const withEntity = tagged.filter(([, v]) => (v.kinds ?? []).includes(kind));
    lines.push(
      `${tag}: ${tagged.length} patterns tagged, ${withEntity.length} carry a ${kind} platform ` +
        `(${pct(withEntity.length, tagged.length)}%)`,
    );
    if (tagged.length > 0 && withEntity.length === 0) allOk = false;
  }
  lines.push('a tag with no entity is a promise the engine does not keep');
  metric(allOk, '8.hazard_entities', lines);
}

// --- selftest ---------------------------------------------------------------------------------
function selftest() {
  const fake = [
    { plan: ['a', 'b'], fallbacks: [] },
    { plan: ['a'], fallbacks: [] },
  ];
  byId.set('a', { gifts: 3, tags: ['hazard:trampoline'], role: 'path' });
  byId.set('b', { gifts: 2, tags: [], role: 'path' });
  const gifts = fake.map((lv) => lv.plan.reduce((s, id) => s + byId.get(id).gifts, 0));
  console.assert(JSON.stringify(gifts) === '[5,3]', 'gift sums');
  console.assert(median([5, 3]) === 4, 'median even');
  console.assert(median([5, 3, 9]) === 5, 'median odd');
  console.assert(mean([5, 3]) === 4, 'mean');
  console.assert(pct(1, 4) === '25.0', 'pct');
  console.log('selftest: ok');
}

// --- main -------------------------------------------------------------------------------------
async function main() {
  if (process.argv.includes('--selftest')) {
    selftest();
    return;
  }
  const levels = await loadLevels();
  if (!levels) {
    console.log('economy-audit: skipped (no level dump; pass --from <ndjson> or set ECONOMY_AUDIT_CMD)');
    return;
  }
  console.log(`economy-audit: ${levels.length} levels\n`);
  auditGiftIncome(levels);
  auditShopMix();
  auditPedestalPool();
  auditGreaterCurses();
  auditHazardTags(levels);
  auditHazardEntities();
  auditRadar();
  auditPatternsPerLevel(levels);
  const failed = results.filter((r) => !r.ok);
  console.log(
    `\n${failed.length === 0 ? 'PASS' : 'FAIL'} — economy-audit ` +
      `(${results.length - failed.length}/${results.length} metrics)`,
  );
  process.exit(failed.length === 0 ? 0 : 1);
}

main().catch((e) => {
  console.error(`economy-audit: ${e.message}`);
  process.exit(2);
});
