import type { NextConfig } from "next";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";

const here = dirname(fileURLToPath(import.meta.url));

const config: NextConfig = {
  reactStrictMode: true,
  // Production builds are a static site (web/out) served by the game server itself: one program = the whole game.
  output: process.env.NODE_ENV === "production" ? "export" : undefined,
  trailingSlash: true,
  // The monorepo root, so the client can import design/palette.json.
  turbopack: { root: join(here, "..") },
  poweredByHeader: false,
  // Do not write AGENTS.md/CLAUDE.md into the repo on `next dev`.
  agentRules: false,
  // Keep screenshots clean; the dev overlay still reports errors.
  devIndicators: false,
};

export default config;
