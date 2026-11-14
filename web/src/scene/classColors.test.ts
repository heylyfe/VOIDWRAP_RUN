import { describe, expect, it } from "vitest";
import { CLASS_BLINKER, CLASS_GLIDER, CLASS_GRAPPLER, CLASS_STRIKER } from "@/protocol/generated";
import { classColor } from "./classColors";

describe("classColor", () => {
  it("returns hex colours for the four known classes", () => {
    for (const cls of [CLASS_STRIKER, CLASS_GRAPPLER, CLASS_BLINKER, CLASS_GLIDER]) {
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
