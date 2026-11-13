"use client";

import { useEffect, useLayoutEffect, useMemo, useRef } from "react";
import type { JSX } from "react";
import { useFrame } from "@react-three/fiber";
import { BackSide, Color, ShaderMaterial, SphereGeometry, Vector3 } from "three";
import type { Mesh } from "three";
import type { Theme } from "./theme";

const SKY_VERTEX_SHADER = `
varying vec3 vWorldPosition;

void main() {
  vec4 worldPosition = modelMatrix * vec4(position, 1.0);
  vWorldPosition = worldPosition.xyz;
  gl_Position = projectionMatrix * viewMatrix * worldPosition;
}
`;

const SKY_FRAGMENT_SHADER = `
uniform vec3 uZenith;
uniform vec3 uHorizon;
uniform vec3 uCloud;
uniform vec3 uCloudDark;
uniform vec3 uCenter;
uniform float uTime;

varying vec3 vWorldPosition;

// Hash-based value noise (no textures, fully procedural).
float hash13(vec3 p) {
  p = fract(p * 0.1031);
  p += dot(p, p.zyx + 31.32);
  return fract((p.x + p.y) * p.z);
}

float valueNoise(vec3 p) {
  vec3 i = floor(p);
  vec3 f = fract(p);
  vec3 u = f * f * (3.0 - 2.0 * f);

  float n000 = hash13(i + vec3(0.0, 0.0, 0.0));
  float n100 = hash13(i + vec3(1.0, 0.0, 0.0));
  float n010 = hash13(i + vec3(0.0, 1.0, 0.0));
  float n110 = hash13(i + vec3(1.0, 1.0, 0.0));
  float n001 = hash13(i + vec3(0.0, 0.0, 1.0));
  float n101 = hash13(i + vec3(1.0, 0.0, 1.0));
  float n011 = hash13(i + vec3(0.0, 1.0, 1.0));
  float n111 = hash13(i + vec3(1.0, 1.0, 1.0));

  return mix(
    mix(mix(n000, n100, u.x), mix(n010, n110, u.x), u.y),
    mix(mix(n001, n101, u.x), mix(n011, n111, u.x), u.y),
    u.z
  );
}

float fbm(vec3 p) {
  float sum = 0.0;
  float amplitude = 0.5;
  for (int i = 0; i < 5; i++) {
    sum += amplitude * valueNoise(p);
    p = p * 2.02 + vec3(11.3, 7.7, 5.1);
    amplitude *= 0.5;
  }
  return sum;
}

void main() {
  vec3 dir = normalize(vWorldPosition - uCenter);

  // Vertical gradient: horizon colour low, zenith colour high. The direction is
  // continuous over the whole sphere, so the dome has no hard seam.
  float height = smoothstep(-0.15, 0.85, dir.y);
  vec3 base = mix(uHorizon, uZenith, height);

  // Keep the zenith clearly brighter than the horizon band so the dome still
  // reads as sky under the heavy cloud cover.
  base = mix(base, uZenith, smoothstep(0.45, 1.0, dir.y) * 0.35);

  // Hard-edged billows: a narrow smoothstep band turns the fbm into crisp cloud
  // silhouettes instead of a milky wash.
  vec3 cloudDomain = dir * 3.0 + vec3(uTime * 0.01, 0.0, uTime * 0.006);
  float cloudNoise = fbm(cloudDomain);
  float cloudMask = smoothstep(0.42, 0.62, cloudNoise);
  vec3 colour = mix(base, uCloud, cloudMask * 0.85);

  // Broad, low-frequency masses: big dark shapes that dominate the lower sky.
  vec3 massDomain = dir * 0.85 + vec3(uTime * 0.003, 0.0, uTime * 0.002);
  float massNoise = fbm(massDomain);
  float massMask = smoothstep(0.38, 0.72, massNoise);
  float lowerSky = 1.0 - smoothstep(-0.05, 0.75, dir.y);
  colour = mix(colour, uCloudDark, massMask * lowerSky * 0.75);

  // Storm wall: noise stretched along x so the bands read as long horizontal
  // streaks rather than blobs.
  vec3 streakDomain = vec3(dir.x * 0.6, dir.y * 6.0, dir.z * 0.6) + vec3(uTime * 0.02, 0.0, 0.0);
  float streakNoise = fbm(streakDomain);
  float streakMask = smoothstep(0.45, 0.75, streakNoise);
  float streakBand = 1.0 - smoothstep(0.0, 0.55, abs(dir.y - 0.12));
  colour = mix(colour, uCloudDark, streakMask * streakBand * 0.35);

  gl_FragColor = vec4(colour, 1.0);

  #include <colorspace_fragment>
}
`;

