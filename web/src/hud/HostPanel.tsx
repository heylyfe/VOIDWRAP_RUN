'use client';

// Shown from the title page. Reads the game program's own hosting endpoint;
// friends open the address shown here in their browser. When the page was
// opened from another computer (a friend's game), the endpoint answers 403
// and only the explanation is shown.
import { useCallback, useEffect, useRef, useState } from 'react';
import type { JSX } from 'react';
import Link from 'next/link';
import { httpBaseUrl } from '../game/net';
import { hostUrl, parseHostResponse } from './host';
import type { HostStatus } from './host';

type LoadState =
  | { kind: 'loading' }
  | { kind: 'unavailable' }
  | { kind: 'ready'; data: HostStatus };

// Fetches and parses the hosting status. Never touches component state and
// never throws, so callers can attach .then handlers and set state there.
async function fetchHostStatus(): Promise<LoadState> {
  try {
    const res = await fetch(`${httpBaseUrl()}/api/host`, { headers: { accept: 'application/json' } });
    if (!res.ok) {
      // 403 (this page was opened from another computer) or any failure:
      // hosting is only for the computer running the game.
      return { kind: 'unavailable' };
    }
    const raw: unknown = await res.json();
    const parsed = parseHostResponse(raw);
    return parsed === null ? { kind: 'unavailable' } : { kind: 'ready', data: parsed };
  } catch {
    return { kind: 'unavailable' };
  }
}

async function copyText(text: string): Promise<boolean> {
  try {
    if (
      typeof navigator !== 'undefined' &&
      navigator.clipboard &&
      typeof navigator.clipboard.writeText === 'function'
    ) {
      await navigator.clipboard.writeText(text);
      return true;
    }
  } catch {
    // Fall through to the legacy path (plain http origins may lack clipboard access).
  }
  try {
    const textarea = document.createElement('textarea');
    textarea.value = text;
    textarea.setAttribute('readonly', '');
    textarea.style.position = 'fixed';
    textarea.style.top = '0';
    textarea.style.left = '0';
    textarea.style.opacity = '0';
    document.body.appendChild(textarea);
    textarea.select();
    const ok = document.execCommand('copy');
    document.body.removeChild(textarea);
    return ok;
  } catch {
    return false;
  }
}

function AddressRow(props: {
  address: string;
  copied: boolean;
  failed: boolean;
  onCopy: (address: string) => void;
}): JSX.Element {
  const { address, copied, failed, onCopy } = props;
  return (
    <li className="flex min-w-0 flex-col gap-1 rounded-lg border border-white/10 bg-black/40 px-3 py-2">
      <span className="flex min-w-0 items-center justify-between gap-2">
        <code className="min-w-0 break-all text-sm text-[#38e8ff]">{address}</code>
        <button
          type="button"
          onClick={() => onCopy(address)}
          className="shrink-0 rounded border border-white/20 px-3 py-1 text-[11px] font-bold tracking-widest text-white/85 transition hover:border-white/40 hover:text-white focus:outline-none focus-visible:ring-2 focus-visible:ring-white/40"
        >
          {copied ? 'COPIED' : 'COPY'}
        </button>
      </span>
      {failed ? (
        <span className="text-[10px] text-[#ff4d6d]">
          Could not copy. Select the address text and copy it by hand.
        </span>
      ) : null}
    </li>
  );
}

