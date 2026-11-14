"use client";

import { useEffect, useMemo, useRef } from "react";
import type { JSX } from "react";
import { useFrame } from "@react-three/fiber";
import {
  BoxGeometry,
  ConeGeometry,
  CylinderGeometry,
  DoubleSide,
  MeshStandardMaterial,
} from "three";
import type { Group } from "three";
import { CLASS_GLIDER } from "@/protocol/generated";
import type { GameSession } from "@/game/session";
import { classColor } from "./classColors";

const FOLDED = 0.95;
const OPEN = 0.06;
const OPEN_SPEED = 9;

/**
 * Glider ability: a small procedural hang-glider (leading-edge spars, swept
 * membrane panels, kingpost, A-frame control bar, keel and hang strap) hinged
 * at the shoulders that unfolds while gliding (CLASS_GLIDER, airborne,
 * vel.y < 0, ability active). Built only from three.js primitives via the
 * existing pipeline — no new assets, no external models. Reads the predicted
 * local controller state each frame; smoothed open/close, no React state.
 */
export function GliderWings({ session }: { session: GameSession }): JSX.Element {
  const rootRef = useRef<Group>(null);
  const leftRef = useRef<Group>(null);
  const rightRef = useRef<Group>(null);
  const phase = useRef(0);
  const color = classColor(CLASS_GLIDER);
  const wingMat = useMemo(
    () =>
      new MeshStandardMaterial({
        color: "#0e2b33",
        emissive: color,
        emissiveIntensity: 1.6,
        side: DoubleSide,
        transparent: true,
        opacity: 0.92,
        toneMapped: false,
      }),
    [color],
  );
  const streamerMat = useMemo(
    () =>
      new MeshStandardMaterial({
        color: "#0e2b33",
        emissive: color,
        emissiveIntensity: 2.2,
        side: DoubleSide,
        transparent: true,
        opacity: 0.55,
        depthWrite: false,
        toneMapped: false,
      }),
    [color],
  );
  const frameMat = useMemo(
    () =>
      new MeshStandardMaterial({
        color: "#23333a",
        emissive: color,
        emissiveIntensity: 0.35,
        metalness: 0.55,
        roughness: 0.45,
      }),
    [color],
  );
  const geometries = useMemo(
    () => ({
      spar: new BoxGeometry(1.32, 0.05, 0.1),
      panel: new BoxGeometry(1.15, 0.03, 0.58),
      tip: new BoxGeometry(0.52, 0.03, 0.4),
      ribbon: new BoxGeometry(0.55, 0.02, 0.1),
      keel: new ConeGeometry(0.08, 0.55, 10),
      kingpost: new CylinderGeometry(0.035, 0.035, 0.34, 8),
      leg: new CylinderGeometry(0.028, 0.028, 0.62, 8),
      grip: new CylinderGeometry(0.03, 0.03, 0.66, 8),
      strap: new CylinderGeometry(0.05, 0.05, 0.78, 8),
    }),
    [],
  );
  useEffect(() => () => wingMat.dispose(), [wingMat]);
  useEffect(() => () => streamerMat.dispose(), [streamerMat]);
  useEffect(() => () => frameMat.dispose(), [frameMat]);
  useEffect(
    () => () => {
      for (const geometry of Object.values(geometries)) geometry.dispose();
    },
    [geometries],
  );

  useFrame((state, delta) => {
    const c = session.playerController();
    const gliding = !!c && c.class === CLASS_GLIDER && !c.grounded && c.vel.y < 0 && c.abilityActiveFrames > 0;
    const target = gliding ? 1 : 0;
    phase.current += (target - phase.current) * Math.min(1, delta * OPEN_SPEED);
    const p = phase.current;
    const root = rootRef.current;
    if (root) root.visible = p > 0.03;
    const fold = FOLDED + (OPEN - FOLDED) * p;
    const flap = Math.sin(state.clock.elapsedTime * 7) * 0.07 * p;
    const left = leftRef.current;
    const right = rightRef.current;
    if (left) left.rotation.z = fold + flap;
    if (right) right.rotation.z = -(fold + flap);
  });

  return (
    <group ref={rootRef} visible={false} position={[0, 1.32, 0.3]}>
      {/* Central airframe: keel nose forward (avatar faces -Z), kingpost and strap. */}
      <mesh geometry={geometries.keel} material={frameMat} position={[0, 0.06, -0.42]} rotation={[-Math.PI / 2, 0, 0]} />
      <mesh geometry={geometries.kingpost} material={frameMat} position={[0, 0.26, 0.08]} />
      <mesh geometry={geometries.strap} material={frameMat} position={[0, -0.36, 0.02]} />
      {/* A-frame control bar hanging below the wing centre. */}
      <mesh geometry={geometries.leg} material={frameMat} position={[-0.3, -0.28, -0.18]} rotation={[0.35, 0, 0.45]} />
      <mesh geometry={geometries.leg} material={frameMat} position={[0.3, -0.28, -0.18]} rotation={[0.35, 0, -0.45]} />
      <mesh geometry={geometries.grip} material={frameMat} position={[0, -0.52, -0.34]} rotation={[0, 0, Math.PI / 2]} />
      <group ref={leftRef} position={[-0.32, 0, 0]}>
        <mesh geometry={geometries.spar} material={frameMat} position={[-0.66, 0.03, 0.02]} rotation={[0, 0.26, 0]} />
        <mesh geometry={geometries.panel} material={wingMat} position={[-0.56, 0.02, 0.14]} rotation={[0, 0.26, 0.09]} />
        <mesh geometry={geometries.tip} material={wingMat} position={[-1.12, 0.0, 0.48]} rotation={[0, 0.52, 0.2]} />
        <mesh geometry={geometries.ribbon} material={streamerMat} position={[-1.18, -0.16, 0.62]} rotation={[0.45, 0.52, 0.15]} />
      </group>
      <group ref={rightRef} position={[0.32, 0, 0]}>
        <mesh geometry={geometries.spar} material={frameMat} position={[0.66, 0.03, 0.02]} rotation={[0, -0.26, 0]} />
        <mesh geometry={geometries.panel} material={wingMat} position={[0.56, 0.02, 0.14]} rotation={[0, -0.26, -0.09]} />
        <mesh geometry={geometries.tip} material={wingMat} position={[1.12, 0.0, 0.48]} rotation={[0, -0.52, -0.2]} />
        <mesh geometry={geometries.ribbon} material={streamerMat} position={[1.18, -0.16, 0.62]} rotation={[0.45, -0.52, -0.15]} />
      </group>
    </group>
  );
}
