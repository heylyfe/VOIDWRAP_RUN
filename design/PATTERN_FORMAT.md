# Pattern format (binding for the pattern catalog and the composition engine)

**Status: FROZEN (T-069).** This document **SUPERSEDES `design/CHUNK_FORMAT.md`** for all new
content. `CHUNK_FORMAT.md` stays in the tree as the historical record of the geometry rules that
this document inherits; where the two disagree, this document wins.

A **pattern** is the same data fragment as a chunk, extended with the closed-vocabulary tags the
composition engine reasons about. The 25 chunks in `design/chunks.json` are **valid patterns** as
soon as their `tags` array gains the new tag entries described in §4 — no other change is required.

---

## 1. Catalog files

The catalog is split across three files so that two authors can work in parallel without touching
the same file:

| File | Owner | Shape |
| --- | --- | --- |
| `design/chunks.json` | frozen (25 legacy chunks) | `{ "version": 1, "chunks": [ ... ] }` |
| `design/patterns-a.json` | T-070 | `{ "version": 1, "patterns": [ ... ] }` |
| `design/patterns-b.json` | T-071 | `{ "version": 1, "patterns": [ ... ] }` |

Rules:

* The loader accepts **both** key names (`chunks` and `patterns`) in any of the three files.
* The three arrays are concatenated in the order above into one catalog at load time.
* A **missing file is not an error**: the game must still run with only `chunks.json`.
* **Duplicate `id` across files is an error.** `validate-content` reports it and refuses to build.
  Ids are unique across the whole merged catalog, not per file.
* `version` must be `1`. Unknown top-level keys are ignored.

---

## 2. The pattern object

```jsonc
{
  "id": "pt07_ember_span",              // unique across all three files, [a-z0-9_]
  "name": "Ember Span",
  "role": "hub" | "path" | "junction" | "dead_end",
  "min_level": 1,                       // first level this pattern may appear on
  "difficulty": 1,                      // 1..3
  "tags": [ ... ],                      // see §4; free-form legacy tags are still allowed
  "platforms": [ ... ],                 // unchanged from CHUNK_FORMAT.md
  "sockets": [ ... ],                   // unchanged, see §5
  "gift_slots": [ ... ],                // unchanged, see §6
  "enemy_slots": [ ... ],               // hub only: exactly 8
  "altar_slots": [ ... ],               // unchanged, see §6
  "beacon": [x, y, z] | null            // hub only: beacon dais top centre
}
```

`id`, `name`, `role`, `min_level`, `difficulty`, `platforms`, `sockets`, `gift_slots`,
`enemy_slots`, `altar_slots` and `beacon` keep exactly the meaning and the constraints they have in
`CHUNK_FORMAT.md`. This document adds only the tag vocabulary (§4) and the composition constraints
(§7).

---

## 3. Tag namespaces

All pattern tags live in the single existing `tags` array. The new fields are carried as
**prefixed entries** so the on-disk schema does not change and legacy free-form tags stay harmless:

```
theme:<value>     visual / atmospheric identity      (closed vocabulary, §4.1)
mech:<value>      what the player does there         (closed vocabulary, §4.2)
hazard:<value>    reserved for later waves           (closed vocabulary, §4.3)
<anything else>   legacy free-form tag, ignored by the engine
```

* A pattern **must** declare at least one `theme:` tag and at least one `mech:` tag.
* A pattern may declare **at most two** `theme:` tags and **at most three** `mech:` tags.
* `hazard:` tags are optional and **nothing consumes them yet**. Declaring them is legal and
  forward-compatible; the engine parses and stores them and ignores them.
* Unknown values after a known prefix are ignored by the loader (forward compatibility), but
  `validate-content` reports them as warnings so typos do not silently disappear.
* Duplicate entries of the same value are collapsed.

### 3.1 Theme vocabulary (closed — 8 values)

