// Lobby shop helpers: resolve the wire offer against design/content.json and
// work out whether a purchase is allowed. Pure and unit-tested.
import type { ShopItem } from '../protocol/generated';
import { SHOP } from '../content';

export interface ResolvedShopItem {
  id: string;
  name: string;
  flavor: string;
  effect: string;
  /** Cost from the wire (server authoritative), not from the manifest. */
  cost: number;
  /** Stock from the wire (server authoritative). */
  stock: number;
}

/**
 * Merge the wire offer (id, cost, stock) with the content manifest (name,
 * flavor, effect). Unknown ids are skipped; wire cost and stock always win.
 */
export function resolveShopItems(items: readonly ShopItem[]): ResolvedShopItem[] {
  const resolved: ResolvedShopItem[] = [];
  for (const item of items) {
    const details = SHOP.find((entry) => entry.id === item.id);
    if (!details) {
      continue;
    }
    resolved.push({
      id: item.id,
      name: details.name,
      flavor: details.flavor,
      effect: details.effect,
      cost: item.cost,
      stock: item.stock,
    });
  }
  return resolved;
}

/**
 * Why the Buy button is disabled, as plain text; null when the purchase is
 * allowed. Sold out wins over an empty purse.
 */
export function buyBlockedReason(item: { cost: number; stock: number }, currency: number): string | null {
  if (item.stock <= 0) {
    return 'Sold out';
  }
  if (currency < item.cost) {
    return `Need ${item.cost - currency} more gold`;
  }
  return null;
}
