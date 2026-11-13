"use client";

import { Canvas, useFrame, useThree } from "@react-three/fiber";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import type { Group } from "three";
import { DEATH_ENEMY, type LevelData } from "@/protocol/generated";
import { GameHud } from "@/hud/GameHud";
import { LevelBanner } from "@/hud/LevelBanner";
import { RunSummary } from "@/hud/RunSummary";
import type { DeathCause } from "@/hud/format";
import { Atmosphere } from "@/scene/Atmosphere";
import { Beacon } from "@/scene/Beacon";
import { BlockyAvatar } from "@/scene/BlockyAvatar";
import { Chaser } from "@/scene/Chaser";
import { Debris } from "@/scene/Debris";
import { Effects } from "@/scene/Effects";
import { GiftBox } from "@/scene/GiftBox";
import { PALETTE } from "@/scene/palette";
import { Platforms } from "@/scene/Platforms";
import { themeForLevel } from "@/scene/theme";
import { bindingFor } from "./input";
import { defaultServerUrl } from "./net";
import { GameSession } from "./session";
import { WasmStepper } from "./sim";
import { useGameStore } from "./store";

const MAX_REMOTE = 8;
const MAX_ENEMIES = 8;
const MOUSE_SENSITIVITY = 0.0025;
const CAMERA_DISTANCE = 8;
const CAMERA_HEIGHT = 1.4;
/** Horizontal speed above which the avatar plays its walk cycle. */
const MOVING_SPEED = 0.5;
const ORIGIN = { x: 0, y: 0, z: 0 };

/** Centre and horizontal radius of a level, for placing the debris swarm around it. */
function levelBounds(level: LevelData): { center: { x: number; y: number; z: number }; radius: number } {
  let minX = Infinity, maxX = -Infinity, minZ = Infinity, maxZ = -Infinity, minY = Infinity, maxY = -Infinity;
  for (const p of level.platforms) {
    minX = Math.min(minX, p.bounds.min.x);
    maxX = Math.max(maxX, p.bounds.max.x);
    minY = Math.min(minY, p.bounds.min.y);
    maxY = Math.max(maxY, p.bounds.max.y);
    minZ = Math.min(minZ, p.bounds.min.z);
    maxZ = Math.max(maxZ, p.bounds.max.z);
  }
  if (!Number.isFinite(minX)) return { center: ORIGIN, radius: 30 };
  const center = { x: (minX + maxX) / 2, y: (minY + maxY) / 2, z: (minZ + maxZ) / 2 };
  return { center, radius: Math.max(20, Math.hypot(maxX - minX, maxZ - minZ) / 2 + 10) };
}

function World({ session, level }: { session: GameSession; level: LevelData }) {
  const player = useRef<Group>(null);
  const remotes = useRef<(Group | null)[]>([]);
  const enemies = useRef<(Group | null)[]>([]);
  const camera = useThree((s) => s.camera);
  const [moving, setMoving] = useState(false);
  const giftsTaken = useGameStore((s) => s.giftsTaken);
  const beaconOpen = giftsTaken.length >= level.gifts.length;
  const theme = themeForLevel(level.level);
  const bounds = useMemo(() => levelBounds(level), [level]);

  useFrame((_, dt) => {
    session.update(dt);
    const p = session.playerPosition(dt);
    const g = player.current;
    if (p && g) {
      g.position.set(p.x, p.y, p.z);
      const v = session.playerVelocity();
      const speed = v ? Math.hypot(v.x, v.z) : 0;
      if (v && speed > MOVING_SPEED) {
        // The avatar faces -Z at rotation 0.
        const target = Math.atan2(-v.x, -v.z);
        let d = target - g.rotation.y;
        d = Math.atan2(Math.sin(d), Math.cos(d)); // shortest turn
        g.rotation.y += d * Math.min(1, dt * 14);
      }
      if (speed > MOVING_SPEED !== moving) setMoving(speed > MOVING_SPEED);
      const cp = Math.cos(session.pitch);
      camera.position.set(
        p.x + Math.sin(session.yaw) * cp * CAMERA_DISTANCE,
        p.y + CAMERA_HEIGHT + Math.sin(session.pitch) * CAMERA_DISTANCE,
        p.z + Math.cos(session.yaw) * cp * CAMERA_DISTANCE,
      );
      camera.lookAt(p.x, p.y + CAMERA_HEIGHT, p.z);
    }
    const now = performance.now();
    let i = 0;
    for (const o of session.remotePlayers(now).values()) {
      const r = remotes.current[i++];
      if (!r) break;
      r.visible = true;
      r.position.set(o.x, o.y, o.z);
    }
    for (; i < MAX_REMOTE; i++) {
      const r = remotes.current[i];
      if (r) r.visible = false;
    }
    let e = 0;
    for (const o of session.enemyPositions(now).values()) {
      const r = enemies.current[e++];
      if (!r) break;
      r.visible = true;
      r.position.set(o.x, o.y, o.z);
    }
    for (; e < MAX_ENEMIES; e++) {
      const r = enemies.current[e];
      if (r) r.visible = false;
    }
  });

  return (
    <>
      <Atmosphere theme={theme} />
      <Debris theme={theme} center={bounds.center} radius={bounds.radius} seed={level.seed + 1} />
      <ambientLight intensity={0.7} />
      <hemisphereLight args={[theme.sky.zenith, theme.sky.horizon, 1.1]} />
      <directionalLight position={[8, 20, 6]} intensity={1.6} />
      <Platforms platforms={level.platforms} theme={theme} />
      {level.gifts.map((gift) => (
        <GiftBox key={gift.id} position={gift.pos} taken={giftsTaken.includes(gift.id)} />
      ))}
      <Beacon position={level.beacon} open={beaconOpen} />
      <group ref={player}>
        <BlockyAvatar moving={moving} />
      </group>
      {Array.from({ length: MAX_REMOTE }, (_, i) => (
        <group key={i} ref={(g) => void (remotes.current[i] = g)} visible={false}>
          <BlockyAvatar bodyColor={PALETTE.edgeGlowCyan} />
        </group>
      ))}
      {Array.from({ length: MAX_ENEMIES }, (_, i) => (
        <group key={i} ref={(g) => void (enemies.current[i] = g)} visible={false}>
          <Chaser position={ORIGIN} />
        </group>
      ))}
      <Effects />
    </>
  );
}

