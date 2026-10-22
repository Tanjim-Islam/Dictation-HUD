import { useEffect, useRef, useState } from 'react';
import { AnimatePresence, motion, MotionConfig, useReducedMotion } from 'motion/react';
import { listen } from '@tauri-apps/api/event';
import { Check, X } from 'lucide-react';
import { getSnapshot, onSnapshot, sendIntent, type SessionSnapshot } from '@/lib/session';
import { transition } from '@/lib/motion';
import { cn } from '@/lib/utils';
import { BAR_COUNT, busyBars, levelFromDb, restingBars, stepBars } from './waveform';

type Level = { session_id: string; rms_db: number; peak_db: number };
type Phase = SessionSnapshot['phase'];

const BUSY: Phase[] = ['finalizing', 'processing', 'delivering'];
/** Pill grows in with a soft spring and shrinks away quickly. Rust hides the window only
 * after `PILL_OUT` ends (`EXIT_MS` in flowbar.rs). */
const PILL_IN = { type: 'spring', stiffness: 460, damping: 28, mass: 0.8, opacity: { duration: 0.14 } } as const;
const PILL_OUT = { duration: 0.18, ease: [0.4, 0, 1, 1] } as const;
const BAR_SPAN = 16;

/** Ten bars painted from native level frames. Heights are written to the DOM per frame. */
function Waveform({ phase, sessionId, vertical }: { phase: Phase; sessionId: string | null; vertical: boolean }) {
  const bars = useRef<(HTMLSpanElement | null)[]>([]);
  const level = useRef(0);
  const reduced = useReducedMotion() ?? false;

  useEffect(() => {
    level.current = 0;
    const un = listen<Level>('session://level', (e) => {
      if (e.payload.session_id === sessionId) level.current = levelFromDb(e.payload.rms_db);
    });
    return () => {
      un.then((f) => f());
    };
  }, [sessionId]);

  useEffect(() => {
    let heights = restingBars();
    let frame = 0;
    let last = performance.now();
    const start = last;
    const paint = (now: number) => {
      const t = now - start;
      heights = BUSY.includes(phase)
        ? busyBars(t, reduced)
        : phase === 'listening'
          ? stepBars(heights, level.current, t, now - last, reduced)
          : restingBars();
      last = now;
      heights.forEach((h, i) => {
        const el = bars.current[i];
        if (el) el.style[vertical ? 'width' : 'height'] = `${Math.max(2, h * BAR_SPAN)}px`;
      });
      frame = requestAnimationFrame(paint);
    };
    frame = requestAnimationFrame(paint);
    return () => cancelAnimationFrame(frame);
  }, [phase, reduced, vertical]);

  return (
    <div className={cn('flex items-center justify-center gap-[2px]', vertical ? 'h-[40px] w-4 flex-col' : 'h-4 w-[40px]')} aria-hidden>
      {Array.from({ length: BAR_COUNT }, (_, i) => (
        <span
          key={i}
          ref={(el) => {
            bars.current[i] = el;
          }}
          className={cn('h-[2px] w-[2px] shrink-0 rounded-full bg-white', phase === 'preparing' && 'opacity-60')}
        />
      ))}
    </div>
  );
}

