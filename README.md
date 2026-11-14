# VOIDWRAP RUN

*Collect every gift. Outrun what you voted for.*

VOIDWRAP RUN is a fast-paced 3D roguelite collectathon platformer. Collect all the gifts across endlessly generated void worlds, survive the enemies and curses you drafted yourself, and see how deep one run can go.

## The game

Every run drops you into a freshly generated sprawl of floating platforms hanging in a dark, chromatic void. The job is simple: collect every gift, reach the beacon, go deeper. The problem: at the start of each run you vote on which enemies hunt you, and every level you clear hands you a new curse to make it hurt. Pick a class, chain upgrades, gamble at altars, dress up your blocky little guy — and find out how deep one run can go. Play solo, or drag up to 7 friends into the void with you.

## Status

The project is a pre-release alpha (alpha 2). Working:

- Desktop game window (no browser needed; `PLAY-IN-BROWSER` falls back to the web browser)
- A lobby with class choice, enemy, upgrade, and curse voting
- Classes: Striker, Grappler, Blinker, Glider, each with an ability
- A lobby shop for golden-gift money: extra lives, rerolls, small permanent boosts
- Altars: one random upgrade and one random curse each
- Randomly generated levels with level events (High Rise, Slick Ice, Crumbling)
- Gifts, golden gifts, and an exit beacon
- Lives, and three new hunters: Bolt Sentry, False Gift, Crumble Wisp
- Play with friends over your network or the internet from your own computer (up to 8), with spectator mode
- Animations
- Per-level visual themes

Still planned: accounts, cosmetics, sound, more classes and events, installer.

## Play

1. Open https://github.com/heylyfe/VOIDWRAP_RUN/releases, click the newest version's file ending in `-windows.zip`.
2. In Downloads, right-click it -> "Extract All..." -> "Extract", open the extracted folder.
3. Double-click `VOIDWRAP RUN`. The game opens in its own window. If it stays empty or white, double-click `PLAY-IN-BROWSER` instead — the game then opens in your web browser (keep the black window open while you play).
4. If "Windows protected your PC" appears: "More info" -> "Run anyway".

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

- WASD (or arrow keys) — move
- Mouse — look (click the game to capture the mouse)
- Space — jump (hold for higher)
- Shift or right-click — class ability
- E — use an altar
- 1–4 — choose a class in the first lobby
- Esc — release the mouse pointer

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
