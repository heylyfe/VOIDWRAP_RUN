#!/usr/bin/env node
// Applies function-level edits from a subordinate response, for files too large to resend whole.
// OVERSEER only, after reading the response. Paths must fall under an --allow prefix.
//
//   node tools/apply-fns.mjs tickets/T-055.r1.response.md --allow server/crates/sim/src/ [--dry-run]
//
// Blocks understood:
//   === FN: <path> :: <name> ===        replace the item named <name> (fn/struct/enum/const/impl header line)
//   <complete new item, including its doc comments and attributes>
//   === END FN ===
//   === APPEND: <path> ===              append text at the end of the file (new items, new test modules)
//   ...
//   === END APPEND ===
// An item is located by the first line matching `^(\s*)(pub(\(crate\))? )?(async )?(fn|struct|enum|const|static|type|impl|mod) <name>\b`;
// its extent runs to the matching closing brace (or the terminating `;`), plus the doc comments/attributes above it.
import { readFileSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { checkPath, stripFence } from "./apply-response.mjs";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");

export function parseBlocks(text) {
  const blocks = [];
  const fnRe = /^=== FN: (.+?) :: (.+?) ===\r?\n([\s\S]*?)^=== END FN ===\s*$/gm;
  for (const m of text.matchAll(fnRe)) blocks.push({ kind: "fn", path: m[1].trim(), name: m[2].trim(), body: stripFence(m[3]) });
  const apRe = /^=== APPEND: (.+?) ===\r?\n([\s\S]*?)^=== END APPEND ===\s*$/gm;
  for (const m of text.matchAll(apRe)) blocks.push({ kind: "append", path: m[1].trim(), body: stripFence(m[2]) });
  return blocks;
}

/** Find [start, end) of the item `name` in `src` (line offsets), including leading docs/attributes. */
export function locateItem(src, name) {
  const lines = src.split("\n");
  const head = new RegExp(`^(\\s*)(pub(\\([a-z]+\\))? )?(async )?(unsafe )?(fn|struct|enum|const|static|type|impl|mod|trait) ${name.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")}\\b`);
  const at = lines.findIndex((l) => head.test(l));
  if (at < 0) return null;
  let start = at;
  while (start > 0 && /^\s*(\/\/\/|#\[|#!\[)/.test(lines[start - 1])) start--;
  // Walk forward counting braces (ignores braces in strings/chars/comments well enough for our code).
  let depth = 0;
  let seenBrace = false;
  for (let i = at; i < lines.length; i++) {
    const code = lines[i].replace(/\/\/.*$/, "").replace(/"(?:[^"\\]|\\.)*"/g, '""').replace(/'(?:[^'\\]|\\.)'/g, "' '");
    for (const ch of code) {
      if (ch === "{") { depth++; seenBrace = true; }
      else if (ch === "}") depth--;
    }
    if (!seenBrace && code.trimEnd().endsWith(";")) return [start, i + 1];
    if (seenBrace && depth === 0) return [start, i + 1];
  }
  return null;
}

function main() {
  const args = process.argv.slice(2);
  const file = args[0];
  const allow = [];
  for (let i = 1; i < args.length; i++) if (args[i] === "--allow") allow.push(args[++i]);
  const dry = args.includes("--dry-run");
  if (!file || allow.length === 0) {
    console.error("usage: apply-fns.mjs <response.md> --allow <prefix> [--allow ...] [--dry-run]");
    process.exit(2);
  }
  const blocks = parseBlocks(readFileSync(file, "utf8"));
  if (blocks.length === 0) { console.error("apply-fns: no FN/APPEND blocks found"); process.exit(1); }
  const cache = new Map();
  const read = (p) => cache.get(p) ?? readFileSync(join(root, p), "utf8");
  for (const b of blocks) {
    const bad = checkPath(b.path, allow);
    if (bad) { console.error(`apply-fns: ${bad}`); process.exit(1); }
    let src = read(b.path);
    if (b.kind === "append") {
      src = `${src.replace(/\s*$/, "\n")}\n${b.body.replace(/\s*$/, "\n")}`;
      console.log(`append ${b.path} (+${b.body.split("\n").length} lines)`);
    } else {
      const lines = src.split("\n");
      const where = locateItem(src, b.name);
      if (!where) { console.error(`apply-fns: '${b.name}' not found in ${b.path}`); process.exit(1); }
      const [s, e] = where;
      lines.splice(s, e - s, ...b.body.replace(/\s*$/, "").split("\n"));
      src = lines.join("\n");
      console.log(`replace ${b.path} :: ${b.name} (lines ${s + 1}-${e})`);
    }
    cache.set(b.path, src);
  }
  if (!dry) for (const [p, src] of cache) writeFileSync(join(root, p), src);
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) main();
