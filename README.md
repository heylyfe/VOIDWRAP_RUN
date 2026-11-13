# VOIDWRAP RUN

*Collect every gift. Outrun what you voted for.*

VOIDWRAP RUN is a fast-paced 3D roguelite collectathon platformer. Collect all the gifts across endlessly generated void worlds, survive the enemies and curses you drafted yourself, and see how deep one run can go.

## The game

Every run drops you into a freshly generated sprawl of floating platforms hanging in a dark, chromatic void. The job is simple: collect every gift, reach the beacon, go deeper. The problem: at the start of each run you vote on which enemies hunt you, and every level you clear hands you a new curse to make it hurt. Pick a class, chain upgrades, gamble at altars, dress up your blocky little guy — and find out how deep one run can go. Play solo, or drag up to 7 friends into the void with you.

## Status

The project is a pre-release alpha. Working:

- A lobby with enemy, upgrade, and curse voting
- Randomly generated levels
- Gifts, golden gifts, and an exit beacon
- Lives
- Spectator mode in multiplayer on the same network
- Animations
- Per-level visual themes

Still planned: a shop for golden-gift money, classes, altars, more enemy types, level events, online multiplayer, accounts, cosmetics.

## Play

1. Open https://github.com/heylyfe/VOIDWRAP_RUN/releases, click the newest version's file ending in `-windows.zip`.
2. In Downloads, right-click it -> "Extract All..." -> "Extract", open the `VOIDWRAP_RUN` folder inside.
3. Double-click PLAY (if "Windows protected your PC" appears: "More info" -> "Run anyway").
4. The browser opens the game; keep the black window open while you play.

For detailed help, see [HOW-TO-PLAY.md](HOW-TO-PLAY.md).

To host a game for friends online for free, see the "Host online for free" section of [HOW-TO-PLAY.md](HOW-TO-PLAY.md#host-online-for-free).

## Build from source

Prerequisites:

- Rust (stable) with the `wasm32-unknown-unknown` target
- Node.js 24+
- pnpm
- `just`

Then:

- `npm start` — build the game and run it
- `npm run dev` — run a development build
- `just verify` (optional) — the full check suite: format, clippy, Rust tests, TypeScript typecheck, eslint, web tests

## Controls

- WASD — move
- Mouse — look (click the game to capture the mouse)
- Space — jump (hold for higher)
- Shift or right-click — dash

For the full control list, see [HOW-TO-PLAY.md](HOW-TO-PLAY.md).

## Development

`just` commands:

- `just setup` — adds the wasm target and installs web dependencies
- `just run` — builds and starts the game server and the web client
- `just capture` — drives the client headlessly and saves screenshots to `shots/`
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
