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

---

## Cycle 2 — 2026-11-15

### Audit baseline (measured at the start of the cycle)

| Audit | Result |
|---|---|
| Matchmaker conformance | 8/8 PASS |
| Voting conformance | 0/10 against the real server; 3/10 against its own stub |
| Pattern audit | 10 of 13 PASS (after Cycle 1's T-092) |
| Economy audit | 7/8 |

### Tickets

| Ticket | Agent | Picked because | Result |
|---|---|---|---|
| T-094 voting kit wiring | deepseek | the kit still scored 0/10 against the real server | **PARTIAL** |
| T-095 dead-end patterns | deepseek | `2.tag-diversity.mechs` failed with `dead_end` never offered | **PASS** |

### T-095 — PASS

`server/crates/sim/src/pattern.rs`: the walk pool at line 665 filtered to
`ChunkRole::Path | ChunkRole::Junction`, so a dead-end pattern could never enter it
and `allowed_after`'s dead-end rule was unreachable. A dead-end has no exit socket,
so it can only terminate a plan: the final slot now offers dead-ends when a seeded
roll (`dead_end_roll`, a splitmix64 finalizer over seed/level/players) falls under
`DEAD_END_CHANCE = 0.25`, and the pick is forced rather than added to the pool so the
bag's theme and history weights cannot dilute the rate. Five tests: a dead-end can
terminate a plan, never appears mid-plan, the same seed reproduces, a catalog with no
dead-ends is unchanged, and the observed rate lands in 0.10..0.45.

The worker's patch was unusable — it invented `PatternEngine`, `Tier::Standard`,
`engine.compose` and `plan.chunks`, none of which exist. Implemented against the real
`CompositionPlan::build_with_len` API instead.

Measured after regenerating the dump: `2.tag-diversity.mechs` **PASS**, 10 of 13
metrics green (was 9). `4.near-duplicates` 20.33%, `5.heights` 13.49%,
`5.reference-scale` 25.94% are unchanged — all three are geometry distributions, not
tag-selection problems.

### T-094 — PARTIAL, stub selftest PARKED

The kit now speaks bincode. `tools/voting-conformance/run.mjs` sends
`encodeClientMsg` as binary frames and decodes with `decodeServerMsg`; the invented
`start_run`/`start_level` vocabulary is gone. **`--real` passes 3/3 against the live
server** — `real-welcome` (Welcome at protocol version 8), `real-runstate` (RunState
phase 0 at level 1), `real-vote-outside-session` (a Vote outside a session is
rejected with code 409). That is the first time this kit has proven anything about
the real voting core.

Getting there required fixing four things the worker got wrong, all of which are
findings in their own right:

- `wire.mjs`'s `STRUCT_FIELDS` table was invented. The real shapes are
  `VoteOption {index, id, name, kind}`, `DraftCard {kind, id}`,
  `ShopItem {id, cost, stock}`, and `RunInfo {phase, level, lives, currency, Tuning,
  fog_density_mul, enemies, upgrades, curses}` with `Tuning` carrying seven fields.
  The invented table decoded a real `RunState` into `{seed, level, hp, max_hp, gold,
  class}`. Corrected; `wire.mjs --selftest` is 24/24 again with the real layouts.
- The kit names messages by `type`, `wire.mjs` by `variant`; both directions now
  normalise.
- The stub had its own hand-rolled codec with **compact** variant indices
  (`RunState: 1`, `Error: 6`) instead of the real ones (`RunState: 6`, `Error: 4`),
  so every frame it emitted was garbage on the real wire. Corrected, along with its
  `RunInfo`, `VoteOption` and `Writer.f32`.
- The kit JSON-stringified a decoded message containing BigInt u64 fields.

**PARKED: the stub selftest.** After the codec fixes the faithful stub reaches 6 of
10 checks. The four that still fail (`session-at-every-level` sees only level 1,
`tie-rules`, `rejoin-safety`, `countdown-late-vote`) are all stub *progression*
problems: the stub does not advance past its first session within the kit's window,
so the checks that need a second and third session never run. Three attempts were
spent; per the discipline rule it is parked rather than ground further. The kit is
env-gated in `just verify`, so the gate stays green.

### Merges and release

`just verify` fully green: 127 server / 234 sim / 279 vitest, clippy `-D warnings`,
protocol-gen --check, check-names, validate-content.

Release: `v0.12.0-alpha.1`.

### Top 3 candidates for Cycle 3

1. **Finish the stub progression** (T-096). The stub must run three sessions inside
   the kit's window and reject a vote sent after `VoteResult`. Everything else in the
   kit is now correct, so this is the last piece before the ten-check matrix means
   something again.
2. **`5.heights` / `5.reference-scale`** (13.49% and 25.94% against 2% and 10%). Both
   are pattern-geometry distributions. The lever is the pattern library's height and
   footprint spread plus the engine's weighting, not tagging.
3. **`4.near-duplicates`** (20.33% against 0.2%). 161,579 near-duplicate pairs over
   3,000 levels. Needs a dedup lever in the generator — a repeat penalty on the
   pattern set, or chunk variants.

---

## Cycle 3

### Audit baseline (measured before any change)

| audit | result |
|---|---|
| pattern audit (3,000 records / 200 seeds) | 10 of 13 PASS |
| economy audit | 7 of 8 PASS (`8.hazard_entities` parked) |
| matchmaker conformance | 8/8 PASS |
| voting conformance `--real` | 3/3 PASS |
| voting conformance stub matrix | 6/10 PASS (parked in Cycle 2) |

Failing pattern metrics: `4.near-duplicates` 20.27% (threshold 0.2%),
`5.heights` 13.54% (threshold 2%), `5.reference-scale` 25.98% (threshold 10%).

### What was picked and why

1. **Voting stub selftest** — parked twice, and the kit is the authoritative gate for
   the voting contract. A gate that cannot fail is not a gate.
2. **`5.heights` / `5.reference-scale`** — the two largest measured deviations.
3. **`4.near-duplicates`** — 20.27% against 0.2%, the largest relative miss.

### T-096 — voting conformance stub selftest: PASS

Root cause found, and it was not what the Cycle 2 note assumed. The stub was not
failing to progress; it was **crashing**. Its `Error` frame encoder wrote `code` as
`u32` where the real protocol uses `u16`, so the two-byte shift corrupted the string
length prefix and `encodeServerMsg` threw. The throw happened inside `onVote` on the
first late vote, which killed the stub process — which is why the observer's log
stopped dead after the first `VoteResult` and never saw the next `RunState`. The
"progression problem" was a dead server.

Fixes, all in `tools/voting-conformance/`:

- `stub/server.mjs`: `Error.code` encoded as `u16`. The stub now survives a late vote
  and answers it with `Error` code 7.
- `stub/server.mjs`: permanence is now tracked in run state. `run.permanents` was
  read by `runInfo()` but never initialised or written, so `RunState.enemies` was
  always `[]` and no permanence check could ever observe anything.
- `stub/server.mjs`: new exported `stubPool()` — the real registry with a
  deterministic permanent-capable subset. Permanence is a run-state property, not a
  registry field, so the stub models it and the kit derives the same subset from the
  same function; both sides agree by construction. `buildPool` guarantees at least
  one permanent-capable option per session.
- `run.mjs`: the `permanence` check compared `VoteResult.permanent` against a
  registry field that is `false` for every enemy, so it could never fail. It now
  compares against the pool's expectation **and** asserts cross-level persistence in
  `RunState.enemies`.
- `run.mjs`: the `rejoin-safety` check accepted any `VoteOpen`, including a later
  session's. It is now scoped to the session that was open at rejoin time.
- `run.mjs`: the selftest runner used `loadPool()` while the stub child used
  `stubPool()`, so the faithful case compared two different pools.
- Debug lines removed.

Result: stub matrix **10/10**, violation selftest **6/6** (every deliberately broken
stub is caught), `--real` **3/3** against the live server. The item parked twice is
closed.

### T-097 — pattern footprint normalization: PARKED

22 patterns place outside the `[10, 20]u` reference extent. Two mechanical
transforms were implemented and measured, and both fail:

1. Scale XZ about the pattern's min corner to clamp the extent into `[10, 20]`:
   `5.reference-scale` passes, but `5.ramps` fails with 7,205 violations (scaling
   shrinks ramp run and width below the 2u / 3u minimums) and `0.input` fails with
   50 malformed records.
2. The same plus Y scaling: identical failures.

Both were reverted; the data files are unchanged. Normalizing footprints needs
per-pattern redesign, not a mechanical scale. **Human decision required.**

### T-098 — near-duplicate reduction: PARTIAL (improved, still failing)

The metric is an all-pairs MinHash-LSH scan at Jaccard >= 0.85, which for 11-pattern
sets means *identical* sets. The uniform-random baseline for 3,000 levels over this
pool is ~0%, so 20.27% was a real repeat, not a threshold artefact.

Measured cause: **every level 1 was identical** — 315 of 315 level-1 records shared
one pattern set. Only 8 patterns had `min_level <= 1` while a plan needs ~11, so the
level-1 pool was starved and the bag had no choice to make.

Fix (data only, additive, reversible): `min_level` 2 -> 1 for the eight level-2
patterns in `design/patterns-a.json` and `design/patterns-b.json`. The level-1 pool
goes from 8 to 16.

| metric | before | after |
|---|---|---|
| `4.near-duplicates` | 20.27% | **17.63%** |
| `5.heights` | 13.54% | 12.69% |
| `5.reference-scale` | 25.98% | 25.05% |
| distinct pattern sets | 2,438 / 3,000 | 2,438 / 3,000 |

Still failing. The remaining 529 flagged levels are spread across all levels, not
concentrated at level 1, so the next lever is the pattern library itself (backlog
T-E, grow toward 100+ patterns) rather than another engine tweak. Parked pending T-E.

### Merges and release

`just verify` fully green: 127 server / 234 sim / 279 vitest, clippy `-D warnings`,
protocol-gen --check, check-names (577 files), validate-content (20 enemies, 10
upgrades, 19 curses, 4 classes, 34 shop items, 3 events, 15 milestones, 4 altars, 10
achievements, 85 patterns).

Release: `v0.13.0-alpha.1`.

### Top 3 candidates for Cycle 4

1. **Pattern library expansion (T-E)** — the only lever left for
   `4.near-duplicates`, `5.heights` and `5.reference-scale` at once. Two batches,
   odd/even slots, frozen format.
2. **`8.hazard_entities`** (economy audit, parked twice) — converting a pattern's
   jump pad breaks `every_graph_edge_kind_is_physically_doable`. Needs a hazard
   placement pass that respects the graph-edge invariant.
3. **Automated playthrough sweep (T-A)** — the audits are all static. Nothing has
   exercised a real 8-player session end to end since Wave 5.

---

## Cycle 4

### Audit baseline (re-run at cycle start)

| audit | result |
|---|---|
| pattern audit (3,000 records / 200 seeds) | 10 of 13 PASS |
| economy audit | 7 of 8 PASS (`8.hazard_entities`) |
| voting conformance stub matrix | 10/10 PASS |
| voting conformance violation selftest | 6/6 PASS |
| voting conformance `--real` | 3/3 PASS |
| matchmaker conformance | 8/8 PASS |

### What was picked and why

1. **Automated playthrough sweep (T-A)** — backlog priority 1, and the only audit
   that exercises a live session. Every other audit is static: the pattern and
   economy audits read a level dump, the conformance kits read the wire. Nothing had
   driven a real 8-player run since Wave 5.
2. **Pattern library expansion (T-E)** — the only remaining lever for the three
   failing pattern metrics. Deferred to Cycle 5: it is a 50-pattern authoring job and
   the sweep was the higher-value first step.

### T-099 — automated playthrough sweep: PASS (tool delivered)

New `tools/playthrough/run.mjs`, env-gated in `just verify` like the other kits.
Connects N clients, sends `Hello` + `ChooseClass` back to back, auto-answers `Draft`,
`VoteOpen` and `Shop`, random-walks with real `Inputs` frames, and reports PASS / FAIL
/ SKIP per check.

`wire.mjs` deliberately refuses `ClientMsg::Inputs` (the conformance kit never sends
it), so the sweep carries its own encoder: `u32` variant, `u64` frame count, then per
frame `u32 seq, i8 move_x, i8 move_z, u8 buttons`.

Clean-server baseline, 8 clients, 90 s: **7/11 passed, 3 skipped, 1 failed**.

| check | result |
|---|---|
| connect | PASS 8/8 |
| welcome | PASS 8 frames |
| runstate | PASS 56 frames |
| level-progression | FAIL highest level 1 |
| draft | SKIP (level never completed) |
| vote | SKIP |
| shop | SKIP |
| phase-coverage | PASS 2 phases [0,1] |
| decode | PASS 0 undecodable frames |
| class-select | PASS 0/8 rejected |
| error-frames | PASS 0 unexpected |

The one FAIL is a harness limitation, not a product bug: the sweep has no navigation
bot, so a level only completes if the random walk happens to collect every gift.
Checks that need a completed level report SKIP rather than FAIL so the sweep's signal
stays about the protocol and the lobby flow.

### Findings from the sweep

- **The class-selection window is under a second.** `ChooseClass` is accepted only
  while `run.phase == PHASE_ENEMY_DRAFT && run.level_num == 1`
  (`server/crates/server/src/game.rs:577`). The draft phase ends as soon as the
  connected players have picked, which with 8 auto-picking clients is well under a
  second. A sweep that waited 250 ms between `Hello` and `ChooseClass` had **8 of 8
  classes rejected**; sending them back to back succeeds. A human player who reads
  the class list before picking will miss the window. **Not changed** — when the run
  should start is a design decision, logged as a Cycle 5 candidate.
- **`RunOver` fires on a solo client that never picks a class.** Observed on a dirty
  server during diagnosis; not reproduced on a clean one. Logged, not chased.
- The server is stateful across sweeps: a second sweep against the same process
  starts mid-run. The sweep is therefore documented as needing a fresh server.

### Merges and release

`just verify` fully green: 127 server / 234 sim / 279 vitest, clippy `-D warnings`,
protocol-gen --check, check-names (578 files), validate-content (20 enemies, 10
upgrades, 19 curses, 4 classes, 34 shop items, 3 events, 15 milestones, 4 altars, 10
achievements, 85 patterns).

Release: `v0.14.0-alpha.1`.

### Top 3 candidates for Cycle 5

1. **Pattern library expansion (T-E)** — 50+ new patterns against the frozen format,
   two batches on odd/even slots. The only lever left for `4.near-duplicates`
   (17.63%), `5.heights` (12.69%) and `5.reference-scale` (25.05%).
2. **Class-selection window** — the finding above. Needs a decision on when a run
   starts, so it is a candidate for the human rather than an autonomous change.
3. **Navigation bot for the playthrough sweep** — turns the 3 SKIPs and the
   `level-progression` FAIL into real coverage of the draft, the vote and the shop.

---

## Cycle 5 — pattern footprint normalization

### Audit baseline (re-run at cycle start)

| audit | result |
|---|---|
| pattern audit (3,000 records / 200 seeds) | 10 of 13 PASS |
| economy audit | 7 of 8 PASS (`8.hazard_entities`) |
| matchmaker conformance | **8/8 PASS** (first re-measure since Cycle 1) |
| voting conformance stub matrix | 10/10 PASS |
| voting conformance violation selftest | 6/6 PASS |
| voting conformance `--real` | 3/3 PASS |
| playthrough sweep (8 clients, 90 s) | 7/11 passed, 3 skipped, 1 failed |

### What was picked and why

`5.reference-scale` was the largest measured deviation (25.05% against a 10% band) and
the one with a clear, bounded fix. The Cycle 3 note parked it as needing per-pattern
redesign; this cycle did that work in two parts.

The key measurement that unlocked it: **expansion does not fix this metric.** Adding
50 in-band patterns would only dilute 26,306 violations across ~192,000 placements,
landing near 13.7% — still failing. The 22 out-of-band patterns had to be fixed
directly.

### T-100 — pattern footprint normalization: PASS

**Part 1, mechanical (6 patterns).** A piecewise-linear map that compresses only the
**gaps between platforms**, never their sizes. This is the transform the Cycle 3
attempt missed: scaling a platform shrinks its ramp run and width, which is what broke
`5.ramps` with 7,205 violations. Moving platforms and leaving their dimensions alone
preserves every ramp exactly. Six patterns fit this way (`pa_garden_span`,
`pb_ember_catwalk`, `pb_void_pillars`, `pb_spire_bridge`, `pb_ember_bounce`,
`pb_frost_gauntlet`). The other 16 have platforms whose own length exceeds the band and
cannot be fixed by moving them.

**Part 2, dispatched (10 patterns).** T-100 went to Inkling with the ten remaining
`patterns-a.json` / `patterns-b.json` patterns inlined, the ramp and slot rules spelled
out, and a `=== PATTERN: <id> ===` output contract. All ten came back inside the band
on the first attempt, no parks. The orchestrator spliced them with a validator that
re-checks extent, ramp run/width/rise, and that every socket, slot and beacon still
sits on a platform top — all ten passed.

Two validator bugs were found and fixed while splicing, both in the checker rather than
the data: gift slots float 0.3 above a platform top (not flush), and `elevated: true`
slots float up to 0.8. The first version rejected all ten blocks on that basis.

The six `chunks.json` patterns are **not** touched: `design/PATTERN_FORMAT.md` marks
that file frozen as the legacy record.

| metric | before | after |
|---|---|---|
| `5.reference-scale` | 25.05% FAIL | **6.98% PASS** |
| `5.ramps` | 0 PASS | **0 PASS** |
| `0.input` | PASS | **PASS** |
| `5.heights` | 12.69% | 12.57% |
| `4.near-duplicates` | 17.63% | 17.63% |
| pattern audit total | 10/13 | **11/13** |

The residual 6.98% is exactly the six frozen `chunks.json` patterns. Bringing them in
would mean editing a file the format spec calls frozen — a human decision, logged.

### Merges and release

`just verify` fully green: 127 server / 234 sim / 279 vitest, clippy `-D warnings`,
protocol-gen --check, check-names (580 files), validate-content (20 enemies, 10
upgrades, 19 curses, 4 classes, 34 shop items, 3 events, 15 milestones, 4 altars, 10
achievements, 85 patterns).

Release: `v0.15.0-alpha.1`.

### Top 3 candidates for Cycle 6

1. **`5.heights`** (12.57% against 2%) — the last pattern-geometry metric. Adjacent
   surface deltas outside [1, 6]u. Same shape of work as T-100: measure which patterns
   contribute, then redesign them.
2. **`4.near-duplicates`** (17.63% against 0.2%) — needs a larger pattern pool. The
   level-1 starvation is fixed; the rest is pool size.
3. **`8.hazard_entities`** (economy audit, parked twice) — needs a hazard placement
   pass that respects the graph-edge invariant.

---

## Cycle 6 — height deltas

### Audit baseline

Pattern audit 11/13. Failing: `4.near-duplicates` 17.63%, `5.heights` 12.57%.
Economy 7/8 (`8.hazard_entities`). Matchmaker 8/8. Voting 10/10 + 6/6 + 3/3.
Playthrough 7/11 passed, 3 skipped, 1 failed.

### What was picked and why

`5.heights` was the last pattern-geometry metric and the smallest remaining fix.
Measured first: only **13 patterns** contribute, and every violation is a **0.5u
half-step** (one 0.3u) between adjacent platform tops — exactly the stair-like small
step the design rejects. The fix is to make those pairs flush, which the metric treats
as a flat continuation and excludes.

### T-101 — height deltas: PARTIAL (improved, parked)

A mechanical pass raises the lower platform of each violating adjacent pair to the
higher one's top, carrying every socket, slot and beacon that sits on it. Twelve of
thirteen patterns converged; `pb_ruin_rest` was reverted because raising it broke a
ramp.

| metric | before | after |
|---|---|---|
| `5.heights` | 12.57% | **12.38%** |
| `5.ramps` | 0 PASS | **0 PASS** |
| `0.input` | PASS | **PASS** |

**PARKED, with the reason measured rather than assumed.** The first run of this pass
also edited the four `chunks.json` patterns that contribute (`ch02_hollow_market`,
`ch04_twin_terrace`, `ch06_descent_of_sighs`, `ch25_hanging_gallery`) and took
`5.heights` to **1.75% PASS**. But it broke the generator: `0.input` went from 0 to
**190 malformed records**, all `no_plan: catalog walk produced no plan`, and the
catalog still reported 85 patterns — so the patterns were loading but the level
assembly was failing. Reverting `chunks.json` alone restored 0 malformed records and
kept the `patterns-b.json` fixes.

So the metric is dominated by the four frozen legacy chunks, and editing them violates
an invariant in the assembly path that is not documented in `PATTERN_FORMAT.md`.
`design/PATTERN_FORMAT.md` marks `chunks.json` **frozen**, so this is a human decision,
not an autonomous one. The `patterns-b.json` half-steps are fixed and kept.

### Merges and release

`just verify` fully green: 127 server / 234 sim / 279 vitest, clippy `-D warnings`,
protocol-gen --check, check-names (580 files), validate-content (20 enemies, 10
upgrades, 19 curses, 4 classes, 34 shop items, 3 events, 15 milestones, 4 altars, 10
achievements, 85 patterns).

Release: `v0.16.0-alpha.1`.

### Top 3 candidates for Cycle 7

1. **`4.near-duplicates`** (17.63% against 0.2%) — the last failing pattern metric that
   is not blocked on a frozen file. Needs a larger pattern pool: 50+ new patterns
   against the frozen format, two batches on odd/even slots.
2. **`8.hazard_entities`** (economy audit, parked twice) — needs a hazard placement
   pass that respects the graph-edge invariant.
3. **`5.heights`** — blocked on the `chunks.json` decision above. If the human allows
   editing the legacy chunks, the measured result is 1.75% PASS; the assembly
   invariant that broke needs finding first.

## Refinement Cycle 7 — pattern pool widened to 100

### What was picked and why

`4.near-duplicates` was the only failing pattern metric not blocked on a frozen file:
**17.63%** of levels shared a pattern set at Jaccard >= 0.85 against a 0.2% threshold.
The cause is pool size, not composition logic — 85 patterns cannot cover 23-slot plans
without repeats. The lever is more patterns, which is additive and touches no contract.

### Tickets dispatched

**T-102 — 15 new patterns** (`inkling-design`, one dispatch, 198 s). Ten `path`
(two sockets), three `junction` (3–4 sockets, width >= 6), two `dead_end` (one socket),
ids `pa2_*`, appended to `design/patterns-a.json`. The ticket inlined the frozen-format
rules and three exemplars; the response was validated by a purpose-written checker
(`/tmp/newpatterns.mjs`) covering id shape, role, `min_level`, `difficulty`, tag counts
and vocabulary, footprint extent [10, 20], socket count per role, socket width and
facing-edge placement, socket `y` on a platform top, ramp run/width/rise, stacking
clearance >= 4.0 u, adjacent top deltas, and gift/altar slots on platform tops.
**15 of 15 accepted on the first pass, no parks.**

### The `0.input` regression, and what it actually was

The first audit run after the splice failed `0.input` with one malformed record:
`no_plan` at `seed=95 lvl=3 players=8`. The level fell through all 40 assembly attempts
and the catalog fallback to `builtin_fallback`.

Bisecting by removing one new pattern at a time was **initially meaningless**: the
catalog is `include_str!`-embedded, so editing `design/patterns-a.json` without a rebuild
changes nothing. Re-run with a rebuild per variant, removing any one of eight patterns
fixed it — so no single pattern was at fault.

Instrumenting `validate` gave the real error: `gift N is not reachable from spawn`. A
temporary per-chunk reachability probe then found the actual defect:

> `pb_machine_gauntlet` has three 4.5–5.5 u pillars with no ramp. They are unreachable
> from its own sockets by construction. `fill_gifts` only sees platform tops, so it can
> put a gift on one, and `validate` then rejects the whole level.

That pattern is **pre-existing** (`patterns-b.json`, `min_level` 6). It had never
surfaced because the old 85-pattern pool never drew it into a plan whose gift placement
landed on a pillar. Widening the pool shifted the RNG stream into that case.

Two generator fixes were attempted and **both reverted**:

1. *Drop gifts on unreachable platforms.* Made `gifts_scale_per_a3` fail (30 gifts vs
   ~46): it silently accepted a level with 27 unreachable platforms instead of retrying,
   which is worse than the bug.
2. *Remove unreachable platforms before gift filling.* Fixed the gift count but broke
   `real_catalog_generates_valid_non_fallback_levels` — platform ids stop being dense and
   the survivors include traps (reachable from spawn, no route back to the beacon). A
   correct version needs id renumbering plus a reverse-reachability pass; that is a
   redesign of the assembly tail, not a polish diff, so it was parked rather than grown.

`server/crates/sim/src/levelgen.rs` is **unchanged** in this cycle.

### The fix that shipped

Data-side, minimal: `pa2_frost_descent` — the one new pattern with a 6 u internal
descent between its sockets — was raised from `min_level` 2 to 4. Each of the nine new
patterns below level 4 was then re-tested individually at its authored level, keeping it
only if `0.input` stayed clean. Eight kept their authored level; only
`pa2_frost_descent` needed raising.

| metric | before | after |
|---|---|---|
| `4.near-duplicates` | 17.63% | **11.80%** |
| `5.heights` | 12.38% | **9.76%** |
| `0.input` | PASS | **PASS** |
| `5.ramps` | 0 PASS | **0 PASS** |
| `5.reference-scale` | 5.80% PASS | **5.82% PASS** |
| `1.pattern-repeat.within-level` | 17.26% | 17.38% |

`5.heights` improved as a side effect: the new patterns dilute the frozen `chunks.json`
contribution that dominates that metric.

### Autonomous choices, for review

- **15 patterns, not 50.** The ticket asked for 15 because the near-duplicate metric is
  driven by low-level pool size, and 15 was the largest batch that could be validated in
  one dispatch without a second round trip. Result: 33% relative improvement.
- **`pa2_frost_descent` at `min_level` 4.** Its 6 u descent is the most aggressive
  geometry in the batch; deferring it to level 4 was cheaper and safer than re-authoring
  it, and it costs nothing at levels 4+.
- **No generator change.** Both attempts made things worse or grew past a polish diff.
  The `pb_machine_gauntlet` pillars and the assembly tail are logged below as candidates.

### PARKED, with the reason measured

- **`4.near-duplicates` at 11.80%** (threshold 0.2%). Still failing. The remaining
  duplicates are concentrated at levels 1–3, where the eligible pool is smallest. Closing
  the gap needs either a much larger low-level pool or a composition change (e.g. a
  per-level no-repeat window), and the latter is a frozen-contract decision.
- **`pb_machine_gauntlet`'s three unreachable pillars** — a real content defect. Fixing
  it means either deleting the pillars (loses the pattern's silhouette) or adding a ramp
  that does not fit the footprint. Human call.
- **The assembly tail** — `validate` tolerates unreachable platforms but rejects gifts on
  them, so any pattern with unreachable scenery is a latent landmine. A correct fix needs
  id renumbering plus reverse-reachability (trap removal). Logged, not attempted.
- **`5.heights` at 9.76%** — still dominated by the four frozen `chunks.json` patterns.
  Unchanged from Cycle 6: editing them breaks the generator.

### Merges and release

`just verify` fully green: 127 server / 234 sim / 279 vitest, clippy `-D warnings`,
protocol-gen --check, check-names (582 files), validate-content (20 enemies, 10 upgrades,
19 curses, 4 classes, 34 shop items, 3 events, 15 milestones, 4 altars, 10 achievements,
**100 patterns**).

Release: `v0.17.0-alpha.1`.

### Top 3 candidates for Cycle 8

1. **`4.near-duplicates`** — 11.80%, still the largest measured gap. Needs a second
   pattern batch aimed specifically at `min_level` 1–3, where the pool is thinnest.
2. **`8.hazard_entities`** (economy audit, parked twice) — needs a hazard placement pass
   that respects the graph-edge invariant.
3. **The assembly tail** — trap removal plus id renumbering would retire the whole class
   of `no_plan` failures and unblock `pb_machine_gauntlet`.

## Refinement Cycle 8 — near-duplicates closed

### Baseline (400 seeds, 6,000 level records, `v0.17.0-alpha.1`)

| audit | result |
|---|---|
| pattern | **11/13** — `4.near-duplicates` 14.93%, `5.heights` 9.75% |
| economy | **7/8** — `8.hazard_entities` |
| matchmaker conformance | **8/8** |
| voting conformance | **10/10** matrix, **6/6** selftest, **3/3** real |
| playthrough | **7/11**, 3 skipped, 1 failed |

The 400-seed run is the honest number: the 200-seed sample in Cycle 7 read 11.80%,
but near-duplicates rise with seed count because more seeds reach the levels where the
pool is thinnest.

### What was picked and why

1. **`4.near-duplicates`** — largest measured gap, and the only failing pattern metric
   not blocked on a frozen file.
2. **Playthrough `level-progression`** — the sweep is the only end-to-end signal and it
   was stuck at level 1, SKIPping draft/vote/shop. Highest leverage per fix.
3. **`8.hazard_entities`** — deferred; parked twice already and needs a hazard placement
   pass that respects the graph-edge invariant. Not attempted this cycle.

### Tickets dispatched

**T-104 — 20 low-level patterns** (`inkling-design`, 20 of 20 accepted). Level-3 plans
are 23 slots and only 36 path/junction patterns were eligible at level 3, so two level-3
plans always overlapped. Result: `4.near-duplicates` **14.93% -> 0.35%**.

**T-105 — 12 level-1 patterns** (`inkling-design`, 12 of 12 accepted). After T-104 every
remaining flagged pair was at level 1, where only 25 path/junction patterns were eligible
for a 13–17 slot plan. Result: `4.near-duplicates` **0.35% -> 0.00% PASS**.

**T-103 — playthrough navigation bot** (`deepseek`, then a lead fix). **PARKED**, see
below.

### The validator bug that rejected 20 good patterns

The first T-104 run rejected all 20 patterns for "gift slot off-platform". The checker
required a non-elevated gift slot to sit within 0.05 u of a platform top, but the shipped
convention is `top + 0.3` — 455 of 552 existing gift slots use it. The checker was wrong,
not the data; it now allows `top - 0.01 .. top + 0.35` (and `+0.85` for elevated). Worth
recording because the same checker gates every future pattern batch.

### T-103: what was actually blocking the sweep

The dispatch returned a 629-line reconstruction with no wire imports — the worker has no
repo access, so it could not reuse the codec. Doing it as a lead fix exposed the real
blocker: `tools/voting-conformance/wire.mjs` **deliberately does not decode `Level`,
`Snapshot` or `RunOver`** — it returns `{variant, truncated: true}` for them. The bot
therefore never saw a gift position.

That was fixed properly: `wire.mjs` gained `Vec3`, `Aabb`, `Platform`, `Gift`, `Altar`,
`LevelData` and `ControllerState` field tables, `Level` now decodes fully, and `Snapshot`
decodes through `you` and stops (the player/enemy views are large and nothing reads them).
Verified: `Level` decodes 88 gifts / 61 platforms, `Snapshot.you.pos` decodes, and the
voting kit still reports **10/10 + 6/6 + 3/3** against a fresh stub.

The bot now steers at the nearest uncollected gift (`Client.steer`) with the random walk
kept as the fallback. It still does not finish a level: a level needs **all 88 gifts**
collected and then the beacon reached, and straight-line steering walks off ledges — the
run reaches `PHASE_OVER` (3) instead of advancing. 200 s does not help. **PARKED**: needs
graph pathfinding over `LevelData.platforms` plus hazard avoidance, which is a project,
not a polish diff.

Also noted: the sweep can only run **once per server start** — the server keeps one global
run, so a second sweep sees `phase 3` and rejects every class. A `Restart` at sweep start
would fix it; logged, not done.

### Results

| metric | before | after |
|---|---|---|
| `4.near-duplicates` | 14.93% | **0.00% PASS** |
| `5.heights` | 9.75% | **7.03%** (still FAIL) |
| `1.pattern-repeat.within-level` | 17.34% | **11.38%** |
| `5.reference-scale` | 5.85% | **4.12% PASS** |
| `0.input` | PASS | **PASS** |
| pattern audit | 11/13 | **12/13** |

`5.heights` improved again as a side effect of pool dilution. It remains dominated by the
four frozen `chunks.json` patterns.

### Autonomous choices, for review

- **Two pattern batches in one cycle.** T-104 alone left 0.35% against a 0.2% threshold;
  a second, smaller batch aimed only at level 1 closed it. Both were the same proven
  dispatch shape, so the second was cheap.
- **Extended the shared wire codec rather than forking it.** `wire.mjs` is used by both
  the voting kit and the sweep; adding the struct tables keeps one decoder. Re-ran the
  voting kit to prove no regression.
- **Kept the steering even though it does not finish a level.** It is strictly better
  than the random walk (the bot now moves toward gifts instead of wandering) and it is
  the foundation the pathfinding work needs. No check was weakened.

### PARKED, with the reason measured

- **Playthrough `level-progression`** — needs graph pathfinding + hazard avoidance over
  `LevelData.platforms`. Straight-line steering dies before collecting 88 gifts.
- **Playthrough repeatability** — one sweep per server start; needs a `Restart` at start.
- **`8.hazard_entities`** (economy audit) — parked twice; needs a hazard placement pass
  that respects the graph-edge invariant.
- **`5.heights` at 7.03%** — dominated by the four frozen `chunks.json` patterns. Editing
  them breaks the generator (measured in Cycle 6: 190 `no_plan` records). Human decision.
- **`pb_machine_gauntlet`'s three unreachable pillars** — unchanged from Cycle 7.

### Merges and release

`just verify` fully green: 127 server / 234 sim / 279 vitest, clippy `-D warnings`,
protocol-gen --check, check-names (582 files), validate-content (20 enemies, 10 upgrades,
19 curses, 4 classes, 34 shop items, 3 events, 15 milestones, 4 altars, 10 achievements,
**132 patterns**).

Release: `v0.18.0-alpha.1`.

### Top 3 candidates for Cycle 9

1. **`5.heights`** — 7.03%, the last failing pattern metric, and blocked on a frozen file.
   Either get the human's decision on `chunks.json` or find the assembly invariant that
   broke in Cycle 6.
2. **Playthrough pathfinding** — would turn 7/11 into 11/11 and give every later cycle
   real coverage of draft, vote and shop. Needs a platform-graph BFS in the sweep.
3. **`8.hazard_entities`** — the last economy-audit failure.

---

## Refinement Cycle 9 — `v0.19.0-alpha.1`

Baseline: `v0.18.0-alpha.1` (`65c2674`). Pattern audit 12/13, economy 7/8,
matchmaker 8/8, voting 10/10 + 6/6 + 3/3, playthrough 7/11 + 3 skip + 1 fail.

### What was picked and why

The audits left only three red lines, and two of them are blocked on decisions
rather than work. Cycle 9 therefore targeted the two items that were genuinely
actionable, plus a root-cause investigation on the third.

1. **`5.heights` — the one editable violation.** Cycle 8 measured 7.03% against a
   2% threshold and attributed it to the frozen `chunks.json`. That attribution was
   re-derived from the design files this cycle: of the 11 pattern-local adjacent-top
   violations in the whole catalog, **10 are in the four frozen chunks**
   (`ch02_hollow_market` 4, `ch25_hanging_gallery` 3, `ch04_twin_terrace` 2,
   `ch06_descent_of_sighs` 1) and **1 is in `pb_ruin_rest`**. The audit counts per
   *placement*, which is why 11 pattern-local violations become 27,068 measured
   deltas. Only the editable one was in scope.
2. **Playthrough repeatability.** The sweep could only run once per server start,
   because a finished run leaves the server in `PHASE_OVER` where every class pick
   is rejected. That made the sweep useless as a per-cycle regression signal.

### Changes

- **`design/patterns-b.json` — `pb_ruin_rest`.** Platform 6 (`x -5..-4, z 9..10`)
  sat at top 1.0 against platform 2's top 0.5, a 0.5 u step. Raised to 1.5, which
  matches the 1.0 u delta platform 4 already has against platform 2 — so the step
  is one the pattern already proves climbable. No ramp is involved; the pattern is
  all `NORMAL` platforms.
- **`tools/playthrough/run.mjs` — `Restart` at sweep start.** Sent between `Hello`
  and `ChooseClass`. `ClientMsg::Restart` is a no-op unless `phase == PHASE_OVER`
  (`game.rs:649`), so it is safe on a fresh server and resets a used one.
  `new_run()` keeps players and their last class (`game.rs:435`), so the following
  `ChooseClass` lands in the fresh draft window.

### Results

| Metric | Before | After |
| --- | --- | --- |
| `5.heights` | 7.03% | **6.02%** (threshold 2%) |
| `5.ramps` | 0 | 0 |
| `5.stairs` | 0 | 0 |
| `4.near-duplicates` | 0.00% | 0.00% |
| `0.input` | PASS | PASS |
| Playthrough, 2nd sweep on same server | all 8 classes rejected | **0/8 rejected**, phases `[3,0,1]` |

The `5.heights` move of 1.01 pp from a single pattern confirms the per-placement
accounting: the remaining 6.02% is the frozen chunks, unchanged.

### PARKED, with the reason measured

- **`5.heights` at 6.02%** — 10 of 11 pattern-local violations are in the four
  frozen `chunks.json` patterns. Editing them took the metric to 1.75% PASS in
  Cycle 6 but broke the generator (190 `no_plan` records). Human decision.
- **`8.hazard_entities`** — root cause now pinned precisely, replacing the earlier
  vague "needs a hazard placement pass". `PLATFORM_TRIPMINE` (`generated.rs:66`) is
  a declared constant with **no consumer anywhere**; `validate` rejects it as an
  unknown kind (`levelgen.rs:2299`); `LevelData` has no `tripmines` field
  (`level, seed, spawn, beacon, kill_y, platforms, gifts, event, altars`); and
  `sim/src/tripmine.rs` is a fully tested module that nothing instantiates. Making
  the `hazard:tripmine` tag real is a protocol change plus generator plus client
  rendering — a ticket, not a polish item. Third park.
- **Playthrough `level-progression`** — needs graph pathfinding + hazard avoidance
  over `LevelData.platforms`. Straight-line steering dies before collecting 88 gifts.
- **`pb_machine_gauntlet`'s three unreachable pillars** — unchanged from Cycle 7.

### Merges and release

`just verify` fully green: 241 sim, 279 vitest, clippy `-D warnings`,
protocol-gen --check, check-names, validate-content (20 enemies, 10 upgrades,
19 curses, 4 classes, 34 shop items, 3 events, 15 milestones, 4 altars,
10 achievements, **132 patterns**).

Release: `v0.19.0-alpha.1`.

### Top 3 candidates for Cycle 10

1. **Playthrough pathfinding** — the only red line that is pure work with no
   decision attached, and it would turn 7/11 into 11/11, giving every later cycle
   real coverage of draft, vote and shop.
2. **`8.hazard_entities`** — now fully specified; needs a `LevelData.tripmines`
   field through `just gen` + `just fixtures`, a generator pass, and client
   rendering. Large but unambiguous.
3. **`5.heights`** — needs the human's call on `chunks.json`. Nothing further can
   be done without it.
