// The repo renders components with `renderToStaticMarkup` (see hud/MatchList.test.tsx and
// hud/vote/__tests__/vote.test.ts) and has no DOM testing library, so interaction is asserted
// through the pure reducer and the rendered markup rather than through simulated clicks.
// Vitest only collects `src/**/*.test.ts`, so this file uses `createElement` instead of JSX.
//
// The node environment has no committed React root, so dispatching from a captured hook
// controller is a silent no-op. Instead the hook is exercised end-to-end by a probe component
// that calls `ingestMessage` during its own render — React's supported "adjusting state during
// render" pattern for the same component — which folds the messages through the real reducer
// synchronously before the probe renders.
import { createElement, useRef } from 'react';
import type { ReactElement } from 'react';
import { renderToStaticMarkup } from 'react-dom/server';
import { describe, expect, it } from 'vitest';
import { initialPollState, isPollOpen, pollReducer, secondsRemaining } from '../model';
import type { PollEvent, PollOption, PollViewState } from '../model';
import { parsePollMessage } from '../pollAdapter';
import { PollWidget } from '../PollWidget';
import { usePoll } from '../usePoll';
import type { PollController } from '../usePoll';
import { PollOverlay } from '../../PollOverlay';

// Test fixtures only — real option ids/names come from the server (content.json).
const themeOptions: PollOption[] = [
  { id: 'theme:ruin', name: 'Ruin', kind: 'theme' },
  { id: 'theme:neon', name: 'Neon Wilds', kind: 'theme' },
  { id: 'theme:frost', name: 'Frostveil', kind: 'theme' },
];

const difficultyOptions: PollOption[] = [
  { id: 'standard', name: 'Standard', kind: 'difficulty' },
  { id: 'extreme', name: 'Extreme', kind: 'difficulty' },
  { id: 'chaos', name: 'Chaos', kind: 'difficulty' },
  { id: 'random', name: 'Random', kind: 'difficulty' },
];

const reduceEvents = (events: PollEvent[]): PollViewState =>
  events.reduce(pollReducer, initialPollState);
const openEvent = (options: PollOption[] = themeOptions): PollEvent => ({
  type: 'opened',
  options,
  durationMs: 20_000,
});
const noop = (): void => undefined;
const html = (node: ReactElement): string => renderToStaticMarkup(node);
const widget = (state: PollViewState): string =>
  html(createElement(PollWidget, { state, onVote: noop }));

describe('poll reducer', () => {
  it('opens the theme question with zeroed counts and a 20 s window', () => {
    const state = reduceEvents([openEvent()]);
    expect(state.phase).toBe('open');
    expect(state.question).toBe('theme');
    expect(state.options).toHaveLength(3);
    expect(state.counts).toEqual([0, 0, 0]);
    expect(state.voted).toEqual([]);
    expect(secondsRemaining(state.remainingMs)).toBe(20);
    expect(isPollOpen(state)).toBe(true);
  });

  it('casts a vote and lets the player change it until the window closes', () => {
    const state = reduceEvents([openEvent(), { type: 'cast', index: 0 }, { type: 'cast', index: 2 }]);
    expect(state.yourVote).toBe(2);
    expect(state.error).toBeNull();
  });

  it('counts down to zero and rejects further votes', () => {
    const state = reduceEvents([openEvent(), { type: 'tick', deltaMs: 20_000 }]);
    expect(state.remainingMs).toBe(0);
    expect(isPollOpen(state)).toBe(false);
    const after = pollReducer(state, { type: 'cast', index: 0 });
    expect(after.yourVote).toBeNull();
    expect(after.error).toMatch(/closed/i);
  });

  it('accepts server progress (counts, secondsLeft, voted)', () => {
    const state = reduceEvents([
      openEvent(),
      { type: 'progress', counts: [2, 3, 1], remainingMs: 9_500, voted: [4, 9] },
    ]);
    expect(state.counts).toEqual([2, 3, 1]);
    expect(state.remainingMs).toBe(9_500);
    expect(state.voted).toEqual([4, 9]);
  });

  it('surfaces an error when counts length does not match options', () => {
    const state = reduceEvents([openEvent(), { type: 'progress', counts: [1, 2] }]);
    expect(state.error).toMatch(/wrong number of counts/i);
  });

  it('reveals the winner on result and keeps the question', () => {
    const state = reduceEvents([
      openEvent(),
      { type: 'result', winnerIndex: 2, counts: [1, 2, 7], question: 'theme' },
    ]);
    expect(state.phase).toBe('revealed');
    expect(state.winnerIndex).toBe(2);
    expect(state.question).toBe('theme');
    expect(state.remainingMs).toBe(0);
  });

  it('reset returns to idle', () => {
    const state = reduceEvents([openEvent(), { type: 'reset' }]);
    expect(state.phase).toBe('idle');
    expect(state.options).toEqual([]);
    expect(state.question).toBeNull();
  });
});

