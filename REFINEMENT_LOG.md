# Refinement Log

The polish phase, one section per cycle. Each cycle: re-run every black-box audit,
pick the top refinements by measured impact, dispatch to fresh worker threads,
verify, merge only on full green, release if green.

Standing order: no human, no questions, conservative autonomous choices, documented.
Frozen contracts (enemy registry, pattern format, voting contract) are additive only.

---

## Cycle 1 — 2026-11-15

### Audit baseline (measured before any change)

| Audit | Result |
|---|---|
| Matchmaker conformance | **8/8 PASS** against the stub (`:8047`) and the real crate (`:8048`) |
| Voting conformance | **0/10 FAIL** against the real server — see the defect below |
| Pattern audit (3,000 levels / 200 seeds) | **4 of 13 FAIL** |
| Economy audit | **6/8**, then **7/8** after T-091 |

**The voting kit cannot talk to the real server.** The kit sends JSON text frames
(`tools/voting-conformance/run.mjs:226-227`); the server speaks bincode over binary
frames and logs `dropping connection: text frame`
(`server/crates/server/src/net.rs:162`). The kit was only ever validated against its
own stub, so T-081's "passes against the real voting core" was never true. This is
the highest-value finding of the cycle.

**Every generated level was on the §6.4 no-tag fallback path** (3,000/3,000). The 25
chunks in `design/chunks.json` carried descriptive tags (`["lift","jump_pad","tower"]`)
but no `theme:*`/`mech:*` tags, so the composition engine could not see them.

**Economy:** a level carried a median of 219 gift currency against a cheapest shop
item of 16 — one level paid for about fourteen of the cheapest item. Currency was not
scarce, so the shop had no decisions in it.

### Tickets

| Ticket | Agent | Picked because | Result |
|---|---|---|---|
| T-090 playthrough & QA sweep | deepseek | no automated whole-run sweep existed | **PASS** |
| T-091 balance: 12x price scale | glm | `1.gift_income` failed at 0.07 levels of income per cheapest item | **PASS** |
| T-092 tag the 25 legacy chunks | inkling | `0.input` failed at 3,000/3,000 on the fallback path | **PARTIAL** |
| T-093 voting kit wire codec | deepseek | voting conformance 0/10 against the real server | **PASS** |

### T-090 — PASS

New `#[cfg(test)] mod qa` in `server/crates/server/src/game.rs` (four tests, 343
lines): `standard_run_reaches_level_five_without_softlock`,
`every_mode_completes_a_level`, `eight_players_all_receive_run_state_and_can_vote`,
`shop_and_altars_do_not_break_the_run`. Every loop is capped by a named constant.

The worker's draft did not compile against the real tree — it assumed
`g.run.level.enemies` (actually `g.run.enemies`), `ClientMsg::Buy { item: u32 }`
(actually `String`), and `g.run.level.beacon: Option<Vec3>` (actually `Vec3`). Fixed
during application. Two real findings came out of making it pass:

- `MAX_GIFTS` had to be raised above `GIFT_CAP` (400): level 3 carries 66 gifts, so a
  64-gift sweep left two on the floor and the collapse window never opened.
- The 8-player test drops connections unless **every** client outbox is drained each
  tick — `Game::send` removes a connection whose outbox is full
  (`game.rs:220-232`). The sweep now drains all eight.

Server tests: 123 → **127**.

### T-091 — PASS

12x price scale, one knob, measured before and after. `design/content.json`: 34
`shop[].cost` values remapped through `{16:200, 18:220, 20:250, 24:300, 28:350,
30:380, 35:440, 40:500, 45:560, 50:620, 60:750}`. `tools/validate-content.mjs`: cost
band 5..80 → 60..960, life band 50..70 → 600..840. `server/crates/server/src/run.rs`:
`SHOP_UPGRADE_COST` 25 → 300, `SHOP_PURGE_COST` 30 → 360, `DEBT_ADVANCE` 30 → 360.
`tools/economy-audit/run.mjs` mirrors `SHOP_UPGRADE_COST`.

