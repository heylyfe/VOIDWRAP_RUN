// Runtime for the generated protocol codec: bincode 1.x "fixint, little-endian" layout.
// Lengths are u64, enum tags are u32, Option is a u8 tag, bool is a u8 (0/1).
// Hand-written and stable; the per-type code lives in generated.ts.

const TEXT_ENC = new TextEncoder();
const TEXT_DEC = new TextDecoder("utf-8", { fatal: true });

function intRange(v: number, min: number, max: number, kind: string): number {
  if (!Number.isInteger(v) || v < min || v > max) throw new RangeError(`${kind} out of range: ${v}`);
  return v;
}

export class Writer {
  private buf = new Uint8Array(64);
  private view = new DataView(this.buf.buffer);
  private len = 0;

  /** Grows the buffer if needed and returns the write offset. Always call it BEFORE reading
   *  `this.view`, since growing replaces the view. */
  private reserve(n: number): number {
    const at = this.len;
    if (at + n > this.buf.length) {
      let cap = this.buf.length * 2;
      while (cap < at + n) cap *= 2;
      const next = new Uint8Array(cap);
      next.set(this.buf.subarray(0, at));
      this.buf = next;
      this.view = new DataView(next.buffer);
    }
    this.len += n;
    return at;
  }

  u8(v: number): void {
    const at = this.reserve(1);
    this.view.setUint8(at, intRange(v, 0, 0xff, "u8"));
  }
  u16(v: number): void {
    const at = this.reserve(2);
    this.view.setUint16(at, intRange(v, 0, 0xffff, "u16"), true);
  }
  u32(v: number): void {
    const at = this.reserve(4);
    this.view.setUint32(at, intRange(v, 0, 0xffffffff, "u32"), true);
  }
  i8(v: number): void {
    const at = this.reserve(1);
    this.view.setInt8(at, intRange(v, -0x80, 0x7f, "i8"));
  }
  i16(v: number): void {
    const at = this.reserve(2);
    this.view.setInt16(at, intRange(v, -0x8000, 0x7fff, "i16"), true);
  }
  i32(v: number): void {
    const at = this.reserve(4);
    this.view.setInt32(at, intRange(v, -0x80000000, 0x7fffffff, "i32"), true);
  }
  f32(v: number): void {
    const at = this.reserve(4);
    this.view.setFloat32(at, v, true);
  }
  f64(v: number): void {
    const at = this.reserve(8);
    this.view.setFloat64(at, v, true);
  }
  bool(v: boolean): void {
    this.u8(v ? 1 : 0);
  }
  /** u64 length prefix; JS lengths always fit in 53 bits. */
  len64(n: number): void {
    intRange(n, 0, Number.MAX_SAFE_INTEGER, "length");
    const at = this.reserve(8);
    this.view.setUint32(at, n % 0x100000000, true);
    this.view.setUint32(at + 4, Math.floor(n / 0x100000000), true);
  }
  string(v: string): void {
    const bytes = TEXT_ENC.encode(v);
    this.len64(bytes.length);
    const at = this.reserve(bytes.length); // reserve first: growing replaces this.buf
    this.buf.set(bytes, at);
  }
  vec<T>(items: readonly T[], each: (x: T) => void): void {
    this.len64(items.length);
    for (const x of items) each(x);
  }
  option<T>(v: T | null, each: (x: T) => void): void {
    if (v === null) this.u8(0);
    else {
      this.u8(1);
      each(v);
    }
  }
  finish(): Uint8Array<ArrayBuffer> {
    return this.buf.slice(0, this.len);
  }
}

export class DecodeError extends Error {}

export class Reader {
  private readonly view: DataView;
  private pos = 0;

  constructor(private readonly bytes: Uint8Array) {
    this.view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  }

  private take(n: number): number {
    if (this.pos + n > this.bytes.length) throw new DecodeError(`unexpected end of message at ${this.pos}+${n}`);
    const at = this.pos;
    this.pos += n;
    return at;
  }
  private remaining(): number {
    return this.bytes.length - this.pos;
  }

  u8(): number {
    return this.view.getUint8(this.take(1));
  }
  u16(): number {
    return this.view.getUint16(this.take(2), true);
  }
  u32(): number {
    return this.view.getUint32(this.take(4), true);
  }
  i8(): number {
    return this.view.getInt8(this.take(1));
  }
  i16(): number {
    return this.view.getInt16(this.take(2), true);
  }
  i32(): number {
    return this.view.getInt32(this.take(4), true);
  }
  f32(): number {
    return this.view.getFloat32(this.take(4), true);
  }
  f64(): number {
    return this.view.getFloat64(this.take(8), true);
  }
  bool(): boolean {
    const b = this.u8();
    if (b > 1) throw new DecodeError(`invalid bool ${b}`);
    return b === 1;
  }
  /** Reads a u64 length and rejects anything larger than the bytes left (every element is >= 1 byte). */
  len64(): number {
    const at = this.take(8);
    const lo = this.view.getUint32(at, true);
    const hi = this.view.getUint32(at + 4, true);
    const n = hi * 0x100000000 + lo;
    if (n > this.remaining()) throw new DecodeError(`length ${n} exceeds remaining ${this.remaining()} bytes`);
    return n;
  }
  string(): string {
    const n = this.len64();
    const at = this.take(n);
    return TEXT_DEC.decode(this.bytes.subarray(at, at + n));
  }
  vec<T>(each: () => T): T[] {
    const n = this.len64();
    const out: T[] = [];
    for (let i = 0; i < n; i++) out.push(each());
    return out;
  }
  option<T>(each: () => T): T | null {
    const tag = this.u8();
    if (tag === 0) return null;
    if (tag === 1) return each();
    throw new DecodeError(`invalid option tag ${tag}`);
  }
  end(): void {
    if (this.pos !== this.bytes.length) throw new DecodeError(`${this.bytes.length - this.pos} trailing bytes`);
  }
}
