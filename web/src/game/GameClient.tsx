"use client";

import { Canvas, useFrame, useThree } from "@react-three/fiber";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import type { Group } from "three";
import {
  DEATH_ENEMY,
  GIFT_GOLDEN,
  PHASE_ENEMY_DRAFT,
  PHASE_PLAYING,
  PLAYER_ACTIVE,
  PLAYER_FINISHED,
  type LevelData,
} from "@/protocol/generated";
import { gradeForTheme } from "@/content";
import { DraftScreen } from "@/hud/DraftScreen";
import { GameHud } from "@/hud/GameHud";
import { RunStatus } from "@/hud/RunStatus";
import { LevelBanner } from "@/hud/LevelBanner";
import { RunSummary } from "@/hud/RunSummary";
import { SpectatorHud } from "@/hud/SpectatorHud";
import { flyStep, followCamera, nextFollow } from "./spectator";
import type { DeathCause } from "@/hud/format";
import { Atmosphere } from "@/scene/Atmosphere";
import { Beacon } from "@/scene/Beacon";
import { BlockyAvatar } from "@/scene/BlockyAvatar";
import type { AvatarMotion } from "@/scene/avatarPose";
import { DashStreak } from "@/scene/DashStreak";
import { Chaser } from "@/scene/Chaser";
import { Crystals } from "@/scene/Crystals";
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

/** Mutable motion record per avatar, read by the animation in useFrame (no React re-renders). */
function newMotion(): { current: AvatarMotion } {
  return { current: { speed: 0, vy: 0, grounded: true, dashing: false } };
}

