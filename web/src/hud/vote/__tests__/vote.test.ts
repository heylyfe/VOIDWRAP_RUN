// The repo renders components with `renderToStaticMarkup` (see hud/MatchList.test.tsx) and has no
// DOM testing library, so interaction is asserted through the pure reducer and the rendered markup
// rather than through simulated clicks. Vitest only collects `src/**/*.test.ts`, so this file uses
// `createElement` instead of JSX.
import { createElement } from 'react';
import type { ReactElement } from 'react';
import { renderToStaticMarkup } from 'react-dom/server';
import { describe, expect, it } from 'vitest';
import { initialVoteState, isVotingOpen, secondsRemaining, voteReducer } from '../model';
import type { VoteEvent, VoteOption, VoteViewState } from '../model';
import { parseVoteMessage } from '../protocolAdapter';
import { VoteWidget } from '../VoteWidget';
import { RunVoteOverlay } from '../../RunVoteOverlay';

// Test fixtures only — real option ids/names come from the server (content.json).
const inRunOptions: VoteOption[] = [
  { id: 'enemy_riftspawn', name: 'Riftspawn', kind: 'enemy' },
  { id: 'curse_glassbone', name: 'Glassbone', kind: 'curse' },
  { id: 'enemy_hollow_warden', name: 'Hollow Warden', kind: 'enemy' },
];

const reduceEvents = (events: VoteEvent[]): VoteViewState =>
  events.reduce(voteReducer, initialVoteState);
const openEvent = (options: VoteOption[] = inRunOptions): VoteEvent => ({
  type: 'opened',
  options,
  durationMs: 20_000,
});
const noop = (): void => undefined;
const html = (node: ReactElement): string => renderToStaticMarkup(node);
const widget = (state: VoteViewState): string =>
  html(createElement(VoteWidget, { state, onVote: noop, variant: 'pedestal' }));

describe('vote reducer', () => {
  it('opens three options with zeroed counts and a 20 s window', () => {
    const state = reduceEvents([openEvent()]);
    expect(state.phase).toBe('open');
    expect(state.options).toHaveLength(3);
    expect(state.counts).toEqual([0, 0, 0]);
    expect(secondsRemaining(state.remainingMs)).toBe(20);
    expect(isVotingOpen(state)).toBe(true);
  });

  it('casts a vote and lets the player change it until the window closes', () => {
    const state = reduceEvents([openEvent(), { type: 'cast', index: 0 }, { type: 'cast', index: 2 }]);
    expect(state.yourVote).toBe(2);
    expect(state.error).toBeNull();
  });

  it('counts down to zero and rejects further votes', () => {
    const state = reduceEvents([openEvent(), { type: 'tick', deltaMs: 20_000 }]);
    expect(state.remainingMs).toBe(0);
    expect(isVotingOpen(state)).toBe(false);
    const after = voteReducer(state, { type: 'cast', index: 0 });
    expect(after.yourVote).toBeNull();
    expect(after.error).toMatch(/closed/i);
  });

  it('accepts server progress (counts, remainingMs, cleared yourVote)', () => {
    const state = reduceEvents([
      openEvent(),
      { type: 'cast', index: 1 },
      { type: 'progress', counts: [2, 3, 1], remainingMs: 9_500, yourVote: null },
    ]);
    expect(state.counts).toEqual([2, 3, 1]);
    expect(state.remainingMs).toBe(9_500);
    expect(state.yourVote).toBeNull();
  });

  it('surfaces an error when counts length does not match options', () => {
    const state = reduceEvents([openEvent(), { type: 'progress', counts: [1, 2] }]);
    expect(state.error).toMatch(/wrong number of counts/i);
  });

  it('reveals the winner on result', () => {
    const state = reduceEvents([
      openEvent(),
      { type: 'result', winnerIndex: 2, counts: [1, 2, 7] },
    ]);
    expect(state.phase).toBe('revealed');
    expect(state.winnerIndex).toBe(2);
    expect(state.remainingMs).toBe(0);
  });

  it('reset returns to idle', () => {
    const state = reduceEvents([openEvent(), { type: 'reset' }]);
    expect(state.phase).toBe('idle');
    expect(state.options).toEqual([]);
  });
});

