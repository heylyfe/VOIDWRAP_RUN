#!/usr/bin/env node
// Validates design content (enemies, upgrades, curses, classes, shop, events, milestones, altars, screen grades)
// against the effect vocabulary and the playability limits. Used by `just verify` on design/content.json and by
// OVERSEER on subordinate drafts.
//   node tools/validate-content.mjs [path=design/content.json]
import { readFileSync } from "node:fs";

const path = process.argv[2] ?? "design/content.json";
const c = JSON.parse(readFileSync(path, "utf8"));
const errors = [];
const err = (m) => errors.push(m);

const MUL = ["run_speed_mul", "jump_mul", "gravity_mul", "dash_speed_mul", "dash_cooldown_mul", "gift_radius_mul",
  "enemy_speed_mul", "enemy_aggro_mul", "enemy_grace_mul", "enemy_size_mul", "fog_density_mul", "beacon_radius_mul",
  "gift_spacing_mul"];
const ADD = ["air_jumps_add", "extra_lives_add", "extra_enemies_add"];
// Movement floors/ceilings that keep every level completable (1.5-unit steps, 4-unit gaps).
const PLAYABLE = { run_speed_mul: [0.8, 2], jump_mul: [0.85, 2], gravity_mul: [0.4, 1.25] };
const THEMES = ["ashfall", "fluxbloom", "slagstone", "tidegrave"];
const PLAYER_SPEED = 9;
const ID = /^[a-z][a-z0-9_]{1,31}$/;
const ids = new Set();

function checkId(where, id) {
  if (!ID.test(id ?? "")) err(`${where}: bad id ${JSON.stringify(id)}`);
  if (ids.has(id)) err(`${where}: duplicate id ${id}`);
  ids.add(id);
}
function checkText(where, field, v, max) {
  if (typeof v !== "string" || v.trim() === "") err(`${where}.${field}: missing`);
  else if (v.length > max) err(`${where}.${field}: ${v.length} > ${max} chars`);
}
function checkMods(where, mods, greater) {
  if (!mods || typeof mods !== "object" || Object.keys(mods).length === 0) return err(`${where}: no mods`);
  const [lo, hi] = greater ? [0.4, 2.0] : [0.5, 1.6];
  for (const [k, v] of Object.entries(mods)) {
    if (MUL.includes(k)) {
      if (typeof v !== "number" || v < lo || v > hi) err(`${where}.${k}=${v} outside [${lo}, ${hi}]`);
      const p = PLAYABLE[k];
      if (p && (v < p[0] || v > p[1])) err(`${where}.${k}=${v} breaks playability [${p[0]}, ${p[1]}]`);
    } else if (ADD.includes(k)) {
      if (!Number.isInteger(v) || v < 0 || v > 3) err(`${where}.${k}=${v} must be an integer 0..3`);
    } else err(`${where}: unknown mod key ${k}`);
  }
}

// --- enemies ---------------------------------------------------------------------------------------------------
// M3 roster. The array order IS the network kind index, so the ids are pinned in place. `params.behavior` repeats
// the id (one state machine per enemy); the M2 archetypes and their tuning fields (fire_interval_s,
// projectile_speed, reveal_radius, corrupt_interval_s) are gone - those numbers live in the sim now.
const ENEMY_IDS = ["pallbearer", "snapshrike", "moteling", "rift_warden", "gale_wight", "lantern_leech", "turret",
  "mimic", "corrupter"];
const ENEMY_PARAMS = ["speed", "aggro", "grace_s", "size", "behavior"];
const STATIONARY = ["turret", "mimic"]; // the only two that never roam

