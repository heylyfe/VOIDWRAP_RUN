import { describe, expect, it } from "vitest";
import { ShopPanel, renderOffer, type ShopOffer } from "./shopPanel";

function offers(): ShopOffer[] {
  return [
    { id: "kept_flame", name: "Kept Flame", effect: "+1 life; you respawn at the level start", cost: 60, stock: 1 },
    { id: "wide_palm", name: "Wide Palm", effect: "+15% gift pickup reach", cost: 16, stock: 3 },
  ];
}

function find(panel: ShopPanel, id: string): ShopOffer {
  const offer = panel.getState().offers.find((entry) => entry.id === id);
  if (!offer) {
    throw new Error(`offer ${id} missing`);
  }
  return offer;
}

describe("the between-level shop panel", () => {
  it("renders every offer with its name, effect, cost, stock and the currency", () => {
    const panel = new ShopPanel({
      offers: offers(),
      currency: 100,
      onBuy: async () => ({ ok: true, currency: 100, stock: 1 }),
    });
    const html = panel.render();
    expect(html).toContain("Kept Flame");
    expect(html).toContain("Wide Palm");
    expect(html).toContain("+1 life");
    expect(html).toContain("data-currency=\"100\"");
    expect(html).toContain('data-id="kept_flame"');
    expect(html).toContain('data-buy="wide_palm"');
    expect(html).not.toContain('data-sold-out="true"');
  });

  it("buys successfully: currency drops, stock decrements", async () => {
    let calls = 0;
    const panel = new ShopPanel({
      offers: offers(),
      currency: 100,
      onBuy: async (itemId) => {
        calls += 1;
        expect(itemId).toBe("kept_flame");
        return { ok: true, currency: 40, stock: 0 };
      },
    });
    expect(await panel.buy("kept_flame")).toBe(true);
    expect(calls).toBe(1);
    const state = panel.getState();
    expect(state.currency).toBe(40);
    expect(find(panel, "kept_flame").stock).toBe(0);
    expect(find(panel, "wide_palm").stock).toBe(3);
    expect(state.error).toBeNull();
    expect(panel.render()).toContain('data-sold-out="true"');
  });

  it("buy fails: the server's error is shown and no state changes", async () => {
    const panel = new ShopPanel({
      offers: offers(),
      currency: 100,
      onBuy: async () => ({ ok: false, error: "Not enough currency" }),
    });
    expect(await panel.buy("kept_flame")).toBe(false);
    const state = panel.getState();
    expect(state.error).toBe("Not enough currency");
    expect(state.currency).toBe(100);
    expect(find(panel, "kept_flame").stock).toBe(1);
    expect(panel.render()).toContain("Not enough currency");
    expect(panel.render()).toContain('role="alert"');
  });

  it("sold-out state renders a disabled buy button and blocks local purchase attempts", async () => {
    let calls = 0;
    const panel = new ShopPanel({
      offers: [{ id: "gone", name: "Sold Thing", effect: "It is gone.", cost: 5, stock: 0 }],
      currency: 100,
      onBuy: async () => {
        calls += 1;
        return { ok: true, currency: 95, stock: 0 };
      },
    });
    const html = panel.render();
    expect(html).toContain("Sold out");
    expect(html).toContain("disabled");
    expect(html).toContain('data-sold-out="true"');
    expect(await panel.buy("gone")).toBe(false);
    expect(calls).toBe(0);
  });

  it("renderOffer marks a stocked offer affordable-looking and enabled", () => {
    const html = renderOffer({ id: "x", name: "Thing", effect: "Stuff.", cost: 10, stock: 2 }, 50);
    expect(html).toContain("Thing");
    expect(html).not.toContain("disabled");
    expect(html).toContain('data-stock="2"');
  });

  it("a clearing attempt after a failure resets the error before the next verdict", async () => {
    let verdict: "fail" | "ok" = "fail";
    const panel = new ShopPanel({
      offers: offers(),
      currency: 100,
      onBuy: async () =>
        verdict === "fail"
          ? { ok: false, error: "Sold out" }
          : { ok: true, currency: 84, stock: 2 },
    });
    expect(await panel.buy("wide_palm")).toBe(false);
    expect(panel.getState().error).toBe("Sold out");
    verdict = "ok";
    expect(await panel.buy("wide_palm")).toBe(true);
    expect(panel.getState().error).toBeNull();
    expect(panel.getState().currency).toBe(84);
    expect(find(panel, "wide_palm").stock).toBe(2);
  });
});
