// Replace one Rust function (by name) with a block from a response file, using brace matching.
import fs from "node:fs";

const [respPath, targetPath, fnName] = process.argv.slice(2);
if (!respPath || !targetPath || !fnName) {
  console.error("usage: replace-fn.mjs <response.md> <target.rs> <fnName>");
  process.exit(2);
}

const resp = fs.readFileSync(respPath, "utf8");
const marker = "=== FN: " + targetPath + " :: " + fnName + " ===";
const start = resp.indexOf(marker);
if (start < 0) throw new Error("no block for " + fnName);
const bodyStart = resp.indexOf("\n", start) + 1;
const endMarker = resp.indexOf("=== END FN ===", bodyStart);
if (endMarker < 0) throw new Error("no END FN for " + fnName);
const block = resp.slice(bodyStart, endMarker).replace(/\s+$/, "");

const src = fs.readFileSync(targetPath, "utf8");
const lines = src.split("\n");

// Find the declaration line: contains "fn <name>(" or "fn <name><".
const needles = ["fn " + fnName + "(", "fn " + fnName + "<"];
let decl = -1;
for (let i = 0; i < lines.length; i++) {
  const t = lines[i].trimStart();
  if (needles.some((n) => t.startsWith(n) || t.startsWith("pub " + n) || t.startsWith("pub(crate) " + n) || t.startsWith("async " + n))) {
    decl = i;
    break;
  }
}
if (decl < 0) throw new Error("fn " + fnName + " not found in " + targetPath);

// Walk back over doc comments / attributes so they are replaced too.
let first = decl;
while (first > 0) {
  const prev = lines[first - 1].trimStart();
  if (prev.startsWith("#[") || prev.startsWith("///") || prev.startsWith("//")) first--;
  else break;
}

// Brace-match from the declaration line.
let depth = 0, seen = false, last = decl;
for (let i = decl; i < lines.length; i++) {
  for (const ch of lines[i]) {
    if (ch === "{") { depth++; seen = true; }
    else if (ch === "}") { depth--; }
  }
  if (seen && depth === 0) { last = i; break; }
}

const out = [...lines.slice(0, first), ...block.split("\n"), ...lines.slice(last + 1)];
fs.writeFileSync(targetPath, out.join("\n"));
console.log("replaced " + fnName + " (lines " + (first + 1) + "-" + (last + 1) + ") with " + block.split("\n").length + " lines");
