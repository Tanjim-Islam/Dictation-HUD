import { motion, AnimatePresence } from 'framer-motion';
import { useEffect, useRef, useState } from 'react';
import { Waveform } from '../components/Waveform';
import { Badge } from '../components/Badge';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { log } from '../lib/log';
import { CAPTURE_PHASES, getSnapshot, onSnapshot, sendIntent, type SessionSnapshot } from '../lib/session';
import type { Stream } from '../lib/deepgram';

type EngineStart = { sessionId: string; provider: string; echoCancellation: boolean; noiseSuppression: boolean };
type RuntimeKeys = [string | null, string | null, string | null, string | null];
type EngineEventPayload =
  | { type: 'ready' | 'finalized' }
  | { type: 'interim' | 'final'; text: string }
  | { type: 'failed'; error: string };

const report = (sessionId: string, event: EngineEventPayload) =>
  invoke('engine_event', { sessionId, event }).catch(() => {});

/**
 * Legacy HUD, now a renderer of the Rust session snapshot plus a temporary engine host
 * (microphone + provider socket) driven by `engine://*` commands. Session state is owned by
 * Rust; this component never decides lifecycle. Replaced by the Flow Bar in Phase 7.
 */
class WebEngine {
  private stream: Stream | null = null;
  private media: MediaStream | null = null;
  private analyserCtx: AudioContext | null = null;
  // Set by finish or cancel; a start still awaiting the mic or socket must then bail out.
  private stopped = false;

  constructor(
    readonly sessionId: string,
    private onAnalyser: (a: AnalyserNode | null) => void,
  ) {}

  async start(cmd: EngineStart) {
    const fail = (error: string) => {
      this.release();
      report(this.sessionId, { type: 'failed', error });
    };
    try {
      this.media = await navigator.mediaDevices.getUserMedia({
        audio: { echoCancellation: cmd.echoCancellation, noiseSuppression: cmd.noiseSuppression, channelCount: 1 },
      });
    } catch {
      return fail('microphone');
    }
    if (this.stopped) return this.release();

    this.analyserCtx = new AudioContext();
    const an = this.analyserCtx.createAnalyser();
    an.fftSize = 1024;
    this.analyserCtx.createMediaStreamSource(this.media).connect(an);
    this.onAnalyser(an);

    const handlers = {
      onOpen: () => report(this.sessionId, { type: 'ready' }),
      onTranscript: (text: string, isFinal: boolean) =>
        report(this.sessionId, { type: isFinal ? 'final' : 'interim', text }),
      onError: (kind: string) => fail(kind),
    };
    try {
      const [, dg, , el] = await invoke<RuntimeKeys>('runtime_keys');
      if (cmd.provider === 'elevenlabs') {
        if (!el) return fail('missing_key');
        const token = await invoke<string>('create_elevenlabs_token').catch(() => null);
        if (!token) return fail('auth');
        const { startElevenLabsStream } = await import('../lib/elevenlabs');
        this.stream = await startElevenLabsStream(token, this.media, handlers);
      } else {
        if (!dg) return fail('missing_key');
        const { startDeepgramStream } = await import('../lib/deepgram');
        this.stream = await startDeepgramStream(dg, this.media, handlers);
      }
      if (this.stopped) {
        this.stream?.cancel();
        this.release();
      }
    } catch {
      fail('network');
    }
  }

  async finish() {
    this.stopped = true;
    const s = this.stream;
    this.releaseMic();
    if (s) await s.finish();
    this.release();
    report(this.sessionId, { type: 'finalized' });
  }

  cancel() {
    this.stopped = true;
    this.stream?.cancel();
    this.release();
  }

  private releaseMic() {
    this.media?.getTracks().forEach((t) => t.stop());
    this.media = null;
  }

  private release() {
    this.releaseMic();
    this.stream = null;
    this.analyserCtx?.close().catch(() => {});
    this.analyserCtx = null;
    this.onAnalyser(null);
  }
}