function Pill({ snapshot, vertical }: { snapshot: SessionSnapshot; vertical: boolean }) {
  const { phase } = snapshot;
  const busy = BUSY.includes(phase);
  const canCancel = phase !== 'delivering';
  const canFinish = phase === 'preparing' || phase === 'listening';
  const label =
    snapshot.reconnecting ? 'Reconnecting, your speech is kept' : phase === 'preparing' ? 'Starting dictation' : phase === 'listening' ? 'Listening' : phase === 'finalizing' ? 'Finishing' : phase === 'delivering' ? 'Inserting text' : 'Processing';
  return (
    <motion.div
      initial={{ opacity: 0, scale: 0.55 }}
      animate={{ opacity: 1, scale: 1, transition: PILL_IN }}
      exit={{ opacity: 0, scale: 0.6, transition: PILL_OUT }}
      role="status"
      aria-label={label}
      title={label}
      className={cn('flex h-full w-full items-center justify-between rounded-full bg-[#0f0f0f] ring-1 ring-inset ring-white/10', vertical ? 'flex-col py-1' : 'px-1')}
    >
      <button
        type="button"
        aria-label="Cancel dictation"
        title="Cancel"
        disabled={!canCancel}
        onClick={() => sendIntent('cancel', 'flow_bar')}
        className="flex size-5 shrink-0 items-center justify-center rounded-full bg-white/[0.14] text-white/85 transition-[background-color,opacity] hover:bg-white/25 disabled:opacity-0"
      >
        <X className="size-[11px]" strokeWidth={2.6} />
      </button>
      {/* While switching keys the waveform dims and pulses; a text label would not fit the side-docked bar. */}
      <div className={cn('transition-opacity', snapshot.reconnecting && 'animate-pulse opacity-50')}>
        <Waveform phase={phase} sessionId={snapshot.session_id} vertical={vertical} />
      </div>
      <button
        type="button"
        aria-label="Finish dictation"
        title="Finish"
        disabled={!canFinish}
        onClick={() => sendIntent('finish', 'flow_bar')}
        className={cn('flex size-5 shrink-0 items-center justify-center rounded-full bg-white text-[#0f0f0f] transition-[transform,opacity] hover:scale-105 active:scale-95 disabled:opacity-0', busy && 'pointer-events-none')}
      >
        <Check className="size-[12px]" strokeWidth={3} />
      </button>
    </motion.div>
  );
}

function Banner({ text, tone }: { text: string; tone: 'notice' | 'error' }) {
  return (
    <motion.div
      initial={{ opacity: 0, y: 4 }}
      animate={{ opacity: 1, y: 0 }}
      exit={{ opacity: 0 }}
      transition={transition.base}
      role={tone === 'error' ? 'alert' : 'status'}
      className="flex h-full w-full items-center gap-2 rounded-full bg-[#0f0f0f] px-3.5 text-[12.5px] leading-none text-white/90 ring-1 ring-inset ring-white/10"
    >
      <span className={cn('size-1.5 shrink-0 rounded-full', tone === 'error' ? 'bg-[#ff6b6b]' : 'bg-white/60')} />
      <span className="min-w-0 truncate">{text}</span>
    </motion.div>
  );
}

function useVertical() {
  const read = () => window.innerHeight > window.innerWidth;
  const [vertical, setVertical] = useState(read);
  useEffect(() => {
    const on = () => setVertical(read());
    window.addEventListener('resize', on);
    return () => window.removeEventListener('resize', on);
  }, []);
  return vertical;
}

/**
 * The Flow Bar window. It renders session truth from Rust and sends intents; it never decides
 * state. Rust sizes and places the window (pill, vertical pill, or banner).
 */
export function FlowBar() {
  const [snapshot, setSnapshot] = useState<SessionSnapshot>({ session_id: null, phase: 'idle', error: null });
  const [notice, setNotice] = useState<string | null>(null);
  const vertical = useVertical();

  useEffect(() => {
    let timer: number | undefined;
    const un = listen<string>('session://notice', (e) => {
      setNotice(e.payload);
      window.clearTimeout(timer);
      timer = window.setTimeout(() => setNotice(null), 3200);
    });
    return () => {
      window.clearTimeout(timer);
      un.then((f) => f());
    };
  }, []);

  useEffect(() => {
    const un = onSnapshot(setSnapshot);
    // After a WebView reload, render current truth. The session itself lives in Rust.
    getSnapshot()
      .then(setSnapshot)
      .catch(() => {});
    return () => {
      un.then((f) => f());
    };
  }, []);

  const { phase } = snapshot;
  const view = phase === 'error' ? 'error' : phase === 'idle' ? (notice ? 'notice' : 'none') : 'pill';

  return (
    <MotionConfig reducedMotion="user">
      <div className="h-screen w-screen select-none overflow-hidden">
        <AnimatePresence mode="wait" initial={false}>
          {view === 'pill' && <Pill key="pill" snapshot={snapshot} vertical={vertical} />}
          {view === 'error' && <Banner key="error" tone="error" text={snapshot.error ?? 'Something went wrong'} />}
          {view === 'notice' && notice && <Banner key="notice" tone="notice" text={notice} />}
        </AnimatePresence>
      </div>
    </MotionConfig>
  );
}
