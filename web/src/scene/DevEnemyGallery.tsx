"use client";

// T-152: the dev-only enemy gallery.
//
// The silhouette audit needs each enemy rendered in isolation against a flat
// background, because the models are animated JSX with conditional rendering and
// refs -- reading their source would measure the declarations, not what a player
// sees. This component is the capture source; `tools/silhouette.mjs` drives it.
//
// The route that mounts this (`app/dev/enemies/page.dev.tsx`) is excluded from
// production builds by `pageExtensions` in next.config.ts, so a shipped build has
// no such page at all.
import { Canvas } from "@react-three/fiber";
import { useMemo } from "react";
import type { JSX } from "react";
import { ENEMY_DORMANT } from "@/protocol/generated";
import { MODEL_COMPONENTS } from "./EnemyField";
import type { EnemyAnim, EnemyAnimRef } from "./enemyLook";

/** Flat background. The capture tool keys the silhouette off this exact colour. */
export const GALLERY_BACKGROUND = "#101014";

/** Roster ids in wire `kind` order, for the capture tool's labels. */
export const GALLERY_IDS = [
  "pallbearer",
  "snapshrike",
  "moteling",
  "rift_warden",
  "gale_wight",
  "lantern_leech",
  "turret",
  "mimic",
  "corrupter",
  "rewind",
  "warden",
  "peal",
  "kite",
  "static",
  "blight",
  "metronome",
  "overlook",
  "chorus",
  "coil",
  "lure",
] as const;

/** A dormant pose: the models' idle animation, which is what a player sees first. */
function dormantAnim(): EnemyAnimRef {
  const anim: EnemyAnim = {
    state: ENEMY_DORMANT,
    prog: 0,
    flags: 0,
    ex: 0,
    ey: 0,
    ez: 0,
    tx: 0,
    ty: 0,
    tz: 0,
  };
  return { current: anim };
}

function OneEnemy({ index, size }: { index: number; size: number }): JSX.Element | null {
  const Model = MODEL_COMPONENTS[index];
  const anim = useMemo(() => dormantAnim(), []);
  if (!Model) return null;
  // The models are authored at unit radius and the game scales them by the server's
  // body radius (`EnemyField` does `scale.setScalar(enemyRadius)`), so the gallery
  // must apply the same scale or it measures a size no player ever sees.
  return (
    <group scale={size}>
      <Model anim={anim} />
    </group>
  );
}

export interface DevEnemyGalleryProps {
  /** Roster index to render. Out of range renders nothing. */
  index: number;
  /** Body radius from design/content.json. The game scales models by this. */
  size?: number;
}

export function DevEnemyGallery({ index, size = 1 }: DevEnemyGalleryProps): JSX.Element {
  return (
    <div style={{ position: "fixed", inset: 0, background: GALLERY_BACKGROUND }}>
      <Canvas
        // Orthographic so the silhouette does not depend on perspective foreshortening.
        orthographic
        // Gameplay view: the in-game camera sits above and behind the player, so a
        // head-on view would measure a projection no player ever sees -- and it would
        // render flat ground-plane models (LURE) as a zero-height line.
        // Zoom frames the largest enemy (rift_warden, body radius 1.5) with margin, so
        // every model is measured in the same fixed frame and size stays a signal.
        camera={{ position: [0, 2.6, 5.0], zoom: 120, near: 0.1, far: 100 }}
        dpr={1}
        gl={{ antialias: false, preserveDrawingBuffer: true }}
        // Flat, unlit-looking setup: the silhouette is what is being measured, so
        // shading must not create or hide edges.
        onCreated={({ gl, camera }) => {
          gl.setClearColor(GALLERY_BACKGROUND, 1);
          camera.lookAt(0, 0.35, 0);
          camera.updateProjectionMatrix();
        }}
      >
        <ambientLight intensity={2.2} />
        <directionalLight position={[2, 3, 4]} intensity={1.4} />
        <OneEnemy index={index} size={size} />
      </Canvas>
      <div
        data-testid="gallery-label"
        style={{
          position: "fixed",
          left: 12,
          top: 12,
          color: "#e8e8f0",
          font: "600 14px ui-monospace, monospace",
          letterSpacing: "0.08em",
        }}
      >
        {GALLERY_IDS[index] ?? "?"} ({index}) size {size}
      </div>
    </div>
  );
}
