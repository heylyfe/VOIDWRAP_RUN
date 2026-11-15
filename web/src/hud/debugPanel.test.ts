import { describe, expect, it } from "vitest";
import {
  DEBUG_SECTIONS,
  EMPTY_PANEL,
  MAX_LOG,
  appendLog,
  applyResult,
  buildArgs,
  commandIds,
  defaultValues,
  findCommand,
  isRevealEvent,
  lockLabel,
  panelVisible,
} from "./debugPanel";

describe("the manifest mirror", () => {
  it("covers every command in design/DEBUG_COMMANDS.md", () => {
    // Pinned: a command added to the manifest without a panel entry (or the reverse)
    // fails here rather than silently 400ing at runtime.
    expect(commandIds().sort()).toEqual(
      [
        "despawn_enemy",
        "give_curse",
        "give_upgrade",
        "grant_gift",
        "grant_gold",
        "remove_gift",
        "set_fog",
        "set_phase",
        "spawn_enemy",
        "state",
        "switch_level",
        "take_curse",
        "take_upgrade",
        "teleport",
        "trigger_altar",
      ].sort(),
    );
  });

  it("has the five sections the ticket asks for", () => {
    expect(DEBUG_SECTIONS.map((s) => s.id)).toEqual(["enemies", "level", "economy", "world", "state"]);
  });

  it("gives every command a label and a hint", () => {
    for (const section of DEBUG_SECTIONS) {
      expect(section.commands.length).toBeGreaterThan(0);
      for (const command of section.commands) {
        expect(command.label.length).toBeGreaterThan(0);
        expect(command.hint.length).toBeGreaterThan(0);
      }
    }
  });

  it("marks set_fog as the only client-owned command", () => {
    const client = DEBUG_SECTIONS.flatMap((s) => s.commands).filter((c) => c.owner === "client");
    expect(client.map((c) => c.id)).toEqual(["set_fog"]);
  });

  it("finds a command by id and returns undefined for an unknown one", () => {
    expect(findCommand("grant_gold")?.label).toBe("Grant gold");
    expect(findCommand("summon_dragon")).toBeUndefined();
  });
});

describe("panelVisible", () => {
  it("shows the panel for zero or one player", () => {
    expect(panelVisible(0)).toBe(true);
    expect(panelVisible(1)).toBe(true);
  });

  it("hides the panel as soon as a second player joins", () => {
    expect(panelVisible(2)).toBe(false);
    expect(panelVisible(8)).toBe(false);
  });
});

describe("defaultValues", () => {
  it("starts booleans at false and everything else empty", () => {
    const spec = findCommand("grant_gift")!;
    expect(defaultValues(spec)).toEqual({ count: "", golden: "false" });
  });

  it("is empty for a command with no args", () => {
    expect(defaultValues(findCommand("state")!)).toEqual({});
  });
});

describe("buildArgs", () => {
  it("omits empty optional args instead of sending them", () => {
    const spec = findCommand("spawn_enemy")!;
    const built = buildArgs(spec, { id: "lure", count: "", x: "", y: "", z: "" });
    expect(built).toEqual({ args: '{"id":"lure"}' });
  });

  it("rejects a missing required arg before the round trip", () => {
    const spec = findCommand("spawn_enemy")!;
    expect(buildArgs(spec, { id: "  ", count: "", x: "", y: "", z: "" })).toEqual({
      error: "Registry id is required",
    });
  });

  it("coerces ints, floats and booleans", () => {
    const spec = findCommand("grant_gift")!;
    expect(buildArgs(spec, { count: "3", golden: "true" })).toEqual({ args: '{"count":3,"golden":true}' });
    expect(buildArgs(spec, { count: "3", golden: "false" })).toEqual({ args: '{"count":3,"golden":false}' });
  });

  it("rejects a non-numeric number and a fractional int", () => {
    expect(buildArgs(findCommand("grant_gold")!, { amount: "lots" })).toEqual({
      error: "Amount (negative subtracts) must be a number",
    });
    expect(buildArgs(findCommand("grant_gold")!, { amount: "1.5" })).toEqual({
      error: "Amount (negative subtracts) must be a whole number",
    });
  });

  it("accepts a negative amount, which subtracts", () => {
    expect(buildArgs(findCommand("grant_gold")!, { amount: "-250" })).toEqual({ args: '{"amount":-250}' });
  });

  it("sends an empty object for a command with no args", () => {
    expect(buildArgs(findCommand("state")!, {})).toEqual({ args: "{}" });
  });
});

describe("appendLog", () => {
  it("numbers entries monotonically", () => {
    const log = appendLog(appendLog([], { command: "a", args: "{}", ok: true, message: "one" }), {
      command: "b",
      args: "{}",
      ok: false,
      message: "two",
    });
    expect(log.map((e) => e.seq)).toEqual([1, 2]);
    expect(log.map((e) => e.message)).toEqual(["one", "two"]);
  });

  it("keeps only the newest MAX_LOG lines", () => {
    let log: ReturnType<typeof appendLog> = [];
    for (let i = 0; i < MAX_LOG + 10; i += 1) {
      log = appendLog(log, { command: "c", args: "{}", ok: true, message: `line ${i}` });
    }
    expect(log).toHaveLength(MAX_LOG);
    expect(log.at(-1)?.message).toBe(`line ${MAX_LOG + 9}`);
    expect(log[0]?.message).toBe("line 10");
  });
});

describe("applyResult", () => {
  it("unlocks on an ok answer", () => {
    const next = applyResult(EMPTY_PANEL, "grant_gold", '{"amount":1}', true, "currency is now 1");
    expect(next.unlocked).toBe(true);
    expect(next.log).toHaveLength(1);
  });

  it("does not unlock on a refusal", () => {
    const next = applyResult(EMPTY_PANEL, "grant_gold", '{"amount":1}', false, "debug commands are not enabled");
    expect(next.unlocked).toBe(false);
    expect(next.log[0]?.ok).toBe(false);
    expect(next.log[0]?.message).toBe("debug commands are not enabled");
  });

  it("stays unlocked after a later refusal", () => {
    const unlocked = applyResult(EMPTY_PANEL, "state", "{}", true, "level=1");
    const next = applyResult(unlocked, "grant_gold", '{"amount":1}', false, "debug commands are singleplayer only");
    expect(next.unlocked).toBe(true);
    expect(next.log).toHaveLength(2);
  });
});

describe("lockLabel", () => {
  it("reads locked until the server says otherwise", () => {
    expect(lockLabel(EMPTY_PANEL)).toBe("Locked");
    expect(lockLabel({ ...EMPTY_PANEL, unlocked: true })).toContain("unlocked");
  });
});

describe("isRevealEvent", () => {
  it("needs ctrl, shift and D together", () => {
    expect(isRevealEvent({ ctrlKey: true, shiftKey: true, key: "D" })).toBe(true);
    expect(isRevealEvent({ ctrlKey: true, shiftKey: true, key: "d" })).toBe(true);
    expect(isRevealEvent({ ctrlKey: false, shiftKey: true, key: "D" })).toBe(false);
    expect(isRevealEvent({ ctrlKey: true, shiftKey: false, key: "D" })).toBe(false);
    expect(isRevealEvent({ ctrlKey: true, shiftKey: true, key: "E" })).toBe(false);
  });
});
