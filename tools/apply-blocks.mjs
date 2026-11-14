import { readFileSync, writeFileSync } from "node:fs";

const [file, ...allows] = process.argv.slice(2);
const text = readFileSync(file, "utf8");
const re = /^=== FN: (.+?) :: (.+?) ===\r?\n([\s\S]*?)^=== END FN ===\s*$/gm;
const blocks = [...text.matchAll(re)].map((m) => ({ path: m[1].trim(), name: m[2].trim(), body: m[3].replace(/\s+$/, "") }));

const cache = new Map();
const read = (p) => cache.get(p) ?? readFileSync(p, "utf8");

function esc(s) {
  let out = "";
  for (const ch of s) out += ".*+?^${}()|[]\\".includes(ch) ? "\\" + ch : ch;
  return out;
}

function locate(src, name) {
  const lines = src.split("\n");
  const pats = [];
  if (name.startsWith("impl ")) pats.push(new RegExp("^\\s*" + esc(name) + "\\b"));
  const last = name.includes("::") ? name.split("::").pop() : name;
  pats.push(new RegExp("^(\\s*)(pub(\\([a-z]+\\))? )?(async )?(unsafe )?(fn|struct|enum|const|static|type|impl|mod|trait) " + esc(last) + "\\b"));
  for (const head of pats) {
    const at = lines.findIndex((l) => head.test(l));
    if (at < 0) continue;
    let start = at;
    while (start > 0 && /^\s*(\/\/\/|#\[|#!\[)/.test(lines[start - 1])) start--;
    let depth = 0;
    let seen = false;
    for (let i = at; i < lines.length; i++) {
      const code = lines[i].replace(/\/\/.*$/, "").replace(/"(?:[^"\\]|\\.)*"/g, '""').replace(/'(?:[^'\\]|\\.)'/g, "' '");
      for (const ch of code) {
        if (ch === "{") { depth++; seen = true; }
        else if (ch === "}") depth--;
      }
      if (!seen && code.trimEnd().endsWith(";")) return [start, i + 1];
      if (seen && depth === 0) return [start, i + 1];
    }
  }
  return null;
}

for (const b of blocks) {
  if (!allows.some((a) => b.path.startsWith(a))) { console.error(`skip (not allowed): ${b.path}`); continue; }
  if (b.name.includes("placement note")) { console.log(`skip note: ${b.name}`); continue; }
  let src = read(b.path);
  const where = locate(src, b.name);
  if (where) {
    const lines = src.split("\n");
    lines.splice(where[0], where[1] - where[0], ...b.body.split("\n"));
    src = lines.join("\n");
    console.log(`replace ${b.path} :: ${b.name}`);
  } else {
    src = `${src.replace(/\s*$/, "\n")}\n${b.body}\n`;
    console.log(`append  ${b.path} :: ${b.name}`);
  }
  cache.set(b.path, src);
}
for (const [p, src] of cache) writeFileSync(p, src);
