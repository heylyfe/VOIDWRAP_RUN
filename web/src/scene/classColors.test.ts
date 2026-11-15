import { readFileSync, readdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";
import { CLASS_BULWARK, CLASS_GLIDER, CLASS_GRAPPLER, CLASS_STRIKER } from "@/protocol/generated";
import { CLASS_COLORS, classColor } from "./classColors";

const here = dirname(fileURLToPath(import.meta.url));

describe("classColor", () => {
  it("returns hex colours for the four known classes", () => {
    for (const cls of [CLASS_STRIKER, CLASS_GRAPPLER, CLASS_BULWARK, CLASS_GLIDER]) {
      expect(classColor(cls)).toMatch(/^#[0-9a-f]{6}$/i);
    }
  });

  it("is deterministic per class", () => {
    expect(classColor(CLASS_GRAPPLER)).toBe(classColor(CLASS_GRAPPLER));
  });

  it("falls back to white for unknown classes", () => {
    expect(classColor(999)).toBe("#ffffff");
  });
});

describe("class colours reach the screen", () => {
  // T-147: `CLASS_COLORS[CLASS_STRIKER]` was defined and never read -- the dash streak
  // used `theme.accent`, so the striker's ability visual was magenta on one level, red on
  // another and teal on a third while the other three classes kept a stable colour. A
  // colour in this table that no renderer asks for is dead code, so assert each one is
  // consumed.
  const sources = (): string[] => {
    const out: string[] = [];
    const walk = (dir: string) => {
      for (const entry of readdirSync(dir, { withFileTypes: true })) {
        const full = join(dir, entry.name);
        if (entry.isDirectory()) walk(full);
        else if (/\.tsx?$/.test(entry.name) && !/\.test\.tsx?$/.test(entry.name)) out.push(full);
      }
    };
    walk(join(here, ".."));
    return out;
  };

  it("has a renderer for every class colour", () => {
    const names: Record<number, string> = {
      [CLASS_STRIKER]: "CLASS_STRIKER",
      [CLASS_GRAPPLER]: "CLASS_GRAPPLER",
      [CLASS_BULWARK]: "CLASS_BULWARK",
      [CLASS_GLIDER]: "CLASS_GLIDER",
    };
    const text = sources().map((f) => readFileSync(f, "utf8")).join("\n");
    const unread = Object.keys(CLASS_COLORS)
      .map(Number)
      .filter((cls) => !text.includes(`classColor(${names[cls]})`))
      .map((cls) => `${names[cls]} ${classColor(cls)}`);
    expect(unread).toEqual([]);
  });
});