function World({ session, level, themeLevel }: { session: GameSession; level: LevelData; themeLevel: number }) {
  const player = useRef<Group>(null);
  const remotes = useRef<(Group | null)[]>([]);
  const enemies = useRef<(Group | null)[]>([]);
  const camera = useThree((s) => s.camera);
  const motion = useMemo(newMotion, []);
  const remoteMotion = useMemo(() => Array.from({ length: MAX_REMOTE }, newMotion), []);
  const remoteLast = useRef<{ x: number; y: number; z: number; ok: boolean }[]>(
    Array.from({ length: MAX_REMOTE }, () => ({ x: 0, y: 0, z: 0, ok: false })),
  );
  const giftsTaken = useGameStore((s) => s.giftsTaken);
  const fogMul = useGameStore((s) => s.runInfo?.fogDensityMul ?? 1);
  const normalGifts = useMemo(() => level.gifts.filter((g) => g.kind !== GIFT_GOLDEN), [level]);
  const normalLeft = normalGifts.filter((g) => !giftsTaken.includes(g.id)).length;
  // The beacon opens and golden (currency) gifts appear once every normal gift is collected.
  const beaconOpen = normalGifts.length > 0 && normalLeft === 0;
  const baseTheme = themeForLevel(themeLevel);
  const theme = useMemo(
    () => ({ ...baseTheme, fog: { ...baseTheme.fog, near: baseTheme.fog.near / fogMul, far: baseTheme.fog.far / fogMul } }),
    [baseTheme, fogMul],
  );
  const grade = gradeForTheme(baseTheme.id);
  const bounds = useMemo(() => levelBounds(level), [level]);
  const myMode = useGameStore((s) => s.myMode);
  const phase = useGameStore((s) => s.runInfo?.phase);
  const spectating = phase === PHASE_PLAYING && myMode !== PLAYER_ACTIVE;
  /** Free-flight camera position while spectating (starts where the player stopped). */
  const flyPos = useRef<{ x: number; y: number; z: number } | null>(null);

  useFrame((_, dt) => {
    session.update(dt);
    const p = session.playerPosition(dt);
    const g = player.current;
    if (g) g.visible = !spectating; // spectators are invisible, to everyone and to themselves
    if (spectating) {
      if (!flyPos.current) {
        const from = p ?? level.spawn;
        flyPos.current = { x: from.x, y: from.y + 3, z: from.z };
      }
      const k = session.keys;
      const keys = { forward: k.forward, back: k.back, left: k.left, right: k.right, up: k.jump, down: k.down, fast: k.ability || session.rightMouse };
      const now = performance.now();
      const targets = [...session.remotePlayers(now).entries()];
      const followed = session.followIndex === null ? undefined : targets[session.followIndex % Math.max(targets.length, 1)];
      if (followed) {
        const cam = followCamera(followed[1], session.yaw, session.pitch, CAMERA_DISTANCE);
        camera.position.set(cam.x, cam.y, cam.z);
        camera.lookAt(followed[1].x, followed[1].y + CAMERA_HEIGHT, followed[1].z);
        flyPos.current = cam;
      } else {
        const f = flyPos.current;
        flyStep(f, keys, session.yaw, session.pitch, dt);
        camera.position.set(f.x, f.y, f.z);
        const cp = Math.cos(session.pitch);
        camera.lookAt(f.x - Math.sin(session.yaw) * cp, f.y - Math.sin(session.pitch), f.z - Math.cos(session.yaw) * cp);
      }
    } else {
      flyPos.current = null;
    }
    const c = session.playerController();
    if (c) {
      const m = motion.current;
      m.speed = Math.hypot(c.vel.x, c.vel.z);
      m.vy = c.vel.y;
      m.grounded = c.grounded;
      m.dashing = c.abilityActiveFrames > 0;
    }
    if (p && g && !spectating) {
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
      const r = remotes.current[i];
      const last = remoteLast.current[i];
      const rm = remoteMotion[i];
      i++;
      if (!r || !last || !rm) break;
      r.visible = true;
      // Estimate motion from the interpolated path so remote avatars animate too.
      if (last.ok && dt > 0) {
        const vx = (o.x - last.x) / dt;
        const vz = (o.z - last.z) / dt;
        const m = rm.current;
        m.speed = Math.hypot(vx, vz);
        m.vy = (o.y - last.y) / dt;
        m.grounded = Math.abs(m.vy) < 0.5;
        m.dashing = m.speed > 15;
        if (m.speed > MOVING_SPEED) r.rotation.y = Math.atan2(-vx, -vz);
      }
      last.x = o.x;
      last.y = o.y;
      last.z = o.z;
      last.ok = true;
      r.position.set(o.x, o.y, o.z);
    }
    for (; i < MAX_REMOTE; i++) {
      const r = remotes.current[i];
      const last = remoteLast.current[i];
      if (r) r.visible = false;
      if (last) last.ok = false;
    }
    let e = 0;
    for (const [id, o] of session.enemyPositions(now)) {
      const r = enemies.current[e++];
      if (!r) break;
      r.visible = true;
      r.position.set(o.x, o.y, o.z);
      r.scale.setScalar(session.enemyRadius.get(id) ?? 0.5);
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
      <Crystals platforms={level.platforms} theme={theme} seed={level.seed + 7} />
      {level.gifts.map((gift) => {
        const golden = gift.kind === GIFT_GOLDEN;
        const hidden = giftsTaken.includes(gift.id) || (golden && !beaconOpen);
        return (
          <group key={gift.id} scale={golden ? 1.25 : 1}>
            <GiftBox position={golden ? scaled(gift.pos, 1 / 1.25) : gift.pos} taken={hidden} />
          </group>
        );
      })}
      {normalGifts.length > 0 && <Beacon position={level.beacon} open={beaconOpen} />}
      <group ref={player}>
        <BlockyAvatar motion={motion} />
        <DashStreak motion={motion} color={theme.accent} />
      </group>
      {Array.from({ length: MAX_REMOTE }, (_, i) => (
        <group key={i} ref={(g) => void (remotes.current[i] = g)} visible={false}>
          <BlockyAvatar bodyColor={PALETTE.edgeGlowCyan} motion={remoteMotion[i]} />
        </group>
      ))}
      {Array.from({ length: MAX_ENEMIES }, (_, i) => (
        <group key={i} ref={(g) => void (enemies.current[i] = g)} visible={false}>
          <Chaser position={ORIGIN} />
        </group>
      ))}
      <Effects grade={grade} />
      <FpsProbe />
    </>
  );
}

function scaled(v: { x: number; y: number; z: number }, k: number) {
  return { x: v.x * k, y: v.y * k, z: v.z * k };
}

/** Counts rendered frames and publishes fps to the store twice a second. */
function FpsProbe() {
  const acc = useRef({ frames: 0, time: 0 });
  useFrame((_, dt) => {
    const a = acc.current;
    a.frames += 1;
    a.time += dt;
    if (a.time >= 0.5) {
      useGameStore.getState().set({ fps: Math.round(a.frames / a.time) });
      a.frames = 0;
      a.time = 0;
    }
  });
  return null;
}

function DebugPanel() {
  const { status, error, serverTick, pingMs, playerCount, fps } = useGameStore();
  return (
    <div className="pointer-events-none absolute bottom-3 left-3 font-mono text-xs text-white/60">
      <div>
        server: <span data-testid="conn-status">{status}</span> · tick:{" "}
        <span data-testid="server-tick">{serverTick}</span> · ping {pingMs ?? "–"} ms · players {playerCount} · <span data-testid="fps">{fps}</span> fps
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
  const { status, pointerLocked, runOver, giftsTaken, runInfo, draft, myMode, activeIds } = useGameStore();
  const [following, setFollowing] = useState<number | null>(null);
  const drafting = !!draft && runInfo?.phase === PHASE_ENEMY_DRAFT;
  // The lobby hub is shown in the colours of the level about to be played.
  const runLevel = runInfo?.level ?? level?.level ?? 1;
  const normalIds = useMemo(() => (level ? level.gifts.filter((g) => g.kind !== GIFT_GOLDEN).map((g) => g.id) : []), [level]);
  const normalTotal = normalIds.length;
  const normalLeft = normalIds.filter((id) => !giftsTaken.includes(id)).length;
  const playing = runInfo ? runInfo.phase === PHASE_PLAYING : true;

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

  // Free the mouse for the run summary and the lobby draft.
  useEffect(() => {
    if ((runOver || drafting) && document.pointerLockElement) document.exitPointerLock();
  }, [runOver, drafting]);

  useEffect(() => {
    if (!session) return;
    const key = (down: boolean) => (e: KeyboardEvent) => {
      const st = useGameStore.getState();
      const spectatingNow = st.runInfo?.phase === PHASE_PLAYING && st.myMode !== PLAYER_ACTIVE;
      if (down && spectatingNow && !e.repeat && (e.code === "KeyF" || e.code === "KeyG")) {
        session.followIndex = e.code === "KeyF" ? nextFollow(session.followIndex, st.activeIds.length) : null;
        setFollowing(session.followIndex);
        e.preventDefault();
        return;
      }
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
  const submitDraft = useCallback((picks: number[]) => session?.submitDraft(picks), [session]);

  return (
    <div
      ref={container}
      className="relative h-screen w-screen overflow-hidden bg-[#07060d]"
      onClick={() => {
        const st = useGameStore.getState();
        const inDraft = !!st.draft && st.runInfo?.phase === PHASE_ENEMY_DRAFT;
        if (!st.runOver && !inDraft) void container.current?.requestPointerLock();
      }}
      onContextMenu={(e) => e.preventDefault()}
    >
      {session && level ? (
        <Canvas camera={{ fov: 70, near: 0.1, far: 1000 }} dpr={[1, 2]}>
          <World session={session} level={level} themeLevel={runLevel} />
        </Canvas>
      ) : (
        <div className="flex h-full items-center justify-center font-mono text-white/70">
          {loadError ?? "entering the void…"}
        </div>
      )}
      {level && playing && normalTotal > 0 && (
        <>
          <GameHud level={runLevel} giftsRemaining={normalLeft} giftsTotal={normalTotal} />
          <LevelBanner level={runLevel} />
        </>
      )}
      {playing && myMode !== PLAYER_ACTIVE && !runOver && (
        <SpectatorHud
          reason={myMode === PLAYER_FINISHED ? "beacon" : "dead"}
          playersLeft={activeIds.length}
          following={following === null ? null : `Player ${activeIds[following % Math.max(activeIds.length, 1)] ?? "?"}`}
        />
      )}
      {runInfo && playing && (
        <RunStatus
          currency={runInfo.currency}
          lives={runInfo.lives}
          enemies={runInfo.enemies}
          curses={runInfo.curses}
          upgrades={runInfo.upgrades}
        />
      )}
      {drafting && draft && runInfo && (
        <DraftScreen
          cards={draft.cards}
          enemyPicks={draft.enemyPicks}
          rewardPicks={draft.rewardPicks}
          nextLevel={runInfo.level}
          currency={runInfo.currency}
          lives={runInfo.lives}
          waiting={draft.submitted}
          onSubmit={submitDraft}
        />
      )}
      {!pointerLocked && status === "connected" && !runOver && !drafting && (
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