if (!Array.isArray(c.enemies) || c.enemies.length !== ENEMY_IDS.length) {
  err(`enemies: need exactly ${ENEMY_IDS.length} (have ${Array.isArray(c.enemies) ? c.enemies.length : 0})`);
}
for (const [i, e] of (c.enemies ?? []).entries()) {
  const w = `enemies[${i}]`;
  checkId(w, e.id);
  if (e.id !== ENEMY_IDS[i]) err(`${w}.id=${JSON.stringify(e.id)} must be ${ENEMY_IDS[i]} (array order is the network kind index)`);
  checkText(w, "name", e.name, 24);
  checkText(w, "flavor", e.flavor, 70);
  checkText(w, "behavior", e.behavior, 110);
  if (!Number.isInteger(e.threat) || e.threat < 1 || e.threat > 5) err(`${w}.threat`);
  const p = e.params ?? {};
  for (const k of Object.keys(p)) if (!ENEMY_PARAMS.includes(k)) err(`${w}.params.${k}: not an enemy param`);
  if (p.behavior !== e.id) err(`${w}.params.behavior=${JSON.stringify(p.behavior)} must equal the enemy id ${JSON.stringify(e.id)}`);
  if (!(typeof p.speed === "number" && p.speed >= 0 && p.speed <= 15)) err(`${w}.params.speed=${p.speed} must be in [0, 15]`);
  else if (p.speed === 0 && !STATIONARY.includes(e.id)) err(`${w}.params.speed=0 is only for ${STATIONARY.join(" / ")}`);
  if (!(typeof p.aggro === "number" && p.aggro >= 0 && p.aggro <= 40)) err(`${w}.params.aggro=${p.aggro} must be in [0, 40]`);
  if (p.grace_s !== 4.0) err(`${w}.params.grace_s=${p.grace_s} must be 4.0 (nothing damages anyone in the first 4 s)`);
  if (!(typeof p.size === "number" && p.size >= 0.3 && p.size <= 1.6)) err(`${w}.params.size=${p.size} must be in [0.3, 1.6]`);
}

// --- upgrades and curses ---------------------------------------------------------------------------------------
// Every mod key a shop item may use has to already exist on an upgrade or a curse.
const usedModKeys = new Set();
for (const [kind, list, min] of [["upgrades", c.upgrades, 8], ["curses", c.curses, 8]]) {
  if (!Array.isArray(list) || list.length < min) err(`need >= ${min} ${kind}`);
  for (const [i, k] of (list ?? []).entries()) {
    const w = `${kind}[${i}]`;
    checkId(w, k.id);
    checkText(w, "name", k.name, 28);
    checkText(w, "flavor", k.flavor, 70);
    checkText(w, "effect", k.effect, 60);
    if (!["common", "rare"].includes(k.rarity)) err(`${w}.rarity`);
    if (kind === "curses" && (!Number.isInteger(k.threat) || k.threat < 1 || k.threat > 5)) err(`${w}.threat`);
    checkMods(w, k.mods, kind === "curses" && k.greater === true);
    for (const key of Object.keys(k.mods ?? {})) usedModKeys.add(key);
  }
}
const greaterCount = (c.curses ?? []).filter((k) => k.greater === true).length;
if (greaterCount < 3) err(`need >= 3 greater curses (have ${greaterCount})`);
// Worst case: fastest enemy under the strongest enemy-speed curse must stay dash-escapable (dash 28 u/s).
const fastest = Math.max(...(c.enemies ?? []).map((e) => e.params?.speed ?? 0));
const worstMul = Math.max(1, ...(c.curses ?? []).map((k) => k.mods?.enemy_speed_mul ?? 1));
if (fastest * worstMul > 14) err(`fastest enemy x worst curse = ${(fastest * worstMul).toFixed(1)} u/s > 14 (cap)`);

// --- classes ---------------------------------------------------------------------------------------------------
// Order is fixed: the client indexes classes by position on the select screen. M3 replaced the Blinker with the
// Bulwark in slot 2; the old id must not survive anywhere in the file.
const CLASS_IDS = ["striker", "grappler", "bulwark", "glider"];
if (!Array.isArray(c.classes)) err("classes: missing");
else {
  if (c.classes.length !== CLASS_IDS.length) err(`classes: need exactly ${CLASS_IDS.length} (have ${c.classes.length})`);
  for (const [i, k] of c.classes.entries()) {
    const w = `classes[${i}]`;
    checkId(w, k.id);
    if (k.id !== CLASS_IDS[i]) err(`${w}.id=${JSON.stringify(k.id)} must be ${CLASS_IDS[i]} (class order is fixed)`);
    checkText(w, "name", k.name, 24);
    checkText(w, "flavor", k.flavor, 70);
    checkText(w, "ability", k.ability, 90);
    checkText(w, "controls", k.controls, 80);
  }
}
if (JSON.stringify(c).includes("blinker")) err('content: "blinker" must not appear anywhere (removed in M3, replaced by bulwark)');

