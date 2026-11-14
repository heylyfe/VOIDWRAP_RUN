import { describe, expect, it } from "vitest";
import {
  buildFragmentHeader,
  buildPadUniforms,
  buildSurfaceUniforms,
  buildVertexHeader,
  GLSL_WARN_FRET,
  PAD_FRAGMENT_COLOR,
  PAD_FRAGMENT_EMISSIVE,
  SURFACE_FRAGMENT_COLOR,
  SURFACE_FRAGMENT_EMISSIVE,
  uniformDeclarations,
  VERTEX_BEGIN,
} from "./platformShaders";
import type { UniformMap } from "./platformShaders";

// Every identifier starting with `u`/`v` followed by an uppercase letter that is
// USED in a shader must also be DECLARED in it (uniform/varying/attribute).
const IDENTIFIER = /\b[uv][A-Z][A-Za-z0-9_]*\b/g;
const DECLARATION = /(?:uniform|varying|attribute)\s+\w+\s+([A-Za-z0-9_]+)\s*;/g;
const UNIFORM_DECLARATION = /uniform\s+\w+\s+([A-Za-z0-9_]+)\s*;/g;
const VARYING_DECLARATION = /varying\s+(\w+)\s+(\w+)\s*;/g;

const timeUniform = { value: 0 };
const palette = { light: "#9fb6cc", side: "#8a8a8a", dark: "#1c2430", edge: "#ffd27a" };

const surfaceUniforms = buildSurfaceUniforms(palette, false, false, timeUniform);
const crumbleUniforms = buildSurfaceUniforms(palette, false, true, timeUniform);
const icyUniforms = buildSurfaceUniforms(palette, true, false, timeUniform);
const padUniforms = buildPadUniforms(timeUniform);

interface Case {
  name: string;
  uniforms: UniformMap;
  colorChunk: string;
  emissiveChunk: string;
}

const cases: Case[] = [
  { name: "surface", uniforms: surfaceUniforms, colorChunk: SURFACE_FRAGMENT_COLOR, emissiveChunk: SURFACE_FRAGMENT_EMISSIVE },
  { name: "crumble", uniforms: crumbleUniforms, colorChunk: SURFACE_FRAGMENT_COLOR, emissiveChunk: SURFACE_FRAGMENT_EMISSIVE },
  { name: "icy", uniforms: icyUniforms, colorChunk: SURFACE_FRAGMENT_COLOR, emissiveChunk: SURFACE_FRAGMENT_EMISSIVE },
  { name: "pad", uniforms: padUniforms, colorChunk: PAD_FRAGMENT_COLOR, emissiveChunk: PAD_FRAGMENT_EMISSIVE },
];

const vertexSource = [buildVertexHeader(), VERTEX_BEGIN].join("\n");
const fragmentOf = (c: Case): string =>
  [buildFragmentHeader(c.uniforms), c.colorChunk, c.emissiveChunk].join("\n");

function usedIdentifiers(source: string): Set<string> {
  return new Set(source.match(IDENTIFIER) ?? []);
}

function declaredIdentifiers(source: string): Set<string> {
  return new Set([...source.matchAll(DECLARATION)].map((m) => m[1] ?? ""));
}

function varyingTypes(source: string): Map<string, string> {
  const map = new Map<string, string>();
  for (const m of source.matchAll(VARYING_DECLARATION)) map.set(m[2] ?? "", m[1] ?? "");
  return map;
}

describe("platform shader chunks", () => {
  it("vertex shader declares the per-instance attribute", () => {
    expect(vertexSource).toMatch(/attribute\s+vec4\s+aState\s*;/);
  });

  for (const c of cases) {
    it(`${c.name}: vertex assembly declares every used u/v identifier`, () => {
      const declared = declaredIdentifiers(vertexSource);
      const missing = [...usedIdentifiers(vertexSource)].filter((id) => !declared.has(id));
      expect(missing).toEqual([]);
    });

    it(`${c.name}: fragment assembly declares every used u/v identifier`, () => {
      const source = fragmentOf(c);
      const declared = declaredIdentifiers(source);
      const missing = [...usedIdentifiers(source)].filter((id) => !declared.has(id));
      expect(missing).toEqual([]);
    });

    it(`${c.name}: every shader.uniforms key is declared, and vice versa`, () => {
      const source = fragmentOf(c);
      const declaredUniforms = [...source.matchAll(UNIFORM_DECLARATION)].map((m) => m[1]).sort();
      expect(declaredUniforms).toEqual(Object.keys(c.uniforms).sort());
    });

    it(`${c.name}: uniform declaration types match their values`, () => {
      const header = buildFragmentHeader(c.uniforms);
      for (const [name, uniform] of Object.entries(c.uniforms)) {
        const expected = typeof uniform.value === "number" ? "float" : "vec3";
        expect(header).toMatch(new RegExp(`uniform ${expected} ${name};`));
      }
    });

    it(`${c.name}: varyings are declared with the same types as the vertex stage`, () => {
      expect(varyingTypes(fragmentOf(c))).toEqual(varyingTypes(vertexSource));
    });

    it(`${c.name}: GLSL_WARN_FRET identifiers are declared`, () => {
      const declared = declaredIdentifiers(fragmentOf(c));
      for (const id of usedIdentifiers(GLSL_WARN_FRET)) {
        expect(declared.has(id)).toBe(true);
      }
    });
  }

  it("declares all five varyings in the vertex stage", () => {
    expect(varyingTypes(vertexSource).size).toBe(5);
  });

  it("uniformDeclarations is deterministic and sorted", () => {
    const a = uniformDeclarations(surfaceUniforms);
    const b = uniformDeclarations(surfaceUniforms);
    expect(a).toBe(b);
    const names = [...a.matchAll(UNIFORM_DECLARATION)].map((m) => m[1]);
    expect(names).toEqual([...names].sort());
  });
});
