"use client";

import { useMemo } from "react";
import type { JSX } from "react";
import {
  Bloom,
  ChromaticAberration,
  EffectComposer,
  Noise,
  Vignette,
} from "@react-three/postprocessing";
import { BlendFunction } from "postprocessing";
import { Vector2 } from "three";

export function Effects(): JSX.Element {
  const aberrationOffset = useMemo(() => new Vector2(0.0011, 0.0008), []);

  return (
    <EffectComposer multisampling={0} enableNormalPass={false}>
      <Bloom
        mipmapBlur
        luminanceThreshold={0.8}
        luminanceSmoothing={0.2}
        intensity={1.2}
        radius={0.75}
      />
      <ChromaticAberration
        offset={aberrationOffset}
        radialModulation
        modulationOffset={0.3}
      />
      <Vignette offset={0.3} darkness={0.5} />
      <Noise
        opacity={0.025}
        premultiply
        blendFunction={BlendFunction.SOFT_LIGHT}
      />
    </EffectComposer>
  );
}