| Value | Meaning |
| --- | --- |
| `ember` | Fire, ash, warm orange light, scorched stone. |
| `void` | Starless emptiness, black sky, floating debris, low ambient light. |
| `frost` | Ice, snow, cold blue light, brittle surfaces. |
| `ruin` | Broken masonry, collapsed halls, rubble, faded banners. |
| `garden` | Overgrown vegetation, water, soft green light, roots. |
| `machine` | Gears, pistons, hot metal, pipes, industrial noise. |
| `spire` | Verticality, thin bridges, height, wind, distant sky. |
| `hollow` | Caverns, echoes, darkness below, dripping stone. |

### 3.2 Mechanic vocabulary (closed — 10 values)

| Value | Meaning |
| --- | --- |
| `climb` | Net vertical gain; the player goes up. |
| `drop` | Net vertical loss; the player goes down. |
| `span` | A gap crossed by a jump or a short bridge. |
| `bounce` | Relies on a jump pad or a bounce surface. |
| `crawl` | Low headroom, tight corridors, slow movement. |
| `gauntlet` | Sustained pressure: enemies, hazards, no safe footing. |
| `rest` | Safe footing, no enemies, a place to breathe. |
| `loot` | Dense gift slots; the reward room. |
| `fork` | A real choice of route; **junction role only**. |
| `dead_end` | Terminates a branch; **dead_end role only**. |

### 3.3 Hazard vocabulary (closed — 4 values, reserved)

| Value | Meaning | Consumed today |
| --- | --- | --- |
| `trampoline` | A bounce surface. | no |
| `tripmine` | A triggered trap. | no |
| `fog` | Vision-limiting volume. | no |
| `crumble` | A surface that collapses. | no |

---

## 4. Geometry (inherited, restated)

Everything in `CHUNK_FORMAT.md` §Coordinates, §Widths, §Ramps and §Stacking applies unchanged.
The rules the composition engine and the assembler depend on are restated here so an author never
has to open the old file:

* Chunk-local, right-handed, **Y up**. Facing: `N = +Z`, `E = +X`, `S = -Z`, `W = -X`, pointing
  **out** of the pattern.
* The footprint is the XZ bounding box of the pattern's platforms.
* A socket's `pos` lies **exactly on the footprint edge** on the side it faces, and `pos.y` is the
  walking height (a platform top) at that socket.
* A platform whose top is at `pos.y` touches that edge across the **full socket width**, centred on
  `pos`, so two joined patterns form a continuous walkway.
* **A pattern must have at least one socket.** Hubs have 3–4, paths 2, junctions 3–4, dead ends 1.
* Socket `width` is an integer `>= 3` and `<= 8`. Walkways are `>= 3` u across the direction of
  travel; hubs and junctions are `>= 6` u. Two sockets only stitch when their widths are equal or
  differ by at most 1 u — the assembler centres the narrower walkway on the wider one.
* Platforms are axis-aligned boxes; ramps are axis-aligned wedges with rise/run `<= 0.5`, run
  `>= 2.0` u and width `>= 3` u. **Stairs do not exist anywhere in the game.**
* Stacking: two platforms of different layers that overlap in XZ need `>= 4.0` u of vertical
  clearance between the lower one's top and the upper one's bottom. This holds between patterns
  too, so a pattern must not rely on a neighbour's volume.
* Jumps inside a pattern stay inside the design hop envelope (M3_DESIGN 4.1, "design max gap").

---

## 5. Spawn points

Unchanged from `CHUNK_FORMAT.md`:

* `gift_slots`: `{ "pos": [x, y, z], "elevated": false }`, `pos.y = platform top + 0.3`, on
  `NORMAL` platforms only (never `CRUMBLE` / `JUMP_PAD`), `>= 2.0` u apart, `>= 0.5` u from platform
  edges, spaced about every 2.0–2.6 u along the walkway.
