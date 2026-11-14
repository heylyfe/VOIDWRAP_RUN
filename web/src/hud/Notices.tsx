'use client';

// Short-lived HUD notices (altar results, purchases, level events), stacked
// top centre. Each lives NOTICE_LIFETIME_MS with a fade over the last
// NOTICE_FADE_MS. A requestAnimationFrame tick held in state drives the
// current time, so expiry never sets state synchronously inside an effect.
import { useEffect, useState } from 'react';
import type { JSX } from 'react';
import type { Notice } from '../game/store';
import { NOTICE_BOUGHT, NOTICE_EVENT } from '../protocol/generated';
import { noticeOpacity, noticeText } from './noticeText';
import type { NoticeText } from './noticeText';

const MAGENTA = '#ff3db8';
const CYAN = '#38e8ff';
const GOLD = '#ffd23d';

interface VisibleNotice {
  notice: Notice;
  text: NoticeText;
  opacity: number;
}

export function Notices({ notices }: { notices: Notice[] }): JSX.Element {
  // 0 keeps server and first client render identical; ages read as negative,
  // which renders every notice fully opaque until the first frame ticks.
  const [now, setNow] = useState(0);
  const hasNotices = notices.length > 0;

  useEffect(() => {
    if (!hasNotices) {
      return;
    }
    let raf = 0;
    const tick = (): void => {
      setNow(performance.now());
      raf = window.requestAnimationFrame(tick);
    };
    raf = window.requestAnimationFrame(tick);
    return () => {
      window.cancelAnimationFrame(raf);
    };
  }, [hasNotices]);

  const visible: VisibleNotice[] = [];
  for (const notice of notices) {
    const opacity = noticeOpacity(now - notice.at);
    if (opacity <= 0) {
      continue;
    }
    const text = noticeText(notice);
    if (text === null) {
      continue;
    }
    visible.push({ notice, text, opacity });
  }

  return (
    <div
      aria-live="polite"
      data-testid="notices"
      className="pointer-events-none fixed inset-x-0 top-3 z-50 flex flex-col items-center gap-2 px-4 font-mono"
    >
      {visible.map(({ notice, text, opacity }) => {
        const isEvent = notice.kind === NOTICE_EVENT;
        const accent = isEvent ? CYAN : notice.kind === NOTICE_BOUGHT ? GOLD : MAGENTA;
        return (
          <div
            key={notice.seq}
            data-testid={`notice-${notice.kind}`}
            style={{ opacity, borderColor: `${accent}55`, boxShadow: `0 0 18px ${accent}33` }}
            className={[
              'w-full rounded-lg border bg-[#120f1f]/90 px-4 text-center backdrop-blur-sm',
              isEvent ? 'max-w-xl py-4' : 'max-w-md py-2',
            ].join(' ')}
          >
            <p
              className={
                isEvent
                  ? 'text-xl font-black tracking-[0.3em] text-white sm:text-2xl'
                  : 'text-[10px] font-bold tracking-[0.3em]'
              }
              style={isEvent ? { color: accent } : { color: accent }}
            >
              {text.title}
            </p>
            <p
              className={
                isEvent
                  ? 'mt-1 text-base leading-snug text-white/95 sm:text-lg'
                  : 'mt-0.5 text-xs leading-snug text-white/90'
              }
            >
              {text.body}
            </p>
          </div>
        );
      })}
    </div>
  );
}
