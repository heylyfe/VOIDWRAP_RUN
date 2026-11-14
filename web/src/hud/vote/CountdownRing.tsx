import type { JSX } from 'react';
import { secondsRemaining } from './model';

const RADIUS = 26;
const CIRCUMFERENCE = 2 * Math.PI * RADIUS;

/** Pure SVG countdown ring for the 20 s vote window. No timers of its own. */
export function CountdownRing({
  remainingMs,
  durationMs,
}: {
  remainingMs: number;
  durationMs: number;
}): JSX.Element {
  const safeDuration = durationMs > 0 ? durationMs : 1;
  const clamped = Math.min(Math.max(remainingMs, 0), safeDuration);
  const fraction = clamped / safeDuration;
  const seconds = secondsRemaining(clamped);
  const stroke = seconds <= 5 ? '#ff4d6d' : '#38e8ff';

  return (
    <svg
      width={72}
      height={72}
      viewBox="0 0 72 72"
      role="timer"
      aria-label={`Vote closes in ${seconds} seconds`}
      className="shrink-0"
    >
      <circle
        cx={36}
        cy={36}
        r={RADIUS}
        stroke="rgba(255,255,255,0.15)"
        strokeWidth={6}
        fill="none"
      />
      <circle
        cx={36}
        cy={36}
        r={RADIUS}
        stroke={stroke}
        strokeWidth={6}
        fill="none"
        strokeLinecap="round"
        strokeDasharray={CIRCUMFERENCE}
        strokeDashoffset={CIRCUMFERENCE * (1 - fraction)}
        transform="rotate(-90 36 36)"
      />
      <text
        x={36}
        y={42}
        textAnchor="middle"
        fontSize={20}
        fontWeight={700}
        fill="#ffffff"
        fontFamily="monospace"
      >
        {seconds}
      </text>
    </svg>
  );
}
