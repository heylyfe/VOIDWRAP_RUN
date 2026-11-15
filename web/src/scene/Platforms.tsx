"use client";

import { useEffect, useMemo, useRef } from "react";
import type { JSX } from "react";
import { useFrame } from "@react-three/fiber";
import {
  BoxGeometry,
  BufferAttribute,
  BufferGeometry,
  DynamicDrawUsage,
  Group,
  InstancedBufferAttribute,
  InstancedMesh,
  Matrix4,
  MeshStandardMaterial,
} from "three";
import {
  PLATFORM_CRUMBLE,
  PLATFORM_JUMP_PAD,
  PLATFORM_NORMAL,
  PLATFORM_TRAMPOLINE,
  PLATFORM_TRIPMINE,
  TILE_FRETTED,
  TILE_GONE,
  TILE_WARN,
} from "@/protocol/generated";
import type { Platform, TileState } from "@/protocol/generated";
import { iceTint } from "./iceTint";
import { isPillar, platformCenter, platformSize, rampInfo, rampSlope } from "./platformUtils";
import type { Vec3Tuple } from "./platformUtils";
import type { Theme } from "./theme";
import {
  buildFragmentHeader,
  buildPadUniforms,
  buildSurfaceUniforms,
  buildVertexHeader,
  PAD_FRAGMENT_COLOR,
  PAD_FRAGMENT_EMISSIVE,
  SURFACE_FRAGMENT_COLOR,
  SURFACE_FRAGMENT_EMISSIVE,
  VERTEX_BEGIN,
} from "./platformShaders";
import type { SurfacePalette } from "./platformShaders";

const TOP_ROUGHNESS = 0.85;
const PAD_ROUGHNESS = 0.55;
const ICE_ROUGHNESS = 0.16;
const SIDE_TINT = "#9a9a9a";
const GONE_SINK_TIME = 1.2;
const GONE_SINK_DEPTH = 14;
const SHAKE_AMP = 0.055;

const scratchMatrix = new Matrix4();
const scratchRotation = new Matrix4();

/** Phase offset per platform id so shakes/blinks do not line up. */
function phaseForId(id: number): number {
  return (id * 1.61803398875) % (Math.PI * 2);
}

interface InstanceRecord {
  id: number;
  center: Vec3Tuple;
  /** World-space extents; size[1] drives the gone-sink depth. */
  size: Vec3Tuple;
  /** Scale applied in the wedge's local axes before rotation (ramps only); equals size for boxes. */
  localScale: Vec3Tuple;
  /** Y rotation of the instance; ramps rotate their wedge per kind, boxes use 0. */
  spin: number;
  phase: number;
  pillar: boolean;
  mesh: InstancedMesh;
  attr: InstancedBufferAttribute;
  index: number;
  touched: boolean;
  activeFrame: boolean;
}

interface BuiltPlatforms {
  holder: Group;
  records: InstanceRecord[];
  timeUniform: { value: number };
}

/** Compose scale * (optional Y rotation) into scratchMatrix and place it at (x, y, z). */
function composeInstanceMatrix(localScale: Vec3Tuple, spin: number, x: number, y: number, z: number): void {
  scratchMatrix.makeScale(localScale[0], localScale[1], localScale[2]);
  if (spin !== 0) {
    scratchRotation.makeRotationY(spin);
    scratchMatrix.premultiply(scratchRotation);
  }
  scratchMatrix.setPosition(x, y, z);
}

/**
 * Unit wedge: 1x1 footprint, flat bottom at y = -0.5, zero thickness at the low edge
 * (z = -0.5), 1 u tall at the high edge (z = +0.5). Non-indexed with computed flat
 * normals; scale per instance by the platform's size and rotate per ramp kind.
 */
