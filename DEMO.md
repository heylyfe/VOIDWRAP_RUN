# Demo — current state: M1 (playable solo vertical slice)

1. Install prerequisites: Rust stable, Node.js 24+, pnpm, `just`.
2. `just setup`
3. `just run` — wait for `[server] ... listening on ws://127.0.0.1:7878/ws` and `[web] ... Ready`.
4. Open http://localhost:3000 and click **Play**. A "LEVEL 1" banner appears; the HUD (top centre) shows the level
   and `GIFTS LEFT 5 / 5`.
5. Click the game to capture the mouse. WASD move, mouse look, Space jump (hold for a higher jump, tap for a short
   one), Shift or right-click to dash. You get a short grace window after walking off an edge (coyote time), and a
   jump pressed just before landing still fires (jump buffer).
6. Collect the five gold gifts. The red enemy wakes after 3 seconds and chases you; touching it ends the run.
7. When the counter reaches 0 the HUD shows **BEACON OPEN** and the cyan pillar lights up: walk into it to go to
   level 2, then level 3 (levels 4+ repeat the three designs with a higher number).
8. Fall into the void or touch the enemy: the **RUN OVER** screen shows level reached, gifts collected and the cause.
   Press Enter or **Run again** to start a new run at level 1.
9. Automated checks:
   - `just verify` — full gate, including a playthrough bot that completes levels 1–3 (every gift + beacon) with the
     real movement controller, frame by frame.
   - With `just run` going, in a second terminal: `just capture` — drives the client headlessly through spawn,
     movement, death, run summary and a new run; fails on console errors; writes screenshots to `shots/`.
