# Enemy registry — frozen contract

**Status: FROZEN (T-072).** This document plus the `enemies` array in `design/content.json` is the
single source of truth for enemy selection. T-074, T-075, T-078, T-080 and T-085 read it; none of
them may add a field, a tag, a class relation or a synergy effect on their own. Changing the shape
means a new ticket that edits this file, `design/content.json`, `server/crates/server/src/content.rs`
and `tools/validate-content.mjs` in the same commit.

The registry is **data**. Nothing here is code, and nothing here may be computed at runtime from
another field: every value below is written out per entry and validated.

---

## 1. Entry shape

Every enemy, existing and new, has exactly one entry in `design/content.json` → `enemies[]`:

| field | type | required | meaning |
|---|---|---|---|
| `id` | string, `^[a-z][a-z0-9_]{1,31}$` | yes | stable id; equals `params.behavior`; array order is the network kind index |
| `name` | string, ≤ 24 chars | yes | plain, what-it-does name; must stay green under `tools/check-names.mjs` |
| `flavor` | string, ≤ 70 chars | yes | one line of voice |
| `behavior` | string, ≤ 110 chars | yes | player-facing instruction: what to do about it |
| `threat` | integer 1..5 | yes | design weight, used for balance review only — never for pool filtering |
| `cue_tier` | integer 1..3 | yes | telegraph class (§2) |
| `min_level` | integer ≥ 1 | yes | first level the enemy may be offered (§3) |
| `selection_cap` | integer 1..2 | yes | permanent copies allowed in one run (§4) |
| `tags` | array of strings | yes, ≥ 1 | closed vocabulary (§5) |
| `class_interactions` | object | yes (may be `{}`) | class id → relation (§6) |
| `synergy_hooks` | array of objects | yes (may be `[]`) | declarative hooks (§7) |
| `params` | object | yes | `speed`, `aggro`, `grace_s`, `size`, `behavior` — unchanged, owned by the sim (§8) |

`params.behavior` must equal `id`. The sim maps behaviour strings to `BEHAVIOR_*` constants; a
string it does not know yet degrades to `BEHAVIOR_PALLBEARER`. That is the intended state for the
eleven entries added in T-072 until T-074 / T-075 land their constants.

---

## 2. `cue_tier` — what the implementation owes the player

The tier is a promise about the **wind-up**, not about the damage.

**Tier 1 — under 1 s telegraph (instant dodge).**
- The tell is on the enemy body itself (shape, colour, aperture); no floor decal is required.
- The wind-up is visible for **0.35 s – 0.95 s** before the hit resolves.
- The punish must be avoidable with **one dash from a standing start** at the enemy's design speed.
- No audio-only tells. No hit that resolves in the same tick the tell appears.

**Tier 2 — 1 s – 2 s (reposition).**
- A floor decal or aim line appears for **1.0 s – 2.0 s** before the hit resolves.
- The decal shows the **exact hit volume**; the player can leave it by walking, no dash needed.
- The decal persists for the whole wind-up and is removed on resolve.

**Tier 3 — ambient (awareness only).**
- No per-attack telegraph. The enemy has a **persistent state cue** (aura, fog density, looping
  sound) that changes when it is active, so the player can read the level without watching it.
- The punish is a **zone or state the player can leave at any time**. A tier-3 enemy may never
  land an unavoidable instant hit.

---

## 3. `min_level`

1-based. Level 1 is the first draft after the lobby. An entry with `min_level = N` is **invisible to
the pool** for every level `< N`. It is not a difficulty rating and it is not a threat gate: it is
the first level at which the enemy may be *offered*.

---

## 4. `selection_cap`

The maximum number of **permanent** copies of that enemy that may exist at once in a single run.
`1` = unique, `2` = a pair is allowed.

- Copies granted by milestones (`milestone_6`, `milestone_10`, `milestone_20`) are permanent and
  count against the cap. A grant that would exceed the cap grants nothing for that enemy.
- **Temporary** copies (overflow, §9) never count against the cap.

---

## 5. `tags` — closed vocabulary

| tag | meaning |
|---|---|
| `fog` | changes what the player can see (fog density, sight radius) |
| `ambush` | attacks from a position the player cannot see: above, behind cover, or disguised |
| `collapse` | removes or falsifies floor |
| `chase` | moves toward the player with intent |
| `zone` | owns an area the player must leave or cross |
| `debuff` | applies a persistent negative state (slow, silence, timer stall) |
| `summon` | creates other actors |
| `small` | body under 0.8 units; counts for `chorus_needs_small_pair` |
| `large` | body 1.2 units or more; blocks sight lines |
| `loot` | interacts with gifts / loot |

