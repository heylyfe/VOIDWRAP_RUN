# T-077 pattern audit

Black-box audit of the pattern composition engine. Node built-ins only, no dependencies.

## Running

    node tools/pattern-audit/run.mjs --selftest

runs the deterministic metric self-test (synthetic records with planted defects). Useful as a
smoke test anywhere, including CI without the generator.

    PATTERN_AUDIT_CMD="<cmd>" node tools/pattern-audit/run.mjs

runs the generator over `PATTERN_AUDIT_SEEDS` (default **5000**) seeds, then prints one
`PASS/FAIL - <metric>` line per metric with raw numbers, a distribution section, a flagged
outliers table, and `RESULT: PASS/FAIL`. Exit code 0 only when every metric is inside its
threshold; 1 on any failing metric; 2 on input/tooling failure.

Because the command is env-gated, `node tools/pattern-audit/run.mjs` with no env prints
`SKIPPED - pattern-audit` and exits 0 — `just verify` needs no change to stay green.

## Generator contract

`PATTERN_AUDIT_CMD` is a shell command. The audit appends the seed count as the command's single
argument and reads NDJSON from stdout — one JSON object per generated level:

```jsonc
{
  "seed": 12345, "level": 3, "players": 1,
  "plan": ["pt01_ember_hub", "..."],        // ordered CompositionPlan pattern ids
  "patterns": [                              // placed instances, plan order
    { "id": "pt07_ember_span",
      "themes": ["ember", "ruin"],           // theme:<value> values §3.1
      "mechs":  ["span", "climb"],           // mech:<value> values §3.2
      "platforms": [ { "kind": "NORMAL" | "CRUMBLE" | "JUMP_PAD" | "RAMP_*",
                       "min": [x,y,z], "max": [x,y,z] } ] } ],
  "enemies":  [ { "type": "chaser" } ],      // placed enemy spawns, may be []
  "fallbacks": [ { "kind": "...", "detail": "..." } ] } // anchor fallbacks, may be []
```

Other env:

| Variable | Meaning |
| --- | --- |
| `PATTERN_AUDIT_SEEDS` | seed count, default 5000 |
| `PATTERN_AUDIT_ENEMY_TYPES` | comma-separated roster; roster types with zero placements across the run are "never offered". Optional. |

`--from <capture.ndjson>` audits a captured stream instead of running the generator.

## Thresholds and why

| Metric | Threshold | Justification |
| --- | --- | --- |
| 1a within-level repeats | mean plan redundancy <= 35%, one pattern <= 50% of a level | the bag (§6.4) bans consecutive repeats and divides seen weights by 32 over the last 6 picks; higher redundancy means eligibility is collapsing |
| 1b cross-seed plan lists | exactly 0 | the plan is derived from `(seed, level, players)`; one collision means the seed was ignored |
| 2 tag diversity | per level: themes mean >= 3 / min >= 2; mechs mean >= 4 / min >= 3; every vocabulary value used somewhere | §6.4 rule 3 biases toward themes used <2 times in-level; a level or a vocabulary value that never appears defeats "every level feels unique" |
| 3 anchor fallbacks | <= 1% of levels | dead_end sealers plus junction budgets make clean stitching the design intent; >1% of ~50k levels is systematic |
| 4 near-duplicates | Jaccard >= 0.85 on pattern-id sets, <= 0.2% of levels | high bar on purpose: levels legitimately share structure (hub, paths); only sets agreeing on 85%+ of members while ordering still differs are real dupes. 64-perm MinHash + LSH (16 bands x 4 rows), flagged pairs re-verified exactly |
| 5 ramps | 0 violations | T-068: rise/run <= 0.5, run >= 2.0u, width >= 3.0u |
| 5 stairs | 0 sequences | "stairs do not exist anywhere in the game": >= 3 adjacent box tops, equal rises, one direction |
| 5 heights | <= 2% of adjacent surface deltas outside 1-6u | reference scale from the ticket; flat continuations (~0u) are excluded |
| 5 reference scale | <= 10% of placed patterns with max XZ footprint extent outside 10-20u | both worked examples in PATTERN_FORMAT measure 16u; slack tolerates narrow alcoves, catches systematic drift |
| 6 enemies never offered | 0 roster types with 0 placements | needs `PATTERN_AUDIT_ENEMY_TYPES` |
| 6 enemies always offered | 0 cells (mode, level) with >= 20 levels where a type appears in 100% of them | below 20 levels, 100% presence is statistical noise |
