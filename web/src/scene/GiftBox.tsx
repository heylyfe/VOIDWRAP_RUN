"use client";

import { useEffect, useMemo, useRef } from "react";
import type { JSX } from "react";
import { useFrame } from "@react-three/fiber";
import {
  AdditiveBlending,
  BoxGeometry,
  CanvasTexture,
  DynamicDrawUsage,
  InstancedMesh,
  Matrix4,
  MeshBasicMaterial,
  MeshStandardMaterial,
  OctahedronGeometry,
  Quaternion,
  RepeatWrapping,
  SRGBColorSpace,
  Vector3,
} from "three";
import {
  GIFT_GOLDEN,
  LEVEL_COLLECT,
  TILE_GONE,
} from "@/protocol/generated";
import type { LevelData, Platform, TileState, Vec3 } from "@/protocol/generated";
import { useGameStore } from "@/game/store";
import { COMMON } from "./theme";
import { isStragglerWindow } from "./straggler";
import {
  BEAM_COLOR,
  GOLD_BODY,
  GOLD_BOW,
  GOLD_EMISSIVE,
  GOLD_RIBBON,
  SPARKLE_COLOR,
} from "./giftPalette";

/** Side length of a gift (design: 0.34 u). Golden gifts are identical in size. */
const GIFT_SIZE = 0.34;
/** Bob amplitude while hovering. */
const BOB_AMP = 0.08;
const BOB_HZ = 1.6;
const SPIN_RAD_PER_S = 0.9;
/** Golden sparkle octahedron hovers this far above the gift centre. */
const SPARKLE_LIFT = 0.34;
const SPARKLE_RADIUS = 0.085;

const GOLD_EMISSIVE_INTENSITY = 1.5;
/**
 * Normal gifts now glow like golden ones (DO 1): pushed above 1.0 so the
 * toneMapped:false emissive clears typical bloom thresholds instead of the
 * previous 0.6, which never glowed. Golden gifts stay brighter and keep their
 * sparkle so they remain distinct.
 */
const NORMAL_EMISSIVE_INTENSITY = 1.35;
/**
 * T-112: when at most this fraction of a level's normal gifts is left, every
 * remaining gift gets a vertical beam so the endgame is not "wander an empty level
 * looking for one gift".
 *
 * T-146: the beam has its own colour (`giftPalette.BEAM_COLOR`). It used to be
 * `#dfe6ff`, byte-identical to the normal gift glow and therefore to LURE's shimmer --
 * so the marker that says "this one is real" wore the fake's colour. The beam's colour
 * is now measured against the normal glow, the golden gift, the beacon and every enemy
 * accent in `giftPalette.test.ts`.
 */
const BEAM_WIDTH = 0.07;
const BEAM_HEIGHT = 7;

interface GiftAssets {
  box: BoxGeometry;
  sparkle: OctahedronGeometry;
  normalMaterial: MeshStandardMaterial;
  goldenMaterial: MeshStandardMaterial;
  sparkleMaterial: MeshBasicMaterial;
  dispose(): void;
}

/** Deterministic little gift texture: body colour with a ribbon cross and a bow dot. */
function createGiftTexture(body: string, ribbon: string, bow: string): CanvasTexture | null {
  if (typeof document === "undefined") return null;
  const size = 128;
  const canvas = document.createElement("canvas");
  canvas.width = size;
  canvas.height = size;
  const ctx = canvas.getContext("2d");
  if (!ctx) return null;
  ctx.fillStyle = body;
  ctx.fillRect(0, 0, size, size);
  ctx.fillStyle = "rgba(0, 0, 0, 0.18)";
  ctx.fillRect(0, size * 0.72, size, size * 0.28);
  const band = Math.floor(size * 0.2);
  ctx.fillStyle = ribbon;
  ctx.fillRect((size - band) / 2, 0, band, size);
  ctx.fillRect(0, (size - band) / 2, size, band);
  ctx.fillStyle = bow;
  ctx.beginPath();
  ctx.arc(size / 2, size / 2, band * 0.45, 0, Math.PI * 2);
  ctx.fill();
  const tex = new CanvasTexture(canvas);
  tex.wrapS = RepeatWrapping;
  tex.wrapT = RepeatWrapping;
  tex.colorSpace = SRGBColorSpace;
  tex.anisotropy = 4;
  return tex;
}

