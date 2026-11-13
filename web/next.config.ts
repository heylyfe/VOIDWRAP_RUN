import type { NextConfig } from "next";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";

const here = dirname(fileURLToPath(import.meta.url));

const config: NextConfig = {
  reactStrictMode: true,
  // The monorepo root, so the client can import design/palette.json.
  turbopack: { root: join(here, "..") },
  poweredByHeader: false,
  // Do not write AGENTS.md/CLAUDE.md into the repo on `next dev`.
  agentRules: false,
  // Keep screenshots clean; the dev overlay still reports errors.
  devIndicators: false,
};

export default config;