describe('parseVoteMessage (mocked protocol messages)', () => {
  it('maps numeric card kinds on VoteOpen', () => {
    const event = parseVoteMessage({
      type: 'VoteOpen',
      options: [
        { id: 'a', name: 'Alpha', kind: 0 },
        { id: 'b', name: 'Beta', kind: 2 },
        { id: 'c', name: 'Gamma', kind: 2 },
      ],
      durationMs: 20_000,
    });
    expect(event).toEqual({
      type: 'opened',
      options: [
        { id: 'a', name: 'Alpha', kind: 'enemy' },
        { id: 'b', name: 'Beta', kind: 'curse' },
        { id: 'c', name: 'Gamma', kind: 'curse' },
      ],
      durationMs: 20_000,
    });
  });

  it('rejects an empty VoteOpen with an error event', () => {
    const event = parseVoteMessage({ type: 'VoteOpen', options: [] });
    expect(event).toEqual({ type: 'failed', message: 'Vote opened with no options.' });
  });

  it('returns null for non-vote messages', () => {
    expect(parseVoteMessage({ type: 'Snapshot' })).toBeNull();
    expect(parseVoteMessage(null)).toBeNull();
    expect(parseVoteMessage('VoteOpen')).toBeNull();
  });

  it('maps VoteState yourVote -1 to a cleared selection', () => {
    const event = parseVoteMessage({
      type: 'VoteState',
      counts: [1, 0, 2],
      remainingMs: 8_000,
      yourVote: -1,
    });
    expect(event).toEqual({
      type: 'progress',
      counts: [1, 0, 2],
      remainingMs: 8_000,
      yourVote: null,
    });
  });

  it('maps VoteResult winner index', () => {
    const event = parseVoteMessage({ type: 'VoteResult', winner: 1, counts: [0, 5, 3] });
    expect(event).toEqual({ type: 'result', winnerIndex: 1, counts: [0, 5, 3] });
  });
});

describe('VoteWidget', () => {
  it('renders three options with live counts', () => {
    const state = reduceEvents([
      openEvent(),
      { type: 'progress', counts: [4, 1, 9], remainingMs: 12_000 },
    ]);
    const out = widget(state);
    expect(out).toContain('Riftspawn');
    expect(out).toContain('Glassbone');
    expect(out).toContain('Hollow Warden');
    expect(out).toContain('>4<');
    expect(out).toContain('>1<');
    expect(out).toContain('>9<');
    expect(out.match(/ENEMY/g)).toHaveLength(2);
    expect(out.match(/CURSE/g)).toHaveLength(1);
    expect(out).toContain('Vote closes in 12 seconds');
    expect(out).not.toContain('new-permanent-banner');
  });

  it('disables voting and reads zero when the countdown reaches zero', () => {
    const state = reduceEvents([openEvent(), { type: 'tick', deltaMs: 20_000 }]);
    const out = widget(state);
    expect(out).toContain('disabled');
    expect(out).toContain('Vote closes in 0 seconds');
    expect(out).toMatch(/waiting for the result/i);
  });

  it('shows the results reveal and the NEW PERMANENT banner naming the winner', () => {
    const state = reduceEvents([
      openEvent(),
      { type: 'result', winnerIndex: 2, counts: [2, 3, 7] },
    ]);
    const out = widget(state);
    expect(out).toContain('VOTE CLOSED');
    expect(out).toContain('Hollow Warden');
    expect(out).toContain('new-permanent-banner');
    expect(out).toContain('NEW PERMANENT');
    expect(out).toContain('HOLLOW WARDEN');
  });

  it('renders the empty state when no vote is running', () => {
    const out = widget(initialVoteState);
    expect(out).toContain('No vote is running.');
    expect(out).not.toContain('Riftspawn');
  });

  it('renders the error state as an alert', () => {
    const state = reduceEvents([{ type: 'failed', message: 'Vote opened with no options.' }]);
    const out = widget(state);
    expect(out).toContain('role="alert"');
    expect(out).toContain('Vote opened with no options.');
  });

  it('marks the current choice and lets the player change it', () => {
    const first = reduceEvents([openEvent(), { type: 'cast', index: 0 }]);
    const firstOut = widget(first);
    expect(firstOut).toContain('aria-pressed="true"');
    expect(firstOut).toContain('YOUR VOTE');

    const changed = reduceEvents([
      openEvent(),
      { type: 'cast', index: 0 },
      { type: 'cast', index: 2 },
    ]);
    expect(changed.yourVote).toBe(2);
    expect(widget(changed)).toContain('aria-pressed="true"');

    // The "Your vote:" recap only appears on the results reveal.
    const revealed = reduceEvents([
      openEvent(),
      { type: 'cast', index: 2 },
      { type: 'result', winnerIndex: 2, counts: [1, 2, 6] },
    ]);
    expect(widget(revealed)).toContain('Your vote: Hollow Warden');
  });
});