Tags are semantic, not a size mirror: `small` and `large` are applied when the size class matters to
the pool, so a 0.7-unit trap that is not a body may carry neither. Tags are unordered and unique per
entry.

---

## 6. `class_interactions` — closed vocabulary

Keys are class ids from `design/content.json` → `classes[]`. **The file ships exactly four classes**
(`striker`, `grappler`, `bulwark`, `glider`) and `tools/validate-content.mjs` pins the array to those
four; there is no fifth class. The vocabulary is closed at four until a fifth class ships, at which
point this section and `CLASS_IDS` in `content.rs` grow together.

Values:

| relation | meaning | pool effect |
|---|---|---|
| `counters` | the class's base ability fully answers the signature threat, no card needed | none |
| `immune` | the signature effect is never applied to that class | none; the implementation must skip the effect |
| `ignores` | the enemy never selects that class — it is not attracted to them | the enemy is **removed from the pool** for a run of that class |

A class that is absent from the object is **neutral**: no counter, no immunity, no exclusion.

---

## 7. `synergy_hooks` — declarative, closed effect set

A hook is data. The engine reads it; it never executes content.

```json
{ "with": "coil", "effect": "peal_lethal_when_overtuned" }
{ "requires": { "tag": "small", "count": 2 }, "effect": "chorus_needs_small_pair" }
```

Exactly one of `with` / `requires` is present per hook.

- `with` — another enemy id. The hook fires when that enemy is a **permanent** in the run.
- `requires` — `{ tag, count }`. The hook is satisfied when the run already holds at least `count`
  permanents carrying `tag`. The level gate is the owning entry's own `min_level`.

Hooks are declared on the enemy that **owns** the effect, never on both sides.

### Closed effect set

| effect | semantics |
|---|---|
| `peal_lethal_when_overtuned` | If `coil` is a permanent in the run and Peal's zone has reached its final ring, the final ring deals lethal damage instead of the debuff. Without Coil the final ring only debuffs. |
| `infection_pauses_timer` | While Blight's infection is on the player, the named enemy's signature timer (Rewind's rewind clock, Kite's tether clock) does not advance. It resumes at its previous value when the infection clears. |
| `chorus_needs_small_pair` | Chorus is only a candidate when the run already holds at least 2 permanents tagged `small`. |

Adding a fourth effect is a spec change. The engine must reject an unknown effect name at load
(`Content::load`) and at validation time (`tools/validate-content.mjs`).

---

## 8. `params`

`speed`, `aggro`, `grace_s`, `size`, `behavior` — unchanged by T-072 and owned by the sim. The
registry never rewrites them; `RunMods::enemy_params` applies the curses on top. `grace_s` is 4.0
for every entry (nothing damages anyone in the first 4 s).

---

## 9. Pool rules (the voting system)

`permanents(run, id)` = permanent copies of `id` currently in the run. Temporary copies are not
counted anywhere in this section.

```
pool(level, run) =
  enemies
    .filter(e => e.min_level <= level)
    .filter(e => permanents(run, e.id) < e.selection_cap)
    .filter(e => every hook with `requires` is satisfied)
    .filter(e => e.class_interactions[run.class] !== "ignores")
    .sort(by min_level ascending, then by content order)
```

- **`min_level`** removes entries above the current level. The pool is never padded with them.
- **`selection_cap` + current permanent count** removes entries already at their cap.
- **`requires` hooks** remove entries whose prerequisite permanents are not held yet.
- **Class composition** removes entries that `ignores` the run's class. `counters` and `immune`
  never filter; they are balance and implementation facts.
- **Order** is deterministic: `min_level` ascending, ties broken by content order. No RNG.

The vote draws the first `POOL_OFFER = 3` candidates in that order. (`POOL_OFFER` is a T-072
invention; the vote UI in T-078 may change the count, never the order.)

### Empty pool — overflow

If the filtered pool is empty, the run does **not** skip the enemy slot. It drafts a **temporary**
enemy instead:

1. Take the permanent enemy with the highest `min_level <= level` (tie → content order) and add one
   temporary copy for the next level only.
2. If the run holds no permanents yet (level 1), take the lowest-`min_level` entry that is not
   excluded by class composition.
3. Temporary copies: do not count toward `selection_cap`, do not satisfy `requires` hooks, are
   removed when the level ends, and are never offered to the vote.

---

