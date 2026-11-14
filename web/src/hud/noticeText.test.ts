import { describe, expect, it } from 'vitest';
import { CURSES, EVENTS, SHOP, UPGRADES } from '../content';
import { NOTICE_ALTAR, NOTICE_BOUGHT, NOTICE_EVENT } from '../protocol/generated';
import type { Notice } from '../game/store';
import { NOTICE_FADE_MS, NOTICE_LIFETIME_MS, noticeOpacity, noticeText } from './noticeText';

const upgrade = UPGRADES[0];
const curse = CURSES[0];
const event = EVENTS[0];
const shopItem = SHOP[0];
if (!upgrade || !curse || !event || !shopItem) {
  throw new Error('design/content.json must define at least one upgrade, curse, event and shop item');
}

const makeNotice = (kind: number, ids: string[]): Notice => ({ seq: 1, kind, ids, at: 0 });

describe('noticeText', () => {
  it('describes an altar notice from the card manifest', () => {
    const text = noticeText(makeNotice(NOTICE_ALTAR, [upgrade.id, curse.id]));
    expect(text).not.toBeNull();
    expect(text?.title).toBe('THE ALTAR');
    expect(text?.body).toContain(upgrade.name);
    expect(text?.body).toContain(curse.name);
  });

  it('returns null when an altar id is unknown or missing', () => {
    expect(noticeText(makeNotice(NOTICE_ALTAR, ['no-such-upgrade', curse.id]))).toBeNull();
    expect(noticeText(makeNotice(NOTICE_ALTAR, [upgrade.id, 'no-such-curse']))).toBeNull();
    expect(noticeText(makeNotice(NOTICE_ALTAR, [upgrade.id]))).toBeNull();
    expect(noticeText(makeNotice(NOTICE_ALTAR, []))).toBeNull();
  });

  it('describes a shop purchase', () => {
    const text = noticeText(makeNotice(NOTICE_BOUGHT, [shopItem.id]));
    expect(text?.title).toBe('SHOP');
    expect(text?.body).toContain(shopItem.name);
  });

  it('returns null for an unknown shop item', () => {
    expect(noticeText(makeNotice(NOTICE_BOUGHT, ['no-such-item']))).toBeNull();
    expect(noticeText(makeNotice(NOTICE_BOUGHT, []))).toBeNull();
  });

  it('describes a level event with its banner text', () => {
    expect(noticeText(makeNotice(NOTICE_EVENT, [event.id]))).toEqual({
      title: event.name,
      body: event.banner,
    });
  });

  it('returns null for an unknown event', () => {
    expect(noticeText(makeNotice(NOTICE_EVENT, ['no-such-event']))).toBeNull();
    expect(noticeText(makeNotice(NOTICE_EVENT, []))).toBeNull();
  });

  it('returns null for an unknown notice kind', () => {
    expect(noticeText(makeNotice(99, [upgrade.id]))).toBeNull();
  });
});

describe('noticeOpacity', () => {
  it('is fully opaque before the fade window, and for future ages', () => {
    expect(noticeOpacity(0)).toBe(1);
    expect(noticeOpacity(NOTICE_LIFETIME_MS - NOTICE_FADE_MS)).toBe(1);
    expect(noticeOpacity(-50)).toBe(1);
  });

  it('fades linearly inside the fade window', () => {
    const mid = NOTICE_LIFETIME_MS - NOTICE_FADE_MS / 2;
    expect(noticeOpacity(mid)).toBeCloseTo(0.5, 5);
  });

  it('is gone at and after the lifetime', () => {
    expect(noticeOpacity(NOTICE_LIFETIME_MS)).toBe(0);
    expect(noticeOpacity(NOTICE_LIFETIME_MS + 1)).toBe(0);
  });

  it('hides on non-finite input instead of crashing', () => {
    expect(noticeOpacity(Number.NaN)).toBe(0);
  });
});
