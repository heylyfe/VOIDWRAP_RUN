// T-161: the pre-match poll panel (T-159) had no client UI at all, so a player
// joining a fresh run sat through 40 s of dead time while the server
// waited on the theme and difficulty polls.
//
// These guards read the source because the defect class is a missing wire, not a wrong
// value: a component can exist, be fully tested, and never be mounted. No runtime
// assertion can see that. Mirrors game/voteWiring.test.ts (T-149).
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";

const here = dirname(fileURLToPath(import.meta.url));
const read = (rel: string): string => readFileSync(join(here, rel), "utf8");

describe("the pre-match poll reaches the player", () => {
  it("mounts PollOverlay in GameClient", () => {
    const src = read("GameClient.tsx");
    expect(src).toContain('from "@/hud/PollOverlay"');
    expect(src).toMatch(/<PollOverlay\b/);
    // The overlay is inert without the message log and the answer callback.
    expect(src).toMatch(/<PollOverlay[^>]*messages=\{/);
    expect(src).toMatch(/<PollOverlay[^>]*onPoll=\{/);
  });

  it("feeds the overlay from the store's poll log", () => {
    const src = read("GameClient.tsx");
    expect(src).toMatch(/useGameStore\(\(s\) => s\.pollMessages\)/);
  });

  it("records every poll message the server sends", () => {
    const src = read("session.ts");
    for (const type of ["PollOpen", "PollState", "PollResult"]) {
      expect(src).toContain(`case "${type}":`);
    }
    expect(src).toContain("pollMessages:");
  });

  it("can send a poll answer", () => {
    const src = read("session.ts");
    expect(src).toMatch(/poll\(option: number\): void/);
    expect(src).toContain('this.conn.send({ type: "Poll", option })');
  });

  it("declares the poll log on the store", () => {
    const src = read("store.ts");
    expect(src).toContain("pollMessages: unknown[];");
    expect(src).toContain("pollMessages: [],");
  });
});
