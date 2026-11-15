import { describe, expect, it } from 'vitest';
import { createElement } from 'react';
import { renderToStaticMarkup } from 'react-dom/server';
import { GameHud } from './GameHud';

const base = { level: 3, giftsRemaining: 4, giftsTotal: 10 };

function render(radar?: {
  giftArrow: boolean;
  enemyRadar: boolean;
  hazardRadar: boolean;
  pingReady: boolean;
}): string {
  return renderToStaticMarkup(createElement(GameHud, { ...base, radar }));
}

describe('GameHud radar chips', () => {
  it('renders no radar row when the run owns no modules', () => {
    expect(render()).not.toContain('radar-chips');
  });

  it('renders one chip per owned module', () => {
    const html = render({
      giftArrow: true,
      enemyRadar: true,
      hazardRadar: true,
      pingReady: true,
    });
    expect(html).toContain('radar-chips');
    expect(html).toContain('radar-chip-compass');
    expect(html).toContain('radar-chip-ping');
    expect(html).toContain('radar-chip-ground');
  });

  it('omits the chips for modules the run does not own', () => {
    const html = render({
      giftArrow: true,
      enemyRadar: false,
      hazardRadar: false,
      pingReady: true,
    });
    expect(html).toContain('radar-chip-compass');
    expect(html).not.toContain('radar-chip-ping');
    expect(html).not.toContain('radar-chip-ground');
  });

  it('dims the ping chip while the radar is on cooldown', () => {
    const ready = render({
      giftArrow: false,
      enemyRadar: true,
      hazardRadar: false,
      pingReady: true,
    });
    const cooling = render({
      giftArrow: false,
      enemyRadar: true,
      hazardRadar: false,
      pingReady: false,
    });
    expect(ready).toContain('text-white/80');
    expect(ready).not.toContain('text-white/40');
    expect(cooling).toContain('text-white/40');
  });

  it('keeps the existing counters intact', () => {
    const html = render();
    expect(html).toContain('data-testid="level-number"');
    expect(html).toContain('data-testid="gift-counter"');
    expect(html).toContain('6 / 10');
  });
});
