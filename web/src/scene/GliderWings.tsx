"use client";

import { useEffect, useMemo, useRef } from "react";
import type { JSX } from "react";
import { useFrame } from "@react-three/fiber";
import { DoubleSide, MeshStandardMaterial } from "three";
import type { Group } from "three";
import { CLASS_GLIDER } from "@/protocol/generated";
import type { GameSession } from "@/game/session";
import { classColor } from "./classColors";

const FOLDED = 0.95;
const OPEN = 0.06;
const OPEN_SPEED = 9;

/**
 * Glider ability: small wing planes (+ streamers) hinged at the shoulders that unfold
 * while gliding (CLASS_GLIDER, airborne, vel.y < 0, ability active). Reads the predicted
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
  useEffect(() => () => wingMat.dispose(), [wingMat]);
  useEffect(() => () => streamerMat.dispose(), [streamerMat]);

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
      <group ref={leftRef} position={[-0.32, 0, 0]}>
        <mesh position={[-0.55, 0.08, 0.05]} rotation={[0, -0.22, 0.18]} material={wingMat}>
          <planeGeometry args={[1.15, 0.42]} />
        </mesh>
        <mesh position={[-1.0, -0.12, 0.18]} rotation={[0.35, -0.35, 0.1]} material={streamerMat}>
          <planeGeometry args={[0.62, 0.09]} />
        </mesh>
      </group>
      <group ref={rightRef} position={[0.32, 0, 0]}>
        <mesh position={[0.55, 0.08, 0.05]} rotation={[0, 0.22, -0.18]} material={wingMat}>
          <planeGeometry args={[1.15, 0.42]} />
        </mesh>
        <mesh position={[1.0, -0.12, 0.18]} rotation={[0.35, 0.35, -0.1]} material={streamerMat}>
          <planeGeometry args={[0.62, 0.09]} />
        </mesh>
      </group>
    </group>
  );
}
