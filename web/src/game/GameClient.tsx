"use client";

import { Canvas, useFrame, useThree } from "@react-three/fiber";
import { useEffect, useRef, useState } from "react";
import type { Group, Mesh } from "three";
import type { LevelData } from "@/protocol/generated";
import { bindingFor } from "./input";
import { defaultServerUrl } from "./net";
import { GameSession } from "./session";
import { WasmStepper } from "./sim";
import { useGameStore } from "./store";

const MAX_REMOTE = 8;
const MOUSE_SENSITIVITY = 0.0025;
const CAMERA_DISTANCE = 8;
const PLAYER_SIZE: [number, number, number] = [0.8, 1.8, 0.8];

function World({ session, level }: { session: GameSession; level: LevelData }) {
  const player = useRef<Mesh>(null);
  const remotes = useRef<(Group | null)[]>([]);
  const camera = useThree((s) => s.camera);

  useFrame((_, dt) => {
    session.update(dt);
    const p = session.playerPosition(dt);
    if (p && player.current) {
      player.current.position.set(p.x, p.y + PLAYER_SIZE[1] / 2, p.z);
      const cp = Math.cos(session.pitch);
      camera.position.set(
        p.x + Math.sin(session.yaw) * cp * CAMERA_DISTANCE,
        p.y + 1.4 + Math.sin(session.pitch) * CAMERA_DISTANCE,
        p.z + Math.cos(session.yaw) * cp * CAMERA_DISTANCE,
      );
      camera.lookAt(p.x, p.y + 1.4, p.z);
    }
    const others = [...session.remotePlayers(performance.now()).values()];
    remotes.current.forEach((g, i) => {
      if (!g) return;
      const o = others[i];
      g.visible = !!o;
      if (o) g.position.set(o.x, o.y + PLAYER_SIZE[1] / 2, o.z);
    });
  });

  return (
    <>
      <color attach="background" args={["#07060d"]} />
      <fog attach="fog" args={["#07060d", 30, 90]} />
      <ambientLight intensity={0.5} />
      <directionalLight position={[8, 20, 6]} intensity={1.6} />
      {level.platforms.map((pl) => {
        const { min, max } = pl.bounds;
        return (
          <mesh key={pl.id} position={[(min.x + max.x) / 2, (min.y + max.y) / 2, (min.z + max.z) / 2]}>
            <boxGeometry args={[max.x - min.x, max.y - min.y, max.z - min.z]} />
            <meshStandardMaterial color="#5b5a66" />
          </mesh>
        );
      })}
      <mesh ref={player}>
        <boxGeometry args={PLAYER_SIZE} />
        <meshStandardMaterial color="#ff3db8" emissive="#3a0830" />
      </mesh>
      {Array.from({ length: MAX_REMOTE }, (_, i) => (
        <group key={i} ref={(g) => void (remotes.current[i] = g)} visible={false}>
          <mesh>
            <boxGeometry args={PLAYER_SIZE} />
            <meshStandardMaterial color="#38e8ff" />
          </mesh>
        </group>
      ))}
    </>
  );
}

function Hud() {
  const { status, error, serverTick, pingMs, playerCount, pointerLocked } = useGameStore();
  return (
    <div className="pointer-events-none absolute inset-0 select-none p-4 font-mono text-sm text-white">
      <div className="inline-block rounded bg-black/50 px-3 py-2">
        <div>
          server: <span data-testid="conn-status">{status}</span>
        </div>
        <div>
          tick: <span data-testid="server-tick">{serverTick}</span> · ping: {pingMs ?? "–"} ms · players: {playerCount}
        </div>
      </div>
      {error && <div className="mt-2 inline-block rounded bg-red-900/80 px-3 py-2">{error}</div>}
      {!pointerLocked && status === "connected" && (
        <div className="absolute bottom-6 left-1/2 -translate-x-1/2 rounded bg-black/60 px-4 py-2">
          click to aim · WASD move · Space jump
        </div>
      )}
    </div>
  );
}

export default function GameClient() {
  const [session, setSession] = useState<GameSession | null>(null);
  const [level, setLevel] = useState<LevelData | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
  const container = useRef<HTMLDivElement>(null);

  useEffect(() => {
    let disposed = false;
    let s: GameSession | null = null;
    WasmStepper.load()
      .then((stepper) => {
        if (disposed) return;
        s = new GameSession(stepper, defaultServerUrl(), "blocky");
        s.onLevel = setLevel;
        setSession(s);
      })
      .catch((e: unknown) => setLoadError(String(e)));
    return () => {
      disposed = true;
      s?.dispose();
    };
  }, []);

  useEffect(() => {
    if (!session) return;
    const key = (down: boolean) => (e: KeyboardEvent) => {
      const b = bindingFor(e.code);
      if (!b) return;
      session.keys[b] = down;
      e.preventDefault();
    };
    const onDown = key(true);
    const onUp = key(false);
    const onMove = (e: MouseEvent) => {
      if (document.pointerLockElement !== container.current) return;
      session.yaw -= e.movementX * MOUSE_SENSITIVITY;
      session.pitch = Math.min(1.3, Math.max(-0.2, session.pitch + e.movementY * MOUSE_SENSITIVITY));
    };
    const onMouse = (down: boolean) => (e: MouseEvent) => {
      if (e.button === 2) session.rightMouse = down;
    };
    const onMouseDown = onMouse(true);
    const onMouseUp = onMouse(false);
    const onLock = () => useGameStore.getState().set({ pointerLocked: document.pointerLockElement === container.current });
    const onBlur = () => {
      for (const k of Object.keys(session.keys) as (keyof typeof session.keys)[]) session.keys[k] = false;
      session.rightMouse = false;
    };
    window.addEventListener("keydown", onDown);
    window.addEventListener("keyup", onUp);
    window.addEventListener("mousemove", onMove);
    window.addEventListener("mousedown", onMouseDown);
    window.addEventListener("mouseup", onMouseUp);
    window.addEventListener("blur", onBlur);
    document.addEventListener("pointerlockchange", onLock);
    return () => {
      window.removeEventListener("keydown", onDown);
      window.removeEventListener("keyup", onUp);
      window.removeEventListener("mousemove", onMove);
      window.removeEventListener("mousedown", onMouseDown);
      window.removeEventListener("mouseup", onMouseUp);
      window.removeEventListener("blur", onBlur);
      document.removeEventListener("pointerlockchange", onLock);
    };
  }, [session]);

  return (
    <div
      ref={container}
      className="relative h-screen w-screen overflow-hidden bg-[#07060d]"
      onClick={() => void container.current?.requestPointerLock()}
      onContextMenu={(e) => e.preventDefault()}
    >
      {session && level ? (
        <Canvas camera={{ fov: 70, near: 0.1, far: 200 }} dpr={[1, 2]}>
          <World session={session} level={level} />
        </Canvas>
      ) : (
        <div className="flex h-full items-center justify-center font-mono text-white/70">
          {loadError ?? "entering the void…"}
        </div>
      )}
      <Hud />
    </div>
  );
}
