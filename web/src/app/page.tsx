'use client';

import { useEffect, useState } from 'react';
import Link from 'next/link';
import type { JSX } from 'react';
import { HostPanel } from '../hud/HostPanel';
import { loadPlayerName, savePlayerName } from '../game/playerName';
import { JoinPanel } from '../hud/JoinPanel';

export default function Title(): JSX.Element {
  const [panel, setPanel] = useState<'none' | 'host' | 'join'>('none');
  const [name, setName] = useState('');

  useEffect(() => {
    // Read after mount (localStorage is browser-only); deferred so the page renders first.
    const t = window.setTimeout(() => setName(loadPlayerName()), 0);
    return () => window.clearTimeout(t);
  }, []);

  useEffect(() => {
    if (panel === 'none') {
      return;
    }
    const onKeyDown = (event: KeyboardEvent): void => {
      if (event.key === 'Escape') {
        setPanel('none');
      }
    };
    window.addEventListener('keydown', onKeyDown);
    return () => {
      window.removeEventListener('keydown', onKeyDown);
    };
  }, [panel]);

  return (
    <main className="flex min-h-screen flex-col items-center justify-center gap-6 px-4 py-8 text-center">
      <h1 className="font-mono text-5xl font-black tracking-[0.2em] text-[#ff3db8] sm:text-7xl">VOIDWRAP RUN</h1>
      <p className="font-mono text-lg text-[#38e8ff]">Collect every gift. Outrun what you voted for.</p>

      <label className="flex items-center gap-2 font-mono text-sm text-white/70">
        Name
        <input
          type="text"
          data-testid="player-name"
          value={name}
          maxLength={16}
          onChange={(e) => {
            setName(e.target.value);
            savePlayerName(e.target.value);
          }}
          className="w-40 rounded border border-white/30 bg-black/40 px-2 py-1 text-white focus:outline-none focus-visible:ring-2 focus-visible:ring-[#38e8ff]/60"
        />
      </label>

      <div className="flex w-full max-w-md flex-col items-stretch gap-3 sm:w-auto sm:max-w-none sm:flex-row sm:items-center sm:justify-center sm:gap-4">
        <Link
          href="/play"
          data-testid="play"
          className="rounded border-2 border-[#ff3db8] px-8 py-3 text-center font-mono text-xl font-bold uppercase tracking-widest text-white transition hover:bg-[#ff3db8] hover:text-black focus:outline-none focus-visible:ring-2 focus-visible:ring-[#ff3db8]/60"
        >
          Play
        </Link>
        <button
          type="button"
          data-testid="play-friends"
          onClick={() => setPanel(panel === 'host' ? 'none' : 'host')}
          className="rounded border-2 border-[#38e8ff] px-8 py-3 font-mono text-xl font-bold uppercase tracking-widest text-[#38e8ff] transition hover:bg-[#38e8ff] hover:text-black focus:outline-none focus-visible:ring-2 focus-visible:ring-[#38e8ff]/60"
        >
          Play with friends
        </button>
        <button
          type="button"
          data-testid="join-friend"
          onClick={() => setPanel(panel === 'join' ? 'none' : 'join')}
          className="rounded border-2 border-white/25 px-8 py-3 font-mono text-xl font-bold uppercase tracking-widest text-white/85 transition hover:border-white/60 hover:text-white focus:outline-none focus-visible:ring-2 focus-visible:ring-white/40"
        >
          Join a friend
        </button>
      </div>

      <p className="max-w-md font-mono text-xs text-white/50">
        One computer hosts. Everyone else joins from their browser.
      </p>

      {panel === 'host' ? <HostPanel onClose={() => setPanel('none')} /> : null}
      {panel === 'join' ? <JoinPanel onClose={() => setPanel('none')} /> : null}
    </main>
  );
}
