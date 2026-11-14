# VOIDWRAP RUN task runner. `just` lists recipes.
set windows-shell := ["powershell.exe", "-NoLogo", "-NoProfile", "-Command"]

default:
    @just --list

# Install web dependencies and the wasm target
setup:
    rustup target add wasm32-unknown-unknown
    pnpm install

# Regenerate protocol code from protocol/schema.json
gen:
    node tools/protocol-gen.mjs

# Rewrite protocol golden fixtures (after an intentional schema change)
fixtures:
    node tools/protocol-gen.mjs --fixtures

# Build the shared movement controller to web/public/sim.wasm
wasm:
    node tools/build-wasm.mjs

# Start the game server and the web client (http://localhost:3000)
run: wasm
    node tools/run.mjs

# Format everything
fmt:
    cd server; cargo fmt --all

# The merge gate: everything must pass
verify: wasm
    node tools/protocol-gen.mjs --check
    node tools/check-names.mjs
    node tools/validate-content.mjs
    cd server; cargo fmt --all --check
    cd server; cargo clippy --workspace --all-targets -- -D warnings
    cd server; cargo test --workspace
    pnpm --dir web exec tsc --noEmit
    pnpm --dir web exec eslint .
    pnpm --dir web exec vitest run
    node tools/matchmaker-conformance/run.mjs
    node tools/voting-conformance/run.mjs

# Screenshot the running client into shots/ (needs `just run` in another terminal)
capture:
    node tools/capture.mjs

# Send a ticket to a subordinate: just dispatch glm tickets/T-011.md
dispatch target ticket *flags:
    node tools/dispatch.mjs --target {{target}} --ticket {{ticket}} {{flags}}
