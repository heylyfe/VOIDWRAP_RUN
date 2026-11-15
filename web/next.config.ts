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
  // T-152: the dev-only enemy gallery lives at `app/dev/enemies/page.dev.tsx`.
  // `pageExtensions` decides which suffixes count as a page, so dropping "dev.tsx"
  // from the production list means the route does not exist in a shipped build --
  // not "exists but 404s", absent. The gallery is the capture source for the
  // silhouette audit, and a measurement tool has no business in a release.
  pageExtensions:
    process.env.NODE_ENV === "production" ? ["tsx", "ts"] : ["dev.tsx", "tsx", "ts"],
};

export default config;
