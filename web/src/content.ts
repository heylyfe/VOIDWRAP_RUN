// Typed access to design/content.json: draft cards (enemies, upgrades, curses), run classes, lobby shop stock,
// level events, milestone notices and altars, plus per-theme screen grades.
// The Rust server reads the same file, so card ids, numbers and text can never drift between client and server.
import content from "../../design/content.json";

export type Mods = Partial<Record<string, number>>;

/** How an enemy moves once it is awake. Absent on a card means "chaser". */
export type EnemyBehavior = "chaser" | "turret" | "mimic" | "corrupter";

export interface EnemyParams {
  speed: number;
  aggro: number;
  grace_s: number;
  size: number;
  /** Movement archetype; "chaser" when omitted. */
  behavior?: EnemyBehavior;
  /** turret: seconds between bolts. */
  fire_interval_s?: number;
  /** turret: bolt travel speed in units per second. */
  projectile_speed?: number;
  /** mimic: distance at which the gift disguise drops. */
  reveal_radius?: number;
  /** corrupter: seconds between platform corruptions. */
  corrupt_interval_s?: number;
}

export interface EnemyCard {
  id: string;
  name: string;
  flavor: string;
  behavior: string;
  threat: number;
  params: EnemyParams;
}

export interface ModCard {
  id: string;
  name: string;
  flavor: string;
  effect: string;
  rarity: "common" | "rare";
  mods: Mods;
  threat?: number;
  greater?: boolean;
}

/** A playable class: one ability, one control line. */
export interface ClassCard {
  id: string;
  name: string;
  flavor: string;
  ability: string;
  controls: string;
}

export type ShopKind = "life" | "reroll" | "mods";

/** A lobby purchase paid for with golden gifts. */
export interface ShopItem {
  id: string;
  name: string;
  flavor: string;
  effect: string;
  cost: number;
  stock: number;
  kind: ShopKind;
  /** Only on kind "mods": permanent for the run. */
  mods?: Mods;
}

/** A level-wide twist, keyed by the wire event number. */
export interface LevelEvent {
  id: string;
  event: number;
  name: string;
  banner: string;
  /** slick_ice only: ground friction multiplier. */
  friction_mul?: number;
}

/**
 * A milestone notice: the phase-2 collapse, or one escalation step from design §5.
 * The server broadcasts these as NOTICE_EVENT ids ("collapse", "milestone_9", ...).
 */
export interface Milestone {
  id: string;
  /** Escalation level that fires it; 0 for the collapse. */
  level: number;
  name: string;
  banner: string;
}

/** Title + body of one notice line. */
export interface NoticeLine {
  title: string;
  body: string;
}

export interface Altar {
  id: string;
  name: string;
  flavor: string;
  effect: string;
}

export interface ScreenGrade {
  tint: string;
  tint_strength: number;
  saturation: number;
  contrast: number;
  brightness: number;
  vignette_color: string;
  vignette_darkness: number;
  aberration: number;
  grain: number;
  scanlines: number;
}

export const ENEMIES: readonly EnemyCard[] = content.enemies as EnemyCard[];
export const UPGRADES: readonly ModCard[] = content.upgrades as ModCard[];
export const CURSES: readonly ModCard[] = content.curses as ModCard[];
export const CLASSES: readonly ClassCard[] = content.classes as ClassCard[];
export const SHOP: readonly ShopItem[] = content.shop as ShopItem[];
export const EVENTS: readonly LevelEvent[] = content.events as LevelEvent[];
export const MILESTONES: readonly Milestone[] = content.milestones as Milestone[];
export const ALTARS: readonly Altar[] = content.altars as Altar[];
const GRADES: Record<string, ScreenGrade> = content.grades;

/** Card kinds as used on the wire (protocol DraftCard.kind). */
export const CARD_ENEMY = 0;
export const CARD_UPGRADE = 1;
export const CARD_CURSE = 2;

export type AnyCard =
  | { kind: typeof CARD_ENEMY; card: EnemyCard }
  | { kind: typeof CARD_UPGRADE; card: ModCard }
  | { kind: typeof CARD_CURSE; card: ModCard };

/** Resolve a wire card (kind + id) to its content; null for unknown ids (never trust the wire blindly). */
export function lookupCard(kind: number, id: string): AnyCard | null {
  if (kind === CARD_ENEMY) {
    const card = ENEMIES.find((e) => e.id === id);
    return card ? { kind: CARD_ENEMY, card } : null;
  }
  if (kind === CARD_UPGRADE) {
    const card = UPGRADES.find((e) => e.id === id);
    return card ? { kind: CARD_UPGRADE, card } : null;
  }
  if (kind === CARD_CURSE) {
    const card = CURSES.find((e) => e.id === id);
    return card ? { kind: CARD_CURSE, card } : null;
  }
  return null;
}

/** Level event by its wire number; null when the level carries no event. */
export function eventInfo(event: number): LevelEvent | null {
  return EVENTS.find((e) => e.event === event) ?? null;
}

/** Milestone by notice id ("collapse", "milestone_9", ...); null for unknown ids. */
export function milestoneInfo(id: string): Milestone | null {
  return MILESTONES.find((m) => m.id === id) ?? null;
}

/**
 * Text for a NOTICE_EVENT id. Level events win over milestones, so an id that
 * exists in both always shows the event banner; unknown ids return null.
 */
export function noticeEventText(id: string): NoticeLine | null {
  const event = EVENTS.find((e) => e.id === id);
  if (event) {
    return { title: event.name, body: event.banner };
  }
  const milestone = milestoneInfo(id);
  if (milestone) {
    return { title: milestone.name, body: milestone.banner };
  }
  return null;
}

const NEUTRAL_GRADE: ScreenGrade = {
  tint: "#ffffff",
  tint_strength: 0,
  saturation: 0,
  contrast: 0,
  brightness: 0,
  vignette_color: "#000000",
  vignette_darkness: 0.4,
  aberration: 0.001,
  grain: 0.02,
  scanlines: 0,
};

export function gradeForTheme(themeId: string): ScreenGrade {
  return GRADES[themeId] ?? NEUTRAL_GRADE;
}
