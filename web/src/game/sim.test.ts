import { existsSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, it } from "vitest";
import type { LevelData } from "@/protocol/generated";
import { encodeTiles, WasmStepper } from "./sim";

const wasmPath = join(__dirname, "../../public/sim.wasm");

// `just verify` builds sim.wasm before running the web tests, so this always runs there.
describe.runIf(existsSync(wasmPath))("WasmStepper (real sim.wasm)", () => {
  it("lands on a platform and is deterministic across resets", async () => {
    const sim = await WasmStepper.fromBytes(readFileSync(wasmPath));
    const level: LevelData = {
      level: 0,
      seed: 0,
      spawn: { x: 0, y: 0, z: 0 },
      beacon: { x: 0, y: 0, z: 0 },
      killY: -30,
      platforms: [{ id: 0, kind: 0, bounds: { min: { x: -5, y: -1, z: -5 }, max: { x: 5, y: 0, z: 5 } } }],
      gifts: [],
      event: 0,
      altars: [],
    };
    sim.loadLevel(level);
    const start = { ...sim.getState(), pos: { x: 0, y: 2, z: 0 } };
    sim.setState(start);
    for (let i = 0; i < 120; i++) sim.step(0, 0, 0);
    const landed = sim.getState();
    expect(landed.grounded).toBe(true);
    expect(landed.pos.y).toBe(0);
    sim.setState(start);
    for (let i = 0; i < 120; i++) sim.step(0, 0, 0);
    expect(sim.getState()).toEqual(landed);
    // A gone tile is not solid: the player falls through; restoring it makes it solid again.
    sim.setTiles([{ id: 0, state: 2 }]);
    sim.setState(start);
    for (let i = 0; i < 120; i++) sim.step(0, 0, 0);
    expect(sim.getState().pos.y).toBeLessThan(-1);
    sim.setTiles([]);
    sim.setState(start);
    for (let i = 0; i < 120; i++) sim.step(0, 0, 0);
    expect(sim.getState()).toEqual(landed);
  });
});

describe("encodeTiles", () => {
  it("matches bincode Vec<TileState> (u64 length, u16 id, u8 state)", () => {
    expect(Array.from(encodeTiles([{ id: 3, state: 1 }, { id: 258, state: 2 }]))).toEqual([
      2, 0, 0, 0, 0, 0, 0, 0, 3, 0, 1, 2, 1, 2,
    ]);
    expect(Array.from(encodeTiles([]))).toEqual([0, 0, 0, 0, 0, 0, 0, 0]);
  });
});