describe('parsePollMessage (adapter accepts both spellings)', () => {
  it('reads the countdown from camelCase PollOpen.countdownS', () => {
    const event = parsePollMessage({
      type: 'PollOpen',
      options: [
        { id: 'a', name: 'Ruin', kind: 0 },
        { id: 'b', name: 'Neon Wilds', kind: 0 },
      ],
      countdownS: 20,
    });
    expect(event).toEqual({
      type: 'opened',
      options: [
        { id: 'a', name: 'Ruin', kind: 'theme' },
        { id: 'b', name: 'Neon Wilds', kind: 'theme' },
      ],
      durationMs: 20_000,
    });
  });

  it('reads the countdown from snake_case PollOpen.countdown_s', () => {
    const event = parsePollMessage({
      type: 'PollOpen',
      options: [{ id: 'standard', name: 'Standard', kind: 1 }],
      countdown_s: 7,
    });
    expect(event).toMatchObject({ type: 'opened', durationMs: 7_000 });
  });

  it('honours a non-default countdown in both spellings', () => {
    const wire = {
      type: 'PollOpen',
      options: [{ id: 'standard', name: 'Standard', kind: 1 }],
    };
    expect(parsePollMessage({ ...wire, countdownS: 3 })).toMatchObject({ durationMs: 3_000 });
    expect(parsePollMessage({ ...wire, countdown_s: 3 })).toMatchObject({ durationMs: 3_000 });
  });

  it('reads the remaining time from camelCase PollState.secondsLeft', () => {
    const event = parsePollMessage({
      type: 'PollState',
      counts: [2, 1, 0],
      voted: [7, 9],
      secondsLeft: 13,
    });
    expect(event).toEqual({ type: 'progress', counts: [2, 1, 0], remainingMs: 13_000, yourVote: undefined, voted: [7, 9] });
  });

  it('reads the remaining time from snake_case PollState.seconds_left', () => {
    const event = parsePollMessage({
      type: 'PollState',
      counts: [2, 1, 0],
      voted: [7],
      seconds_left: 6,
    });
    expect(event).toEqual({ type: 'progress', counts: [2, 1, 0], remainingMs: 6_000, yourVote: undefined, voted: [7] });
  });

  it('maps numeric poll kinds on PollOpen options (POLL_THEME=0, POLL_DIFFICULTY=1)', () => {
    const event = parsePollMessage({
      type: 'PollOpen',
      options: [
        { id: 'standard', name: 'Standard', kind: 1 },
        { id: 'chaos', name: 'Chaos', kind: 1 },
      ],
      countdownS: 20,
    });
    expect(event).toMatchObject({
      type: 'opened',
      options: [
        { id: 'standard', kind: 'difficulty' },
        { id: 'chaos', kind: 'difficulty' },
      ],
    });
  });

  it('rejects an empty PollOpen with an error event', () => {
    const event = parsePollMessage({ type: 'PollOpen', options: [] });
    expect(event).toEqual({ type: 'failed', message: 'Poll opened with no options.' });
  });

  it('returns null for non-poll messages', () => {
    expect(parsePollMessage({ type: 'Snapshot' })).toBeNull();
    expect(parsePollMessage(null)).toBeNull();
    expect(parsePollMessage('PollOpen')).toBeNull();
  });

  it('reads the winner and the question kind from PollResult', () => {
    const event = parsePollMessage({
      type: 'PollResult',
      winner: 1,
      id: 'extreme',
      name: 'Extreme',
      kind: 1,
    });
    expect(event).toEqual({ type: 'result', winnerIndex: 1, counts: undefined, question: 'difficulty' });
  });

  it('maps POLL_THEME PollResult kind to theme', () => {
    const event = parsePollMessage({ type: 'PollResult', winner: 0, id: 'theme:ruin', name: 'Ruin', kind: 0 });
    expect(event).toMatchObject({ type: 'result', winnerIndex: 0, question: 'theme' });
  });
});

