// T-152: dev-only enemy gallery route.
//
// The `.dev.tsx` suffix is what keeps this out of a shipped build: next.config.ts
// lists "dev.tsx" in `pageExtensions` only when NODE_ENV is not production, so in a
// release this file is not a page and `/dev/enemies` does not exist.
import { DevEnemyGallery } from "@/scene/DevEnemyGallery";

export default async function DevEnemiesPage({
  searchParams,
}: {
  searchParams: Promise<{ enemy?: string; size?: string }>;
}) {
  const params = await searchParams;
  const index = Number(params.enemy ?? "0");
  // `size` is the enemy's body radius from design/content.json. The capture tool
  // passes it so the gallery draws the model at the size a player actually sees;
  // without it every enemy would be measured at its authored unit size.
  const size = Number(params.size ?? "1");
  return (
    <DevEnemyGallery
      index={Number.isFinite(index) ? Math.max(0, Math.trunc(index)) : 0}
      size={Number.isFinite(size) && size > 0 ? size : 1}
    />
  );
}
