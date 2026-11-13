"use client";

import { useRef } from "react";
import type { JSX } from "react";
import { useFrame } from "@react-three/fiber";
import type { Group } from "three";
import { PALETTE } from "./palette";

export const AVATAR_HEIGHT = 1.8;

export function BlockyAvatar(props: {
  bodyColor?: string;
  moving?: boolean;
}): JSX.Element {
  const leftArmRef = useRef<Group>(null);
  const rightArmRef = useRef<Group>(null);
  const leftLegRef = useRef<Group>(null);
  const rightLegRef = useRef<Group>(null);

  useFrame((state, delta) => {
    const ease = Math.min(1, delta * 8);
    const swing = Math.sin(state.clock.elapsedTime * 9) * 0.6;
    const leftArm = leftArmRef.current;
    const rightArm = rightArmRef.current;
    const leftLeg = leftLegRef.current;
    const rightLeg = rightLegRef.current;
    if (props.moving) {
      if (leftArm !== null) {
        leftArm.rotation.x = swing;
      }
      if (rightArm !== null) {
        rightArm.rotation.x = -swing;
      }
      if (leftLeg !== null) {
        leftLeg.rotation.x = -swing;
      }
      if (rightLeg !== null) {
        rightLeg.rotation.x = swing;
      }
    } else {
      if (leftArm !== null) {
        leftArm.rotation.x += (0 - leftArm.rotation.x) * ease;
      }
      if (rightArm !== null) {
        rightArm.rotation.x += (0 - rightArm.rotation.x) * ease;
      }
      if (leftLeg !== null) {
        leftLeg.rotation.x += (0 - leftLeg.rotation.x) * ease;
      }
      if (rightLeg !== null) {
        rightLeg.rotation.x += (0 - rightLeg.rotation.x) * ease;
      }
    }
  });

  const body = props.bodyColor ?? PALETTE.playerBody;

  return (
    <group>
      {/* Legs: pivot groups at the hips (y = 0.72), feet at y = 0 */}
      <group ref={leftLegRef} position={[0.13, 0.72, 0]}>
        <mesh position={[0, -0.36, 0]}>
          <boxGeometry args={[0.22, 0.72, 0.24]} />
          <meshStandardMaterial color={PALETTE.playerLegs} />
        </mesh>
      </group>
      <group ref={rightLegRef} position={[-0.13, 0.72, 0]}>
        <mesh position={[0, -0.36, 0]}>
          <boxGeometry args={[0.22, 0.72, 0.24]} />
          <meshStandardMaterial color={PALETTE.playerLegs} />
        </mesh>
      </group>

      {/* Torso */}
      <mesh position={[0, 1.01, 0]}>
        <boxGeometry args={[0.5, 0.58, 0.3]} />
        <meshStandardMaterial color={body} />
      </mesh>

      {/* Arms: pivot groups at the shoulders (y = 1.22), hands in skin tone */}
      <group ref={leftArmRef} position={[0.31, 1.22, 0]}>
        <mesh position={[0, -0.19, 0]}>
          <boxGeometry args={[0.14, 0.44, 0.14]} />
          <meshStandardMaterial color={body} />
        </mesh>
        <mesh position={[0, -0.48, 0]}>
          <boxGeometry args={[0.16, 0.16, 0.16]} />
          <meshStandardMaterial color={PALETTE.playerHeadHands} />
        </mesh>
      </group>
      <group ref={rightArmRef} position={[-0.31, 1.22, 0]}>
        <mesh position={[0, -0.19, 0]}>
          <boxGeometry args={[0.14, 0.44, 0.14]} />
          <meshStandardMaterial color={body} />
        </mesh>
        <mesh position={[0, -0.48, 0]}>
          <boxGeometry args={[0.16, 0.16, 0.16]} />
          <meshStandardMaterial color={PALETTE.playerHeadHands} />
        </mesh>
      </group>

      {/* Head (skin tone), y from 1.30 to 1.80 */}
      <mesh position={[0, 1.55, 0]}>
        <boxGeometry args={[0.5, 0.5, 0.42]} />
        <meshStandardMaterial color={PALETTE.playerHeadHands} />
      </mesh>

      {/* Face features on the -Z (forward) face of the head */}
      <mesh position={[0.12, 1.61, -0.215]}>
        <boxGeometry args={[0.07, 0.09, 0.03]} />
        <meshStandardMaterial color={PALETTE.playerFace} />
      </mesh>
      <mesh position={[-0.12, 1.61, -0.215]}>
        <boxGeometry args={[0.07, 0.09, 0.03]} />
        <meshStandardMaterial color={PALETTE.playerFace} />
      </mesh>
      <mesh position={[0, 1.45, -0.215]}>
        <boxGeometry args={[0.2, 0.05, 0.03]} />
        <meshStandardMaterial color={PALETTE.playerFace} />
      </mesh>
    </group>
  );
}
