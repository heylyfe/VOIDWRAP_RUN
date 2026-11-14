"use client";

import { useRef } from "react";
import type { JSX } from "react";
import { useFrame } from "@react-three/fiber";
import { AdditiveBlending, DoubleSide } from "three";
import type { Group, MeshBasicMaterial } from "three";
import { CLASS_BULWARK } from "@/protocol/generated";
import type { GameSession } from "@/game/session";
import { classColor } from "./classColors";

/** Covers the local 0.8 x 1.8 box with a little headroom. */
const DOME_RADIUS = 1.7;
const DOME_MIN_OPACITY = 0.18;
const DOME_OPACITY_RANGE = 0.14;
const WIRE_OPACITY = 0.5;
const RING_OPACITY = 0.55;

/**
 * Bulwark Aegis Bloom ward: a translucent hexagonal dome around the local player
 * while the predicted controller has wardFrames > 0. Remote players are skipped
 * (their ward state rides in the per-tick payload, not in our prediction).
 */
export function WardDome({ session }: { session: GameSession }): JSX.Element {
  const groupRef = useRef<Group>(null);
  const domeMatRef = useRef<MeshBasicMaterial>(null);
  const ringMatRef = useRef<MeshBasicMaterial>(null);
  const color = classColor(CLASS_BULWARK);

  useFrame((state) => {
    const group = groupRef.current;
    if (!group) return;
    const c = session.playerController();
    if (!c || c.wardFrames <= 0) {
      group.visible = false;
      return;
    }
    group.visible = true;
    group.position.set(c.pos.x, c.pos.y, c.pos.z);
    const t = state.clock.elapsedTime;
    const pulse = Math.sin(t * 6.0) * 0.5 + 0.5;
    const dome = domeMatRef.current;
    if (dome) dome.opacity = DOME_MIN_OPACITY + pulse * DOME_OPACITY_RANGE;
    const ring = ringMatRef.current;
    if (ring) ring.opacity = RING_OPACITY * (0.75 + 0.25 * pulse);
  });

  return (
    <group ref={groupRef} visible={false}>
      {/* Solid additive dome; 6 phi segments gives the hexagonal silhouette. */}
      <mesh>
        <sphereGeometry args={[DOME_RADIUS, 6, 4, 0, Math.PI * 2, 0, Math.PI / 2]} />
        <meshBasicMaterial
          ref={domeMatRef}
          color={color}
          transparent
          opacity={0.26}
          blending={AdditiveBlending}
          depthWrite={false}
          side={DoubleSide}
          toneMapped={false}
          fog={false}
        />
      </mesh>
      {/* Faint hex wireframe shell for readability against bright bloom. */}
      <mesh>
        <sphereGeometry args={[DOME_RADIUS * 1.02, 6, 3, 0, Math.PI * 2, 0, Math.PI / 2]} />
        <meshBasicMaterial
          color={color}
          transparent
          opacity={WIRE_OPACITY}
          wireframe
          blending={AdditiveBlending}
          depthWrite={false}
          toneMapped={false}
          fog={false}
        />
      </mesh>
      {/* Hexagon base ring on the ground. */}
      <mesh position={[0, 0.02, 0]} rotation={[-Math.PI / 2, 0, 0]}>
        <ringGeometry args={[DOME_RADIUS * 0.78, DOME_RADIUS * 1.02, 6]} />
        <meshBasicMaterial
          ref={ringMatRef}
          color={color}
          transparent
          opacity={RING_OPACITY}
          blending={AdditiveBlending}
          depthWrite={false}
          side={DoubleSide}
          toneMapped={false}
          fog={false}
        />
      </mesh>
    </group>
  );
}
