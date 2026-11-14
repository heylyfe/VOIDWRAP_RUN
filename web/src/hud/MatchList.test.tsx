import { describe, expect, it } from 'vitest';
import { renderToStaticMarkup } from 'react-dom/server';
import { MatchListView } from './MatchList';
import type { MatchEntry } from './matchmaker';
import { resolveJoinUrl } from './matchmaker';

const openLobby: MatchEntry = {
  name: 'Alpha Keep',
  address: '203.0.113.10:7879',
  players: 3,
  max_players: 8,
  level: 'Neon Ruins',
  phase: 'Lobby',
  joinable: true,
  version: '0.4.0-alpha.1',
};

const unreachableLobby: MatchEntry = {
  name: 'Beta Keep',
  address: '198.51.100.7:7879',
  players: 8,
  max_players: 8,
  level: 'Neon Ruins',
  phase: 'In match',
  joinable: false,
  version: '0.4.0-alpha.1',
};

const noop = (): void => undefined;

describe('MatchListView', () => {
  it('renders the mocked matchmaker entries', () => {
    const html = renderToStaticMarkup(
      <MatchListView state={{ status: 'ready', entries: [openLobby, unreachableLobby] }} onJoin={noop} />,
    );
    expect(html).toContain('Alpha Keep');
    expect(html).toContain('203.0.113.10:7879');
    expect(html).toContain('3/8');
    expect(html).toContain('Neon Ruins');
    expect(html).toContain('Lobby');
    expect(html).toContain('Beta Keep');
    expect(html).toContain('8/8');
    expect(html).toContain('In match');
  });

  it('greys out joinable:false entries with the internet-unreachable note and a disabled button', () => {
    const html = renderToStaticMarkup(
      <MatchListView state={{ status: 'ready', entries: [unreachableLobby] }} onJoin={noop} />,
    );
    expect(html).toContain('host is unreachable from the internet');
    expect(html).toContain('aria-disabled="true"');
    expect(html).toContain('opacity-50');
  });

  it('does not render the unreachable note for joinable entries', () => {
    const html = renderToStaticMarkup(
      <MatchListView state={{ status: 'ready', entries: [openLobby] }} onJoin={noop} />,
    );
    expect(html).not.toContain('host is unreachable from the internet');
  });

  it('shows the cold-start "Waking server…" state during a slow first fetch', () => {
    const html = renderToStaticMarkup(
      <MatchListView state={{ status: 'loading', waking: true }} onJoin={noop} />,
    );
    expect(html).toContain('Waking server…');
    expect(html).toContain('role="status"');
  });

  it('shows a contacting note before the wake threshold', () => {
    const html = renderToStaticMarkup(
      <MatchListView state={{ status: 'loading', waking: false }} onJoin={noop} />,
    );
    expect(html).toContain('Contacting matchmaker…');
  });

  it('renders the empty state when no lobbies are listed', () => {
    const html = renderToStaticMarkup(
      <MatchListView state={{ status: 'ready', entries: [] }} onJoin={noop} />,
    );
    expect(html).toContain('No open lobbies');
  });

  it('renders a role="alert" error state', () => {
    const html = renderToStaticMarkup(<MatchListView state={{ status: 'error' }} onJoin={noop} />);
    expect(html).toContain('role="alert"');
  });

  it('hands the listed address through the existing join path', () => {
    expect(resolveJoinUrl(openLobby.address)).toBe('http://203.0.113.10:7879/play/');
    expect(resolveJoinUrl(unreachableLobby.address)).toBe('http://198.51.100.7:7879/play/');
  });
});
