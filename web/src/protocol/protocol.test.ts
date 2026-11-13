import { readFileSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, it } from "vitest";
import { DecodeError } from "./codec";
import { CODECS, decodeClientMsg, decodeServerMsg, encodeClientMsg, encodeServerMsg } from "./generated";

const protocolDir = join(__dirname, "../../../protocol");
type FieldList = [string, unknown][];
const schema = JSON.parse(readFileSync(join(protocolDir, "schema.json"), "utf8")) as {
  fixtures: { name: string; type: keyof typeof CODECS; value: unknown }[];
  types: { name: string; struct?: FieldList; enum?: [string, FieldList][] }[];
};

const camel = (s: string) => s.replace(/_([a-z0-9])/g, (_m, c: string) => c.toUpperCase());
const typeByName = new Map(schema.types.map((t) => [t.name, t]));

/** Convert serde's externally-tagged JSON form into the generated TS shape. */
function fromSerde(ty: unknown, v: unknown): unknown {
  if (Array.isArray(ty)) {
    if (ty[0] === "vec") return (v as unknown[]).map((x) => fromSerde(ty[1], x));
    return v === null ? null : fromSerde(ty[1], v);
  }
  const t = typeByName.get(ty as string);
  if (!t) return ty === "f32" && typeof v === "number" ? Math.fround(v) : v; // primitive; f32 rounds like the wire
  const fields = (list: FieldList, body: unknown) =>
    Object.fromEntries(list.map(([f, fty]) => [camel(f), fromSerde(fty, (body as Record<string, unknown>)[f])]));
  if (t.struct) return fields(t.struct, v);
  const [tag, body] = typeof v === "string" ? [v, {}] : Object.entries(v as object)[0]!;
  const variant = t.enum!.find(([n]) => n === tag)!;
  return { type: tag, ...fields(variant[1], body) };
}

const hex = (b: Uint8Array) => Array.from(b, (x) => x.toString(16).padStart(2, "0")).join("");
const unhex = (s: string) => new Uint8Array(s.trim().match(/../g)!.map((h) => parseInt(h, 16)));

describe("golden fixtures shared with Rust", () => {
  for (const fx of schema.fixtures) {
    it(fx.name, () => {
      const bytes = unhex(readFileSync(join(protocolDir, "fixtures", `${fx.name}.hex`), "utf8"));
      const codec = CODECS[fx.type] as { encode(v: unknown): Uint8Array; decode(b: Uint8Array): unknown };
      const expected = fromSerde(fx.type, fx.value);
      expect(codec.decode(bytes)).toEqual(expected);
      expect(hex(codec.encode(expected))).toBe(hex(bytes));
    });
  }
});

describe("codec hardening", () => {
  it("rejects trailing bytes, truncation and absurd lengths", () => {
    const ok = encodeClientMsg({ type: "Ping", t: 5 });
    expect(() => decodeClientMsg(new Uint8Array([...ok, 0]))).toThrow(DecodeError);
    expect(() => decodeClientMsg(ok.slice(0, 3))).toThrow(DecodeError);
    const huge = new Uint8Array([1, 0, 0, 0, 255, 255, 255, 255, 255, 255, 0, 0]);
    expect(() => decodeClientMsg(huge)).toThrow(DecodeError);
  });

  it("survives buffer growth in the middle of strings and vectors", () => {
    const long = "void".repeat(50);
    const msg = { type: "Error" as const, code: 1, message: long };
    const back = decodeServerMsg(encodeServerMsg(msg));
    expect(back).toEqual(msg);
    const many = { type: "Inputs" as const, frames: Array.from({ length: 30 }, (_, i) => ({ seq: i, moveX: -1, moveZ: 1, buttons: 0 })) };
    expect(decodeClientMsg(encodeClientMsg(many))).toEqual(many);
  });

  it("refuses to encode out-of-range integers", () => {
    expect(() => encodeClientMsg({ type: "Inputs", frames: [{ seq: 1, moveX: 200, moveZ: 0, buttons: 0 }] })).toThrow(
      RangeError,
    );
  });
});
