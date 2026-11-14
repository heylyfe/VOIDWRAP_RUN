// The between-level shop panel. Framework-free on purpose: all state and HTML rendering live
// here, so the panel is testable under plain vitest (no DOM, no network). The server owns the
// truth; every purchase round-trips through the injected `onBuy` callback, which is expected to
// send the existing `Buy` client message and resolve with the server's verdict.

export interface ShopOffer {
  /** Item id as offered by the server (content id or a contextual `upgrade::` / `purge::` id). */
  id: string;
  name: string;
  effect: string;
  cost: number;
  stock: number;
}

/** The server's verdict on a purchase, reduced to what the panel needs. */
export type BuyResult =
  | { ok: true; currency: number; stock: number }
  | { ok: false; error: string };

export interface ShopPanelOptions {
  offers: ReadonlyArray<ShopOffer>;
  currency: number;
  onBuy: (itemId: string) => Promise<BuyResult>;
}

export interface ShopPanelState {
  readonly offers: ReadonlyArray<ShopOffer>;
  readonly currency: number;
  readonly error: string | null;
  readonly busy: boolean;
}

interface MutableState {
  offers: ShopOffer[];
  currency: number;
  error: string | null;
  busy: boolean;
}

function escapeHtml(text: string): string {
  return text
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;")
    .replace(/'/g, "&#39;");
}

/** Render a single offer card. Exported so it can be unit-tested in isolation. */
export function renderOffer(offer: ShopOffer, currency: number, busy = false): string {
  const soldOut = offer.stock <= 0;
  const disabled = soldOut || busy;
  const badge = soldOut ? `<span class="shop-soldout" data-sold-out="true">Sold out</span>` : "";
  const label = soldOut ? "Sold out" : `Buy — ${offer.cost} gold`;
  const disabledAttr = disabled ? " disabled" : "";
  const affordability = currency >= offer.cost ? "affordable" : "unaffordable";
  return (
    `<article class="shop-offer ${affordability}" data-id="${escapeHtml(offer.id)}">` +
    `<h3 class="shop-offer-name">${escapeHtml(offer.name)}</h3>` +
    `<p class="shop-offer-effect">${escapeHtml(offer.effect)}</p>` +
    `<div class="shop-offer-footer">` +
    `<span class="shop-offer-stock" data-stock="${offer.stock}">${offer.stock} left</span>` +
    badge +
    `<button type="button" class="shop-buy" data-buy="${escapeHtml(offer.id)}"${disabledAttr}>${escapeHtml(label)}</button>` +
    `</div>` +
    `</article>`
  );
}

export class ShopPanel {
  private state: MutableState;
  private readonly onBuy: (itemId: string) => Promise<BuyResult>;

  constructor(options: ShopPanelOptions) {
    this.state = {
      offers: options.offers.map((offer) => ({ ...offer })),
      currency: options.currency,
      error: null,
      busy: false,
    };
    this.onBuy = options.onBuy;
  }

  /** A snapshot of the panel state (offers are copied). */
  getState(): ShopPanelState {
    return {
      offers: this.state.offers.map((offer) => ({ ...offer })),
      currency: this.state.currency,
      error: this.state.error,
      busy: this.state.busy,
    };
  }

  /** The currency shown in the header. */
  currency(): number {
    return this.state.currency;
  }

  render(): string {
    const cards = this.state.offers
      .map((offer) => renderOffer(offer, this.state.currency, this.state.busy))
      .join("");
    const error = this.state.error
      ? `<div class="shop-error" role="alert">${escapeHtml(this.state.error)}</div>`
      : "";
    return (
      `<section class="shop" aria-label="The shop">` +
      `<header class="shop-header">` +
      `<span class="shop-title">The Chandlery</span>` +
      `<span class="shop-currency" data-currency="${this.state.currency}">${this.state.currency} gold</span>` +
      `</header>` +
      error +
      `<div class="shop-offers">${cards}</div>` +
      `</section>`
    );
  }

  /**
   * Attempt a purchase. Returns true when the server accepted it. On rejection the panel shows
   * the server's error and every other piece of state is left untouched (server-authoritative:
   * we never apply a purchase locally before the server confirms it).
   */
  async buy(itemId: string): Promise<boolean> {
    const offer = this.state.offers.find((entry) => entry.id === itemId);
    if (!offer || offer.stock <= 0 || this.state.busy) {
      return false;
    }
    this.state = { ...this.state, busy: true, error: null };
    let result: BuyResult;
    try {
      result = await this.onBuy(itemId);
    } catch {
      result = { ok: false, error: "The shop did not answer." };
    }
    if (result.ok) {
      this.state = {
        ...this.state,
        busy: false,
        currency: result.currency,
        offers: this.state.offers.map((entry) =>
          entry.id === itemId ? { ...entry, stock: result.stock } : entry
        ),
      };
      return true;
    }
    this.state = { ...this.state, busy: false, error: result.error };
    return false;
  }
}