describe('usePoll (real hook, ingested during the probe render)', () => {
  /** Renders the probe, ingesting `messages` via the real hook before the returned markup. */
  const probe = (messages: unknown[]): PollController => {
    let captured: PollController | null = null;
    const Probe = (): ReactElement => {
      const controller = usePoll();
      const ingested = useRef(false);
      if (!ingested.current) {
        ingested.current = true;
        for (const message of messages) controller.ingestMessage(message);
      }
      captured = controller;
      return createElement('div');
    };
    renderToStaticMarkup(createElement(Probe));
    if (captured === null) throw new Error('usePoll probe never rendered');
    return captured;
  };

  it('starts idle with the expected controller shape', () => {
    const controller = probe([]);
    expect(controller.state.phase).toBe('idle');
    expect(typeof controller.sendPoll).toBe('function');
    expect(typeof controller.ingestMessage).toBe('function');
    expect(typeof controller.reset).toBe('function');
  });

  it('ingests PollOpen and opens with the question derived from the options kind', () => {
    const controller = probe([
      {
        type: 'PollOpen',
        options: [
          { id: 'theme:ruin', name: 'Ruin', kind: 0 },
          { id: 'theme:neon', name: 'Neon Wilds', kind: 0 },
        ],
        countdownS: 20,
      },
    ]);
    expect(controller.state.phase).toBe('open');
    expect(controller.state.question).toBe('theme');
    expect(controller.state.options.map((o) => o.name)).toEqual(['Ruin', 'Neon Wilds']);
    expect(controller.state.counts).toEqual([0, 0]);
    expect(controller.state.remainingMs).toBe(20_000);
  });

  it('ingests PollState and moves the counts and the ballot list', () => {
    const controller = probe([
      {
        type: 'PollOpen',
        options: [
          { id: 'standard', name: 'Standard', kind: 1 },
          { id: 'chaos', name: 'Chaos', kind: 1 },
        ],
        countdownS: 20,
      },
      { type: 'PollState', counts: [2, 1], voted: [7], secondsLeft: 13 },
    ]);
    expect(controller.state.phase).toBe('open');
    expect(controller.state.counts).toEqual([2, 1]);
    expect(controller.state.voted).toEqual([7]);
    expect(controller.state.remainingMs).toBe(13_000);
  });

  it('ingests PollResult and reveals the winner', () => {
    const controller = probe([
      {
        type: 'PollOpen',
        options: [
          { id: 'theme:ruin', name: 'Ruin', kind: 0 },
          { id: 'theme:neon', name: 'Neon Wilds', kind: 0 },
        ],
        countdownS: 20,
      },
      { type: 'PollState', counts: [1, 4], voted: [7, 8, 9], secondsLeft: 3 },
      { type: 'PollResult', winner: 1, id: 'theme:neon', name: 'Neon Wilds', kind: 0 },
    ]);
    expect(controller.state.phase).toBe('revealed');
    expect(controller.state.winnerIndex).toBe(1);
    expect(controller.state.question).toBe('theme');
    expect(controller.state.remainingMs).toBe(0);
  });

  it('ignores non-poll messages', () => {
    const controller = probe([{ type: 'Snapshot' }, { type: 'VoteOpen', options: [] }]);
    expect(controller.state.phase).toBe('idle');
  });
});