`1.gift_income` moved from **0.07 → 0.91 levels of income per cheapest item** and now
passes. Six server tests needed their fixture wallets raised from 100 to 1,000; one
(`a_rejected_purchase_is_fully_refunded`) also needed its insufficient-funds cost
raised from 200 to 2,000 so the case still tests insufficient funds rather than the
new purge price.

### T-092 — PARTIAL

Additive only: `theme:*`/`mech:*` tags appended to all 25 chunks in
`design/chunks.json`, existing tags untouched and in order. The worker's patch list
matched the ticket table exactly.

Measured after regenerating the dump (`pattern_dump -- 200`, 3,000 records):

| Metric | Before | After |
|---|---|---|
| `0.input` fallback path | 3,000/3,000 **FAIL** | 0/3,000 **PASS** |
| `2.tag-diversity.themes` | 7.07 mean, 8/8 vocab **PASS** | 7.29 mean, 8/8 vocab **PASS** |
| `2.tag-diversity.mechs` | 9/10, `dead_end` never offered **FAIL** | 9/10, `dead_end` never offered **FAIL** |
| `4.near-duplicates` | 20.20% **FAIL** | 20.33% **FAIL** |
| `5.heights` | 14.52% outside [1,6]u **FAIL** | 13.49% **FAIL** |
| `5.reference-scale` | 25.43% outside [10,20]u **FAIL** | 25.94% **FAIL** |

The fallback path is gone and theme diversity is real. `dead_end` is still never
offered even though `design/patterns-a.json` carries three `mech:dead_end` patterns
and ch16/17/18 now carry the tag — so the gap is in the **composition engine**, which
never opens a branch that terminates. That is an engine change, not a data change, and
is the top pattern candidate for Cycle 2. `4.near-duplicates` and the two scale
metrics are likewise engine-level and untouched by tagging.

### T-093 — PASS

`tools/voting-conformance/wire.mjs`: a dependency-free bincode 1.x (fixint LE) codec
with `encodeClientMsg` / `decodeServerMsg` / `tryDecodeServerMsg`, covering the eight
`ClientMsg` variants the kit sends and the thirteen `ServerMsg` variants it reads.
`--selftest` asserts exact byte vectors for the encoders and hand-built buffers for
the decoders, plus truncated-buffer, absurd-length-prefix and unknown-variant
rejection. Wiring the kit and the stub onto it is T-094.

### Merges and release

`just verify` fully green after every merge: wasm build, protocol-gen --check,
check-names, validate-content, cargo fmt --check, clippy `-D warnings`, cargo test
--workspace (127 server / 229 sim), tsc, eslint, vitest (279), and the three
env-gated audits.

Release: `v0.11.0-alpha.1`.

### Top 3 candidates for Cycle 2

1. **T-094 — wire the voting kit and stub onto `wire.mjs`.** The kit still scores 0/10
   against the real server; until it speaks bincode it proves nothing about the voting
   core. Highest value item in the backlog.
2. **Composition engine: open dead-end branches.** `mech:dead_end` is in the pattern
   library and in the chunk catalog but the engine never selects it. Also the lever
   most likely to move `4.near-duplicates` (20.33%, threshold 0.2%).
3. **`5.heights` / `5.reference-scale`.** 13.49% and 25.94% against thresholds of 2%
   and 10%. Both are pattern-geometry distributions, so they are a pattern-library
   and weighting problem, not a tagging one.

### Carried forward, parked

- Hazard entity placement (trampolines/tripmines in generated levels) — failed twice;
  converting a pattern's jump pad breaks `every_graph_edge_kind_is_physically_doable`.
- Hush's HUD gift-hint half; CHAOS-exclusive curse pool; radar HUD fed by the server;
  five enemy-specific achievement events; lobby poll theme → server; matchmaker
  `phase: "voting"`; `release-notes/` stops at `v0.6.0-alpha.1`.
