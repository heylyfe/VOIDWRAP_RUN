import { describe, expect, it } from 'vitest';
import { SHOP } from '../content';
import { buyBlockedReason, resolveShopItems } from './shop';

const firstShop = SHOP[0];
if (!firstShop) {
  throw new Error('design/content.json must define at least one shop item');
}

describe('resolveShopItems', () => {
  it('resolves known ids and prefers the wire cost and stock', () => {
    const resolved = resolveShopItems([{ id: firstShop.id, cost: firstShop.cost + 5, stock: 3 }]);
    expect(resolved).toHaveLength(1);
    expect(resolved[0]?.id).toBe(firstShop.id);
    expect(resolved[0]?.name).toBe(firstShop.name);
    expect(resolved[0]?.effect).toBe(firstShop.effect);
    expect(resolved[0]?.cost).toBe(firstShop.cost + 5);
    expect(resolved[0]?.stock).toBe(3);
  });

  it('skips unknown ids', () => {
    expect(resolveShopItems([{ id: 'no-such-item', cost: 1, stock: 1 }])).toEqual([]);
  });
});

describe('buyBlockedReason', () => {
  it('returns Sold out when stock is zero or negative', () => {
    expect(buyBlockedReason({ cost: 1, stock: 0 }, 100)).toBe('Sold out');
    expect(buyBlockedReason({ cost: 1, stock: -1 }, 100)).toBe('Sold out');
  });

  it('reports the exact shortfall in gold', () => {
    expect(buyBlockedReason({ cost: 10, stock: 1 }, 4)).toBe('Need 6 more gold');
  });

  it('returns null when affordable and in stock', () => {
    expect(buyBlockedReason({ cost: 10, stock: 1 }, 10)).toBeNull();
    expect(buyBlockedReason({ cost: 10, stock: 2 }, 99)).toBeNull();
  });

  it('sold out wins over an empty purse', () => {
    expect(buyBlockedReason({ cost: 10, stock: 0 }, 0)).toBe('Sold out');
  });
});
