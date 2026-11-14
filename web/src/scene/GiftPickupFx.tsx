"use client";

// Sparkle burst when a gift is collected. Purely visual. The store is the
// source of truth: `giftsTaken` / `goldenTaken` only grow, so bursts are keyed
// off newly-appeared ids and positioned at the matching level gift.
import { useEffect, useMemo, useRef } from "react";
import type { JSX } from "react";
import { useFrame } from "@react-three/fiber";
import {
  AdditiveBlending,
  Color,
  DynamicDrawUsage,
  InstancedMesh,
  Matrix4,
  MeshBasicMaterial,
  OctahedronGeometry,
  Quaternion,
  Vector3,
} from "three";
import type { LevelData } from "@/protocol/generated";
import { useGameStore } from "@/game/store";
import { COMMON } from "./theme";
import { burstParticles, findGiftPosition, newlyTaken } from "./giftPickup";
import type { BurstParticle } from "./giftPickup";

const PARTICLES_PER_BURST = 12;
const MAX_BURSTS = 8;
const LIFETIME_S = 0.7;
const GRAVITY = 4.0;
const PARTICLE_RADIUS = 0.06;

const GOLD_BURST = "#ffd75e";

interface LiveBurst {
  golden: boolean;
  origin: { x: number; y: number; z: number };
  /** Set on the first frame so the burst lives on the scene clock. */
  start: number | null;
  particles: readonly BurstParticle[];
}

export function GiftPickup({ level }: { level: LevelData }): JSX.Element {
  const geometry = useMemo(() => new OctahedronGeometry(PARTICLE_RADIUS), []);
  const material = useMemo(
    () =>
      new MeshBasicMaterial({
        color: "#ffffff",
        transparent: true,
        opacity: 0.95,
        blending: AdditiveBlending,
        depthWrite: false,
        toneMapped: false,
        fog: false,
      }),
    [],
  );
  useEffect(() => () => geometry.dispose(), [geometry]);
  useEffect(() => () => material.dispose(), [material]);

  const capacity = MAX_BURSTS * PARTICLES_PER_BURST;
  const meshRef = useRef<InstancedMesh>(null);
  useEffect(() => {
    const mesh = meshRef.current;
    if (mesh) {
      mesh.instanceMatrix.setUsage(DynamicDrawUsage);
      mesh.count = 0;
      mesh.instanceMatrix.needsUpdate = true;
    }
  }, [geometry, material]);

  const burstsRef = useRef<LiveBurst[]>([]);
  const scratchRef = useRef<{
    mat: Matrix4;
    pos: Vector3;
    quat: Quaternion;
    scale: Vector3;
    up: Vector3;
  } | null>(null);
  const colorRef = useRef(new Color());

  const giftsTaken = useGameStore((s) => s.giftsTaken);
  const goldenTaken = useGameStore((s) => s.goldenTaken);
  const prevRef = useRef<{ normal: readonly number[]; golden: readonly number[] } | null>(null);

  useEffect(() => {
    const prev = prevRef.current;
    if (prev === null) {
      // First observation on this level: remember what is already taken
      // instead of bursting every gift when joining an in-progress run.
      prevRef.current = { normal: giftsTaken, golden: goldenTaken };
      return;
    }
    const spawn = (id: number, golden: boolean) => {
      const position = findGiftPosition(level.gifts, id);
      if (position === null) return;
      const bursts = burstsRef.current;
      if (bursts.length >= MAX_BURSTS) bursts.shift();
      bursts.push({
        golden,
        origin: position,
        start: null,
        particles: burstParticles(`${golden ? "golden" : "normal"}:${id}`, PARTICLES_PER_BURST),
      });
    };
    for (const id of newlyTaken(prev.normal, giftsTaken)) spawn(id, false);
    for (const id of newlyTaken(prev.golden, goldenTaken)) spawn(id, true);
    prevRef.current = { normal: giftsTaken, golden: goldenTaken };
  }, [giftsTaken, goldenTaken, level]);

  useFrame((state) => {
    const mesh = meshRef.current;
    if (!mesh) return;
    let scratch = scratchRef.current;
    if (scratch === null) {
      scratch = {
        mat: new Matrix4(),
        pos: new Vector3(),
        quat: new Quaternion(),
        scale: new Vector3(),
        up: new Vector3(0, 1, 0),
      };
      scratchRef.current = scratch;
    }
    const t = state.clock.elapsedTime;
    const live: LiveBurst[] = [];
    let n = 0;
    for (const burst of burstsRef.current) {
      const start = burst.start ?? t;
      const age = t - start;
      if (age >= LIFETIME_S) continue;
      live.push(start === burst.start ? burst : { ...burst, start });
      const lifeT = age / LIFETIME_S;
      const fade = Math.max(0.05, 1 - lifeT);
      colorRef.current.set(burst.golden ? GOLD_BURST : COMMON.gift.glow);
      for (const p of burst.particles) {
        if (n >= capacity) break;
        const travelled = p.speed * age * (1 - 0.45 * lifeT);
        scratch.pos.set(
          burst.origin.x + p.dirX * travelled,
          burst.origin.y + p.dirY * travelled - 0.5 * GRAVITY * age * age,
          burst.origin.z + p.dirZ * travelled,
        );
        scratch.quat.setFromAxisAngle(scratch.up, age * 5 + p.size);
        scratch.scale.setScalar(p.size * fade);
        scratch.mat.compose(scratch.pos, scratch.quat, scratch.scale);
        mesh.setMatrixAt(n, scratch.mat);
        mesh.setColorAt(n, colorRef.current);
        n += 1;
      }
    }
    burstsRef.current = live;
    mesh.count = n;
    mesh.instanceMatrix.needsUpdate = true;
    if (mesh.instanceColor) mesh.instanceColor.needsUpdate = true;
  });

  return (
    <instancedMesh
      ref={meshRef}
      args={[geometry, material, capacity]}
      frustumCulled={false}
      dispose={null}
    />
  );
}