## 10. The registry

### 10.1 Numbers

| id | name | cue | min_level | cap | threat | tags |
|---|---|---|---|---|---|---|
| `pallbearer` | Footprint Chaser | 2 | 1 | 2 | 3 | chase, large |
| `snapshrike` | Orbit Dasher | 1 | 3 | 1 | 4 | chase, small |
| `moteling` | Stillness Shrieker | 2 | 1 | 1 | 2 | zone, debuff, small |
| `rift_warden` | Wall Drifter | 2 | 2 | 1 | 2 | zone, large |
| `gale_wight` | Shell Bomber | 2 | 4 | 1 | 4 | zone, ambush |
| `lantern_leech` | Gift Stinger | 2 | 2 | 1 | 3 | chase, loot, small |
| `turret` | Bolt Turret | 2 | 2 | 1 | 3 | zone |
| `mimic` | False Gift | 1 | 3 | 2 | 3 | ambush, small |
| `corrupter` | Floor Rotter | 3 | 2 | 1 | 2 | zone, debuff, collapse |
| `rewind` | Path Replayer | 2 | 1 | 2 | 3 | chase, zone |
| `warden` | Gift Guard | 2 | 2 | 1 | 3 | chase, loot |
| `peal` | Bell Ringer | 1 | 4 | 1 | 4 | zone, debuff |
| `kite` | Tether Tracker | 2 | 6 | 1 | 4 | chase, zone |
| `static` | Fog Dasher | 3 | 5 | 2 | 3 | fog, chase |
| `blight` | Ground Infector | 3 | 8 | 1 | 4 | debuff, chase |
| `metronome` | Beat Keeper | 1 | 10 | 1 | 5 | zone, debuff |
| `overlook` | Marking Eye | 2 | 12 | 1 | 4 | zone, chase |
| `chorus` | Choir Summoner | 3 | 10 | 1 | 4 | summon, small |
| `coil` | Spring Leaper | 1 | 4 | 2 | 3 | zone, debuff |
| `lure` | Fog Decoy | 1 | 3 | 2 | 3 | fog, ambush, collapse |

### 10.2 Interactions and hooks

| id | class_interactions | synergy_hooks |
|---|---|---|
| `pallbearer` | striker: counters, glider: ignores | — |
| `snapshrike` | striker: counters, bulwark: immune | — |
| `moteling` | bulwark: immune | — |
| `rift_warden` | grappler: counters, bulwark: immune | — |
| `gale_wight` | glider: counters | — |
| `lantern_leech` | grappler: counters | — |
| `turret` | bulwark: immune, glider: counters | — |
| `mimic` | striker: counters | — |
| `corrupter` | glider: immune, grappler: counters | — |
| `rewind` | striker: counters, grappler: immune | — |
| `warden` | grappler: counters, bulwark: immune | — |
| `peal` | bulwark: immune, glider: counters | with `coil` → `peal_lethal_when_overtuned` |
| `kite` | striker: counters, grappler: counters | — |
| `static` | glider: ignores, bulwark: immune | — |
| `blight` | glider: immune, grappler: counters | with `rewind` → `infection_pauses_timer`; with `kite` → `infection_pauses_timer` |
| `metronome` | glider: counters, bulwark: immune | — |
| `overlook` | striker: counters, glider: ignores | — |
| `chorus` | striker: counters, bulwark: immune | requires `small` × 2 → `chorus_needs_small_pair` |
| `coil` | striker: counters, glider: ignores | — |
| `lure` | glider: immune, grappler: counters | — |

---

## 11. Contracts for the eleven new entries (T-074 / T-075)

Behaviour is implemented in T-074 / T-075. This section is the contract they build against; the
`behavior` string in `content.json` is the player-facing version of the same thing.

### `rewind` — Path Replayer (cue 2, min 1, cap 2, chase/zone)
- Cue: the body trembles in place for 1.5 s with a floor mark on the exact hit volume.
- Punish: it then replays the nearest player's path from 1.5 s ago. Touching it is lethal.
- Counter: sharp direction changes and jump pads break the replay; a Striker dash breaks the drag;
  a Grappler is immune (the grapple anchors).

### `warden` — Gift Guard (cue 2, min 2, cap 1, chase/loot)
- Cue: a ring around the gift it guards, 1.5 s, before its reach extends to the ring edge.
- Punish: standing inside the ring when it closes is lethal; it lunges at whoever takes a guarded
  gift and frenzies in the collapse phase.
- Counter: Grappler pulls the gift out; Bulwark is immune (the ward shoves it off its post).

