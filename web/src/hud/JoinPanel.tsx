'use client';

// Shown from the title page. The player pastes the address the host saw in
// their hosting panel; we navigate the window to <address>/play/.
import { useMemo, useState } from 'react';
import type { FormEvent, JSX } from 'react';
import { parseJoinAddress } from './join';

const MAGENTA = '#ff3db8';
const CYAN = '#38e8ff';

export function JoinPanel({ onClose }: { onClose: () => void }): JSX.Element {
  const [value, setValue] = useState('');
  const [error, setError] = useState<string | null>(null);
  const parsed = useMemo(() => parseJoinAddress(value), [value]);

  const onSubmit = (event: FormEvent<HTMLFormElement>): void => {
    event.preventDefault();
    if (parsed === null) {
      setError('That does not look like an address. Try something like 192.168.1.23:7879');
      return;
    }
    window.location.assign(`${parsed}/play/`);
  };

  return (
    <div
      className="fixed inset-0 z-40 overflow-y-auto bg-black/60 font-mono backdrop-blur-sm"
      onClick={onClose}
    >
      <div
        role="dialog"
        aria-modal="true"
        aria-labelledby="join-panel-title"
        className="mx-auto my-6 flex w-full max-w-md flex-col gap-4 break-words rounded-xl border border-white/10 bg-[#120f1f] p-5 text-white"
        onClick={(event) => event.stopPropagation()}
      >
        <div className="flex items-center justify-between gap-4 border-b border-white/10 pb-3">
          <h2 id="join-panel-title" className="text-lg font-bold tracking-[0.25em] text-[#38e8ff]">
            JOIN A FRIEND
          </h2>
          <button
            type="button"
            onClick={onClose}
            className="rounded border border-white/15 px-3 py-1 text-[11px] tracking-widest text-white/70 transition hover:text-white focus:outline-none focus-visible:ring-2 focus-visible:ring-white/40"
          >
            CLOSE
          </button>
        </div>

        <p className="text-sm leading-snug text-white/80">
          Ask the player hosting the game for their address, then paste it here.
        </p>

        <form onSubmit={onSubmit} className="flex flex-col gap-3">
          <label
            htmlFor="join-address"
            className="text-[11px] font-bold tracking-[0.25em] text-white/70"
          >
            FRIEND ADDRESS
          </label>
          <input
            id="join-address"
            name="address"
            type="text"
            inputMode="url"
            autoCapitalize="none"
            autoCorrect="off"
            spellCheck={false}
            placeholder="192.168.1.23:7879"
            value={value}
            onChange={(event) => {
              setValue(event.target.value);
              setError(null);
            }}
            aria-invalid={error !== null}
            aria-describedby="join-address-help"
            className="rounded border border-white/15 bg-black/40 px-3 py-2 text-sm text-white placeholder-white/30 focus:outline-none focus-visible:ring-2 focus-visible:ring-[#38e8ff]/60"
          />
          {error !== null ? (
            <p id="join-address-help" role="alert" className="text-xs text-[#ff4d6d]">
              {error}
            </p>
          ) : parsed !== null ? (
            <p id="join-address-help" className="text-xs" style={{ color: CYAN }}>
              Opens {parsed}/play/
            </p>
          ) : (
            <p id="join-address-help" className="text-xs text-white/55">
              Works with or without http:// - a bare address uses the game port 7879.
            </p>
          )}
          <button
            type="submit"
            className="rounded-lg border-2 px-6 py-3 text-center text-sm font-bold tracking-[0.3em] transition hover:text-black focus:outline-none focus-visible:ring-2 focus-visible:ring-[#ff3db8]/60"
            style={{ borderColor: MAGENTA, color: MAGENTA }}
          >
            JOIN
          </button>
        </form>
      </div>
    </div>
  );
}
