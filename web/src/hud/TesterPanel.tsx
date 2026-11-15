'use client';

// Tester panel (T-111). A thin client over the frozen command manifest in
// design/DEBUG_COMMANDS.md. It renders nothing at all until a dev reveals it with
// Ctrl+Shift+D, and it stays locked until the SERVER accepts the code: the client
// never decides whether a command is allowed, because only the server knows whether
// the route is open and how many players are connected.
import { useCallback, useEffect, useMemo, useState, type JSX } from 'react';
import { useGameStore } from '../game/store';
import {
  DEBUG_SECTIONS,
  EMPTY_PANEL,
  applyResult,
  buildArgs,
  defaultValues,
  isRevealEvent,
  panelVisible,
  type CommandSpec,
  type DebugPanelState,
} from './debugPanel';

const PANEL_BG = 'rgba(8,8,16,0.94)';
const BORDER = 'rgba(255,255,255,0.16)';
const MUTED = 'rgba(255,255,255,0.55)';
const OK = '#5ef2a0';
const BAD = '#ff4d6d';
const ACCENT = '#38e8ff';

export function TesterPanel(props: {
  playerCount: number;
  onCommand: (code: string, command: string, args: string) => void;
}): JSX.Element | null {
  const { playerCount, onCommand } = props;
  const debugResult = useGameStore((s) => s.debugResult);
  const [revealed, setRevealed] = useState(false);
  const [state, setState] = useState<DebugPanelState>(EMPTY_PANEL);
  const [formError, setFormError] = useState<string | null>(null);

  // The reveal gesture. Registered once; a normal build never sees the panel.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (isRevealEvent(e)) {
        e.preventDefault();
        setRevealed((v) => !v);
      }
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, []);

  // Fold each server answer into the log. `seq` makes a repeated identical answer count.
  // This is React's documented "adjust state when a prop changes" pattern: the update
  // happens during render, guarded by the seq, so there is no effect and no extra pass.
  const [seenSeq, setSeenSeq] = useState(0);
  if (debugResult && debugResult.seq !== seenSeq) {
    setSeenSeq(debugResult.seq);
    setState((prev) => applyResult(prev, debugResult.command, debugResult.args, debugResult.ok, debugResult.message));
  }

  const send = useCallback(
    (command: string, args: string) => {
      onCommand(state.code, command, args);
    },
    [onCommand, state.code],
  );

  const submit = useCallback(
    (spec: CommandSpec) => {
      const values = state.values[spec.id] ?? defaultValues(spec);
      const built = buildArgs(spec, values);
      if ('error' in built) {
        setFormError(built.error);
        return;
      }
      setFormError(null);
      send(spec.id, built.args);
    },
    [send, state.values],
  );

  const setValue = useCallback((commandId: string, key: string, value: string) => {
    setState((prev) => ({
      ...prev,
      values: { ...prev.values, [commandId]: { ...(prev.values[commandId] ?? {}), [key]: value } },
    }));
  }, []);

  const sections = useMemo(() => DEBUG_SECTIONS, []);

  // Singleplayer only, mirroring the server gate. Hiding is a courtesy; the server
  // rejects the command either way.
  if (!revealed || !panelVisible(playerCount)) return null;

  return (
    <div
      data-testid="debug-panel"
      className="pointer-events-auto absolute right-3 top-3 z-50 flex max-h-[85vh] w-[22rem] flex-col gap-2 overflow-y-auto rounded-md border p-3 font-mono text-xs"
      style={{ background: PANEL_BG, borderColor: BORDER, color: 'white' }}
    >
      <div className="flex items-center justify-between">
        <span className="text-[11px] tracking-widest" style={{ color: ACCENT }}>
          TESTER PANEL
        </span>
        <span data-testid="debug-lock" style={{ color: state.unlocked ? OK : MUTED }}>
          {state.unlocked ? 'unlocked' : 'locked'}
        </span>
      </div>

      {!state.unlocked ? (
        <div className="flex flex-col gap-1">
          <label htmlFor="debug-code" style={{ color: MUTED }}>
            Dev code (checked by the server)
          </label>
          <div className="flex gap-1">
            <input
              id="debug-code"
              data-testid="debug-code"
              type="password"
              autoComplete="off"
              value={state.code}
              onChange={(e) => setState((prev) => ({ ...prev, code: e.target.value }))}
              className="min-w-0 flex-1 rounded border bg-black/60 px-2 py-1"
              style={{ borderColor: BORDER }}
            />
            <button
              type="button"
              data-testid="debug-unlock"
              onClick={() => send('state', '{}')}
              className="rounded border px-2 py-1"
              style={{ borderColor: ACCENT, color: ACCENT }}
            >
              Unlock
            </button>
          </div>
          <p style={{ color: MUTED }}>
            The panel stays locked until the server answers. A wrong code is rate-limited server-side.
          </p>
        </div>
      ) : null}

      {state.unlocked
        ? sections.map((section) => (
            <section key={section.id} data-testid={`debug-section-${section.id}`} className="flex flex-col gap-1">
              <h3 className="text-[11px] tracking-widest" style={{ color: ACCENT }}>
                {section.label.toUpperCase()}
              </h3>
              {section.commands.map((spec) => {
                const open = state.open === spec.id;
                const values = state.values[spec.id] ?? defaultValues(spec);
                return (
                  <div key={spec.id} className="rounded border" style={{ borderColor: BORDER }}>
                    <button
                      type="button"
                      data-testid={`debug-cmd-${spec.id}`}
                      onClick={() => setState((prev) => ({ ...prev, open: open ? null : spec.id }))}
                      className="flex w-full items-center justify-between px-2 py-1 text-left"
                    >
                      <span>{spec.label}</span>
                      <span style={{ color: MUTED }}>{spec.owner === 'client' ? 'client' : ''}</span>
                    </button>
                    {open ? (
                      <div className="flex flex-col gap-1 border-t px-2 py-1" style={{ borderColor: BORDER }}>
                        <p style={{ color: MUTED }}>{spec.hint}</p>
                        {spec.args.map((arg) => (
                          <label key={arg.key} className="flex items-center gap-2">
                            <span className="w-24 shrink-0" style={{ color: MUTED }}>
                              {arg.label}
                              {arg.required ? '' : ' (opt)'}
                            </span>
                            {arg.kind === 'bool' ? (
                              <input
                                type="checkbox"
                                data-testid={`debug-arg-${spec.id}-${arg.key}`}
                                checked={values[arg.key] === 'true'}
                                onChange={(e) => setValue(spec.id, arg.key, e.target.checked ? 'true' : 'false')}
                              />
                            ) : (
                              <input
                                data-testid={`debug-arg-${spec.id}-${arg.key}`}
                                value={values[arg.key] ?? ''}
                                onChange={(e) => setValue(spec.id, arg.key, e.target.value)}
                                className="min-w-0 flex-1 rounded border bg-black/60 px-1 py-0.5"
                                style={{ borderColor: BORDER }}
                              />
                            )}
                          </label>
                        ))}
                        <button
                          type="button"
                          data-testid={`debug-run-${spec.id}`}
                          onClick={() => submit(spec)}
                          className="self-start rounded border px-2 py-0.5"
                          style={{ borderColor: OK, color: OK }}
                        >
                          Run
                        </button>
                      </div>
                    ) : null}
                  </div>
                );
              })}
            </section>
          ))
        : null}

      {formError ? (
        <p data-testid="debug-form-error" style={{ color: BAD }}>
          {formError}
        </p>
      ) : null}

      {state.log.length > 0 ? (
        <section data-testid="debug-log" className="flex flex-col gap-0.5 border-t pt-1" style={{ borderColor: BORDER }}>
          <h3 className="text-[11px] tracking-widest" style={{ color: ACCENT }}>
            OUTPUT
          </h3>
          {[...state.log].reverse().map((entry) => (
            <div key={entry.seq} style={{ color: entry.ok ? OK : BAD }}>
              <span style={{ color: MUTED }}>{entry.command}</span> {entry.message}
            </div>
          ))}
        </section>
      ) : null}
    </div>
  );
}
