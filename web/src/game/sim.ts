// Client access to the shared Rust movement controller (compiled to /sim.wasm).
// The server runs the identical code natively, so prediction cannot drift by construction.
import {
  decodeControllerState,
  encodeControllerState,
  encodeLevelData,
  encodeTuning,
  type ControllerState,
  type LevelData,
  type TileState,
  type Tuning,
} from "@/protocol/generated";

/** bincode (fixint, little endian) of `Vec<TileState>`: u64 length, then (u16 id, u8 state) per tile. */
export function encodeTiles(tiles: readonly TileState[]): Uint8Array {
  const out = new Uint8Array(8 + tiles.length * 3);
  const view = new DataView(out.buffer);
  view.setUint32(0, tiles.length, true); // high 32 bits stay 0
  tiles.forEach((t, i) => {
    view.setUint16(8 + i * 3, t.id, true);
    view.setUint8(10 + i * 3, t.state);
  });
  return out;
}

/** One fixed physics step of the character controller. */
export interface Stepper {
  loadLevel(level: LevelData): void;
  setState(state: ControllerState): void;
  step(moveX: number, moveZ: number, buttons: number): void;
  getState(): ControllerState;
  /** Movement multipliers from upgrades/curses; must match the server's for exact prediction. */
  setTuning(tuning: Tuning): void;
  /** Platforms that are crumbling/gone (TILE_GONE ones are not solid); must match the server's world. */
  setTiles(tiles: readonly TileState[]): void;
}

interface SimExports {
  memory: WebAssembly.Memory;
  vw_io_buf(len: number): number;
  vw_io_ptr(): number;
  vw_load_level(len: number): number;
  vw_set_state(len: number): number;
  vw_step(moveX: number, moveZ: number, buttons: number): void;
  vw_get_state(): number;
  vw_set_tuning(len: number): number;
  vw_set_tiles(len: number): number;
}

const REQUIRED = [
  "memory",
  "vw_io_buf",
  "vw_io_ptr",
  "vw_load_level",
  "vw_set_state",
  "vw_step",
  "vw_get_state",
  "vw_set_tuning",
  "vw_set_tiles",
];

export class WasmStepper implements Stepper {
  private constructor(private readonly x: SimExports) {}

  static async fromBytes(bytes: BufferSource): Promise<WasmStepper> {
    const { instance } = await WebAssembly.instantiate(bytes, {});
    const missing = REQUIRED.filter((k) => !(k in instance.exports));
    if (missing.length) throw new Error(`sim.wasm is missing exports: ${missing.join(", ")}`);
    return new WasmStepper(instance.exports as unknown as SimExports);
  }

  static async load(url = "/sim.wasm"): Promise<WasmStepper> {
    const res = await fetch(url);
    if (!res.ok) throw new Error(`failed to load ${url}: HTTP ${res.status}`);
    return WasmStepper.fromBytes(await res.arrayBuffer());
  }

  private write(bytes: Uint8Array): number {
    const ptr = this.x.vw_io_buf(bytes.length);
    if (ptr === 0) throw new Error(`sim io buffer refused ${bytes.length} bytes`);
    new Uint8Array(this.x.memory.buffer, ptr, bytes.length).set(bytes);
    return bytes.length;
  }

  loadLevel(level: LevelData): void {
    if (this.x.vw_load_level(this.write(encodeLevelData(level))) !== 0) throw new Error("sim rejected level");
  }

  setState(state: ControllerState): void {
    if (this.x.vw_set_state(this.write(encodeControllerState(state))) !== 0) throw new Error("sim rejected state");
  }

  setTuning(tuning: Tuning): void {
    if (this.x.vw_set_tuning(this.write(encodeTuning(tuning))) !== 0) throw new Error("sim rejected tuning");
  }

  setTiles(tiles: readonly TileState[]): void {
    if (this.x.vw_set_tiles(this.write(encodeTiles(tiles))) !== 0) throw new Error("sim rejected tiles");
  }

  step(moveX: number, moveZ: number, buttons: number): void {
    this.x.vw_step(moveX, moveZ, buttons);
  }

  getState(): ControllerState {
    const len = this.x.vw_get_state();
    const ptr = this.x.vw_io_ptr();
    // Copy out: the next call may reallocate WASM memory.
    return decodeControllerState(new Uint8Array(this.x.memory.buffer, ptr, len).slice());
  }
}
