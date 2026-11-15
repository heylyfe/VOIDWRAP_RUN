# economy-audit (T-089)

Black-box economy and flow audit for VOIDWRAP RUN. Node built-ins only, no new
dependencies. It reports **numbers**, not judgement: every metric prints
`PASS — name` or `FAIL — name` followed by the figures behind it, so the owner's
later balancing pass starts from evidence.

## Running it

```sh
# Skipped by default so `just verify` stays green without a generator.
node tools/economy-audit/run.mjs

# Audit a captured generator dump.
node tools/economy-audit/run.mjs --from tools/economy-audit/levels.ndjson

# Let the audit run the generator itself.
ECONOMY_AUDIT_CMD="cargo run -q -p sim --example pattern_dump --" \
  node tools/economy-audit/run.mjs

# Deterministic self-test of the metric maths.
node tools/economy-audit/run.mjs --selftest
```

Exit code is `0` only when every metric passes, `1` when any fails, `2` on a
tool error. `just verify` runs it env-gated, so it skips unless a dump or a
generator command is supplied.

## Capturing a dump

```sh
cd server && cargo run -q -p sim --example pattern_dump -- 200 \
  > ../tools/economy-audit/levels.ndjson
```

The record shape is the contract documented in `tools/pattern-audit/README.md`.
The audit only reads `plan` (the ordered catalog ids) and `fallbacks`.

## Metrics

| # | metric | what it measures | pass band |
|---|---|---|---|
| 1 | `gift_income` | gifts per level from the plan's gift slots, against the cheapest shop item | 0.5–4 levels of income per cheapest item |
| 2 | `shop_offer_mix` | items a shop can show before counters | 4–12 |
| 3 | `pedestal_pool` | permanent capacity and pool size per mode; the level overflow starts | pool ≥ 3 at level 1 in every mode |
| 4 | `greater_curse_rate` | greater curses available vs the every-10-levels draft | ≥ 3 |
| 5 | `hazard_tags` | `hazard:*` tag placements across the sampled plans | > 0 |
| 6 | `radar_offering` | radar modules in the shop | exactly 3 |
| 7 | `patterns_per_level` | plan length and fallback rate | median ≥ 3 |
| 8 | `hazard_entities` | patterns tagged `hazard:*` that actually carry the matching platform kind | every tagged pattern carries its entity |

## Findings at the v0.10.0-alpha.1 cut

Two metrics fail, and both are real:

- **`1.gift_income`** — a level carries a median of **219** gift slots against a
  cheapest shop item of **16** currency, so one level pays for roughly fourteen
  of the cheapest item. Currency is not scarce; the shop is a formality. This is
  the single biggest number for the owner's balancing pass.
- **`8.hazard_entities`** — `hazard:trampoline` is on 2 patterns and
  `hazard:tripmine` on 1, and **none** of them carries the matching platform
  kind. Trampolines work as a mechanic (T-087: launch branch, tuning, one real
  placement in `ch24_cinder_switchyard`) but the tagged patterns cannot host one:
  converting their jump pads breaks the from-rest playthrough test, and their
  footprints have no free floor for a bonus platform. Tripmines additionally
  need a level-data array and a client telegraph. Both are parked, and this
  metric is deliberately left failing rather than relaxed — a green audit that
  hides an unplaced feature is worse than a red one that names it.

`5.hazard_tags` passes (15,048 hazard-tagged placements across 3,000 levels:
crumble 43.8%, fog 30.9%, trampoline 16.8%, tripmine 8.5%). It measures tag
coverage only; metric 8 is the one that proves the entity landed.

## Modelling caveat

The audit models the shop and pedestal maths from `design/content.json` and the
constants in `server/crates/server/src/run.rs`. It does not execute the Rust
pool builder, so a change to `vote_pool_with` needs the constants here updated
by hand.
