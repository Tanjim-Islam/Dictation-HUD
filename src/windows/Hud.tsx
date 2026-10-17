import { motion, AnimatePresence } from 'framer-motion';
import { useEffect, useRef, useState } from 'react';
import { Waveform } from '../components/Waveform';
import { Badge } from '../components/Badge';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { log } from '../lib/log';

type Recorder = { stop: () => Promise<void> | void };
type Behavior = { echo_cancellation?: boolean; noise_suppression?: boolean; stt_provider?: string };
type RuntimeKeys = [string | null, string | null, string | null, string | null];

// Legacy HUD. Replaced by the Flow Bar in Phase 7.
export function Hud() {
  const [show, setShow] = useState(false);
  const [seconds, setSeconds] = useState(0);
  const [badge, setBadge] = useState<string | null>(null);
  const [analyser, setAnalyser] = useState<AnalyserNode | null>(null);
  const [isConnecting, setIsConnecting] = useState(false);
  const timerRef = useRef<number | null>(null);
  const recRef = useRef<Recorder | null>(null);
  const streamRef = useRef<MediaStream | null>(null);
  const partialRef = useRef<string[]>([]);
  const latestTranscriptRef = useRef('');
  const activeRef = useRef(false);

  const setBackendState = (newState: 'recording' | 'stopping' | 'inactive') =>
    invoke('set_recording_active', { newState }).catch(() => {});

  function releaseMic() {
    streamRef.current?.getTracks().forEach((t) => t.stop());
    streamRef.current = null;
  }

  async function begin() {
    if (recRef.current) {
      await recRef.current.stop();
      recRef.current = null;
    }
    if (timerRef.current) {
      clearInterval(timerRef.current);
      timerRef.current = null;
    }
    releaseMic();
    partialRef.current = [];
    latestTranscriptRef.current = '';
    activeRef.current = true;
    setSeconds(0);
    setAnalyser(null);
    setIsConnecting(true);
    setBadge('Initializing microphone...');
    setShow(true);

    const fail = async (message: string) => {
      setBadge(message);
      setIsConnecting(false);
      activeRef.current = false;
      releaseMic();
      await setBackendState('inactive');
    };

    try {
      const behavior = await invoke<Behavior>('get_behavior').catch((): Behavior => ({}));
      const sttProvider = behavior.stt_provider === 'elevenlabs' ? 'elevenlabs' : 'deepgram';
      const stream = await navigator.mediaDevices.getUserMedia({
        audio: {
          echoCancellation: behavior.echo_cancellation !== false,
          noiseSuppression: behavior.noise_suppression !== false,
          channelCount: 1,
        },
      });
      streamRef.current = stream;

      const ctx = new AudioContext();
      const an = ctx.createAnalyser();
      an.fftSize = 1024;
      ctx.createMediaStreamSource(stream).connect(an);
      setAnalyser(an);

      const onOpen = () => {
        setIsConnecting(false);
        setBackendState('recording');
        setBadge(null);
        timerRef.current = window.setInterval(() => setSeconds((s) => s + 1), 1000);
      };
      const onTranscript = (t: string, final: boolean) => {
        if (t) latestTranscriptRef.current = t;
        if (t && final) partialRef.current.push(t);
      };
      const onError = () => {
        setBadge('Network error. Try again');
        setIsConnecting(false);
        setBackendState('inactive');
      };

      const [, dg, , el] = await invoke<RuntimeKeys>('runtime_keys');
      if (sttProvider === 'elevenlabs') {
        if (!el) return fail('ElevenLabs key not configured');
        const token = await invoke<string>('create_elevenlabs_token');
        const { startElevenLabsStream } = await import('../lib/elevenlabs');
        recRef.current = await startElevenLabsStream(token, stream, { onTranscript, onOpen, onError });
      } else {
        if (!dg) return fail('Deepgram key not configured');
        const { startDeepgramStream } = await import('../lib/deepgram');
        recRef.current = await startDeepgramStream(dg, stream, { onTranscript, onOpen, onError });
      }
      log(`HUD started provider=${sttProvider}`);
    } catch (e) {
      log('HUD start failed: ' + (e instanceof Error ? e.name : 'unknown'));
      await fail(e instanceof DOMException && e.name === 'NotAllowedError' ? 'Microphone permission required' : 'Failed to start dictation');
    }
  }

  async function stop() {
    const recorder = recRef.current;
    if (!activeRef.current && !recorder) return;
    activeRef.current = false;

    if (timerRef.current) {
      clearInterval(timerRef.current);
      timerRef.current = null;
    }
    if (recorder) {
      recRef.current = null;
      try {
        await recorder.stop();
      } catch {
        log('HUD recorder stop failed');
      }
    }
    releaseMic();
    setIsConnecting(false);

    let raw = partialRef.current.join(' ').trim();
    const latest = latestTranscriptRef.current.trim();
    if (raw && latest && !raw.includes(latest)) raw = `${raw} ${latest}`.trim();
    if (!raw) raw = latest;
    partialRef.current = [];
    latestTranscriptRef.current = '';

    try {
      if (!raw) {
        await setBackendState('inactive');
        setShow(false);
        await invoke('stop_dictation');
        return;
      }
      await setBackendState('stopping');
      // refine_text always returns usable text; AI failures fall back to deterministic text.
      const refined = await invoke<string>('refine_text', { rawText: raw }).catch(() => raw);
      setShow(false);
      await invoke('stop_dictation');
      await new Promise((r) => setTimeout(r, 250));
      await invoke<boolean>('insert_text', { text: refined });
    } catch {
      log('HUD stop failed');
      setShow(false);
      await invoke('stop_dictation').catch(() => {});
    } finally {
      await setBackendState('inactive');
    }
  }

  // Keep the latest handlers reachable from long-lived event listeners.
  const handlersRef = useRef({ begin, stop });
  handlersRef.current = { begin, stop };

  useEffect(() => {
    const unlisteners = [
      listen<string>('hud-badge', (e) => {
        setBadge(String(e.payload || ''));
        setTimeout(() => setBadge(null), 3000);
      }),
      listen('dictation-start', () => handlersRef.current.begin()),
      listen('dictation-stop', () => handlersRef.current.stop()),
    ];
    return () => {
      unlisteners.forEach((p) => p.then((un) => un()));
    };
  }, []);

  const mm = String(Math.floor(seconds / 60)).padStart(2, '0');
  const ss = String(seconds % 60).padStart(2, '0');

  return (
    <div className="pointer-events-none">
      <AnimatePresence>
        {show && (
          <div className="fixed bottom-5 left-1/2 -translate-x-1/2 flex items-center gap-4 z-50">
            <motion.div
              initial={{ x: -10, opacity: 0 }}
              animate={{ x: 0, opacity: 0.9 }}
              exit={{ x: -10, opacity: 0 }}
              transition={{ duration: 0.2, ease: 'easeOut' }}
              className="text-[#f2f1ea] font-mono text-sm tracking-wider select-none"
            >
              {mm}:{ss}
            </motion.div>
            <motion.div
              initial={{ scale: 0.9, y: 10, opacity: 0 }}
              animate={{ scale: 1, y: 0, opacity: 1 }}
              exit={{ scale: 0.9, y: 10, opacity: 0 }}
              transition={{ duration: 0.2, ease: 'easeOut' }}
              className="w-[90px] h-[40px] bg-[#0f0f0f]/95 border border-white/5 rounded-full shadow-lg flex items-center justify-center"
            >
              {isConnecting ? (
                <div className="w-3 h-3 border-2 border-white/30 border-t-white/80 rounded-full animate-spin" aria-hidden="true" />
              ) : (
                analyser && <Waveform analyser={analyser} />
              )}
            </motion.div>
            <motion.button
              initial={{ x: 10, opacity: 0 }}
              animate={{ x: 0, opacity: 1 }}
              exit={{ x: 10, opacity: 0 }}
              transition={{ duration: 0.2, ease: 'easeOut' }}
              onClick={() => stop()}
              aria-label="Stop dictation"
              className="w-[32px] h-[32px] rounded-full bg-[#1c1c1c] border border-white/5 flex items-center justify-center hover:scale-105 active:scale-95 transition-all pointer-events-auto focus:outline-none"
            >
              <div className="w-[10px] h-[10px] bg-[#f2f1ea] rounded-sm"></div>
            </motion.button>
          </div>
        )}
      </AnimatePresence>
      <AnimatePresence>
        {badge && (
          <motion.div
            initial={{ y: 8, opacity: 0 }}
            animate={{ y: 0, opacity: 1 }}
            exit={{ y: 8, opacity: 0 }}
            transition={{ duration: 0.18 }}
            className="fixed left-1/2 -translate-x-1/2 bottom-[70px]"
          >
            <Badge text={badge} />
          </motion.div>
        )}
      </AnimatePresence>
    </div>
  );
}