function createGiftAssets(): GiftAssets {
  const normalTex = createGiftTexture(COMMON.gift.body, COMMON.gift.ribbon, COMMON.gift.glow);
  const goldenTex = createGiftTexture(GOLD_BODY, GOLD_RIBBON, GOLD_BOW);
  const box = new BoxGeometry(GIFT_SIZE, GIFT_SIZE, GIFT_SIZE);
  const sparkle = new OctahedronGeometry(SPARKLE_RADIUS);
  const normalMaterial = new MeshStandardMaterial({
    map: normalTex ?? undefined,
    color: normalTex ? "#ffffff" : COMMON.gift.body,
    emissive: COMMON.gift.glow,
    emissiveIntensity: NORMAL_EMISSIVE_INTENSITY,
    roughness: 0.55,
    metalness: 0.1,
    toneMapped: false,
  });
  const goldenMaterial = new MeshStandardMaterial({
    map: goldenTex ?? undefined,
    color: goldenTex ? "#ffffff" : GOLD_BODY,
    emissive: GOLD_EMISSIVE,
    emissiveIntensity: GOLD_EMISSIVE_INTENSITY,
    roughness: 0.32,
    metalness: 0.35,
    toneMapped: false,
  });
  const sparkleMaterial = new MeshBasicMaterial({
    color: SPARKLE_COLOR,
    transparent: true,
    opacity: 0.9,
    blending: AdditiveBlending,
    depthWrite: false,
    toneMapped: false,
    fog: false,
  });
  return {
    box,
    sparkle,
    normalMaterial,
    goldenMaterial,
    sparkleMaterial,
    dispose() {
      box.dispose();
      sparkle.dispose();
      normalMaterial.dispose();
      goldenMaterial.dispose();
      sparkleMaterial.dispose();
      normalTex?.dispose();
      goldenTex?.dispose();
    },
  };
}

interface Scratch {
  mat: Matrix4;
  pos: Vector3;
  quat: Quaternion;
  scale: Vector3;
  axis: Vector3;
}

/** The platform supporting a gift: highest platform top below the gift whose XZ contains it. */
function platformUnder(pos: Vec3, platforms: readonly Platform[]): number | null {
  let best: Platform | null = null;
  for (const p of platforms) {
    if (pos.x < p.bounds.min.x || pos.x > p.bounds.max.x) continue;
    if (pos.z < p.bounds.min.z || pos.z > p.bounds.max.z) continue;
    const top = p.bounds.max.y;
    if (top > pos.y + 0.01) continue;
    if (best === null || top > best.bounds.max.y) best = p;
  }
  return best === null ? null : best.id;
}

/**
 * All gifts on the level, drawn as three instanced meshes (normal, golden, sparkle).
 * Phase 1: untaken normal gifts. Phase 2 (levelPhase != LEVEL_COLLECT): one golden gift at
 * every collected normal gift position, unless its id is in store.goldenTaken or its platform
 * is TILE_GONE (golden gifts stay collectable during TILE_WARN, like on the server).
 */