type SkyUniforms = {
  uZenith: { value: Color };
  uHorizon: { value: Color };
  uCloud: { value: Color };
  uCloudDark: { value: Color };
  uCenter: { value: Vector3 };
  uTime: { value: number };
};

const DOME_RADIUS = 450;

export function Atmosphere(props: { theme: Theme }): JSX.Element {
  const { theme } = props;

  const domeRef = useRef<Mesh>(null);

  const uniforms = useMemo<SkyUniforms>(
    () => ({
      uZenith: { value: new Color() },
      uHorizon: { value: new Color() },
      uCloud: { value: new Color() },
      uCloudDark: { value: new Color() },
      uCenter: { value: new Vector3() },
      uTime: { value: 0 },
    }),
    [],
  );

  const geometry = useMemo(() => new SphereGeometry(DOME_RADIUS, 48, 24), []);

  const material = useMemo(
    () =>
      new ShaderMaterial({
        uniforms,
        vertexShader: SKY_VERTEX_SHADER,
        fragmentShader: SKY_FRAGMENT_SHADER,
        side: BackSide,
        depthWrite: false,
        fog: false,
      }),
    [uniforms],
  );

  // Theme changes only mutate uniform values through the live material: the
  // material (and its uniform record) is never re-created. A layout effect is
  // used so the colours are in place before the first rendered frame.
  useLayoutEffect(() => {
    const dome = domeRef.current;
    if (!dome) return;
    const mat = dome.material;
    if (!(mat instanceof ShaderMaterial)) return;

    const zenith = mat.uniforms.uZenith;
    if (zenith && zenith.value instanceof Color) zenith.value.set(theme.sky.zenith);

    const horizon = mat.uniforms.uHorizon;
    if (horizon && horizon.value instanceof Color) horizon.value.set(theme.sky.horizon);

    const cloud = mat.uniforms.uCloud;
    if (cloud && cloud.value instanceof Color) cloud.value.set(theme.sky.cloud);

    const cloudDark = mat.uniforms.uCloudDark;
    if (cloudDark && cloudDark.value instanceof Color) cloudDark.value.set(theme.sky.cloudDark);
  }, [theme]);

  useEffect(
    () => () => {
      geometry.dispose();
    },
    [geometry],
  );

  useEffect(
    () => () => {
      material.dispose();
    },
    [material],
  );

  useFrame((state) => {
    const dome = domeRef.current;
    if (!dome) return;

    const { x, y, z } = state.camera.position;
    dome.position.set(x, y, z);

    const mat = dome.material;
    if (!(mat instanceof ShaderMaterial)) return;

    const center = mat.uniforms.uCenter;
    if (center && center.value instanceof Vector3) center.value.set(x, y, z);

    const time = mat.uniforms.uTime;
    if (time && typeof time.value === "number") time.value = state.clock.elapsedTime;
  });

  return (
    <>
      <color attach="background" args={[theme.sky.horizon]} />
      <fog attach="fog" args={[theme.fog.color, theme.fog.near, theme.fog.far]} />
      <mesh
        ref={domeRef}
        geometry={geometry}
        material={material}
        frustumCulled={false}
        renderOrder={-1}
      />
    </>
  );
}
