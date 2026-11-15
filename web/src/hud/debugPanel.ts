/**
 * Tester panel logic (T-111).
 *
 * The panel is a thin client over the frozen command manifest in
 * `design/DEBUG_COMMANDS.md`. Everything here is pure so it can be tested without a
 * DOM: the React component in `DebugPanel.tsx` only renders this and forwards events.
 *
 * The panel never decides whether a command is allowed. It sends the code to the
 * server and shows the server's answer; a client-side check would be a lie, because
 * the server is the only thing that knows whether the route is open and how many
 * players are connected.
 */

export type ArgKind = "string" | "int" | "float" | "bool";

export interface ArgSpec {
  key: string;
  kind: ArgKind;
  required: boolean;
  label: string;
  /** Known values, for a string arg that names a registry entry. */
  choices?: string[];
}

export interface CommandSpec {
  id: string;
  label: string;
  /** Where the effect lands. `client` commands are gated by the server, applied locally. */
  owner: "server" | "client";
  args: ArgSpec[];
  hint: string;
}

export interface SectionSpec {
  id: string;
  label: string;
  commands: CommandSpec[];
}

const str = (key: string, label: string, required = true, choices?: string[]): ArgSpec => ({
  key,
  kind: "string",
  required,
  label,
  choices,
});
const int = (key: string, label: string, required = true): ArgSpec => ({
  key,
  kind: "int",
  required,
  label,
});
const flt = (key: string, label: string, required = true): ArgSpec => ({
  key,
  kind: "float",
  required,
  label,
});
const bool = (key: string, label: string): ArgSpec => ({
  key,
  kind: "bool",
  required: false,
  label,
});

/**
 * The manifest, mirrored for the UI. Ids and arg keys must match
 * `design/DEBUG_COMMANDS.md` exactly; `debugPanel.test.ts` pins the command ids so a
 * drift between the two shows up as a test failure rather than a silent 400.
 */
export const DEBUG_SECTIONS: SectionSpec[] = [
  {
    id: "enemies",
    label: "Enemies",
    commands: [
      {
        id: "spawn_enemy",
        label: "Spawn enemy",
        owner: "server",
        args: [str("id", "Registry id"), int("count", "Count", false), flt("x", "X", false), flt("y", "Y", false), flt("z", "Z", false)],
        hint: "Spawns at the level spawn point unless x/y/z are all given.",
      },
      {
        id: "despawn_enemy",
        label: "Despawn all of type",
        owner: "server",
        args: [str("id", "Registry id")],
        hint: "Removes every live enemy of that registry id.",
      },
    ],
  },
  {
    id: "level",
    label: "Level",
    commands: [
      {
        id: "switch_level",
        label: "Jump to level",
        owner: "server",
        args: [int("level", "Level"), int("seed", "Seed", false)],
        hint: "Keeps currency, permanents, curses and upgrades.",
      },
      {
        id: "set_phase",
        label: "Set phase",
        owner: "server",
        args: [int("phase", "Phase (0-3)")],
        hint: "0 draft, 1 playing, 2 reward draft, 3 over.",
      },
      {
        id: "teleport",
        label: "Teleport",
        owner: "server",
        args: [flt("x", "X"), flt("y", "Y"), flt("z", "Z")],
        hint: "Moves you to the point.",
      },
    ],
  },
  {
    id: "economy",
    label: "Economy",
    commands: [
      {
        id: "grant_gold",
        label: "Grant gold",
        owner: "server",
        args: [int("amount", "Amount (negative subtracts)")],
        hint: "Clamps at zero.",
      },
      {
        id: "grant_gift",
        label: "Grant gifts",
        owner: "server",
        args: [int("count", "Count", false), bool("golden", "Golden")],
        hint: "Golden gifts unlock only once the normal gifts are gone.",
      },
      {
        id: "remove_gift",
        label: "Remove gifts",
        owner: "server",
        args: [int("count", "Count", false)],
        hint: "Removes gifts without awarding currency.",
      },
      {
        id: "give_upgrade",
        label: "Give upgrade",
        owner: "server",
        args: [str("id", "Upgrade id")],
        hint: "Applies its mods exactly as a draft pick would.",
      },
      {
        id: "take_upgrade",
        label: "Take upgrade",
        owner: "server",
        args: [str("id", "Upgrade id")],
        hint: "Removes one stack and rebuilds the mods.",
      },
      {
        id: "give_curse",
        label: "Give curse",
        owner: "server",
        args: [str("id", "Curse id")],
        hint: "Applies its mods exactly as a draft pick would.",
      },
      {
        id: "take_curse",
        label: "Take curse",
        owner: "server",
        args: [str("id", "Curse id")],
        hint: "Removes one stack and rebuilds the mods.",
      },
    ],
  },
  {
    id: "world",
    label: "World",
    commands: [
      {
        id: "trigger_altar",
        label: "Trigger altar",
        owner: "server",
        args: [int("id", "Altar id")],
        hint: "Fires the benefit and the cost, as standing on it would.",
      },
      {
        id: "set_fog",
        label: "Set fog",
        owner: "client",
        args: [flt("near", "Near"), flt("far", "Far")],
        hint: "Client-side visual; the server validates the range.",
      },
    ],
  },
  {
    id: "state",
    label: "State",
    commands: [
      {
        id: "state",
        label: "Read run state",
        owner: "server",
        args: [],
        hint: "Level, phase, seed, currency, permanents, curses, upgrades, enemies.",
      },
    ],
  },
];