// RunVoteOverlay is a thin `useVote` container: it ingests server messages in an effect, which
// does not run under renderToStaticMarkup. Its message-driven states are covered above through
// parseVoteMessage + voteReducer + VoteWidget; here we pin the idle render.
describe('RunVoteOverlay', () => {
  it('renders nothing while no vote is running', () => {
    expect(html(createElement(RunVoteOverlay, { messages: [] }))).toBe('');
  });
});

// T-149: the adapter was written against assumed field names while T-078 was in flight.
// These pin it to the shapes `protocol/generated.ts` actually produces, so wiring the
// overlay up cannot silently fall back to the local 20 s default.
describe('parseVoteMessage (real protocol shapes)', () => {
  it('reads the countdown from VoteOpen.countdownS', () => {
    const event = parseVoteMessage({
      type: 'VoteOpen',
      level: 4,
      options: [
        { index: 0, id: 'a', name: 'Alpha', kind: 0 },
        { index: 1, id: 'b', name: 'Beta', kind: 2 },
        { index: 2, id: 'c', name: 'Gamma', kind: 0 },
      ],
      countdownS: 20,
    });
    expect(event).toEqual({
      type: 'opened',
      options: [
        { id: 'a', name: 'Alpha', kind: 'enemy' },
        { id: 'b', name: 'Beta', kind: 'curse' },
        { id: 'c', name: 'Gamma', kind: 'enemy' },
      ],
      durationMs: 20_000,
    });
  });

  it('honours a countdown that is not the local default', () => {
    const event = parseVoteMessage({
      type: 'VoteOpen',
      level: 1,
      options: [{ index: 0, id: 'a', name: 'Alpha', kind: 0 }],
      countdownS: 7,
    });
    expect(event).toMatchObject({ type: 'opened', durationMs: 7_000 });
  });

  it('reads the remaining time from VoteState.secondsLeft', () => {
    const event = parseVoteMessage({
      type: 'VoteState',
      counts: [2, 1, 0],
      voted: [7, 9],
      secondsLeft: 13,
    });
    expect(event).toEqual({ type: 'progress', counts: [2, 1, 0], remainingMs: 13_000, yourVote: undefined });
  });

  it('reads the winner from VoteResult.winner', () => {
    const event = parseVoteMessage({
      type: 'VoteResult',
      winner: 1,
      id: 'b',
      name: 'Beta',
      kind: 2,
      permanent: false,
    });
    expect(event).toEqual({ type: 'result', winnerIndex: 1, counts: undefined });
  });
});

// T-149: end-to-end through the chain the overlay actually runs -- a real VoteOpen frame
// from the server, parsed by the adapter, folded by the reducer, rendered by the widget.
// This is the proof that mounting the overlay puts pedestals on screen; a browser would
// prove the same thing more slowly.
describe('a real VoteOpen reaches the screen', () => {
  it('renders every pedestal the server offered', () => {
    const frame = {
      type: 'VoteOpen',
      level: 3,
      options: [
        { index: 0, id: 'pallbearer', name: 'The Pallbearer', kind: 0 },
        { index: 1, id: 'leadened_soles', name: 'Leadened Soles', kind: 2 },
        { index: 2, id: 'snapshrike', name: 'Snapshrike', kind: 0 },
      ],
      countdownS: 20,
    };
    const event = parseVoteMessage(frame);
    expect(event).not.toBeNull();
    const state = reduceEvents([event as VoteEvent]);
    expect(isVotingOpen(state)).toBe(true);
    expect(secondsRemaining(state.remainingMs)).toBe(20);
    const markup = widget(state);
    expect(markup).toContain('The Pallbearer');
    expect(markup).toContain('Leadened Soles');
    expect(markup).toContain('Snapshrike');
  });
});
