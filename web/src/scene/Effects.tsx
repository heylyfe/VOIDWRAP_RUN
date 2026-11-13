"use client";

import { useEffect, useMemo, useRef } from "react";
import type { JSX } from "react";
import {
  Bloom,
  ChromaticAberration,
  EffectComposer,
  Noise,
} from "@react-three/postprocessing";
import { BlendFunction } from "postprocessing";
import { Vector2 } from "three";
import type { ScreenGrade } from "../content";
import { ColorGrade } from "./ColorGrade";

export interface EffectsProps {
  grade: ScreenGrade;
}

export function Effects({ grade }: EffectsProps): JSX.Element {
  // One Vector2 for the lifetime of the composer: ChromaticAberration keeps the reference,
  // so writing into it retunes the offset without rebuilding the effect.
  const aberrationOffset = useMemo(() => new Vector2(0, 0), []);
  const aberrationRef = useRef(aberrationOffset);

  useEffect(() => {
    aberrationRef.current.set(grade.aberration, grade.aberration * 0.7);
  }, [grade]);

  return (
    <EffectComposer multisampling={0} enableNormalPass={false}>
      <Bloom
        mipmapBlur
        luminanceThreshold={0.8}
        luminanceSmoothing={0.2}
        intensity={1.2}
        radius={0.75}
      />
      <ColorGrade grade={grade} />
      <ChromaticAberration
        offset={aberrationOffset}
        radialModulation
        modulationOffset={0.3}
      />
      <Noise
        opacity={grade.grain}
        premultiply
        blendFunction={BlendFunction.SOFT_LIGHT}
      />
    </EffectComposer>
  );
}
