"use client";

import { useRef } from "react";
import type { JSX } from "react";
import { useFrame } from "@react-three/fiber";
import { AdditiveBlending } from "three";
import type { Group, MeshBasicMaterial } from "three";
import type { AvatarMotion } from "./avatarPose";

const FADE_RATE = 50; // exponential approach; ~95% faded in/out within ~60 ms
const MIN_VISIBLE = 0.01;

export function DashStreak(props: {
  motion: { current: AvatarMotion };
  color: string;
}): JSX.Element {
  const groupRef = useRef<Group>(null);
  const material0Ref = useRef<MeshBasicMaterial>(null);
  const material1Ref = useRef<MeshBasicMaterial>(null);
  const material2Ref = useRef<MeshBasicMaterial>(null);
  const fadeRef = useRef(0);

  useFrame((_state, delta) => {
    const fade = fadeRef.current;
    const target = props.motion.current.dashing ? 1 : 0;
    const k = Math.min(1, delta * FADE_RATE);
    const next = fade + (target - fade) * k;
    fadeRef.current = next;

    const group = groupRef.current;
    if (group === null) {
      return;
    }
    group.visible = next > MIN_VISIBLE;

    const m0 = material0Ref.current;
    if (m0 !== null) {
      m0.opacity = 0.35 * next;
    }
    const m1 = material1Ref.current;
    if (m1 !== null) {
      m1.opacity = 0.2 * next;
    }
    const m2 = material2Ref.current;
    if (m2 !== null) {
      m2.opacity = 0.1 * next;
    }
  });

  return (
    <group ref={groupRef} visible={false}>
      {/* Streaks trail behind the avatar: local -Z is forward, so +Z is behind */}
      <mesh position={[0, 0.8, 0.5]}>
        <boxGeometry args={[0.6, 1.6, 0.3]} />
        <meshBasicMaterial
          ref={material0Ref}
          color={props.color}
          transparent
          opacity={0}
          blending={AdditiveBlending}
          depthWrite={false}
        />
      </mesh>
      <mesh position={[0, 0.8, 1.0]}>
        <boxGeometry args={[0.6, 1.6, 0.3]} />
        <meshBasicMaterial
          ref={material1Ref}
          color={props.color}
          transparent
          opacity={0}
          blending={AdditiveBlending}
          depthWrite={false}
        />
      </mesh>
      <mesh position={[0, 0.8, 1.5]}>
        <boxGeometry args={[0.6, 1.6, 0.3]} />
        <meshBasicMaterial
          ref={material2Ref}
          color={props.color}
          transparent
          opacity={0}
          blending={AdditiveBlending}
          depthWrite={false}
        />
      </mesh>
    </group>
  );
}
