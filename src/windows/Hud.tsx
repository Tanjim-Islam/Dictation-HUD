import { motion, AnimatePresence } from 'motion/react';
import { useEffect, useRef, useState } from 'react';
import { listen } from '@tauri-apps/api/event';
import { getSnapshot, onSnapshot, sendIntent, type SessionSnapshot } from '../lib/session';

type Level = { session_id: string; rms_db: number; peak_db: number };

/** Ten bars driven by native level events. Rebuilt with the full waveform model in Phase 7. */
function LevelBars({ sessionId }: { sessionId: string | null }) {
  const [level, setLevel] = useState(0);
  const smooth = useRef(0);
  useEffect(() => {
    const un = listen<Level>('session://level', (e) => {
      if (e.payload.session_id !== sessionId) return;
      // -60 dB floor .. -10 dB full.
      const target = Math.max(0, Math.min(1, (e.payload.rms_db + 60) / 50));
      smooth.current = target > smooth.current ? target : smooth.current * 0.8 + target * 0.2;
      setLevel(smooth.current);
    });
    return () => {
      un.then((f) => f());
    };
  }, [sessionId]);
  return (
    <div className="flex h-5 items-center gap-[3px]">
      {Array.from({ length: 10 }, (_, i) => {
        const taper = 1 - Math.abs(i - 4.5) / 6;
        const h = Math.max(3, level * 20 * taper);
        return <span key={i} className="w-[2.5px] rounded-full bg-white/85 transition-[height] duration-75" style={{ height: h }} />;
      })}
    </div>
  );
}

export function Hud() {
  const [snapshot, setSnapshot] = useState<SessionSnapshot>({ session_id: null, phase: 'idle', error: null });
  const [notice, setNotice] = useState<string | null>(null);

  useEffect(() => {
    let timer: number | undefined;
    const un = listen<string>('session://notice', (e) => {
      setNotice(e.payload);
      window.clearTimeout(timer);
      timer = window.setTimeout(() => setNotice(null), 3000);
    });
    return () => {
      window.clearTimeout(timer);
      un.then((f) => f());
    };
  }, []);

  useEffect(() => {
    const un = onSnapshot(setSnapshot);
    // After a WebView reload, render current truth. The session itself lives in Rust.
    getSnapshot().then(setSnapshot).catch(() => {});
    return () => {
      un.then((f) => f());
    };
  }, []);

  const { phase } = snapshot;
  const busy = phase === 'preparing' || phase === 'finalizing' || phase === 'processing' || phase === 'delivering';

  return (
    <div className="pointer-events-none">
      <AnimatePresence>
        {phase !== 'idle' && phase !== 'error' && (
          <div className="fixed bottom-5 left-1/2 flex -translate-x-1/2 items-center gap-3">
            <motion.div
              initial={{ scale: 0.9, y: 10, opacity: 0 }}
              animate={{ scale: 1, y: 0, opacity: 1 }}
              exit={{ scale: 0.9, y: 10, opacity: 0 }}
              transition={{ duration: 0.2, ease: [0.22, 1, 0.36, 1] }}
              className="flex h-[40px] w-[90px] items-center justify-center rounded-full border border-white/5 bg-[#0f0f0f]/95 shadow-lg"
            >
              {phase === 'listening' ? (
                <LevelBars sessionId={snapshot.session_id} />
              ) : (
                busy && <div className="size-3 animate-spin rounded-full border-2 border-white/30 border-t-white/80" aria-hidden="true" />
              )}
            </motion.div>
            <motion.button
              initial={{ x: 10, opacity: 0 }}
              animate={{ x: 0, opacity: 1 }}
              exit={{ x: 10, opacity: 0 }}
              onClick={() => sendIntent('finish', 'flow_bar')}
              aria-label="Finish dictation"
              className="pointer-events-auto flex size-[32px] items-center justify-center rounded-full border border-white/5 bg-[#1c1c1c] transition-transform hover:scale-105 active:scale-95"
            >
              <div className="size-[10px] rounded-sm bg-[#f2f1ea]" />
            </motion.button>
          </div>
        )}
      </AnimatePresence>
      <AnimatePresence>
        {phase === 'idle' && notice && (
          <motion.div
            key="notice"
            initial={{ y: 8, opacity: 0 }}
            animate={{ y: 0, opacity: 1 }}
            exit={{ y: 8, opacity: 0 }}
            className="fixed bottom-[30px] left-1/2 max-w-[560px] -translate-x-1/2 truncate rounded-full bg-[#0f0f0f]/95 px-3.5 py-1.5 text-xs text-white/90 shadow-lg"
            role="status"
          >
            {notice}
          </motion.div>
        )}
      </AnimatePresence>
      <AnimatePresence>
        {phase === 'error' && snapshot.error && (
          <motion.div
            initial={{ y: 8, opacity: 0 }}
            animate={{ y: 0, opacity: 1 }}
            exit={{ y: 8, opacity: 0 }}
            className="fixed bottom-[30px] left-1/2 -translate-x-1/2 rounded-full border border-[#7a1f1f] bg-[#fbe9e9] px-3 py-1 text-xs text-[#7a1f1f] shadow-sm"
          >
            {snapshot.error}
          </motion.div>
        )}
      </AnimatePresence>
    </div>
  );
}