function DebugPanel() {
  const { status, error, serverTick, pingMs, playerCount } = useGameStore();
  return (
    <div className="pointer-events-none absolute bottom-3 left-3 font-mono text-xs text-white/60">
      <div>
        server: <span data-testid="conn-status">{status}</span> · tick:{" "}
        <span data-testid="server-tick">{serverTick}</span> · ping {pingMs ?? "–"} ms · players {playerCount}
      </div>
      {error && <div className="mt-1 rounded bg-red-900/80 px-2 py-1 text-sm text-white">{error}</div>}
    </div>
  );
}

function causeOf(cause: number): DeathCause {
  return cause === DEATH_ENEMY ? "enemy" : "void";
}

export default function GameClient() {
  const [session, setSession] = useState<GameSession | null>(null);
  const [level, setLevel] = useState<LevelData | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
  const container = useRef<HTMLDivElement>(null);
  const { status, pointerLocked, runOver, giftsTaken, giftsTotal } = useGameStore();

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

  // Free the mouse for the run summary.
  useEffect(() => {
    if (runOver && document.pointerLockElement) document.exitPointerLock();
  }, [runOver]);

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

  const restart = useCallback(() => session?.restart(), [session]);

  return (
    <div
      ref={container}
      className="relative h-screen w-screen overflow-hidden bg-[#07060d]"
      onClick={() => {
        if (!useGameStore.getState().runOver) void container.current?.requestPointerLock();
      }}
      onContextMenu={(e) => e.preventDefault()}
    >
      {session && level ? (
        <Canvas camera={{ fov: 70, near: 0.1, far: 1000 }} dpr={[1, 2]}>
          <World session={session} level={level} />
        </Canvas>
      ) : (
        <div className="flex h-full items-center justify-center font-mono text-white/70">
          {loadError ?? "entering the void…"}
        </div>
      )}
      {level && (
        <>
          <GameHud level={level.level} giftsRemaining={giftsTotal - giftsTaken.length} giftsTotal={giftsTotal} />
          <LevelBanner level={level.level} />
        </>
      )}
      {!pointerLocked && status === "connected" && !runOver && (
        <div className="pointer-events-none absolute bottom-8 left-1/2 -translate-x-1/2 rounded bg-black/60 px-4 py-2 font-mono text-sm text-white">
          click to aim · WASD move · Space jump · Shift dash
        </div>
      )}
      {runOver && (
        <RunSummary
          levelReached={runOver.levelReached}
          giftsCollected={runOver.giftsCollected}
          cause={causeOf(runOver.cause)}
          onRestart={restart}
        />
      )}
      <DebugPanel />
    </div>
  );
}