function createWedgeGeometry(): BufferGeometry {
  // Corner shorthand: L* = low edge, H* = high edge, B* = bottom at high edge.
  const L0: Vec3Tuple = [-0.5, -0.5, -0.5];
  const L1: Vec3Tuple = [0.5, -0.5, -0.5];
  const B0: Vec3Tuple = [-0.5, -0.5, 0.5];
  const B1: Vec3Tuple = [0.5, -0.5, 0.5];
  const H0: Vec3Tuple = [-0.5, 0.5, 0.5];
  const H1: Vec3Tuple = [0.5, 0.5, 0.5];
  const triangles: Array<[Vec3Tuple, Vec3Tuple, Vec3Tuple]> = [
    // Bottom face (normal -Y).
    [L0, B1, B0],
    [L0, L1, B1],
    // Sloped top face (normal 0/1/-1 up toward the low edge).
    [L0, H1, L1],
    [H0, H1, L0],
    // High end (normal +Z). The low end has zero thickness, so no face.
    [B0, B1, H1],
    [B0, H1, H0],
    // Side faces (normals +/-X).
    [L1, H1, B1],
    [L0, B0, H0],
  ];
  const positions = new Float32Array(triangles.length * 9);
  let p = 0;
  for (const [a, b, c] of triangles) {
    for (const v of [a, b, c]) {
      positions[p] = v[0];
      positions[p + 1] = v[1];
      positions[p + 2] = v[2];
      p += 3;
    }
  }
  // No uv attribute: the surface shader checkers from vWorldPos.xz, so a uv here
  // would be dead weight. The previous one was also wrong — its V coordinate picked
  // whichever of height/depth was larger per vertex, which is discontinuous across a
  // face. T-106 tracks the real fix (arc-length checker coordinates on inclines).
  const geometry = new BufferGeometry();
  geometry.setAttribute("position", new BufferAttribute(positions, 3));
  geometry.computeVertexNormals();
  return geometry;
}

/** Resolve the tile palette to concrete colors, applying the ice tint when frozen. */
function resolveSurfacePalette(theme: Theme, icy: boolean): SurfacePalette {
  return {
    light: icy ? iceTint(theme.tile.light) : theme.tile.light,
    side: icy ? iceTint(theme.tile.side || SIDE_TINT) : theme.tile.side || SIDE_TINT,
    dark: icy ? iceTint(theme.tile.dark) : theme.tile.dark,
    edge: icy ? iceTint(theme.tile.edge) : theme.tile.edge,
  };
}

function injectVertex(shader: { vertexShader: string }): void {
  shader.vertexShader = shader.vertexShader
    .replace("#include <common>", buildVertexHeader())
    .replace("#include <begin_vertex>", VERTEX_BEGIN);
}

function createSurfaceMaterial(
  kind: number,
  palette: SurfacePalette,
  icy: boolean,
  timeUniform: { value: number },
): MeshStandardMaterial {
  const isCrumble = kind === PLATFORM_CRUMBLE;
  const mat = new MeshStandardMaterial({
    color: "#ffffff",
    roughness: icy ? ICE_ROUGHNESS : TOP_ROUGHNESS,
    metalness: 0,
    emissive: "#000000",
  });
  // The fragment header is generated from this exact map, so every uniform
  // assigned on shader.uniforms is declared in the shader.
  const uniforms = buildSurfaceUniforms(palette, icy, isCrumble, timeUniform);
  mat.onBeforeCompile = (shader) => {
    Object.assign(shader.uniforms, uniforms);
    injectVertex(shader);
    shader.fragmentShader = shader.fragmentShader
      .replace("#include <common>", buildFragmentHeader(uniforms))
      .replace("#include <color_fragment>", SURFACE_FRAGMENT_COLOR)
      .replace("#include <emissivemap_fragment>", SURFACE_FRAGMENT_EMISSIVE);
  };
  mat.customProgramCacheKey = () => (isCrumble ? "vw-platform-crumble" : "vw-platform-normal");
  return mat;
}

function createPadMaterial(timeUniform: { value: number }): MeshStandardMaterial {
  const mat = new MeshStandardMaterial({
    color: "#ffffff",
    roughness: PAD_ROUGHNESS,
    metalness: 0.35,
    emissive: "#000000",
  });
  const uniforms = buildPadUniforms(timeUniform);
  mat.onBeforeCompile = (shader) => {
    Object.assign(shader.uniforms, uniforms);
    injectVertex(shader);
    shader.fragmentShader = shader.fragmentShader
      .replace("#include <common>", buildFragmentHeader(uniforms))
      .replace("#include <color_fragment>", PAD_FRAGMENT_COLOR)
      .replace("#include <emissivemap_fragment>", PAD_FRAGMENT_EMISSIVE);
  };
  mat.customProgramCacheKey = () => "vw-platform-pad";
  return mat;
}

