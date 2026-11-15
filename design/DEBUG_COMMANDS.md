# DEBUG COMMAND MANIFEST — FROZEN

The contract between the server command layer (T-110) and the tester panel
(T-111). **Frozen**: adding a command is additive (new row, new id); changing
an existing id, payload shape, or effect is a breaking change and needs a
human decision.

## Transport

One protocol variant carries every command, so the wire format stays small and
the manifest — not the protocol — is the source of truth:

```
ClientMsg::Debug  { code: string, command: string, args: string }
ServerMsg::DebugResult { ok: bool, message: string }
```

`args` is a JSON object encoded as a string. An unknown key is an error, not a
silent no-op: a tester who mistypes must find out. `message` is the human
readable result — a summary on success, the reason on failure.

## Gating (all three dimensions, enforced server-side)

1. **Availability.** The `Debug` route exists only when the server was started
   with `VOIDWRAP_DEV=1` **or** the request carries a code equal to
   `VOIDWRAP_DEV_CODE`. With neither, the reply is `ok: false` and
   `"debug commands are not enabled"` — the command is never parsed.
2. **Singleplayer.** Every command is rejected while more than one player is
   connected. The check runs before the command is parsed, so a rejected
   command has no effect at all.
3. **Rate limit.** Wrong codes are counted per connection. After
   `MAX_BAD_CODES = 5` the connection is refused debug access for the rest of
   its life, even if the correct code arrives later.

The code is read from the environment at startup and never leaves the server.
It is not in the client bundle, not in the protocol, and not in this file.

## Audit log

Every executed command appends one line to the server log at `info`:
`debug command conn=<id> player=<id> cmd=<id> args=<json> ok=<bool>`.
Rejected commands log at `warn` with the rejection reason.

## Commands

`owner` is where the effect lands. `server` commands mutate authoritative run
state. `client` commands are validated and gated by the server, then applied
locally by the panel — fog is a client-side visual and has no server state.

| id | owner | args | effect |
| --- | --- | --- | --- |
| `spawn_enemy` | server | `{ "id": string, "count"?: u8 = 1, "x"?: f32, "y"?: f32, "z"?: f32 }` | Spawns `count` enemies of registry id `id` at the given point, or at a random valid spawn when omitted. Respects the registry's selection cap: spawning past the cap is an error. |
| `despawn_enemy` | server | `{ "id": string }` | Removes every live enemy of that registry id. |
| `switch_level` | server | `{ "level": u32, "seed"?: u32 }` | Jumps to level `level`, regenerating the world from `seed` (or the run's current seed). Resets the level's gifts, altars, and enemies; keeps the run's permanents, curses, upgrades, and currency. |
| `grant_gold` | server | `{ "amount": i32 }` | Adds `amount` currency. Negative amounts subtract and clamp at zero. |
| `grant_gift` | server | `{ "count"?: u8 = 1, "golden"?: bool = false }` | Marks `count` uncollected gifts as collected, as if the player had picked them up. Golden gifts are only grantable once the normal gifts are gone, matching the real rule. |
| `remove_gift` | server | `{ "count"?: u8 = 1 }` | Removes `count` uncollected gifts from the level without awarding currency. |
| `set_phase` | server | `{ "phase": u8 }` | Forces the run phase (`PHASE_ENEMY_DRAFT` / `PHASE_PLAYING` / `PHASE_REWARD_DRAFT` / `PHASE_OVER`). |
| `give_upgrade` | server | `{ "id": string }` | Applies an upgrade's mods and records it, exactly as a draft pick would. |
| `take_upgrade` | server | `{ "id": string }` | Removes one stack of an upgrade and unloads its mods. |
| `give_curse` | server | `{ "id": string }` | Applies a curse's mods and records it. |
| `take_curse` | server | `{ "id": string }` | Removes one stack of a curse and unloads its mods. |
| `trigger_altar` | server | `{ "id": u16 }` | Fires an altar's benefit and cost as if the player had used it. |
| `teleport` | server | `{ "x": f32, "y": f32, "z": f32 }` | Moves the calling player to the point. |
| `set_fog` | client | `{ "near": f32, "far": f32 }` | Sets the client fog range. `near` must be less than `far`; both must be positive. |
| `state` | server | `{}` | Readout: level, phase, seed, currency, permanents, curses, upgrades, live enemy ids, remaining gift count, vote state, and the caller's feet as `pos=x,y,z`. |

## Error codes

`message` carries the reason; `ok` is the only machine-readable signal. The
reasons are stable strings so the panel can show them verbatim:

- `debug commands are not enabled`
- `debug commands are singleplayer only`
- `too many wrong codes`
- `unknown command: <id>`
- `bad args: <detail>`
- `unknown enemy: <id>` / `unknown upgrade: <id>` / `unknown curse: <id>`
- `selection cap reached for <id>`
