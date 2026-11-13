# Demo — current state: M0 (bootstrap)

1. Install prerequisites: Rust stable, Node.js 24+, pnpm, `just`.
2. `just setup`
3. `just run` — wait for `[server] ... listening on ws://127.0.0.1:7878/ws` and `[web] ... Ready`.
4. Open http://localhost:3000 and click **Play**.
5. You should see a pink block on a grey platform in the void, and a HUD reading `server: connected` with a
   climbing tick counter. The server terminal prints `tick N | 20.0 Hz | players 1` every second.
6. Click the game to capture the mouse; move with WASD, look with the mouse, jump with Space.
   Walk off the edge: you fall into the void and respawn.
7. Optional automated check (with `just run` still going, in a second terminal): `just capture` —
   drives the client headlessly, fails on console errors, writes screenshots to `shots/`.
8. `just verify` — the full gate (fmt, clippy, Rust tests, tsc, eslint, web tests, protocol drift, name check).