// --- shop ------------------------------------------------------------------------------------------------------
// M3 doubled every price (golden gifts are worth 1 g each), so the cost ceiling moved with them.
const SHOP_KINDS = ["life", "reroll", "mods"];
if (!Array.isArray(c.shop)) err("shop: missing");
else {
  if (c.shop.length !== 7) err(`shop: need exactly 7 items (have ${c.shop.length})`);
  const counts = { life: 0, reroll: 0, mods: 0 };
  for (const [i, s] of c.shop.entries()) {
    const w = `shop[${i}]`;
    checkId(w, s.id);
    checkText(w, "name", s.name, 28);
    checkText(w, "flavor", s.flavor, 70);
    checkText(w, "effect", s.effect, 60);
    if (!SHOP_KINDS.includes(s.kind)) err(`${w}.kind=${JSON.stringify(s.kind)} not one of ${SHOP_KINDS.join(", ")}`);
    else counts[s.kind] += 1;
    if (!Number.isInteger(s.cost) || s.cost < 5 || s.cost > 80) err(`${w}.cost=${s.cost} must be an integer 5..80`);
    if (!Number.isInteger(s.stock) || s.stock < 1 || s.stock > 3) err(`${w}.stock=${s.stock} must be an integer 1..3`);
    if (s.kind === "life" && !(s.cost >= 50 && s.cost <= 70)) err(`${w}.cost=${s.cost} must be 50..70 for a life`);
    if (s.kind === "mods") {
      checkMods(w, s.mods, false);
      for (const key of Object.keys(s.mods ?? {})) {
        if (!usedModKeys.has(key)) err(`${w}.mods.${key}: not a key used by any upgrade or curse`);
      }
    } else if (s.mods !== undefined) err(`${w}.mods: only "mods" shop items carry mods`);
  }
  if (counts.life !== 1) err(`shop: need exactly 1 life item (have ${counts.life})`);
  if (counts.reroll !== 1) err(`shop: need exactly 1 reroll item (have ${counts.reroll})`);
  if (counts.mods !== 5) err(`shop: need exactly 5 mods items (have ${counts.mods})`);
}

// --- level events ----------------------------------------------------------------------------------------------
const EVENT_ORDER = [
  { id: "high_rise", event: 1 },
  { id: "slick_ice", event: 2 },
  { id: "crumbling", event: 3 },
];
if (!Array.isArray(c.events)) err("events: missing");
else {
  if (c.events.length !== EVENT_ORDER.length) err(`events: need exactly ${EVENT_ORDER.length} (have ${c.events.length})`);
  for (const [i, want] of EVENT_ORDER.entries()) {
    const e = c.events[i];
    const w = `events[${i}]`;
    if (!e) { err(`${w}: missing`); continue; }
    checkId(w, e.id);
    if (e.id !== want.id) err(`${w}.id=${JSON.stringify(e.id)} must be ${want.id}`);
    if (e.event !== want.event) err(`${w}.event=${e.event} must be ${want.event}`);
    checkText(w, "name", e.name, 24);
    checkText(w, "banner", e.banner, 70);
  }
  const ice = c.events.find((e) => e?.id === "slick_ice");
  if (ice && ice.friction_mul !== 0.25) err(`events[slick_ice].friction_mul=${ice.friction_mul} must be 0.25`);
}

// --- milestones ------------------------------------------------------------------------------------------------
// Escalation notices (design §5) plus the phase-2 collapse. The client resolves a NOTICE_EVENT id against events
// first and milestones second, so the id set is pinned to the design table and must not shadow an event id.
const MILESTONE_LEVELS = [4, 5, 6, 7, 8, 9, 10, 12, 14, 15, 17, 18, 20, 25];
const MILESTONE_IDS = ["collapse", ...MILESTONE_LEVELS.map((level) => `milestone_${level}`)];
if (!Array.isArray(c.milestones)) err("milestones: missing");
else {
  const seen = new Set();
  for (const [i, m] of c.milestones.entries()) {
    const w = `milestones[${i}]`;
    checkId(w, m.id);
    if (seen.has(m.id)) err(`${w}.id=${m.id}: duplicate milestone id`);
    seen.add(m.id);
    if (!Number.isInteger(m.level) || m.level < 0) err(`${w}.level=${m.level} must be an integer >= 0`);
    else if (m.level === 0 && m.id !== "collapse") err(`${w}.id=${JSON.stringify(m.id)} must be collapse at level 0`);
    else if (m.level > 0 && m.id !== `milestone_${m.level}`) err(`${w}.id=${JSON.stringify(m.id)} must be milestone_${m.level}`);
    checkText(w, "name", m.name, 28);
    checkText(w, "banner", m.banner, 100);
  }
  for (const id of MILESTONE_IDS) if (!seen.has(id)) err(`milestones: missing ${id}`);
  for (const id of seen) if (!MILESTONE_IDS.includes(id)) err(`milestones: ${id} is not an escalation level or the collapse`);
  for (const e of c.events ?? []) if (seen.has(e.id)) err(`milestones: id ${e.id} collides with a level event (events win the lookup)`);
}

