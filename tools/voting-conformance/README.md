# Voting conformance kit

Black-box conformance kit for the end-of-level vote session described in T-081 and
`design/ENEMY_REGISTRY.md` §9 ("Pool rules (the voting system)"). It never reads the voting core's
source; it drives real WebSocket clients and prints `PASS - <check>` / `FAIL - <check>` per check.
Exit code 0 only on a full pass.

```sh
VOTING_URL=ws://127.0.0.1:9001 node tools/voting-conformance/run.mjs
node tools/voting-conformance/run.mjs --url ws://127.0.0.1:9001
node tools/voting-conformance/run.mjs --selftest   # kit-vs-stub, ~1 min
```

Without `--url` / `VOTING_URL` the kit prints `skipped` and exits 0, so `just verify` stays green
when no server is running (matches the matchmaker kit's convention).

## Files

- `run.mjs` — the check suite, selftest driver and a hand-rolled RFC 6455 WebSocket client
  (works on any Node ≥ 18; the global `WebSocket` needs Node ≥ 22, and the kit must stay
  dependency free).
- `registry.mjs` — the frozen enemy registry (§10) transcribed from `design/ENEMY_REGISTRY.md`;
  shared by the kit's assertions and the stub's pool builder. Spec data, not server source.
- `stub/server.mjs` — a faithful implementation of §9 on this wire contract, plus single
  deliberate violations for `--selftest` (`VOTE_STUB_VIOLATION`).

## Wire contract (what the server under test must implement)

JSON text frames. The kit connects three clients (all HTTP Upgrade handshakes to the given URL,
optionally with a shared `?room=<id>` query parameter). The earliest-joined connected client of a
room is the host.

Client → server:

| message | meaning |
|---|---|
| `{"type":"hello","name":str,"token"?:str}` | join; with a previous `welcome.token` reclaims the old session |
| `{"type":"start_run","seed":num,"class"?:str,"permanents"?:[enemyId,...]}` | begin a fresh run (resets permanents/sessions); duplicate ids in `permanents` seed permanent copies and must not exceed `selection_cap` — needed by the cap (`cap-aware-pool`) and overflow (`overflow-temporary`) checks |
| `{"type":"start_level","level":num}` | begin a level (usually the host); the server broadcasts the spawn set, then opens the vote session during the collapse window |
| `{"type":"vote","optionId":str}` | cast a vote in the open session; one vote per client per session |

Server → client:

| message | meaning |
|---|---|
| `{"type":"welcome","clientId","token","host":bool,"rejoin":bool,"runId"?}` | reply to `hello`; `rejoin:true` means the session (including an already-cast vote) was restored |
| `{"type":"run_started","runId","seed","class"}` | run created; all subsequent messages carry this `runId` |
| `{"type":"spawn_set","runId","level","permanents":[ids],"temporaries":[ids],"enemies":[ids]}` | level start; `permanents` lists permanent copies (with repeats), `temporaries` the overflow copies living this level only |
| `{"type":"vote_open","runId","sessionId","level","options":[{id,name,kind}],"countdown_ms"?}` | session opened during the collapse window; `kind` is `"permanent"` or `"temporary"` |
| `{"type":"vote_update","runId","sessionId","tally":{optionId:count},"voter"}` | a vote registered (including a restored vote after rejoin) |
| `{"type":"vote_closed","runId","sessionId","level","winnerId","winnerName"?,"temporary":bool}` | countdown elapsed or resolved; `temporary:true` for an overflow winner |
| `{"type":"vote_rejected","reason","runId"?}` (or `{"type":"error","reason"}`) | late/invalid vote rejected |

Sequencing per level: `start_level` → `spawn_set(level)` → `vote_open(level)` during the collapse →
votes → `vote_closed` (countdown, even with zero votes) → next `start_level`. The kit always
awaits `vote_closed` before starting the next level. The server, not the kit, simulates the level
and its collapse window.

## The ten checks

| check | asserts |
|---|---|
| `session-at-every-level` | a vote session opens at level 1 and at every later level, after the level's spawn set (i.e. in the collapse window) |
| `options-shape` | every session offers exactly 3 options, each with a nonempty `id` (a registry enemy), a nonempty `name` and a `kind` of `"permanent"`/`"temporary"`, ids unique |
| `majority-wins` | 2-1 vote ⇒ the 2-option wins |
| `tie-rules` | 1-1 tie ⇒ host's vote wins; 1-1 tie with no host vote ⇒ same winner on an identical re-run (same seed + level) |
| `permanence` | unanimous winner appears in the next level's spawn set (exactly once) and is still there two levels later |
| `cap-aware-pool` | with 2 permanents of `pallbearer` (its cap), it is never offered; offered options stay `permanent` |
| `min-level-gate` | a level-1 session never offers a registry enemy with `min_level` > 1 |
| `overflow-temporary` | with every entry capped, all offered options are `temporary`; the winner is flagged temporary, appears among level 2's temporaries, adds no permanent, and is gone at level 3 |
| `rejoin-safety` | a disconnect + reconnect with the old token keeps the client's identity (`rejoin:true`) and its pre-disconnect vote (2-1 outcome proves it was counted) |
| `countdown-late-vote` | with no votes the countdown still closes the session with a winner, and a vote sent afterwards is rejected (`vote_rejected`/`error`) |

## Selftest

`--selftest` spawns `stub/server.mjs` on an ephemeral port, runs the full suite against the
faithful stub (must pass everything), then, for each violation, runs the violation's paired check
and requires it to FAIL:

| `VOTE_STUB_VIOLATION` | paired check |
|---|---|
| `majority_inverted` | `majority-wins` |
| `tie_ignored` | `tie-rules` |
| `cap_ignored` | `cap-aware-pool` |
| `permanence_dropped` | `permanence` |
| `rejoin_forgotten` | `rejoin-safety` |
| `late_vote_accepted` | `countdown-late-vote` |

Exit 0 only when the faithful stub fully passes and every violation trips its paired check.
(Each violation run is scoped to its paired check; other checks are irrelevant there because only
one behaviour is broken at a time.)

## Assumptions and limits

- The kit defines the wire contract above; a server speaking a different protocol fails loudly
  — that failure is a reportable implementation/kit mismatch, per T-081.
- `start_run.permanents` seeding must be honoured for `cap-aware-pool` and `overflow-temporary`;
  organically filling 24 permanents through the vote would make those checks prohibitively slow.
- `vote_rejected` must be explicit for a late vote; silently ignoring it is a FAIL (check
  `countdown-late-vote`).
- Determinism is compared across two identical runs; a pick that depends only on the offered set
  (not the seed) also passes — the ticket's observable requirement is re-run stability.
