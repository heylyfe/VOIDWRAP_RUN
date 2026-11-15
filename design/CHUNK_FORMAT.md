# Map chunk format (binding for the catalog and the assembler)

One file: `design/chunks.json`:

    { "version": 1, "chunks": [ CHUNK, ... ] }

CHUNK:

    {
      "id": "ch05_ramp_of_embers",          // unique, [a-z0-9_]
      "name": "Ramp of Embers",
      "role": "hub" | "path" | "junction" | "dead_end",
      "min_level": 1, "difficulty": 1,      // 1..3
      "tags": ["ramp", "climb"],
      "platforms": [ { "kind": "NORMAL" | "CRUMBLE" | "JUMP_PAD" | "TRAMPOLINE" | "RAMP_PX" | "RAMP_NX" | "RAMP_PZ" | "RAMP_NZ", "min": [x, y, z], "max": [x, y, z] } ],
      "sockets": [ { "id": "a", "pos": [x, y, z], "facing": "N" | "E" | "S" | "W", "width": 4 } ],
      "gift_slots": [ { "pos": [x, y, z], "elevated": false } ],
      "enemy_slots": [ [x, y, z] ],          // hub only: exactly 8, around the beacon
      "altar_slots": [ [x, y, z] ],
      "beacon": [x, y, z]                    // hub only: beacon dais top centre
    }

Coordinates (units = metres, player 0.8 x 1.8 x 0.8):
- Chunk-local, right-handed, Y up. Facing: N = +Z, E = +X, S = -Z, W = -X (direction pointing OUT of the chunk).
- The chunk's footprint is the XZ bounding box of its platforms. A socket's `pos` lies exactly on that footprint's
  edge on the side it faces; `pos.y` is the walking height (a platform top) at the socket; a platform whose top is
  at `pos.y` touches that edge across the full socket `width` (>= 3) centred on `pos`, so two joined chunks form a
  continuous walkway.
- Joining: chunk B is rotated by a multiple of 90 degrees about Y (sockets, slots and platforms rotate with it) and
  translated so that B's socket `pos` equals A's socket `pos`, with opposite facings (A faces N, B's socket faces S).
- Platforms are axis-aligned boxes (after 90 degree rotations they stay axis-aligned); ramps are axis-aligned wedges
  (see Ramps). Walk surface = `max.y` for boxes; ramps walk on the sloped line (see Ramps).
  Typical thickness 0.5-1.0 u; pillars are tall thin NORMAL boxes under walkways (decorative support, still solid).
- Widths: walkways >= 3 u across the direction of travel; hubs/junctions >= 6 u. A ramp's rise over its run must not
  exceed 0.5 (about 26.6 degrees), its run must be at least 2.0 u, and its width across the direction of travel must be
  at least 3 u. Stairs no longer exist anywhere in the game.
- Jumps inside a chunk must stay inside the design hop-envelope table (M3_DESIGN 4.1, "design max gap").
- JUMP_PAD: a NORMAL-sized box (>= 2 x 2 u) that launches the player to ~4 u above its top.
- TRAMPOLINE: same box rules as JUMP_PAD, but launches to ~10.6 u above its top and *keeps* the
  run-up's horizontal speed (x1.15) instead of killing it, so it routes over gaps rather than up
  ledges. Reachability is checked with the conservative generic rule, so a trampoline must never be
  the only way to reach a platform.
- Gift slots: `pos.y = platform top + 0.3`, on NORMAL platforms only (never CRUMBLE/JUMP_PAD), >= 2.0 u apart,
  >= 0.5 u from platform edges. Altar slots: on NORMAL tops, >= 1.5 u from edges. Gift slots line the walkways
  (about every 2.0-2.6 u) so the assembler can fill up to the level's gift count.
- Stacking: any two platforms of different layers that overlap in XZ must have >= 4.0 u vertical clearance between
  the lower one's top and the upper one's bottom (the same rule applies between chunks).

Ramps:
- Four kinds: `RAMP_PX` low at min.x high at max.x; `RAMP_NX` low at max.x high at min.x;
  `RAMP_PZ` low at min.z high at max.z; `RAMP_NZ` low at max.z high at min.z.
- `bounds.min.y` is the surface height at the LOW edge; `bounds.max.y` is the surface height at the HIGH edge.
- The walk surface is a straight line between them across the slope axis: surface height at (x, z) is linear along
  that axis from `bounds.min.y` (low edge) to `bounds.max.y` (high edge).
- The solid volume is everything below that surface. The low end has no thickness; the high end is
  `max.y - min.y` tall.
- Ramps use the same socket, stacking-clearance and hop-envelope rules as boxes. Ramp limits (rise/run <= 0.5,
  run >= 2.0 u, width >= 3 u) are stated under Widths above.
