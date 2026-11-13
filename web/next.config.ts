import type { NextConfig } from "next";

const config: NextConfig = {
  reactStrictMode: true,
  poweredByHeader: false,
  // Do not write AGENTS.md/CLAUDE.md into the repo on `next dev`.
  agentRules: false,
  // Keep screenshots clean; the dev overlay still reports errors.
  devIndicators: false,
};

export default config;
