#!/usr/bin/env node
// Gate check: the original game's / platform's names must never appear in the repo or the
// built client. Patterns are stored encoded so this file does not trip its own check.
import { execSync } from "node:child_process";
import { existsSync, readdirSync, readFileSync, statSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const banned = ["bnVsbHNjYXBl", "cm9ibG94"].map((b) => Buffer.from(b, "base64").toString("utf8"));
const re = new RegExp(banned.join("|"), "i");

const tracked = execSync("git ls-files -co --exclude-standard", { cwd: root, encoding: "utf8" }).split("\n").filter(Boolean);
const extra = [];
const walk = (d) => {
  if (!existsSync(d)) return;
  for (const f of readdirSync(d)) {
    const p = join(d, f);
    if (statSync(p).isDirectory()) walk(p);
    else extra.push(p);
  }
};
walk(join(root, "web/.next/static")); // built client, when present

const hits = [];
for (const f of [...tracked.map((t) => join(root, t)), ...extra]) {
  if (!existsSync(f) || statSync(f).size > 5_000_000) continue;
  if (re.test(f)) hits.push(`${f} (file name)`);
  const text = readFileSync(f, "latin1");
  const m = text.match(re);
  if (m) hits.push(`${f}: "${m[0]}"`);
}
if (hits.length) {
  console.error(`check-names: forbidden names found:\n  ${hits.join("\n  ")}`);
  process.exit(1);
}
console.log(`check-names: ok (${tracked.length + extra.length} files)`);
