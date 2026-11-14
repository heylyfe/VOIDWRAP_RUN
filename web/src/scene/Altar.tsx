"use client";

import { useEffect, useMemo, useRef } from "react";
import type { JSX } from "react";
import { useFrame } from "@react-three/fiber";
import { CanvasTexture, SRGBColorSpace } from "three";
import type { Group, Mesh, MeshStandardMaterial, Sprite } from "three";
import type { Altar } from "@/protocol/generated";
import type { GameSession } from "@/game/session";
import { COMMON } from "./theme";
import { LightPool } from "./LightPool";

const STONE = "#71717a";
const STONE_DARK = "#4a4a52";
const SIGIL_LIT = COMMON.beacon.beam;
const SIGIL_DARK = "#16161c";
const CAP_DARK = "#33333c";
const PROMPT_HEIGHT = 2.55;
const PROMPT_RANGE_SQ = 4; // 2 units

function traceRoundRect(
  ctx: CanvasRenderingContext2D,
  x: number,
  y: number,
  w: number,
  h: number,
  r: number,
): void {
  ctx.beginPath();
  ctx.moveTo(x + r, y);
  ctx.arcTo(x + w, y, x + w, y + h, r);
  ctx.arcTo(x + w, y + h, x, y + h, r);
  ctx.arcTo(x, y + h, x, y, r);
  ctx.arcTo(x, y, x + w, y, r);
  ctx.closePath();
}

/** Canvas-drawn "E" key prompt; created once, shared by every altar. */
function createPromptTexture(): CanvasTexture | null {
  if (typeof document === "undefined") return null;
  const canvas = document.createElement("canvas");
  canvas.width = 128;
  canvas.height = 128;
  const ctx = canvas.getContext("2d");
  if (!ctx) return null;
  ctx.clearRect(0, 0, 128, 128);
  ctx.fillStyle = "rgba(8, 8, 14, 0.72)";
  traceRoundRect(ctx, 14, 18, 100, 92, 18);
  ctx.fill();
  ctx.strokeStyle = "rgba(255, 255, 255, 0.85)";
  ctx.lineWidth = 5;
  traceRoundRect(ctx, 14, 18, 100, 92, 18);
  ctx.stroke();
  ctx.fillStyle = "#ffffff";
  ctx.font = "bold 64px ui-monospace, Menlo, monospace";
  ctx.textAlign = "center";
  ctx.textBaseline = "middle";
  ctx.fillText("E", 64, 66);
  const tex = new CanvasTexture(canvas);
  tex.colorSpace = SRGBColorSpace;
  return tex;
}

function AltarShrine(props: {
  altar: Altar;
  used: boolean;
  prompt: CanvasTexture | null;
  session: GameSession;
}): JSX.Element {
  const { altar, used, prompt, session } = props;
  const sigilRef = useRef<Group>(null);
  const ringRef = useRef<Mesh>(null);
  const promptRef = useRef<Sprite>(null);
  const haloRef = useRef<MeshStandardMaterial>(null);

  useEffect(() => {
    const m = haloRef.current;
    if (!m) return;
    m.color.set(used ? CAP_DARK : SIGIL_LIT);
    m.emissive.set(used ? SIGIL_DARK : SIGIL_LIT);
  }, [used]);

  useFrame((state) => {
    const t = state.clock.elapsedTime;
    const sigil = sigilRef.current;
    if (sigil) {
      sigil.rotation.y = t * (used ? 0.25 : 1.4);
      sigil.position.y = 1.95 + Math.sin(t * 1.7) * 0.08;
    }
    const ring = ringRef.current;
    if (ring) ring.rotation.x = Math.PI / 2 + Math.sin(t * 1.9) * 0.16;
    const m = haloRef.current;
    if (m) m.emissiveIntensity = used ? 0.05 : 2.2 + Math.sin(t * 2.4) * 0.8;
    const spr = promptRef.current;
    if (spr) {
      const p = session.playerController();
      if (!p || used) {
        spr.visible = false;
      } else {
        const dx = p.pos.x - altar.pos.x;
        const dy = p.pos.y - altar.pos.y;
        const dz = p.pos.z - altar.pos.z;
        spr.visible = dx * dx + dy * dy + dz * dz <= PROMPT_RANGE_SQ;
      }
    }
  });

  const { x, y, z } = altar.pos;
  return (
    <group position={[x, y, z]}>
      <mesh position={[0, 0.18, 0]}>
        <cylinderGeometry args={[0.62, 0.78, 0.36, 8]} />
        <meshStandardMaterial color={STONE_DARK} roughness={0.95} />
      </mesh>
      <mesh position={[0, 0.85, 0]}>
        <cylinderGeometry args={[0.3, 0.42, 1.0, 8]} />
        <meshStandardMaterial color={STONE} roughness={0.9} />
      </mesh>
      <mesh position={[0, 1.42, 0]}>
        <boxGeometry args={[0.72, 0.2, 0.72]} />
        <meshStandardMaterial color={STONE_DARK} roughness={0.95} />
      </mesh>
      <group ref={sigilRef} position={[0, 1.95, 0]}>
        <mesh>
          <octahedronGeometry args={[0.2, 0]} />
          <meshStandardMaterial
            ref={haloRef}
            color={SIGIL_LIT}
            emissive={SIGIL_LIT}
            emissiveIntensity={2.6}
            toneMapped={false}
          />
        </mesh>
        <mesh ref={ringRef} rotation={[Math.PI / 2, 0, 0]}>
          <torusGeometry args={[0.36, 0.035, 8, 24]} />
          <meshStandardMaterial color={STONE} roughness={0.7} metalness={0.4} />
        </mesh>
      </group>
      {!used && <LightPool color={SIGIL_LIT} radius={1.5} intensity={0.4} />}
      {prompt && (
        <sprite ref={promptRef} position={[0, PROMPT_HEIGHT, 0]} scale={[0.74, 0.74, 1]} visible={false}>
          <spriteMaterial map={prompt} transparent depthWrite={false} toneMapped={false} />
        </sprite>
      )}
    </group>
  );
}

/** Small stone shrines with a floating sigil; go dark once used. "E" prompt within 2 units. */
export function Altars(props: {
  altars: readonly Altar[];
  used: readonly number[];
  session: GameSession;
}): JSX.Element {
  const prompt = useMemo(() => createPromptTexture(), []);
  useEffect(() => () => prompt?.dispose(), [prompt]);
  return (
    <group>
      {props.altars.map((altar) => (
        <AltarShrine
          key={altar.id}
          altar={altar}
          used={props.used.includes(altar.id)}
          prompt={prompt}
          session={props.session}
        />
      ))}
    </group>
  );
}
