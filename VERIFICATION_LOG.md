# Verification Log

Every ticket, every gate run. Gate = spec · security · static (`just verify`) · runtime smoke · line review · logged.
Route: `plan` = Vibe / Mistral plan endpoint (unlimited) · `zen` = OpenCode Zen · `api` = paid fallback.

## Routing

| Slot | Route | Model |
|---|---|---|
| DEEPSEEK | plan | deepseek-v4-1-flash-1017 (alias `deepseek-v4`) |
| GLM | plan | zai-glm-5-3-flash-vibe-free (alias `zai-glm-5-3-flash`) |
| INKLING-DESIGN | zen | inkling-2-small-free, max reasoning |
| INKLING-VISION | zen | inkling-2-small-free, max reasoning, images |
| NEMOTRON-VISION | retired | second vision opinion skipped; OVERSEER reviews screenshots |
| UTILITY | retired | chores → GLM (docs) / DEEPSEEK (fixtures, seed data) |

### Incident R-1 — 2026-11-13 — Zen free quota exhausted → retargeted, zero impact
The OpenCode Zen free quota ran out account-wide during dispatch probing (every `*-free` model returned
"Free usage limit reached"; non-interactive `opencode run` blocks on stdin instead of erroring unless stdin is
closed — dispatch now always closes it). Per the human's routing decision, DEEPSEEK and GLM moved to the
unlimited `plan` route; NEMOTRON-VISION and UTILITY were retired and their work retargeted (table above).
No ticket was queued or delayed. Zen is used only for the two Inkling slots.

### Incident R-2 — 2026-11-13 — Windows Smart App Control blocked Rust build artefacts
SAC (enforcement mode) blocked freshly compiled build scripts / proc-macro DLLs (os error 4551), making
builds non-deterministic. The human turned SAC off; builds are clean since.

## Tickets

| Ticket | Slot | Route | Attempts | Spec | Security | Static | Smoke | Review | Result | Notes |
|---|---|---|---|---|---|---|---|---|---|---|
| M0 scaffold | OVERSEER | — | — | ✓ | ✓ | ✓ | ✓ | ✓ | merged | workspace, protocol codegen + golden fixtures, sim + WASM bridge, server tick loop, client prediction, CI, tooling |
| T-001 dispatch | OVERSEER | — | — | ✓ | ✓ | ✓ | ✓ | ✓ | merged | `tools/dispatch.mjs` + `apply-response.mjs`; sandboxed, tools disabled, stdin closed, timeouts, retries |
| T-000 dry run | DEEPSEEK | plan | 2 | ✓ | ✓ | ✓ | n/a | ✓ | dry run (not merged) | attempt 1 wasted on a denied tool call → disabled all Vibe tools |
| T-000b dry run | GLM | plan | 3 | ✓ | ✓ | ✓ | n/a | ✓ | dry run (not merged) | same tool-call issue (fixed before T-002) |
| T-000d dry run | INKLING-DESIGN | zen | 1 | ✓ | ✓ | ✓ | n/a | ✓ | dry run (not merged) | |
| T-000v dry run | INKLING-VISION | zen + image | 1 | ✓ | ✓ | n/a | n/a | ✓ | dry run | HUD transcription exact; misread the dev-mode badge as a "compass" |
| T-002 README | GLM | plan | 3 | ✓ | ✓ | ✓ | n/a | ✓ | merged, 0 rejections | official copy verified byte-exact; 2 attempts lost to turn limit → max-turns 4 |

Paid route usage: 0 tickets (0%).
