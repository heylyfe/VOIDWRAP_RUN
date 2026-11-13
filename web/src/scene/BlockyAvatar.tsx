"use client";

import { useRef } from "react";
import type { JSX } from "react";
import { useFrame } from "@react-three/fiber";
import type { Group } from "three";
import { PALETTE } from "./palette";
import { createPoseState, stepPose } from "./avatarPose";
import type { AvatarMotion } from "./avatarPose";

export const AVATAR_HEIGHT = 1.8;

const IDLE_MOTION: AvatarMotion = { speed: 0, vy: 0, grounded: true, dashing: false };

export function BlockyAvatar(props: {
  bodyColor?: string;
  motion?: { current: AvatarMotion };
}): JSX.Element {
  const poseStateRef = useRef(createPoseState());
  const outerRef = useRef<Group>(null);
  const torsoRef = useRef<Group>(null);
  const leftArmRef = useRef<Group>(null);
  const rightArmRef = useRef<Group>(null);
  const leftLegRef = useRef<Group>(null);
  const rightLegRef = useRef<Group>(null);

  useFrame((_state, delta) => {
    const motion = props.motion?.current ?? IDLE_MOTION;
    const pose = stepPose(poseStateRef.current, motion, delta);

    const outer = outerRef.current;
    if (outer !== null) {
      // Scale around the feet origin (group sits at y = 0) so feet stay planted.
      outer.position.y = pose.bodyY;
      outer.scale.set(pose.scaleXZ, pose.scaleY, pose.scaleXZ);
    }

    const torso = torsoRef.current;
    if (torso !== null) {
      torso.rotation.x = pose.torsoPitch;
    }
    const leftArm = leftArmRef.current;
    if (leftArm !== null) {
      leftArm.rotation.x = pose.leftArm;
      leftArm.rotation.z = pose.leftArmRoll;
    }
    const rightArm = rightArmRef.current;
    if (rightArm !== null) {
      rightArm.rotation.x = pose.rightArm;
      rightArm.rotation.z = -pose.rightArmRoll; // mirrored: outward is -Z rot on -X side
    }
    const leftLeg = leftLegRef.current;
    if (leftLeg !== null) {
      leftLeg.rotation.x = pose.leftLeg;
    }
    const rightLeg = rightLegRef.current;
    if (rightLeg !== null) {
      rightLeg.rotation.x = pose.rightLeg;
    }
  });

  const body = props.bodyColor ?? PALETTE.playerBody;

  return (
    <group ref={outerRef}>
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

      {/* Torso pivot at the hips: torsoPitch leans upper body + head + arms */}
      <group ref={torsoRef} position={[0, 0.72, 0]}>
        {/* Torso (was y = 1.01 abs, now 0.29 relative to hips) */}
        <mesh position={[0, 0.29, 0]}>
          <boxGeometry args={[0.5, 0.58, 0.3]} />
          <meshStandardMaterial color={body} />
        </mesh>

        {/* Arms: pivot groups at the shoulders (y = 1.22 abs, 0.50 rel), hands in skin tone */}
        <group ref={leftArmRef} position={[0.31, 0.5, 0]}>
          <mesh position={[0, -0.19, 0]}>
            <boxGeometry args={[0.14, 0.44, 0.14]} />
            <meshStandardMaterial color={body} />
          </mesh>
          <mesh position={[0, -0.48, 0]}>
            <boxGeometry args={[0.16, 0.16, 0.16]} />
            <meshStandardMaterial color={PALETTE.playerHeadHands} />
          </mesh>
        </group>
        <group ref={rightArmRef} position={[-0.31, 0.5, 0]}>
          <mesh position={[0, -0.19, 0]}>
            <boxGeometry args={[0.14, 0.44, 0.14]} />
            <meshStandardMaterial color={body} />
          </mesh>
          <mesh position={[0, -0.48, 0]}>
            <boxGeometry args={[0.16, 0.16, 0.16]} />
            <meshStandardMaterial color={PALETTE.playerHeadHands} />
          </mesh>
        </group>

        {/* Head (skin tone), y from 1.30 to 1.80 abs (0.58 to 1.08 rel) */}
        <mesh position={[0, 0.83, 0]}>
          <boxGeometry args={[0.5, 0.5, 0.42]} />
          <meshStandardMaterial color={PALETTE.playerHeadHands} />
        </mesh>

        {/* Face features on the -Z (forward) face of the head */}
        <mesh position={[0.12, 0.89, -0.215]}>
          <boxGeometry args={[0.07, 0.09, 0.03]} />
          <meshStandardMaterial color={PALETTE.playerFace} />
        </mesh>
        <mesh position={[-0.12, 0.89, -0.215]}>
          <boxGeometry args={[0.07, 0.09, 0.03]} />
          <meshStandardMaterial color={PALETTE.playerFace} />
        </mesh>
        <mesh position={[0, 0.73, -0.215]}>
          <boxGeometry args={[0.2, 0.05, 0.03]} />
          <meshStandardMaterial color={PALETTE.playerFace} />
        </mesh>
      </group>
    </group>
  );
}
