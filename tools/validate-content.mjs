#!/usr/bin/env node
// Validates design content (enemies, upgrades, curses, screen grades) against the effect vocabulary and the
// playability limits. Used by `just verify` on design/content.json and by OVERSEER on subordinate drafts.
//   node tools/validate-content.mjs [path=design/content.json]
import { readFileSync } from "node:fs";

const path = process.argv[2] ?? "design/content.json";
const c = JSON.parse(readFileSync(path, "utf8"));
const errors = [];
const err = (m) => errors.push(m);

const MUL = ["run_speed_mul", "jump_mul", "gravity_mul", "dash_speed_mul", "dash_cooldown_mul", "gift_radius_mul",
  "enemy_speed_mul", "enemy_aggro_mul", "enemy_grace_mul", "enemy_size_mul", "fog_density_mul", "beacon_radius_mul"];
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

if (!Array.isArray(c.enemies) || c.enemies.length < 4) err("need >= 4 enemies");
for (const [i, e] of (c.enemies ?? []).entries()) {
  const w = `enemies[${i}]`;
  checkId(w, e.id);
  checkText(w, "name", e.name, 24);
  checkText(w, "flavor", e.flavor, 70);
  checkText(w, "behavior", e.behavior, 110);
  if (!Number.isInteger(e.threat) || e.threat < 1 || e.threat > 5) err(`${w}.threat`);
  const p = e.params ?? {};
  if (!(p.speed > 0 && p.speed <= 8.5)) err(`${w}.params.speed=${p.speed} must be in (0, 8.5]`);
  if (!(p.aggro >= 4 && p.aggro <= 40)) err(`${w}.params.aggro=${p.aggro}`);
  if (!(p.grace_s >= 1.5 && p.grace_s <= 5)) err(`${w}.params.grace_s=${p.grace_s}`);
  if (!(p.size >= 0.2 && p.size <= 1.5)) err(`${w}.params.size=${p.size}`);
}
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
  }
}
const greaterCount = (c.curses ?? []).filter((k) => k.greater === true).length;
if (greaterCount < 3) err(`need >= 3 greater curses (have ${greaterCount})`);
// Worst case: fastest enemy under the strongest enemy-speed curse must stay dash-escapable (dash 28 u/s).
const fastest = Math.max(...(c.enemies ?? []).map((e) => e.params?.speed ?? 0));
const worstMul = Math.max(1, ...(c.curses ?? []).map((k) => k.mods?.enemy_speed_mul ?? 1));
if (fastest * worstMul > 14) err(`fastest enemy x worst curse = ${(fastest * worstMul).toFixed(1)} u/s > 14 (cap)`);
for (const t of THEMES) {
  const g = c.grades?.[t];
  if (!g) { err(`grades.${t}: missing`); continue; }
  const range = (k, lo, hi) => { if (!(typeof g[k] === "number" && g[k] >= lo && g[k] <= hi)) err(`grades.${t}.${k}=${g[k]} outside [${lo}, ${hi}]`); };
  for (const k of ["tint", "vignette_color"]) if (!/^#[0-9a-f]{6}$/i.test(g[k] ?? "")) err(`grades.${t}.${k}`);
  range("tint_strength", 0, 0.35); range("saturation", -0.3, 0.4); range("contrast", -0.2, 0.35);
  range("brightness", -0.15, 0.1); range("vignette_darkness", 0.2, 0.8); range("aberration", 0.0004, 0.003);
  range("grain", 0, 0.08); range("scanlines", 0, 0.15);
}
if (errors.length) {
  console.error(`validate-content: ${path}: ${errors.length} problem(s)\n  ${errors.join("\n  ")}`);
  process.exit(1);
}
console.log(`validate-content: ${path} ok (${c.enemies.length} enemies, ${c.upgrades.length} upgrades, ${c.curses.length} curses; fastest enemy worst case ${(fastest * worstMul).toFixed(1)} u/s vs player ${PLAYER_SPEED})`);
