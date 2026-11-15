// T-151: a module that nothing imports is dead code, and its tests keep it looking alive.
//
// `scene/checker.ts` (466 lines) was the texture-based checkerboard, superseded by the
// world-space procedural checker in `scene/platformShaders.ts`. `shop/shopPanel.ts` (149
// lines) was a framework-free shop panel, superseded by `hud/ShopPanel.tsx`. Both had
// passing tests, because a test that imports a module is still an import -- so the suite
// reported 11 green tests for 615 lines of code no player could reach.
//
// This guard is the general form of the defect T-149 found in the vote UI: the piece
// exists, the tests pass, and nothing renders it.
import { readdirSync, readFileSync, statSync } from "node:fs";
import { dirname, join, relative, sep } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";

const here = dirname(fileURLToPath(import.meta.url));

/**
 * Modules that are deliberately not imported by anything but their own tests.
 *
 * Each entry needs a reason. An entry here is a promise that the module is a
 * measurement or test helper, not a feature that forgot to be wired up.
 */
const TEST_ONLY = new Map<string, string>([
  ["scene/enemyPalette.ts", "CIELAB + delta-E maths; the palette itself is asserted by enemyLook.test.ts"],
]);

/** Every .ts/.tsx under web/src, excluding tests. */
function sources(dir: string, out: string[] = []): string[] {
  for (const entry of readdirSync(dir)) {
    const full = join(dir, entry);
    if (statSync(full).isDirectory()) {
      sources(full, out);
      continue;
    }
    if (!entry.endsWith(".ts") && !entry.endsWith(".tsx")) continue;
    if (entry.endsWith(".test.ts") || entry.endsWith(".test.tsx")) continue;
    out.push(full);
  }
  return out;
}

/**
 * Next.js loads `app/` route files by convention, so they are importers but never
 * need to be imported themselves.
 */
const isRoute = (p: string): boolean => rel(p).startsWith("app/");

const rel = (p: string): string => relative(here, p).split(sep).join("/");

/** The module specifiers a source file imports, as written. */
function specifiers(src: string): string[] {
  const out: string[] = [];
  const marker = "from ";
  let i = src.indexOf(marker);
  while (i !== -1) {
    let j = i + marker.length;
    while (j < src.length && (src[j] === " " || src[j] === "\t")) j += 1;
    const quote = src[j];
    if (quote === '"' || quote === "'") {
      const end = src.indexOf(quote, j + 1);
      if (end !== -1) out.push(src.slice(j + 1, end));
    }
    i = src.indexOf(marker, i + marker.length);
  }
  return out;
}

describe("every module is reachable from something that ships", () => {
  it("has no module imported only by its own test", () => {
    const files = sources(here);
    const texts = new Map(files.map((f) => [f, readFileSync(f, "utf8")]));
    const orphans: string[] = [];

    for (const file of files) {
      if (isRoute(file)) continue;
      const base = rel(file).replace(/\.tsx?$/, "").split("/").pop() as string;
      // A module is reachable when some OTHER non-test file imports it by path.
      const imported = files.some((other) => {
        if (other === file) return false;
        const src = texts.get(other) as string;
        return specifiers(src).some((spec) => spec === base || spec.endsWith("/" + base));
      });
      if (imported) continue;
      if (TEST_ONLY.has(rel(file))) continue;
      orphans.push(rel(file));
    }

    expect(orphans).toEqual([]);
  });

  it("keeps the test-only allowlist honest", () => {
    // An allowlist entry for a module that no longer exists is a stale promise.
    const files = new Set(sources(here).filter((f) => !isRoute(f)).map(rel));
    const stale = [...TEST_ONLY.keys()].filter((p) => !files.has(p));
    expect(stale).toEqual([]);
  });
});