function buildGroups(platforms: readonly Platform[], theme: Theme, icy: boolean): BuiltPlatforms {
  const timeUniform = { value: 0 };
  const holder = new Group();
  const records: InstanceRecord[] = [];
  const palette = resolveSurfacePalette(theme, icy);

  // Every kind that has collision must also be drawn: a TRAMPOLINE or TRIPMINE deck
  // that is missing here is solid but invisible, which reads as a hole in the floor.
  for (const kind of [
    PLATFORM_NORMAL,
    PLATFORM_CRUMBLE,
    PLATFORM_JUMP_PAD,
    PLATFORM_TRAMPOLINE,
    PLATFORM_TRIPMINE,
  ]) {
    const list = platforms.filter((p) => p.kind === kind);
    const geometry = new BoxGeometry(1, 1, 1);
    // Bounce decks share the pad shader so they read as launchable; a tripmine deck
    // is an ordinary surface with a hazard on it, so it keeps the surface shader.
    const material =
      kind === PLATFORM_JUMP_PAD || kind === PLATFORM_TRAMPOLINE
        ? createPadMaterial(timeUniform)
        : createSurfaceMaterial(kind, palette, icy, timeUniform);
    const count = Math.max(list.length, 1);
    const mesh = new InstancedMesh(geometry, material, count);
    mesh.count = list.length;
    mesh.frustumCulled = false; // instances are spread across the map; bounding sphere of a unit box is wrong
    mesh.instanceMatrix.setUsage(DynamicDrawUsage);
    const attr = new InstancedBufferAttribute(new Float32Array(count * 4), 4);
    attr.setUsage(DynamicDrawUsage);
    geometry.setAttribute("aState", attr);

    list.forEach((p, i) => {
      const size = platformSize(p);
      const center = platformCenter(p);
      composeInstanceMatrix(size, 0, center[0], center[1], center[2]);
      mesh.setMatrixAt(i, scratchMatrix);
      const pillar = isPillar(size);
      attr.setXYZW(i, 0, 0, 1, pillar ? 1 : 0);
      records.push({
        id: p.id,
        center,
        size,
        localScale: size,
        spin: 0,
        phase: phaseForId(p.id),
        pillar,
        mesh,
        attr,
        index: i,
        touched: false,
        activeFrame: false,
      });
    });
    mesh.instanceMatrix.needsUpdate = true;
    attr.needsUpdate = true;
    holder.add(mesh);
  }

  // Ramps: one shared InstancedMesh with a unit wedge geometry, rotated per ramp kind.
  // Ramps use the same non-crumble surface material as NORMAL platforms and participate in
  // the same per-instance tile-state animation (shake / sink), so a ramp with WARN/GONE
  // tiles shakes and sinks exactly like a box.
  const rampList = platforms.filter((p) => rampInfo(p.kind).isRamp);
  const rampGeometry = createWedgeGeometry();
  const rampMaterial = createSurfaceMaterial(PLATFORM_NORMAL, palette, icy, timeUniform);
  const rampCount = Math.max(rampList.length, 1);
  const rampMesh = new InstancedMesh(rampGeometry, rampMaterial, rampCount);
  rampMesh.count = rampList.length;
  rampMesh.frustumCulled = false;
  rampMesh.instanceMatrix.setUsage(DynamicDrawUsage);
  const rampAttr = new InstancedBufferAttribute(new Float32Array(rampCount * 4), 4);
  rampAttr.setUsage(DynamicDrawUsage);
  rampGeometry.setAttribute("aState", rampAttr);
  // Per-instance slope for the checker shader: (axis, cos). The top face is
  // inclined, so world XZ compresses the pattern along the slope axis by
  // cos(theta); the shader divides that axis back out so the cells stay square.
  const rampSlopeAttr = new InstancedBufferAttribute(new Float32Array(rampCount * 2), 2);
  rampSlopeAttr.setUsage(DynamicDrawUsage);
  rampGeometry.setAttribute("aSlope", rampSlopeAttr);

  rampList.forEach((p, i) => {
    const size = platformSize(p);
    const center = platformCenter(p);
    const info = rampInfo(p.kind);
    const alongX = info.axis === "x";
    const spin = info.sign >= 0 ? (alongX ? Math.PI / 2 : 0) : alongX ? -Math.PI / 2 : Math.PI;
    // The wedge's run lies on local z; for X-axis ramps we scale local z by the world x extent.
    const localScale: Vec3Tuple = alongX ? [size[2], size[1], size[0]] : size;
    composeInstanceMatrix(localScale, spin, center[0], center[1], center[2]);
    rampMesh.setMatrixAt(i, scratchMatrix);
    const pillar = isPillar(size);
    rampAttr.setXYZW(i, 0, 0, 1, pillar ? 1 : 0);
    const slope = rampSlope(p.kind, size);
    rampSlopeAttr.setXY(i, slope.axis, slope.cos);
    records.push({
      id: p.id,
      center,
      size,
      localScale,
      spin,
      phase: phaseForId(p.id),
      pillar,
      mesh: rampMesh,
      attr: rampAttr,
      index: i,
      touched: false,
      activeFrame: false,
    });
  });
  rampMesh.instanceMatrix.needsUpdate = true;
  rampAttr.needsUpdate = true;
  rampSlopeAttr.needsUpdate = true;
  holder.add(rampMesh);

  return { holder, records, timeUniform };
}