describe('PollWidget', () => {
  it('renders the theme question with its options and live counts', () => {
    const state = reduceEvents([
      openEvent(),
      { type: 'progress', counts: [4, 1, 9], remainingMs: 12_000, voted: [1, 2, 3, 4] },
    ]);
    const out = widget(state);
    expect(out).toContain('CHOOSE THE THEME');
    expect(out).toContain('Ruin');
    expect(out).toContain('Neon Wilds');
    expect(out).toContain('Frostveil');
    expect(out).toContain('>4<');
    expect(out).toContain('>1<');
    expect(out).toContain('>9<');
    expect(out).toMatch(/ballots? cast/);
    expect(out).toContain('role="timer"');
    expect((out.match(/THEME/g) ?? []).length).toBeGreaterThanOrEqual(3);
  });

  it('renders the difficulty question when the options are difficulty kind', () => {
    const state = reduceEvents([openEvent(difficultyOptions)]);
    const out = widget(state);
    expect(out).toContain('CHOOSE THE DIFFICULTY');
    expect(out).toContain('Standard');
    expect(out).toContain('Extreme');
    expect(out).toContain('Chaos');
    expect(out).toContain('Random');
    expect((out.match(/DIFFICULTY/g) ?? []).length).toBeGreaterThanOrEqual(4);
  });

  it('disables voting and reads zero when the countdown reaches zero', () => {
    const state = reduceEvents([openEvent(), { type: 'tick', deltaMs: 20_000 }]);
    const out = widget(state);
    expect(out).toContain('disabled');
    expect(out).toMatch(/waiting for the result/i);
  });

  it('shows the result reveal naming the winner', () => {
    const state = reduceEvents([
      openEvent(),
      { type: 'result', winnerIndex: 2, counts: [2, 3, 7], question: 'theme' },
    ]);
    const out = widget(state);
    expect(out).toContain('POLL CLOSED');
    expect(out).toContain('Frostveil');
    expect(out).toContain('Frostveil 7');
  });

  it('renders the empty state when no poll is running', () => {
    const out = widget(initialPollState);
    expect(out).toContain('No poll is running.');
    expect(out).not.toContain('Ruin');
  });

  it('renders the error state as an alert', () => {
    const state = reduceEvents([{ type: 'failed', message: 'Poll opened with no options.' }]);
    const out = widget(state);
    expect(out).toContain('role="alert"');
    expect(out).toContain('Poll opened with no options.');
  });

  it('marks the current choice and lets the player change it', () => {
    const first = reduceEvents([openEvent(), { type: 'cast', index: 0 }]);
    const firstOut = widget(first);
    expect(firstOut).toContain('aria-pressed="true"');
    expect(firstOut).toContain('YOUR VOTE');

    const revealed = reduceEvents([
      openEvent(),
      { type: 'cast', index: 2 },
      { type: 'result', winnerIndex: 2, counts: [1, 2, 6] },
    ]);
    expect(revealed.yourVote).toBe(2);
    expect(widget(revealed)).toMatch(/Your answer:[^]*Frostveil/);
  });
});

// PollOverlay is a thin `usePoll` container: it ingests server messages in an effect, which
// does not run under renderToStaticMarkup. Its message-driven states are covered above through
// parsePollMessage + pollReducer + PollWidget; here we pin the idle render.
describe('PollOverlay', () => {
  it('renders nothing while no poll is open', () => {
    expect(html(createElement(PollOverlay, { messages: [] }))).toBe('');
  });
});

// End-to-end through the chain the overlay actually runs — a real PollOpen frame from the
// server, parsed by the adapter, folded by the reducer, rendered by the widget. This is the
// proof that mounting the overlay puts the poll on screen without waiting on the UI.
describe('a real PollOpen reaches the screen', () => {
  it('renders every option the server offered for the theme question', () => {
    const frame = {
      type: 'PollOpen',
      options: [
        { id: 'theme:ruin', name: 'Ruin', kind: 0 },
        { id: 'theme:neon', name: 'Neon Wilds', kind: 0 },
        { id: 'theme:frost', name: 'Frostveil', kind: 0 },
      ],
      countdownS: 20,
    };
    const event = parsePollMessage(frame);
    expect(event).not.toBeNull();
    const state = reduceEvents([event as PollEvent]);
    expect(isPollOpen(state)).toBe(true);
    expect(secondsRemaining(state.remainingMs)).toBe(20);
    const markup = widget(state);
    expect(markup).toContain('Ruin');
    expect(markup).toContain('Neon Wilds');
    expect(markup).toContain('Frostveil');
    expect(markup).toContain('CHOOSE THE THEME');
  });

  it('renders the difficulty question from snake_case frames', () => {
    const frame = {
      type: 'PollOpen',
      options: [
        { id: 'standard', name: 'Standard', kind: 1 },
        { id: 'chaos', name: 'Chaos', kind: 1 },
      ],
      countdown_s: 20,
    };
    const event = parsePollMessage(frame) as PollEvent;
    const state = reduceEvents([event]);
    expect(state.question).toBe('difficulty');
    const markup = widget(state);
    expect(markup).toContain('Standard');
    expect(markup).toContain('Chaos');
    expect(markup).toContain('CHOOSE THE DIFFICULTY');
  });
});
