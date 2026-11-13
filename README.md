# VOIDWRAP RUN

*Collect every gift. Outrun what you voted for.*

VOIDWRAP RUN is a fast-paced 3D roguelite collectathon platformer. Collect all the gifts across endlessly generated void worlds, survive the enemies and curses you drafted yourself, and see how deep one run can go.

## The game

Every run drops you into a freshly generated sprawl of floating platforms hanging in a dark, chromatic void. The job is simple: collect every gift, reach the beacon, go deeper. The problem: at the start of each run you vote on which enemies hunt you, and every level you clear hands you a new curse to make it hurt. Pick a class, chain upgrades, gamble at altars, dress up your blocky little guy — and find out how deep one run can go. Play solo, or drag up to 7 friends into the void with you.

## Status

This project is at milestone M0 (bootstrap). Only the foundations exist:

- A grey platform floating in the void with a controllable blocky player.
- An authoritative game server ticking at 20 Hz.
- Client-side prediction for movement.

Not built yet, but on the roadmap: gifts, enemies, curses, classes, and multiplayer lobbies.

## Quick start

Prerequisites:

- Rust (stable) with the `wasm32-unknown-unknown` target
- Node.js 24+
- pnpm
- `just`

Then:

1. `just setup` — adds the wasm target and installs web dependencies
2. `just run` — builds and starts the game server (ws://127.0.0.1:7878/ws) and the web client; open http://localhost:3000

To run the full check suite at any time: `just verify` — format, clippy, Rust tests, TypeScript typecheck, eslint, web tests.

## Controls

Current:

- WASD — move
- Mouse — look (click the game to capture the mouse)
- Space — jump

Planned:

- Shift / right-click — class ability
- E — interact
- Tab — run overlay

## Development

`just` commands:

- `just setup` — adds the wasm target and installs web dependencies
- `just run` — builds and starts the game server and the web client
- `just verify` — the full check suite (format, clippy, Rust tests, TypeScript typecheck, eslint, web tests)
- `just gen` — regenerate protocol code after editing `protocol/schema.json`

The movement controller is shared Rust compiled to WebAssembly for client prediction. The wire protocol is generated from `protocol/schema.json` for both languages.

Repo layout:

- `server/` — Rust authoritative server workspace (crates `protocol`, `sim`, `sim-wasm`, `server`); tokio + axum WebSockets, bincode protocol
- `web/` — Next.js + TypeScript + react-three-fiber client
- `protocol/` — wire protocol schema and golden fixtures
- `tools/` — codegen, run, capture, and dispatch scripts
- `design/` — game data tables (later)
- `tickets/` — development work log

## Art & originality

All art, names, sounds, and UI in VOIDWRAP RUN are original to this project.
