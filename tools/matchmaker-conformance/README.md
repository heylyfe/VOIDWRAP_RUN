# matchmaker conformance kit + canonical stub

Black-box conformance kit for the frozen matchmaker HTTP contract, plus the canonical dev stub it
targets (`../matchmaker-stub/`). Node built-ins only, no dependencies, plain `.mjs`.

## Run the stub

    node tools/matchmaker-stub/server.mjs            # http://127.0.0.1:8047
    node tools/matchmaker-stub/server.mjs 9000       # custom port

## Run the kit

    MATCHMAKER_URL=http://127.0.0.1:8047 node tools/matchmaker-conformance/run.mjs
    node tools/matchmaker-conformance/run.mjs --url http://127.0.0.1:8047

Prints `PASS — <check>` / `FAIL — <check>` per check and exits 0 **only** on a full pass. Without
`MATCHMAKER_URL` / `--url` the kit is a no-op (exit 0), so `just verify` can include it unconditionally:

    # one line appended to the verify recipe:
    node tools/matchmaker-conformance/run.mjs

## Checks

| check | what it exercises |
| --- | --- |
| health | `GET /health` -> 200, empty body |
| register-list | `POST /matches` (id/secret random + unguessable, supplied address verbatim, source-IP fallback), full-field exact listing |
| version-filter | `GET /matches?version=...` is an exact match (a prefix sibling must not match), unknown version -> `[]` |
| cors-header | `Access-Control-Allow-Origin: *` on `GET /matches` |
| heartbeat-refresh | `POST /matches/{id}` refreshes players/level/phase/joinable for that entry only |
| wrong-secret | wrong secret -> 403 on heartbeat AND delete, state untouched |
| delete-removes | `DELETE /matches/{id}` truly removes the entry (404 afterwards, unknown ids 404) |
| ttl-expiry | un-heartbeated entry vanishes within ~40 s; a heartbeat-refreshed one survives (~50 s run) |

## Self-test

    node tools/matchmaker-conformance/run.mjs --selftest

Runs the suite against the canonical stub (must fully pass), then against the stub with each single
deliberate violation (`MATCHMAKER_STUB_VIOLATION=<name>`, 11 of them, listed in
`tools/matchmaker-stub/server.mjs`), requiring each violation to trip its paired check. Exits 0 only
when both hold. Takes ~3 minutes (three 42 s TTL waits; the other eight phases are quick).