// --- altars ----------------------------------------------------------------------------------------------------
if (!Array.isArray(c.altars) || c.altars.length !== 1) {
  err(`altars: need exactly 1 (have ${Array.isArray(c.altars) ? c.altars.length : 0})`);
}
for (const [i, a] of (c.altars ?? []).entries()) {
  const w = `altars[${i}]`;
  checkId(w, a.id);
  if (a.id !== "bargain_altar") err(`${w}.id=${JSON.stringify(a.id)} must be bargain_altar`);
  checkText(w, "name", a.name, 28);
  checkText(w, "flavor", a.flavor, 70);
  checkText(w, "effect", a.effect, 90);
}

// --- screen grades ---------------------------------------------------------------------------------------------
for (const t of THEMES) {
  const g = c.grades?.[t];
  if (!g) { err(`grades.${t}: missing`); continue; }
  const range = (k, lo, hi) => { if (!(typeof g[k] === "number" && g[k] >= lo && g[k] <= hi)) err(`grades.${t}.${k}=${g[k]} outside [${lo}, ${hi}]`); };
  for (const k of ["tint", "vignette_color"]) if (!/^#[0-9a-f]{6}$/i.test(g[k] ?? "")) err(`grades.${t}.${k}`);
  range("tint_strength", 0, 0.35); range("saturation", -0.3, 0.4); range("contrast", -0.2, 0.35);
  range("brightness", -0.15, 0.1); range("vignette_darkness", 0.2, 0.8); range("aberration", 0.0004, 0.003);
  range("grain", 0, 0.08); range("scanlines", 0, 0.15);
}

// Pattern catalogs: design/chunks.json plus the two authored pattern files. Ids must be
// unique across all three, because the sim crate merges them into one catalog at load time.
const PATTERN_FILES = ["design/chunks.json", "design/patterns-a.json", "design/patterns-b.json"];
const patternIds = new Map();
let patternCount = 0;
for (const file of PATTERN_FILES) {
  let doc;
  try {
    doc = JSON.parse(readFileSync(file, "utf8"));
  } catch (e) {
    if (e.code === "ENOENT") continue; // a missing pattern file is not an error
    err(`${file}: ${e.message}`);
    continue;
  }
  const list = doc.chunks ?? doc.patterns;
  if (!Array.isArray(list)) {
    err(`${file}: expected a "chunks" or "patterns" array`);
    continue;
  }
  for (const entry of list) {
    patternCount += 1;
    const id = entry?.id;
    if (typeof id !== "string" || !ID.test(id)) {
      err(`${file}: bad pattern id ${JSON.stringify(id)}`);
      continue;
    }
    if (patternIds.has(id)) err(`${file}: duplicate pattern id ${id} (also in ${patternIds.get(id)})`);
    else patternIds.set(id, file);
  }
}

if (errors.length) {
  console.error(`validate-content: ${path}: ${errors.length} problem(s)\n  ${errors.join("\n  ")}`);
  process.exit(1);
}
console.log(`validate-content: ${path} ok (${c.enemies.length} enemies, ${c.upgrades.length} upgrades, ${c.curses.length} curses, ${c.classes.length} classes, ${c.shop.length} shop items, ${c.events.length} events, ${(c.milestones ?? []).length} milestones, ${c.altars.length} altars, ${patternCount} patterns; fastest enemy worst case ${(fastest * worstMul).toFixed(1)} u/s vs player ${PLAYER_SPEED})`);
