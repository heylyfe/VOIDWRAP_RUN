"use client";

import { useRef } from "react";
import type { JSX } from "react";
import { useFrame } from "@react-three/fiber";
import { AdditiveBlending, Quaternion, Vector3 } from "three";
import type { Group, MeshBasicMaterial } from "three";
import { CLASS_GRAPPLER } from "@/protocol/generated";
import type { GameSession } from "@/game/session";
import { classColor } from "./classColors";

const UP = new Vector3(0, 1, 0);
const SCRATCH_FROM = new Vector3();
const SCRATCH_TO = new Vector3();
const SCRATCH_MID = new Vector3();
const SCRATCH_DIR = new Vector3();
const SCRATCH_Q = new Quaternion();

/**
 * Grappler ability: a thin glowing line from the local player's chest to the grapple
 * target point. The server sends the target point in ControllerState.abilityDir while
 * the ability is active, so no local simulation of the rope is needed.
 */
export function GrappleLine({ session }: { session: GameSession }): JSX.Element {
  const groupRef = useRef<Group>(null);
  const coreMatRef = useRef<MeshBasicMaterial>(null);
  const color = classColor(CLASS_GRAPPLER);

  useFrame((state) => {
    const group = groupRef.current;
    if (!group) return;
    const c = session.playerController();
    const show = !!c && c.class === CLASS_GRAPPLER && c.abilityActiveFrames > 0;
    group.visible = show;
    if (!c || !show) return;
    SCRATCH_FROM.set(c.pos.x, c.pos.y + 1.1, c.pos.z);
    SCRATCH_TO.set(c.abilityDir.x, c.abilityDir.y, c.abilityDir.z);
    SCRATCH_MID.addVectors(SCRATCH_FROM, SCRATCH_TO).multiplyScalar(0.5);
    SCRATCH_DIR.subVectors(SCRATCH_TO, SCRATCH_FROM);
    const len = SCRATCH_DIR.length();
    if (len < 0.05) {
      group.visible = false;
      return;
    }
    group.position.copy(SCRATCH_MID);
    SCRATCH_Q.setFromUnitVectors(UP, SCRATCH_DIR.normalize());
    group.quaternion.copy(SCRATCH_Q);
    group.scale.set(1, len, 1);
    const coreMat = coreMatRef.current;
    if (coreMat) coreMat.opacity = 0.8 + 0.2 * Math.sin(state.clock.elapsedTime * 18);
  });

  return (
    <group ref={groupRef} visible={false}>
      <mesh>
        <cylinderGeometry args={[0.028, 0.028, 1, 6, 1, true]} />
        <meshBasicMaterial
          ref={coreMatRef}
          color={color}
          transparent
          opacity={0.95}
          blending={AdditiveBlending}
          depthWrite={false}
          toneMapped={false}
        />
      </mesh>
      <mesh>
        <cylinderGeometry args={[0.075, 0.075, 1, 6, 1, true]} />
        <meshBasicMaterial
          color={color}
          transparent
          opacity={0.22}
          blending={AdditiveBlending}
          depthWrite={false}
          toneMapped={false}
        />
      </mesh>
    </group>
  );
}
