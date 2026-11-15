// T-149: the pedestal vote had a complete, tested client UI that nothing mounted.
//
// `VoteWidget`, `useVote`, `protocolAdapter`, `LobbyPoll` and `RunVoteOverlay` all existed
// and all had tests, but no file imported `LobbyPoll` or `RunVoteOverlay` outside their own
// tests, `session.ts` had no way to send a ballot, and `GameClient` never read a vote
// message. (`LobbyPoll` was deleted in T-153: its pre-match poll had no protocol message
// and no server handler, so it promised a feature that did not exist.) The server opened a session every level at the collapse window, nobody could
// answer it, and `VoteSession::resolve` fell through to its random pick -- so the pedestal
// mechanic ran invisibly and picked for the players.
//
// These guards read the source because the defect is a missing wire, not a wrong value:
// no runtime assertion can see a component that was never rendered.
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";

const here = dirname(fileURLToPath(import.meta.url));
const read = (rel: string): string => readFileSync(join(here, rel), "utf8");

describe("the pedestal vote reaches the player", () => {
  it("mounts RunVoteOverlay in GameClient", () => {
    const src = read("GameClient.tsx");
    expect(src).toContain('from "@/hud/RunVoteOverlay"');
    expect(src).toMatch(/<RunVoteOverlay\b/);
    // The overlay is inert without the message log and the ballot callback.
    expect(src).toMatch(/<RunVoteOverlay[^>]*messages=\{/);
    expect(src).toMatch(/<RunVoteOverlay[^>]*onVote=\{/);
  });

  it("feeds the overlay from the store's vote log", () => {
    const src = read("GameClient.tsx");
    expect(src).toMatch(/useGameStore\(\(s\) => s\.voteMessages\)/);
  });

  it("records every vote message the server sends", () => {
    const src = read("session.ts");
    for (const type of ["VoteOpen", "VoteState", "VoteResult"]) {
      expect(src).toContain(`case "${type}":`);
    }
    expect(src).toContain("voteMessages:");
  });

  it("can send a ballot", () => {
    const src = read("session.ts");
    expect(src).toMatch(/vote\(option: number\): void/);
    expect(src).toContain('this.conn.send({ type: "Vote", option })');
  });

  it("declares the vote log on the store", () => {
    const src = read("store.ts");
    expect(src).toContain("voteMessages: unknown[];");
    expect(src).toContain("voteMessages: [],");
  });
});