* `enemy_slots`: `[x, y, z]`, **hub only, exactly 8**, arranged around the beacon.
* `altar_slots`: `[x, y, z]` on `NORMAL` tops, `>= 1.5` u from platform edges.
* `beacon`: `[x, y, z]`, **hub only**, beacon dais top centre. Non-hub patterns write `null` or
  omit the key.

---

## 6. Composition constraints (the engine honours these)

The composition engine builds a `CompositionPlan` — an ordered list of pattern ids — from
`(seed, level, players)` **before any geometry is placed**, then the growth walk drains that plan.
The constraints below are the contract between the catalog and that engine.

### 6.1 Role order

| Role | May appear | May follow | May be followed by |
| --- | --- | --- | --- |
| `hub` | exactly once, **first** | — | `path`, `junction` |
| `path` | any number | `hub`, `path`, `junction` | `path`, `junction`, `dead_end` |
| `junction` | up to the level's junction budget | `hub`, `path`, `junction` | `path`, `junction`, `dead_end` |
| `dead_end` | at most one per six patterns | `path`, `junction` | — (terminates the branch) |

A `dead_end` is never placed by the growth walk; the assembler seals leftover sockets with them.
A `hub` is never placed by the growth walk either — it is placed first, at the origin, unrotated.

### 6.2 Run lengths

* **Theme run**: at most **3** consecutive patterns may share a theme tag. The fourth must
  introduce a different theme.
* **Mechanic run**: at most **2** consecutive patterns may share the same `mech:` tag.
* **Level length**: the plan is `10 + level` entries, clamped to `[10, 48]`.

### 6.3 Tag interactions

* Two `rest` patterns may **not** be adjacent.
* Two `gauntlet` patterns may **not** be adjacent.
* `fork` may only appear on a `junction` role pattern.
* `dead_end` may only appear on a `dead_end` role pattern.
* `loot` may appear at most once every three patterns.
* A pattern with `mech:climb` must actually gain height; a pattern with `mech:drop` must actually
  lose height. `mech:span` requires a real gap of at least 2.0 u.
* `hazard:` tags never constrain composition in this wave.

### 6.4 Selection bias

The bag that fills the plan is seeded and integer-only. It:

1. never returns the same pattern twice in a row (hard rule whenever two or more patterns are
   eligible);
2. divides the weight of any pattern seen in the last **6** picks by **32**;
3. adds a bonus to patterns carrying a theme that has been used fewer than **2** times so far in
   the level, so a level has a coherent but varied theme mix;
4. subtracts a small penalty for each difficulty step above the level's tier.

If the catalog carries no pattern tags at all, the engine falls back to the pre-T-069 weighted
pick and the level is assembled exactly as before.

---

## 7. Determinism

Same `(seed, level, players)` must always produce the same plan and the same level. Nothing in the
engine may read the wall clock, and no float transcendental (`sin`, `cos`, `powf`, …) may appear in
the selection path — the sim crate is compiled to `wasm32-unknown-unknown` and must stay bit-exact.
The bag uses integer weights and an integer PRNG only.

---

## 8. Worked examples

### 8.1 A path pattern

```json
{
  "id": "pt07_ember_span",
  "name": "Ember Span",
  "role": "path",
  "min_level": 1,
  "difficulty": 2,
  "tags": ["theme:ember", "theme:ruin", "mech:span", "mech:climb", "hazard:crumble", "bridge", "gap"],
  "platforms": [
    { "kind": "NORMAL",  "min": [-3.0, 0.0, -4.0], "max": [3.0, 0.5, 4.0] },
    { "kind": "CRUMBLE", "min": [-3.0, 0.0,  4.0], "max": [3.0, 0.5, 8.0] },
    { "kind": "NORMAL",  "min": [-3.0, 0.0,  8.0], "max": [3.0, 0.5, 12.0] }
  ],
  "sockets": [
    { "id": "a", "pos": [0.0, 0.5, -4.0], "facing": "S", "width": 6 },
    { "id": "b", "pos": [0.0, 0.5, 12.0], "facing": "N", "width": 6 }
  ],
  "gift_slots": [
    { "pos": [-1.5, 0.8, 0.0], "elevated": false },
    { "pos": [ 1.5, 0.8, 10.0], "elevated": false }
  ],
  "enemy_slots": [],
  "altar_slots": [[0.0, 0.5, 10.0]],
  "beacon": null
}
```