function disposeBuilt(built: BuiltPlatforms): void {
  for (const child of [...built.holder.children]) {
    if (child instanceof InstancedMesh) {
      child.geometry.dispose();
      const material = child.material;
      if (Array.isArray(material)) {
        for (const m of material) m.dispose();
      } else {
        material.dispose();
      }
    }
  }
}

function resetRecord(rec: InstanceRecord): void {
  composeInstanceMatrix(rec.localScale, rec.spin, rec.center[0], rec.center[1], rec.center[2]);
  rec.mesh.setMatrixAt(rec.index, scratchMatrix);
  rec.attr.setXYZW(rec.index, 0, 0, 1, rec.pillar ? 1 : 0);
  rec.mesh.instanceMatrix.needsUpdate = true;
  rec.attr.needsUpdate = true;
  rec.touched = false;
}

/**
 * Platforms at scale: four InstancedMeshes (NORMAL, CRUMBLE, JUMP_PAD boxes, plus one
 * shared wedge mesh for all ramp kinds), world-space procedural checkerboard tops, glowing
 * rim on the top face, dark-stone shader path for tall thin platforms (pillars). Tile
 * states animate via per-instance matrix/attribute writes — no geometry rebuilds.
 */
export function Platforms(props: {
  platforms: readonly Platform[];
  theme: Theme;
  tiles: readonly TileState[];
  icy: boolean;
}): JSX.Element {
  const { platforms, theme, tiles, icy } = props;
  const built = useMemo(() => buildGroups(platforms, theme, icy), [platforms, theme, icy]);
  const builtRef = useRef<BuiltPlatforms | null>(null);
  useEffect(() => {
    builtRef.current = built;
  }, [built]);
  useEffect(() => () => disposeBuilt(built), [built]);

  const tilesRef = useRef(tiles);
  const tileMapRef = useRef(new Map<number, number>());
  const goneStartRef = useRef(new Map<number, number>());
  useEffect(() => {
    tilesRef.current = tiles;
  }, [tiles]);
  useEffect(() => {
    goneStartRef.current.clear();
  }, [built]);

  useFrame((state) => {
    const current = builtRef.current;
    if (!current) return;
    const now = state.clock.elapsedTime;
    current.timeUniform.value = now;
    const tileMap = tileMapRef.current;
    tileMap.clear();
    for (const t of tilesRef.current) {
      tileMap.set(t.id, t.state);
      if (t.state === TILE_GONE && !goneStartRef.current.has(t.id)) {
        goneStartRef.current.set(t.id, now);
      }
    }

    for (const rec of current.records) {
      // Per-instance animation bookkeeping on the three.js records built once per level; mutated only here in
      // useFrame (never during render), which is the intended R3F pattern.
      // eslint-disable-next-line react-hooks/immutability
      rec.activeFrame = false;
      const t = tileMap.get(rec.id);
      if (t !== TILE_WARN && t !== TILE_FRETTED && t !== TILE_GONE) continue;

      let ox = 0;
      let oz = 0;
      let sink = 0;
      let fade = 1;
      let hide = false;
      if (t === TILE_WARN) {
        ox = Math.sin(now * 34 + rec.phase) * SHAKE_AMP;
        oz = Math.cos(now * 27 + rec.phase) * SHAKE_AMP;
      }
      if (t === TILE_GONE) {
        const start = goneStartRef.current.get(rec.id) ?? now;
        const k = Math.min(1, Math.max(0, (now - start) / GONE_SINK_TIME));
        sink = k * k * (GONE_SINK_DEPTH + rec.size[1]);
        fade = 1 - k;
        if (k >= 1) {
          fade = 0;
          hide = true;
        }
      }

      if (hide) {
        composeInstanceMatrix([0.0001, 0.0001, 0.0001], rec.spin, rec.center[0] + ox, rec.center[1] - sink, rec.center[2] + oz);
      } else {
        composeInstanceMatrix(rec.localScale, rec.spin, rec.center[0] + ox, rec.center[1] - sink, rec.center[2] + oz);
      }
      rec.mesh.setMatrixAt(rec.index, scratchMatrix);
      rec.attr.setXYZW(rec.index, t === TILE_WARN ? 1 : 0, t === TILE_FRETTED ? 1 : 0, fade, rec.pillar ? 1 : 0);
      rec.mesh.instanceMatrix.needsUpdate = true;
      rec.attr.needsUpdate = true;
      rec.touched = true;
      rec.activeFrame = true;
    }
    for (const rec of current.records) {
      if (rec.touched && !rec.activeFrame) resetRecord(rec);
    }
  });

  return <primitive object={built.holder} />;
}
