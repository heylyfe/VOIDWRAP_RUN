// T-088: the death screen shows the cosmetic code the server derived from the run.
// Rendered with `renderToStaticMarkup` because vitest only collects `src/**/*.test.ts`
// (no `.tsx`), so the component is built with `createElement` instead of JSX.
import { createElement } from 'react';
import { renderToStaticMarkup } from 'react-dom/server';
import { describe, expect, it } from 'vitest';

import { RunSummary } from './RunSummary';

const noop = (): void => {};

function render(code?: string, achievements?: readonly string[]): string {
  return renderToStaticMarkup(
    createElement(RunSummary, {
      levelReached: 12,
      giftsCollected: 57,
      cause: 'void' as const,
      code,
      achievements,
      onRestart: noop,
    }),
  );
}

describe('RunSummary death code', () => {
  it('renders the code when the server sent one', () => {
    const html = render('VW-GAP7-HU7F');
    expect(html).toContain('data-testid="death-code"');
    expect(html).toContain('VW-GAP7-HU7F');
    expect(html).toContain('Death code');
  });

  it('omits the block entirely when there is no code', () => {
    const html = render(undefined);
    expect(html).not.toContain('data-testid="death-code"');
    expect(html).not.toContain('Death code');
  });

  it('omits the block for an empty string', () => {
    const html = render('');
    expect(html).not.toContain('data-testid="death-code"');
  });

  it('still renders the run stats alongside the code', () => {
    const html = render('VW-ABCD-2345');
    expect(html).toContain('RUN OVER');
    expect(html).toContain('12');
    expect(html).toContain('57');
    expect(html).toContain('VW-ABCD-2345');
  });

  it('renders a badge per unlocked achievement, in unlock order', () => {
    const html = render('VW-ABCD-2345', ['clean_level', 'first_trampoline']);
    expect(html).toContain('data-testid="achievement-badges"');
    expect(html).toContain('Untouched');
    expect(html).toContain('Springboard');
    expect(html.indexOf('Untouched')).toBeLessThan(html.indexOf('Springboard'));
  });

  it('drops ids the content file does not know', () => {
    const html = render('VW-ABCD-2345', ['clean_level', 'not_a_real_achievement']);
    expect(html).toContain('Untouched');
    expect(html).not.toContain('not_a_real_achievement');
  });

  it('omits the badge list when nothing was unlocked', () => {
    const html = render('VW-ABCD-2345', []);
    expect(html).not.toContain('data-testid="achievement-badges"');
  });
});