### `peal` — Bell Ringer (cue 1, min 4, cap 1, zone/debuff)
- Cue: the bell body opens for 0.6 s before each ring.
- Punish: it teleports beside the closest player every 6 s and rings three times. The rings are
  harmless unless the target carries OVERTUNED, in which case the ring kills.
- Counter: Bulwark is immune (the ward ignores the ring); Glider crosses above it.
- Hook: with `coil` permanent the final ring is lethal outright.

### `kite` — Tether Tracker (cue 2, min 6, cap 1, chase/zone)
- Cue: the tether brightens and the strike marker tracks the target for 1.4 s, then freezes for
  0.4 s before the blast rings resolve.
- Punish: the frozen marker detonates into blast rings.
- Counter: Striker and Grappler both close the gap and cut the line; height breaks it too.

### `static` — Fog Dasher (cue 3, min 5, cap 2, fog/chase)
- Cue: ambient — the fog patch around the body is the persistent cue and it is also the hit volume,
  so a player can walk out of it at any time.
- Punish: it dashes, strikes, then goes invisible and reappears beside the next-closest player.
- Counter: Glider is never selected (silent); Bulwark is immune to the strike.

### `blight` — Ground Infector (cue 3, min 8, cap 1, debuff/chase)
- Cue: ambient — infected ground is discoloured and spreads one tile per beat.
- Punish: crossing infected ground applies the infection; it slows the victim and stalls timers.
- Counter: Glider is immune (airborne); Grappler crosses above the infection.
- Hook: while infected, `rewind` and `kite` timers do not advance.

### `metronome` — Beat Keeper (cue 1, min 10, cap 1, zone/debuff)
- Cue: the arm flashes for 0.5 s before each swing resolves.
- Punish: a beacon shows a number that ticks up; the marked player must use their class ability
  exactly on that number or be struck.
- Counter: Glider crosses the swing; Bulwark is immune (never struck).

### `overlook` — Marking Eye (cue 2, min 12, cap 1, zone/chase)
- Cue: a 1.6 s sight line onto the marked player before the laser locks.
- Punish: a slow tracking laser that only the marked player can see and be harmed by. Teammates can
  draw its path during the collapse phase.
- Counter: Striker closes and kills it; Glider is never marked (out of its floor-bound sight).

### `chorus` — Choir Summoner (cue 3, min 10, cap 1, summon/small)
- Cue: ambient — the choir murmurs before anything can hurt you.
- Punish: instruments spawn as special gifts; each one collected freezes the choir and extends its
  timer. When the timer runs out it moves and hunts.
- Counter: Striker kills the instruments; Bulwark is immune (the ward shoves them off).
- Hook: `chorus_needs_small_pair` — only a candidate once two `small` permanents are held.

### `coil` — Spring Leaper (cue 1, min 4, cap 2, zone/debuff)
- Cue: the coil visibly winds, 0.8 s, before release.
- Punish: winds tighter every second the player stays near it, then leaps and releases a shockwave
  that inflicts OVERTUNED.
- Counter: Striker dashes out; Glider is never attracted (never stands in it).

### `lure` — Fog Decoy (cue 1, min 3, cap 2, fog/ambush/collapse)
- Cue: it sits in fog glowing like a gift; the fog pulses slowly and that pulse is the telegraph.
  A 0.6 s flare-up precedes the shockwave.
- Punish: a player inside the short radius gets the flare, a small shockwave (knockback, no damage),
  then a few seconds of pursuit before it re-dorms elsewhere.
- Counter: a jump-pad launch or a Grappler's ability escapes the pursuit; a Glider sees the true
  star silhouette through the glimmer.

---

## 12. Migration notes

- The nine M3 entries keep their `id`, `name`, `flavor`, `behavior`, `threat` and `params` byte for
  byte. Only the registry fields were added.
- The eleven new entries are **appended** after `corrupter`: array order is the network kind index,
  so the first nine positions must never move.
- `params.behavior` for the new entries equals their id. Until T-074 / T-075 add the matching
  `BEHAVIOR_*` constants, `behavior_from_str` degrades them to `BEHAVIOR_PALLBEARER` — the entries
  load and validate, they simply reuse the pallbearer brain.
- `small` was added to the four existing bodies under 0.8 units (`snapshrike`, `moteling`,
  `lantern_leech`, `mimic`) so that `chorus_needs_small_pair` is satisfiable: without them only
  Chorus itself carries `small`, and Chorus has `selection_cap` 1.
