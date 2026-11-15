// Gift identity colours, in one place so they can be measured (ticket T-146).
//
// Four things share the gift's visual space and must never be confused:
//
//   normal gift   black body, pale glow        <- LURE's shimmer wears exactly this
//   golden gift   gold body, gold emissive
//   straggler     a vertical beam on every remaining gift in the last 20% of a level
//   beacon        the level-exit marker
//
// The straggler beam used to be `#dfe6ff` -- byte-identical to the normal gift glow, and
// therefore identical to LURE's disguise. The beam's whole job is to say "this one is
// real", so wearing the same colour as the fake is the one thing it must not do. The
// original comment reasoned that "a third colour would collide with both"; that is only
// true of a badly chosen third colour. `#7cff4f` sits 43 delta-E from every other colour
// in the game, so the premise was wrong and the beam now has its own identity.
//
// No three.js / React imports: unit-testable in isolation.

/** Golden gift body. */
export const GOLD_BODY = "#ffd75e";
/** Golden gift ribbon. */
export const GOLD_RIBBON = "#8f6a10";
/** Golden gift bow. */
export const GOLD_BOW = "#fff6c8";
/** Golden gift emissive. */
export const GOLD_EMISSIVE = "#ffc63f";
/** Golden sparkle octahedron. */
export const SPARKLE_COLOR = "#fff3b0";

/**
 * Straggler beam colour.
 *
 * Deliberately outside the gift palette: the beam marks a *real* gift, and LURE's
 * shimmer wears the normal glow, so the beam must not be the normal glow.
 */
export const BEAM_COLOR = "#7cff4f";

/**
 * Minimum CIE76 delta-E between the straggler beam and anything it could be mistaken
 * for: the normal gift glow (and therefore LURE), the golden gift, the beacon, and every
 * enemy accent.
 */
export const BEAM_MIN_DELTA_E = 30;
