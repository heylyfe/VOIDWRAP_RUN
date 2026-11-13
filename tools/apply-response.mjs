#!/usr/bin/env node
// Applies the FILE blocks of a subordinate response to the working tree — OVERSEER only,
// after reading the response. Every path must fall under one of the --allow prefixes.
//
//   node tools/apply-response.mjs tickets/T-010.response.md --allow server/crates/sim/src/ [--allow ...] [--dry-run]
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join, normalize, sep } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");

export function parseFiles(text) {
  const files = [];
  const re = /^=== FILE: (.+?) ===\r?\n([\s\S]*?)^=== END FILE ===\s*$/gm;
  for (const m of text.matchAll(re)) files.push({ path: m[1].trim(), content: stripFence(m[2]) });
  return files;
}

/** Models sometimes wrap a FILE body in one markdown fence (```ts ... ```); remove exactly that wrapper. */
export function stripFence(content) {
  const m = content.match(/^```[\w.+-]*\r?\n([\s\S]*?)\r?\n```\s*$/);
  return m ? `${m[1]}\n` : content;
}

export function checkPath(path, allow) {
  const norm = normalize(path).split(sep).join("/");
  if (norm.startsWith("/") || /^[a-zA-Z]:/.test(norm) || norm.split("/").includes("..")) return `unsafe path ${path}`;
  if (!allow.some((a) => norm === a || norm.startsWith(a.endsWith("/") ? a : `${a}/`))) return `path ${path} not allowed`;
  return null;
}

function main() {
  const argv = process.argv.slice(2);
  const allow = [];
  let response;
  let dry = false;
  for (let i = 0; i < argv.length; i++) {
    if (argv[i] === "--allow") allow.push(argv[++i]);
    else if (argv[i] === "--dry-run") dry = true;
    else response = argv[i];
  }
  if (!response || allow.length === 0) {
    console.error("usage: apply-response.mjs <response.md> --allow <prefix> [--allow ...] [--dry-run]");
    process.exit(2);
  }
  const files = parseFiles(readFileSync(response, "utf8"));
  if (files.length === 0) {
    console.error("no FILE blocks found");
    process.exit(1);
  }
  const errors = files.map((f) => checkPath(f.path, allow)).filter(Boolean);
  if (errors.length) {
    console.error(errors.join("\n"));
    process.exit(1);
  }
  for (const f of files) {
    console.log(`${dry ? "would write" : "write"} ${f.path} (${f.content.split("\n").length} lines)`);
    if (!dry) {
      const abs = join(root, f.path);
      mkdirSync(dirname(abs), { recursive: true });
      writeFileSync(abs, f.content);
    }
  }
}

if (process.argv[1] && normalize(process.argv[1]) === normalize(fileURLToPath(import.meta.url))) main();
