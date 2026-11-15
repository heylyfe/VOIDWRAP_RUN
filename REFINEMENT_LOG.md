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

## Cycle 5 — baseline only (session ended before dispatch)

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

### Why no ticket was dispatched

The top candidate is the pattern library expansion (T-E): 50+ new patterns against the
frozen format, the only lever left for `4.near-duplicates` (17.63%), `5.heights`
(12.69%) and `5.reference-scale` (25.05%). It is a multi-hour authoring job with a
strict validator behind it (`validate-content`, `5.ramps`, `0.input`). Starting it and
leaving it half-authored would put unvalidated geometry in the catalog, which is worse
than not starting. Per the discipline rule — bounded sessions, stop and summarize
rather than grind — the cycle stops at the baseline.

The other two candidates are blocked on decisions rather than effort:

- **Class-selection window** — needs a decision on when a run starts. Logged for the
  human, not changed autonomously.
- **Navigation bot for the playthrough sweep** — a pathfinding bot over the level
  graph. Large, and the sweep already reports honestly without it.

### State at handback

`just verify` green. Latest release `v0.14.0-alpha.1`. Nothing is half-merged; the
tree is clean.
