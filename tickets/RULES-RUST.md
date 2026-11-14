Rust rules for every VOIDWRAP RUN ticket:
- Edition 2024, stable toolchain. `cargo clippy --workspace --all-targets -- -D warnings` must pass (pedantic lints are
  NOT enabled; default clippy only). `cargo fmt` is run by the lead (formatting never rejects a ticket).
- `sim` is shared by the server and by the client's WASM predictor and MUST be deterministic: only `+ - * /`, `sqrt`,
  `abs`, `min`, `max`, `clamp` on f32 - no `sin`/`cos`/`atan2`/`powf`/`exp`, no `HashMap` iteration, no randomness, no time.
- No `unwrap()`/`expect()`/panics outside `#[cfg(test)]`; no `unsafe` outside the existing WASM bridge.
- No new crates unless the ticket names them. Use only APIs visible in the ticket (protocol types are generated from
  `protocol/schema.json`; field names are snake_case exactly as shown there).
- Every behaviour in the Task gets a unit test. Existing tests must keep passing unchanged unless the ticket says so.
- Output COMPLETE files (not diffs). Doc-comment every public item in one or two lines.
