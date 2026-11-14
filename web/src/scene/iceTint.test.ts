import { describe, expect, it } from "vitest";
import { iceTint } from "./iceTint";

describe("iceTint", () => {
  it("returns a lowercase #rrggbb colour", () => {
    expect(iceTint("#a0b0c0")).toMatch(/^#[0-9a-f]{6}$/);
  });

  it("accepts 3-digit hex", () => {
    expect(iceTint("#abc")).toMatch(/^#[0-9a-f]{6}$/);
  });

  it("shifts the hue towards blue (b - r increases)", () => {
    const out = iceTint("#808080");
    const bIn = 0x80 - 0x80;
    const rOut = parseInt(out.slice(1, 3), 16);
    const bOut = parseInt(out.slice(5, 7), 16);
    expect(bOut - rOut).toBeGreaterThan(bIn);
  });

  it("is deterministic", () => {
    expect(iceTint("#123456")).toBe(iceTint("#123456"));
  });

  it("throws on invalid input", () => {
    expect(() => iceTint("not-a-colour")).toThrow(/iceTint/);
  });
});