export function HostPanel({ onClose }: { onClose: () => void }): JSX.Element {
  const [load, setLoad] = useState<LoadState>({ kind: 'loading' });
  const [busy, setBusy] = useState(false);
  const [actionError, setActionError] = useState<string | null>(null);
  const [copied, setCopied] = useState<string | null>(null);
  const [copyFailed, setCopyFailed] = useState<string | null>(null);
  const copyTimer = useRef<number | null>(null);

  // Start the fetch in the effect; state is only set in the promise callback.
  useEffect(() => {
    let alive = true;
    void fetchHostStatus().then((state) => {
      if (alive) {
        setLoad(state);
      }
    });
    return () => {
      alive = false;
    };
  }, []);

  const hosting = load.kind === 'ready' && load.data.hosting;
  const upnp = load.kind === 'ready' ? load.data.upnp : null;

  // Poll only while the router is still opening a port.
  useEffect(() => {
    if (!hosting || upnp !== 'trying') {
      return;
    }
    let alive = true;
    const id = window.setInterval(() => {
      void fetchHostStatus().then((state) => {
        if (alive) {
          setLoad(state);
        }
      });
    }, 2000);
    return () => {
      alive = false;
      window.clearInterval(id);
    };
  }, [hosting, upnp]);

  useEffect(() => {
    return () => {
      if (copyTimer.current !== null) {
        window.clearTimeout(copyTimer.current);
      }
    };
  }, []);

  const toggleHosting = useCallback(async (): Promise<void> => {
    if (load.kind !== 'ready' || busy) {
      return;
    }
    const enable = !load.data.hosting;
    setBusy(true);
    setActionError(null);
    try {
      const res = await fetch('/api/host', {
        method: 'POST',
        headers: { 'content-type': 'application/json', accept: 'application/json' },
        body: JSON.stringify({ enable }),
      });
      if (!res.ok) {
        setActionError(
          res.status === 403
            ? 'Hosting is only available on the computer running the game.'
            : 'Could not change hosting right now. Try again.',
        );
        return;
      }
      const raw: unknown = await res.json();
      const parsed = parseHostResponse(raw);
      if (parsed === null) {
        setActionError('The game gave an unexpected answer. Try again.');
        return;
      }
      setLoad({ kind: 'ready', data: parsed });
    } catch {
      setActionError('Could not reach the game. Is it still running?');
    } finally {
      setBusy(false);
    }
  }, [busy, load]);

  const copy = useCallback((address: string): void => {
    void copyText(address).then((ok) => {
      setCopied(ok ? address : null);
      setCopyFailed(ok ? null : address);
      if (copyTimer.current !== null) {
        window.clearTimeout(copyTimer.current);
      }
      copyTimer.current = window.setTimeout(() => {
        setCopied(null);
        setCopyFailed(null);
      }, 2500);
    });
  }, []);

  const renderAddress = (address: string): JSX.Element => (
    <AddressRow
      key={address}
      address={address}
      copied={copied === address}
      failed={copyFailed === address}
      onCopy={copy}
    />
  );

  const on = load.kind === 'ready' && load.data.hosting;

  return (
    <div
      className="fixed inset-0 z-40 overflow-y-auto bg-black/60 font-mono backdrop-blur-sm"
      onClick={onClose}
    >
      <div
        role="dialog"
        aria-modal="true"
        aria-labelledby="host-panel-title"
        className="mx-auto my-6 flex w-full max-w-lg flex-col gap-4 break-words rounded-xl border border-white/10 bg-[#120f1f] p-5 text-white"
        onClick={(event) => event.stopPropagation()}
      >
        <div className="flex items-center justify-between gap-4 border-b border-white/10 pb-3">
          <h2 id="host-panel-title" className="text-lg font-bold tracking-[0.25em] text-[#38e8ff]">
            PLAY WITH FRIENDS
          </h2>
          <button
            type="button"
            onClick={onClose}
            className="rounded border border-white/15 px-3 py-1 text-[11px] tracking-widest text-white/70 transition hover:text-white focus:outline-none focus-visible:ring-2 focus-visible:ring-white/40"
          >
            CLOSE
          </button>
        </div>

        {load.kind === 'loading' ? (
          <p className="text-sm text-white/70">Checking if this computer can host the game...</p>
        ) : null}

        {load.kind === 'unavailable' ? (
          <div className="flex flex-col gap-2">
            <p className="text-sm text-white/85">
              Hosting is only available on the computer running the game.
            </p>
            <p className="text-xs text-white/55">
              Open this page on that computer, then choose PLAY WITH FRIENDS again.
            </p>
          </div>
        ) : null}

        {load.kind === 'ready' ? (
          <>
            <p className="text-sm leading-snug text-white/80">
              Your computer runs the game for your friends. They open the address you send them in
              their browser and they are in.
            </p>

            <button
              type="button"
              role="switch"
              aria-checked={on}
              disabled={busy}
              onClick={() => {
                void toggleHosting();
              }}
              className="flex items-center justify-between gap-4 rounded-lg border border-white/15 bg-black/40 px-4 py-3 text-left transition focus:outline-none focus-visible:ring-2 focus-visible:ring-[#38e8ff]/60 disabled:opacity-60"
            >
              <span className="flex flex-col gap-0.5">
                <span className="text-sm font-bold tracking-[0.25em] text-white">
                  LET FRIENDS JOIN
                </span>
                <span className="text-[11px] text-white/55">
                  {on ? 'Hosting is on' : 'Hosting is off'}
                </span>
              </span>
              <span
                aria-hidden="true"
                className={[
                  'flex h-7 w-12 shrink-0 items-center rounded-full border p-1 transition',
                  on ? 'justify-end border-[#38e8ff]/60 bg-[#38e8ff]/25' : 'justify-start border-white/15 bg-white/5',
                ].join(' ')}
              >
                <span className={`h-5 w-5 rounded-full ${on ? 'bg-[#38e8ff]' : 'bg-white/40'}`} />
              </span>
            </button>

            {actionError !== null ? (
              <p role="alert" className="text-xs text-[#ff4d6d]">{actionError}</p>
            ) : null}

            {!load.data.hosting ? (
              <p className="text-xs text-white/55">
                Turn on LET FRIENDS JOIN to see the addresses to send.
              </p>
            ) : (
              <>
                <section className="flex flex-col gap-2">
                  <h3 className="text-[11px] font-bold tracking-[0.25em] text-white/70">
                    FRIENDS ON YOUR WI-FI
                  </h3>
                  <p className="text-xs text-white/60">
                    Send one of these to friends on the same Wi-Fi. They paste it into the browser.
                  </p>
                  {load.data.lan.length === 0 ? (
                    <p className="text-xs text-white/50">
                      No Wi-Fi address found on this computer.
                    </p>
                  ) : (
                    <ul className="flex flex-col gap-2">
                      {load.data.lan.map((ip) => renderAddress(hostUrl(ip, load.data.port)))}
                    </ul>
                  )}
                </section>

                <section className="flex flex-col gap-2">
                  <h3 className="text-[11px] font-bold tracking-[0.25em] text-white/70">
                    FRIENDS ANYWHERE
                  </h3>
                  {load.data.public !== null ? (
                    <>
                      <p className="text-xs text-white/60">
                        Send this to friends who are not on your Wi-Fi.
                      </p>
                      <ul className="flex flex-col gap-2">
                        {renderAddress(hostUrl(load.data.public, load.data.port))}
                      </ul>
                    </>
                  ) : load.data.upnp === 'trying' ? (
                    <p className="text-xs text-white/60">
                      Looking for a public address... this can take a moment. This page updates by
                      itself.
                    </p>
                  ) : load.data.upnp === 'failed' ? (
                    <p className="text-xs text-white/60">
                      The router did not open a port, so there is no public address right now.
                      Friends on your Wi-Fi can still join with the address above.
                    </p>
                  ) : (
                    <p className="text-xs text-white/60">
                      No public address. Friends on your Wi-Fi can join with the address above.
                    </p>
                  )}
                </section>

                {load.data.message.length > 0 ? (
                  <p className="rounded border border-white/10 bg-black/30 px-3 py-2 text-[11px] leading-snug text-white/70">
                    {load.data.message}
                  </p>
                ) : null}

                <Link
                  href="/play"
                  className="rounded-lg border-2 border-[#ff3db8] px-6 py-3 text-center text-sm font-bold tracking-[0.3em] text-[#ff3db8] transition hover:bg-[#ff3db8] hover:text-black focus:outline-none focus-visible:ring-2 focus-visible:ring-[#ff3db8]/60"
                >
                  START PLAYING
                </Link>
              </>
            )}
          </>
        ) : null}
      </div>
    </div>
  );
}