Why it is legal: footprint is `x ∈ [-3, 3]`, `z ∈ [-4, 12]`; socket `a` sits on the `z = -4` edge
facing `S`, socket `b` on the `z = 12` edge facing `N`, both at walking height `0.5` and both 6 u
wide, matching the 6 u walkway. Gift slots are at `top + 0.3 = 0.8`, on `NORMAL` tops, 10 u apart
and 1.5 u from the side edges. The altar slot is on a `NORMAL` top, 2 u from the `z = 12` edge.
All platforms share one layer, so the stacking rule is vacuous.

### 8.2 A hub pattern

```json
{
  "id": "pt01_ember_hub",
  "name": "Ember Hub",
  "role": "hub",
  "min_level": 1,
  "difficulty": 1,
  "tags": ["theme:ember", "mech:rest", "mech:loot", "mech:fork", "arena"],
  "platforms": [
    { "kind": "NORMAL", "min": [-8.0, 0.0, -8.0], "max": [8.0, 0.5, 8.0] }
  ],
  "sockets": [
    { "id": "n", "pos": [ 0.0, 0.5,  8.0], "facing": "N", "width": 6 },
    { "id": "e", "pos": [ 8.0, 0.5,  0.0], "facing": "E", "width": 6 },
    { "id": "s", "pos": [ 0.0, 0.5, -8.0], "facing": "S", "width": 6 },
    { "id": "w", "pos": [-8.0, 0.5,  0.0], "facing": "W", "width": 6 }
  ],
  "gift_slots": [
    { "pos": [-4.0, 0.8, 0.0], "elevated": false },
    { "pos": [ 4.0, 0.8, 0.0], "elevated": false }
  ],
  "enemy_slots": [
    [0.0, 0.5, 5.0], [3.5, 0.5, 3.5], [5.0, 0.5, 0.0], [3.5, 0.5, -3.5],
    [0.0, 0.5, -5.0], [-3.5, 0.5, -3.5], [-5.0, 0.5, 0.0], [-3.5, 0.5, 3.5]
  ],
  "altar_slots": [[0.0, 0.5, -6.0]],
  "beacon": [0.0, 0.5, 0.0]
}
```

Why it is legal: 16 × 16 u arena, four 6 u sockets centred on each edge at walking height, exactly
8 enemy slots on a radius-5 ring around the beacon at the dais centre, gift slots 8 u apart and 4 u
from the side edges, altar slot 2 u from the `z = -8` edge.

---

## 9. Authoring checklist

Before submitting a pattern:

1. `id` is unique across all three files and matches `[a-z0-9_]+`.
2. At least one `theme:` tag, at least one `mech:` tag, at most two themes, at most three
   mechanics. Every value is from §3.
3. `fork` only on `junction`, `dead_end` only on `dead_end`.
4. At least one socket; every socket sits on the footprint edge it faces, at walking height, with
   `3 <= width <= 8`.
5. Walkways `>= 3` u, hubs and junctions `>= 6` u, ramps within rise/run `<= 0.5`.
6. Gift slots on `NORMAL` tops at `top + 0.3`, `>= 2.0` u apart, `>= 0.5` u from edges.
7. Altar slots on `NORMAL` tops, `>= 1.5` u from edges.
8. Hubs: exactly 8 enemy slots and a beacon. Non-hubs: no enemy slots, `beacon: null`.
9. No reliance on a neighbour's volume for stacking clearance.
10. `cargo run -p <validate-content>` reports no errors and no tag warnings.

