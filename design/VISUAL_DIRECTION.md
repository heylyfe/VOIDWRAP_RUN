# Visual direction

Every visual ticket and visual QA pass is judged against these rules. Colours live in `design/palette.json`;
code reads them from there (never hard-code a themed colour in a component).

1. **One hue family per level.** Each level picks a theme (`(level - 1) % 4`): Ashfall (grey storm + lavender),
   Fluxbloom (magenta haze), Slagstone (slate dusk, black/white tiles, red danger), Tidegrave (deep teal). The world
   is chromatic across a run, monochrome-ish within a level. Gifts, enemies, beacon and player keep their common colours
   so they always pop against any theme.
2. **Checkerboard tiles are the signature surface.** Every walkable top is a 1×1-unit world-aligned checker of the
   theme's light/dark tile colours, with a subtle grungy/marbled noise so it never looks like a flat test texture.
   Sides use the darker `tile.side` tone (still checkered, dimmer). Thin glowing `tile.edge` trims outline tops.
3. **The void is a coloured atmosphere, not black.** A full sky dome with a vertical gradient (zenith → horizon) and
   slow-moving procedural clouds/marbling in the theme's cloud colours; heavy distance fog in `fog.color` so far
   platforms melt into the sky. Black should be under ~20% of a typical frame.
4. **Bloom makes it glow.** Emissive accents (gift glow, enemy, beacon, edge trims, accent sparks) bloom strongly;
   tiles and sky do not. Subtle chromatic aberration at the frame edges, light vignette.
5. **The exit is a blinding pillar of light** that is visible from anywhere on the level once open (dim, desaturated
   pillar while closed).
6. **Debris and motion.** Swarms of small dark cubes (`debris`) drift and tumble through the void around the level;
   occasional accent-coloured sparks. The world should feel alive and slightly chaotic, never static.
7. **Danger reads red.** Enemies are saturated red with glow and a red hazard ring on the ground below them.
8. **Readable at speed.** Silhouettes over everything: the player, gifts and enemies must be identifiable at 30 units
   in every theme (checked in visual QA per theme).
9. **HUD.** Large gift counter `collected / total` at bottom centre in warm light text with a dark shadow; level and
   status top centre; event banners as translucent tinted bars low on screen. Original layout and type.
10. **Geometry (M2 generator).** Long narrow walkways (1–3 units wide) and sprawling multi-height paths — a maze of
    roads — rather than isolated square pads.