export function Gifts({
  level,
  tiles,
}: {
  level: LevelData;
  tiles: readonly TileState[];
}): JSX.Element {
  const assets = useMemo(() => createGiftAssets(), []);
  useEffect(
    () => () => {
      assets.dispose();
    },
    [assets],
  );

  const scratchRef = useRef<Scratch | null>(null);

  const giftsTaken = useGameStore((s) => s.giftsTaken);
  const goldenTaken = useGameStore((s) => s.goldenTaken);
  const levelPhase = useGameStore((s) => s.levelPhase);
  const phase2 = levelPhase !== LEVEL_COLLECT;

  const normalGifts = useMemo(
    () => level.gifts.filter((g) => g.kind !== GIFT_GOLDEN),
    [level],
  );
  // Resolved once per level; only TILE_GONE re-filters the visible golden set.
  const giftPlatform = useMemo(
    () => normalGifts.map((g) => platformUnder(g.pos, level.platforms)),
    [normalGifts, level.platforms],
  );
  const taken = useMemo(() => new Set<number>(giftsTaken), [giftsTaken]);
  const goldTaken = useMemo(() => new Set<number>(goldenTaken), [goldenTaken]);
  const gone = useMemo(() => {
    const s = new Set<number>();
    for (const t of tiles) {
      if (t.state === TILE_GONE) s.add(t.id);
    }
    return s;
  }, [tiles]);

  const visibleNormal = useMemo(() => {
    const out: number[] = [];
    if (phase2) return out;
    for (let i = 0; i < normalGifts.length; i += 1) {
      const g = normalGifts[i];
      if (!g || taken.has(g.id)) continue;
      out.push(i);
    }
    return out;
  }, [phase2, normalGifts, taken]);

  // T-112: the straggler window. `visibleNormal` already excludes collected gifts,
  // so this is the live remaining count.
  const straggler = useMemo(
    () => isStragglerWindow(normalGifts.length, visibleNormal.length),
    [normalGifts.length, visibleNormal.length],
  );

  const visibleGolden = useMemo(() => {
    const out: number[] = [];
    if (!phase2) return out;
    for (let i = 0; i < normalGifts.length; i += 1) {
      const g = normalGifts[i];
      if (!g) continue;
      if (!taken.has(g.id)) continue;
      if (goldTaken.has(g.id)) continue;
      const p = giftPlatform[i];
      if (p !== undefined && p !== null && gone.has(p)) continue;
      out.push(i);
    }
    return out;
  }, [phase2, normalGifts, taken, goldTaken, giftPlatform, gone]);

  const normalRef = useRef<InstancedMesh>(null);
  const goldenRef = useRef<InstancedMesh>(null);
  const sparkleRef = useRef<InstancedMesh>(null);
  const beamRef = useRef<InstancedMesh>(null);
  const beamAssets = useMemo(() => {
    const geometry = new BoxGeometry(BEAM_WIDTH, BEAM_HEIGHT, BEAM_WIDTH);
    const material = new MeshBasicMaterial({
      color: BEAM_COLOR,
      transparent: true,
      opacity: 0.55,
      blending: AdditiveBlending,
      depthWrite: false,
      toneMapped: false,
    });
    return { geometry, material };
  }, []);
  useEffect(() => {
    for (const m of [normalRef.current, goldenRef.current, sparkleRef.current, beamRef.current]) {
      if (m) {
        m.instanceMatrix.setUsage(DynamicDrawUsage);
        m.count = 0;
        m.instanceMatrix.needsUpdate = true;
      }
    }
  }, [assets, normalGifts.length]);

  useFrame((state) => {
    let s = scratchRef.current;
    if (s === null) {
      s = {
        mat: new Matrix4(),
        pos: new Vector3(),
        quat: new Quaternion(),
        scale: new Vector3(1, 1, 1),
        axis: new Vector3(0, 1, 0),
      };
      scratchRef.current = s;
    }
    const t = state.clock.elapsedTime;

    const normalMesh = normalRef.current;
    if (normalMesh) {
      let n = 0;
      for (const gi of visibleNormal) {
        const g = normalGifts[gi];
        if (!g) continue;
        const phase = (gi * 1.61803398875) % (Math.PI * 2);
        s.pos.set(g.pos.x, g.pos.y + Math.sin(t * BOB_HZ + phase) * BOB_AMP, g.pos.z);
        s.quat.setFromAxisAngle(s.axis, t * SPIN_RAD_PER_S + phase);
        s.scale.set(1, 1, 1);
        s.mat.compose(s.pos, s.quat, s.scale);
        normalMesh.setMatrixAt(n, s.mat);
        n += 1;
      }
      normalMesh.count = n;
      normalMesh.instanceMatrix.needsUpdate = true;
    }

    // T-112 straggler beams: one vertical beam per remaining gift once the level is
    // down to its last 20%. The beam is centred on the gift and rises from its base,
    // so it reads through fog without changing the gift's own colour.
    const beamMesh = beamRef.current;
    if (beamMesh) {
      let b = 0;
      if (straggler) {
        for (const gi of visibleNormal) {
          const g = normalGifts[gi];
          if (!g) continue;
          const phase = (gi * 1.61803398875) % (Math.PI * 2);
          const pulse = 0.85 + 0.15 * Math.sin(t * 2.4 + phase);
          s.pos.set(g.pos.x, g.pos.y + BEAM_HEIGHT * 0.5 - 0.3, g.pos.z);
          s.quat.identity();
          s.scale.set(pulse, 1, pulse);
          s.mat.compose(s.pos, s.quat, s.scale);
          beamMesh.setMatrixAt(b, s.mat);
          b += 1;
        }
      }
      beamMesh.count = b;
      beamMesh.instanceMatrix.needsUpdate = true;
    }

    const goldenMesh = goldenRef.current;
    const sparkleMesh = sparkleRef.current;
    if (goldenMesh) {
      let n = 0;
      for (const gi of visibleGolden) {
        const g = normalGifts[gi];
        if (!g) continue;
        const phase = (gi * 2.39996322973) % (Math.PI * 2);
        s.pos.set(g.pos.x, g.pos.y + Math.sin(t * BOB_HZ + phase) * BOB_AMP, g.pos.z);
        s.quat.setFromAxisAngle(s.axis, t * SPIN_RAD_PER_S * 1.5 + phase);
        s.scale.set(1, 1, 1);
        s.mat.compose(s.pos, s.quat, s.scale);
        goldenMesh.setMatrixAt(n, s.mat);
        if (sparkleMesh) {
          const pulse = 0.75 + 0.45 * Math.sin(t * 3.1 + phase * 2);
          s.pos.set(
            g.pos.x,
            g.pos.y + SPARKLE_LIFT + Math.sin(t * 2.2 + phase) * 0.06,
            g.pos.z,
          );
          s.quat.setFromAxisAngle(s.axis, t * 2.4 + phase);
          s.scale.setScalar(Math.max(0.2, pulse));
          s.mat.compose(s.pos, s.quat, s.scale);
          sparkleMesh.setMatrixAt(n, s.mat);
        }
        n += 1;
      }
      goldenMesh.count = n;
      goldenMesh.instanceMatrix.needsUpdate = true;
      if (sparkleMesh) {
        sparkleMesh.count = n;
        sparkleMesh.instanceMatrix.needsUpdate = true;
      }
    }
  });

  const capacity = Math.max(1, normalGifts.length);
  return (
    <group>
      <instancedMesh
        ref={normalRef}
        args={[assets.box, assets.normalMaterial, capacity]}
        frustumCulled={false}
        dispose={null}
      />
      <instancedMesh
        ref={goldenRef}
        args={[assets.box, assets.goldenMaterial, capacity]}
        frustumCulled={false}
        dispose={null}
      />
      <instancedMesh
        ref={sparkleRef}
        args={[assets.sparkle, assets.sparkleMaterial, capacity]}
        frustumCulled={false}
        dispose={null}
      />
      <instancedMesh
        ref={beamRef}
        args={[beamAssets.geometry, beamAssets.material, capacity]}
        frustumCulled={false}
        dispose={null}
      />
    </group>
  );
}

/**
 * One stand-alone gift with exactly the look and size of the instanced gifts (used by the False Gift
 * enemy while it is disguised, so it is indistinguishable from a real gift).
 */
export function GiftBox(props: { position: Vec3; taken: boolean }): JSX.Element {
  const assets = useMemo(() => createGiftAssets(), []);
  useEffect(() => () => assets.dispose(), [assets]);
  return (
    <mesh
      position={[props.position.x, props.position.y, props.position.z]}
      visible={!props.taken}
      geometry={assets.box}
      material={assets.normalMaterial}
    />
  );
}