/** One line of the panel's output log. */
export interface DebugLogEntry {
  /** Monotonic id, so React keys are stable across identical messages. */
  seq: number;
  command: string;
  args: string;
  ok: boolean;
  message: string;
}

/** The panel's whole state. `unlocked` is set only by a server `ok` answer. */
export interface DebugPanelState {
  unlocked: boolean;
  code: string;
  log: DebugLogEntry[];
  /** The command whose form is open, or null. */
  open: string | null;
  /** Form values, keyed by command id then arg key. */
  values: Record<string, Record<string, string>>;
}

export const EMPTY_PANEL: DebugPanelState = {
  unlocked: false,
  code: "",
  log: [],
  open: null,
  values: {},
};

/** How many log lines the panel keeps. Older lines are dropped. */
export const MAX_LOG = 50;

/**
 * The panel is singleplayer-only, mirroring the server gate. Hiding it when a second
 * player joins is a courtesy, not a security boundary: the server rejects the command
 * either way.
 */
export function panelVisible(playerCount: number): boolean {
  return playerCount <= 1;
}

/** Every command id in the manifest, in section order. */
export function commandIds(): string[] {
  return DEBUG_SECTIONS.flatMap((s) => s.commands.map((c) => c.id));
}

/** The spec for a command id, or undefined. */
export function findCommand(id: string): CommandSpec | undefined {
  for (const section of DEBUG_SECTIONS) {
    const found = section.commands.find((c) => c.id === id);
    if (found) return found;
  }
  return undefined;
}

/** The starting form values for a command: empty strings, and `false` for booleans. */
export function defaultValues(spec: CommandSpec): Record<string, string> {
  const out: Record<string, string> = {};
  for (const arg of spec.args) {
    out[arg.key] = arg.kind === "bool" ? "false" : "";
  }
  return out;
}

/**
 * Turn form values into the JSON args string the server expects.
 *
 * Empty optional args are omitted rather than sent as `""` or `0`: the server treats a
 * present-but-wrong-typed key as an error, and an omitted key as "use the default".
 * A required arg that is empty is an error here, so the tester sees it before a round trip.
 */
export function buildArgs(spec: CommandSpec, values: Record<string, string>): { args: string } | { error: string } {
  const out: Record<string, string | number | boolean> = {};
  for (const arg of spec.args) {
    const raw = (values[arg.key] ?? "").trim();
    if (raw === "") {
      if (arg.required) return { error: `${arg.label} is required` };
      continue;
    }
    if (arg.kind === "bool") {
      out[arg.key] = raw === "true";
      continue;
    }
    if (arg.kind === "string") {
      out[arg.key] = raw;
      continue;
    }
    const n = Number(raw);
    if (!Number.isFinite(n)) return { error: `${arg.label} must be a number` };
    if (arg.kind === "int" && !Number.isInteger(n)) return { error: `${arg.label} must be a whole number` };
    out[arg.key] = n;
  }
  return { args: JSON.stringify(out) };
}

/** Append a log line, keeping the newest `MAX_LOG`. */
export function appendLog(log: DebugLogEntry[], entry: Omit<DebugLogEntry, "seq">): DebugLogEntry[] {
  const seq = (log.at(-1)?.seq ?? 0) + 1;
  const next = [...log, { ...entry, seq }];
  return next.length > MAX_LOG ? next.slice(next.length - MAX_LOG) : next;
}

/**
 * Apply a server answer. Only an `ok` answer unlocks the panel: a wrong code must not
 * leave the UI looking open.
 */
export function applyResult(state: DebugPanelState, command: string, args: string, ok: boolean, message: string): DebugPanelState {
  return {
    ...state,
    unlocked: state.unlocked || ok,
    log: appendLog(state.log, { command, args, ok, message }),
  };
}

/** The label to show on the lock button. */
export function lockLabel(state: DebugPanelState): string {
  return state.unlocked ? "Locked? no — unlocked" : "Locked";
}

/** The reveal gesture: Ctrl+Shift+D. Nothing renders before it, so a normal build shows no panel. */
export const REVEAL_KEY = "D";

export function isRevealEvent(e: { ctrlKey: boolean; shiftKey: boolean; key: string }): boolean {
  return e.ctrlKey && e.shiftKey && e.key.toUpperCase() === REVEAL_KEY;
}
