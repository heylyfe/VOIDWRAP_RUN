// Typed access to design/content.json: draft cards (enemies, upgrades, curses) and per-theme screen grades.
// The Rust server reads the same file, so card ids, numbers and text can never drift between client and server.
import content from "../../design/content.json";

export type Mods = Partial<Record<string, number>>;

export interface EnemyCard {
  id: string;
  name: string;
  flavor: string;
  behavior: string;
  threat: number;
  params: { speed: number; aggro: number; grace_s: number; size: number };
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

export const ENEMIES: readonly EnemyCard[] = content.enemies;
export const UPGRADES: readonly ModCard[] = content.upgrades as ModCard[];
export const CURSES: readonly ModCard[] = content.curses as ModCard[];
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