export function Hud() {
  const [snapshot, setSnapshot] = useState<SessionSnapshot>({ session_id: null, phase: 'idle', error: null });
  const [analyser, setAnalyser] = useState<AnalyserNode | null>(null);
  const engineRef = useRef<WebEngine | null>(null);

  useEffect(() => {
    const subs = [
      onSnapshot(setSnapshot),
      listen<EngineStart>('engine://start', (e) => {
        engineRef.current?.cancel();
        const engine = new WebEngine(e.payload.sessionId, setAnalyser);
        engineRef.current = engine;
        engine.start(e.payload);
        log(`engine start provider=${e.payload.provider}`);
      }),
      listen<string>('engine://finish', (e) => {
        const engine = engineRef.current;
        if (engine?.sessionId === e.payload) engine.finish();
      }),
      listen<string>('engine://cancel', (e) => {
        const engine = engineRef.current;
        if (engine?.sessionId === e.payload) {
          engine.cancel();
          engineRef.current = null;
        }
      }),
    ];
    // Recover after a WebView reload: render current truth, and report a capture session
    // whose engine died with the previous page.
    getSnapshot()
      .then((s) => {
        setSnapshot(s);
        if (s.session_id && CAPTURE_PHASES.includes(s.phase) && engineRef.current?.sessionId !== s.session_id) {
          report(s.session_id, { type: 'failed', error: 'lost' });
        }
      })
      .catch(() => {});
    return () => {
      subs.forEach((p) => p.then((un) => un()));
    };
  }, []);

  const { phase } = snapshot;
  const show = phase !== 'idle';
  const capturing = phase === 'listening';
  const busy = phase === 'preparing' || phase === 'finalizing' || phase === 'processing' || phase === 'delivering';

  return (
    <div className="pointer-events-none">
      <AnimatePresence>
        {show && phase !== 'error' && (
          <div className="fixed bottom-5 left-1/2 -translate-x-1/2 flex items-center gap-4 z-50">
            <motion.div
              initial={{ scale: 0.9, y: 10, opacity: 0 }}
              animate={{ scale: 1, y: 0, opacity: 1 }}
              exit={{ scale: 0.9, y: 10, opacity: 0 }}
              transition={{ duration: 0.2, ease: 'easeOut' }}
              className="w-[90px] h-[40px] bg-[#0f0f0f]/95 border border-white/5 rounded-full shadow-lg flex items-center justify-center"
            >
              {capturing && analyser ? (
                <Waveform analyser={analyser} />
              ) : (
                busy && <div className="w-3 h-3 border-2 border-white/30 border-t-white/80 rounded-full animate-spin" aria-hidden="true" />
              )}
            </motion.div>
            <motion.button
              initial={{ x: 10, opacity: 0 }}
              animate={{ x: 0, opacity: 1 }}
              exit={{ x: 10, opacity: 0 }}
              transition={{ duration: 0.2, ease: 'easeOut' }}
              onClick={() => sendIntent('finish', 'flow_bar')}
              aria-label="Finish dictation"
              className="w-[32px] h-[32px] rounded-full bg-[#1c1c1c] border border-white/5 flex items-center justify-center hover:scale-105 active:scale-95 transition-all pointer-events-auto focus:outline-none"
            >
              <div className="w-[10px] h-[10px] bg-[#f2f1ea] rounded-sm"></div>
            </motion.button>
          </div>
        )}
      </AnimatePresence>
      <AnimatePresence>
        {phase === 'error' && snapshot.error && (
          <motion.div
            initial={{ y: 8, opacity: 0 }}
            animate={{ y: 0, opacity: 1 }}
            exit={{ y: 8, opacity: 0 }}
            transition={{ duration: 0.18 }}
            className="fixed left-1/2 -translate-x-1/2 bottom-[30px]"
          >
            <Badge text={snapshot.error} />
          </motion.div>
        )}
      </AnimatePresence>
    </div>
  );
}
