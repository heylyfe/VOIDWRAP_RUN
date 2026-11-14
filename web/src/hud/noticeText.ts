// Pure notice helpers: what a notice says, and how visible it is right now.
// Unit-tested; never throws.
import type { Notice } from '../game/store';
import { NOTICE_ALTAR, NOTICE_BOUGHT, NOTICE_EVENT } from '../protocol/generated';
import { CARD_CURSE, CARD_UPGRADE, EVENTS, SHOP, lookupCard } from '../content';

export const NOTICE_LIFETIME_MS = 4000;
export const NOTICE_FADE_MS = 500;

export interface NoticeText {
  title: string;
  body: string;
}

/**
 * Resolve a notice to plain text. Returns null for unknown kinds, unknown
 * content ids, or malformed notices; never throws.
 */
export function noticeText(notice: Notice): NoticeText | null {
  try {
    if (notice.kind === NOTICE_ALTAR) {
      const [upgradeId, curseId] = notice.ids;
      if (!upgradeId || !curseId) {
        return null;
      }
      const upgrade = lookupCard(CARD_UPGRADE, upgradeId);
      const curse = lookupCard(CARD_CURSE, curseId);
      if (upgrade === null || upgrade.kind !== CARD_UPGRADE) {
        return null;
      }
      if (curse === null || curse.kind !== CARD_CURSE) {
        return null;
      }
      return {
        title: 'THE ALTAR',
        body: `The altar gave you ${upgrade.card.name} and ${curse.card.name}.`,
      };
    }
    if (notice.kind === NOTICE_BOUGHT) {
      const [itemId] = notice.ids;
      if (!itemId) {
        return null;
      }
      const item = SHOP.find((entry) => entry.id === itemId);
      if (!item) {
        return null;
      }
      return { title: 'SHOP', body: `Bought ${item.name}.` };
    }
    if (notice.kind === NOTICE_EVENT) {
      const [eventId] = notice.ids;
      if (!eventId) {
        return null;
      }
      const event = EVENTS.find((entry) => entry.id === eventId);
      if (!event) {
        return null;
      }
      return { title: event.name, body: event.banner };
    }
    return null;
  } catch {
    return null;
  }
}

/**
 * Opacity for a notice of the given age. Opaque for the first
 * NOTICE_LIFETIME_MS - NOTICE_FADE_MS, then a linear fade to 0. Future or
 * invalid ages are clamped; non-finite input hides the notice.
 */
export function noticeOpacity(ageMs: number): number {
  if (!Number.isFinite(ageMs)) {
    return 0;
  }
  if (ageMs <= NOTICE_LIFETIME_MS - NOTICE_FADE_MS) {
    return 1;
  }
  if (ageMs >= NOTICE_LIFETIME_MS) {
    return 0;
  }
  return 1 - (ageMs - (NOTICE_LIFETIME_MS - NOTICE_FADE_MS)) / NOTICE_FADE_MS;
}
