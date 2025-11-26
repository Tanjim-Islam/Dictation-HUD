import { motion, AnimatePresence } from 'framer-motion';
import { useEffect, useRef, useState } from 'react';
import { Waveform } from '../components/Waveform';
import { Badge } from '../components/Badge';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';

export function Hud() {
  const [show, setShow] = useState(false);
  const [seconds, setSeconds] = useState(0);
  const [badge, setBadge] = useState<string | null>(null);
  const [analyser, setAnalyser] = useState<AnalyserNode | null>(null);
  const [isRecording, setIsRecording] = useState(false);
  const [isConnecting, setIsConnecting] = useState(false);
  const timerRef = useRef<number | null>(null);
  const recRef = useRef<{ stop: () => Promise<void> | void } | null>(null);
  const partialRef = useRef<string[]>([]);
  const latestTranscriptRef = useRef<string>(''); // Store latest transcript even if not final
  const wsRef = useRef<WebSocket | null>(null);
  const isReadyRef = useRef(false); // Track if WebSocket is actually open

  // Log when HUD component mounts
  useEffect(() => {
    const log = (msg: string) => {
      console.log(msg);
      invoke('log_to_terminal', { message: msg }).catch(() => {});
    };

    log('🚀🚀🚀 HUD COMPONENT MOUNTED 🚀🚀🚀');
  }, []);

  useEffect(() => {
    const log = (msg: string) => {
      console.log(msg);
      invoke('log_to_terminal', { message: msg }).catch(() => {});
    };

    log('📡 Setting up hud-badge listener...');
    let unsub: any;
    (async () => {
      unsub = await listen('hud-badge', (e:any)=>{
        log('📩 Received hud-badge event: ' + e?.payload);
        setBadge(String(e?.payload||'No text field is focused'));
        setTimeout(()=> setBadge(null), 3000);
      });
      log('✅ hud-badge listener registered');
    })();
    return () => {
      log('🧹 Cleaning up hud-badge listener');
      unsub?.();
    };
  }, []);

  async function begin() {
    const log = (msg: string) => {
      console.log(msg);
      invoke('log_to_terminal', { message: msg }).catch(() => {});
    };

    log('🎤🎤🎤 HUD begin() CALLED 🎤🎤🎤');

    // CRITICAL: Stop any existing recording first to prevent multiple simultaneous recordings
    if (recRef.current) {
      log('⚠️ Found existing recording, stopping it first...');
      recRef.current.stop();
      recRef.current = null;
    }

    // CRITICAL: Clear any existing timer to prevent multiple timers
    if (timerRef.current) {
      log('⚠️ Found existing timer, clearing it first...');
      clearInterval(timerRef.current);
      timerRef.current = null;
    }

    // Reset state
    partialRef.current = [];
    latestTranscriptRef.current = '';
    setSeconds(0);
    setAnalyser(null);
    setIsRecording(false); // Not recording yet, just connecting
    setIsConnecting(true);
    isReadyRef.current = false;
    setBadge('Initializing microphone...');
    setShow(true); // Show HUD immediately with "Initializing" badge

    try {
      // Load behavior prefs for audio + providers
      const behavior = await invoke<any>('get_behavior').catch(() => ({}));
      const echoCancellation = behavior?.echo_cancellation !== false;
      const noiseSuppression = behavior?.noise_suppression !== false;
      const sttProvider = (behavior?.stt_provider || 'deepgram') as string;

      if (sttProvider !== 'deepgram' && sttProvider !== 'elevenlabs') {
        log('?? Unsupported STT provider selected: ' + sttProvider);
        setBadge('Selected STT provider not supported yet');
        setIsConnecting(false);
        await invoke('set_recording_active', { newState: 'inactive' });
        return;
      }

      // Request microphone access
      log('📱 Requesting microphone access...');
      const stream = await navigator.mediaDevices.getUserMedia({
        audio: { echoCancellation, noiseSuppression, channelCount: 1 }
      });
      log('✅ Microphone access granted');
      log('Stream tracks: ' + JSON.stringify(stream.getTracks().map(t => ({ kind: t.kind, label: t.label, enabled: t.enabled }))));

      // Inline loading indicator is shown in the pill while connecting

      // Set up audio analyser for waveform
      log('🎵 Setting up audio analyser...');
      const ctx = new (window.AudioContext || (window as any).webkitAudioContext)();
      const src = ctx.createMediaStreamSource(stream);
      const an = ctx.createAnalyser();
      an.fftSize = 1024;
      src.connect(an);
      setAnalyser(an);
      log('✅ Audio analyser set up, fftSize: ' + an.fftSize);

      // Get provider keys
      log('?? Getting runtime keys...');
      const [or, dg, mg, el] = await invoke<[string|null,string|null,string|null,string|null]>('runtime_keys');
      log('Keys retrieved - OpenRouter: ' + (or ? 'present' : 'missing') + ', Deepgram: ' + (dg ? 'present' : 'missing') + ', MegaLLM: ' + (mg ? 'present' : 'missing') + ', ElevenLabs: ' + (el ? 'present' : 'missing'));

      if (sttProvider === 'elevenlabs') {
        if (!el) {
          log('No ElevenLabs key found');
          setBadge('ElevenLabs key not configured');
          stream.getTracks().forEach(t => t.stop());
          setIsRecording(false);
          setIsConnecting(false);
          await invoke('set_recording_active', { newState: 'inactive' });
          return;
        }

        log('Starting ElevenLabs stream with scribe_v1...');
        log('Starting ElevenLabs stream with scribe_v2_realtime (realtime)...');
        const token = await invoke<string>('create_elevenlabs_token', { api_key: null });
        const { startElevenLabsStream } = await import('../lib/elevenlabs');
        const rec = await startElevenLabsStream(token, stream, {
          onTranscript: (t, final) => {
            log('[EL] Transcript received - final: ' + final + ', text: ' + t);
            if (t) {
              latestTranscriptRef.current = t;
            }
            if (t && final) {
              partialRef.current.push(t);
              log('[EL] Added to partials, total: ' + partialRef.current.length);
            }
          },
          onOpen: () => {
            log('[EL] WebSocket OPENED');
            isReadyRef.current = true;
            setIsConnecting(false);
            setIsRecording(true);
            invoke('set_recording_active', { newState: 'recording' }).catch(() => {});
            setBadge(null);
            timerRef.current = window.setInterval(()=> setSeconds(s=>s+1), 1000);
          },
          onError: (e) => {
            log('[EL] WebSocket ERROR: ' + String(e));
            setBadge('Network error. Try again');
            setIsConnecting(false);
            setIsRecording(false);
            invoke('set_recording_active', { newState: 'inactive' }).catch(() => {});
          },
          onClose: () => {
            log('[EL] WebSocket CLOSED');
            isReadyRef.current = false;
            invoke('set_recording_active', { newState: 'inactive' }).catch(() => {});
          }
        });
        recRef.current = rec;
        log('ElevenLabs recorder stored in ref');
      } else {
        if (!dg) {
          log('No Deepgram key found');
          setBadge('Deepgram key not configured');
          stream.getTracks().forEach(t => t.stop());
          setIsRecording(false);
          setIsConnecting(false);
          await invoke('set_recording_active', { newState: 'inactive' });
          return;
        }

        log('Starting Deepgram stream with key: ' + (dg as string).substring(0, 10) + '...');
        const { startDeepgramStream } = await import('../lib/deepgram');
        const rec = await startDeepgramStream(dg as string, stream, {
          onTranscript: (t, final) => {
            log('[DG] Transcript received - final: ' + final + ', text: ' + t);
            if (t) {
              latestTranscriptRef.current = t;
            }
            if (t && final) {
              partialRef.current.push(t);
              log('[DG] Added to partials, total: ' + partialRef.current.length);
            }
          },
          onOpen: () => {
            log('[DG] WebSocket OPENED');
            isReadyRef.current = true;
            setIsConnecting(false);
            setIsRecording(true);
            invoke('set_recording_active', { newState: 'recording' }).then(() => {
              log('[DG] Backend state set to RECORDING');
            }).catch(e => {
              log('[DG] Failed to set backend state: ' + String(e));
            });
            setBadge(null);
            timerRef.current = window.setInterval(()=> setSeconds(s=>s+1), 1000);
          },
          onError: (e) => {
            log('[DG] WebSocket ERROR: ' + String(e));
            setBadge('Network error. Try again');
            setIsConnecting(false);
            setIsRecording(false);
            invoke('set_recording_active', { newState: 'inactive' }).catch(() => {});
          },
          onClose: () => {
            log('[DG] WebSocket CLOSED');
            isReadyRef.current = false;
            invoke('set_recording_active', { newState: 'inactive' }).catch(() => {});
          }
        });
        recRef.current = rec;
        log('Deepgram recorder stored in ref');
      }

      log('HUD begin() COMPLETED SUCCESSFULLY');
    } catch (e) {
      log('❌❌❌ HUD begin() ERROR ❌❌❌');
      log('Error: ' + String(e));
      log('Error type: ' + (e instanceof Error ? e.constructor.name : typeof e));
      log('Error message: ' + (e instanceof Error ? e.message : String(e)));
      log('Error stack: ' + (e instanceof Error ? e.stack : 'no stack'));

      setIsRecording(false);
      setIsConnecting(false);
      isReadyRef.current = false;
      await invoke('set_recording_active', { newState: 'inactive' }).catch(() => {});

      if (e instanceof DOMException && e.name === 'NotAllowedError') {
        log('Microphone permission denied');
        setBadge('Microphone permission required');
      } else {
        setBadge('Failed to start dictation');
      }
    }
  }

  useEffect(() => {
    const log = (msg: string) => {
      console.log(msg);
      invoke('log_to_terminal', { message: msg }).catch(() => {});
    };

    log('📡 Setting up dictation-start listener...');
    let unstart: any;
    (async () => {
      unstart = await listen('dictation-start', ()=> {
        log('🎯🎯🎯 RECEIVED dictation-start EVENT 🎯🎯🎯');
        log('Calling begin()...');
        begin();
      });
      log('✅ dictation-start listener registered');
    })();
    return () => {
      log('🧹 Cleaning up dictation-start listener');
      unstart?.();
    };
  }, []);

  useEffect(() => {
    const log = (msg: string) => {
      console.log(msg);
      invoke('log_to_terminal', { message: msg }).catch(() => {});
    };

    log('📡 Setting up dictation-stop listener...');
    let unstop: any;
    (async () => {
      unstop = await listen('dictation-stop', ()=> {
        log('🎯🎯🎯 RECEIVED dictation-stop EVENT 🎯🎯🎯');
        log('Calling stop()...');
        stop();
      });
      log('✅ dictation-stop listener registered');
    })();
    return () => {
      log('🧹 Cleaning up dictation-stop listener');
      unstop?.();
    };
  }, []);

  const mm = String(Math.floor(seconds/60)).padStart(2,'0');
  const ss = String(seconds % 60).padStart(2,'0');

  async function stop() {
    const log = (msg: string) => {
      console.log(msg);
      invoke('log_to_terminal', { message: msg }).catch(() => {});
    };

    log('?????? HUD stop() called ??????');

    const recorder = recRef.current;

    if (!isConnecting && !isRecording && !recorder) {
      log('?? Nothing is active, ignoring stop request');
      return;
    }

    log('?? Canceling active dictation session...');

    // Clear timer if any
    if (timerRef.current) {
      clearInterval(timerRef.current);
      timerRef.current = null;
    }

    // Stop recorder (await so manual commit can arrive)
    if (recorder) {
      recRef.current = null;
      try {
        await recorder.stop();
      } catch (err) {
        log('?? Error while stopping recorder: ' + String(err));
      }
    }

    setIsRecording(false);
    setIsConnecting(false);
    isReadyRef.current = false;

    // After stopping, check again if we captured anything (final or latest interim)
    const hasAnyTranscript =
      partialRef.current.length > 0 ||
      (latestTranscriptRef.current && latestTranscriptRef.current.trim().length > 0);

    if (!hasAnyTranscript) {
      partialRef.current = [];
      latestTranscriptRef.current = '';
      await invoke('set_recording_active', { newState: 'inactive' });
      setShow(false);
      await invoke('stop_dictation');
      log('? Session canceled, HUD hidden');
      return;
    }

    try {
      // CRITICAL: Set state to STOPPING immediately to prevent duplicate stop requests
      await invoke('set_recording_active', { newState: 'stopping' });
      log('? Backend state set to STOPPING');

      // Collect transcript - use partials if available, otherwise use latest non-final transcript
      let raw = partialRef.current.join(' ').trim();
      const latest = latestTranscriptRef.current.trim();
      if (raw && latest) {
        if (!raw.includes(latest)) {
          log('?? Appending latest non-final transcript to raw: "' + latest + '"');
          raw = `${raw} ${latest}`.trim();
        } else {
          log('?? Latest non-final transcript already included in raw, not appending');
        }
      }
      if (!raw && latest) {
        log('?? No final transcripts, using latest non-final: "' + latest + '"');
        raw = latest;
      }
      partialRef.current = [];
      latestTranscriptRef.current = '';
      log('Raw transcript: "' + raw + '"');

      if (!raw) {
        log('?? No speech detected');
        setBadge('No speech detected');
        await invoke('set_recording_active', { newState: 'inactive' });
        setShow(false);
        await invoke('stop_dictation');
        return;
      }

      // Refine text using OpenRouter (with fast timeout)
      log('?? Refining text with OpenRouter...');
      let refined = raw; // Default to raw text
      try {
        refined = await invoke<string>('refine_text', { rawText: raw, openrouterKey: null });
        log('? Refined text: "' + refined + '"');
      } catch (e) {
        log('?? OpenRouter failed or timed out, using raw text: ' + String(e));
        // Use raw text as fallback
      }

      // Hide HUD BEFORE pasting to ensure focus returns to the target app
      // Clicking the Stop button focuses the HUD window; if we paste while HUD is focused,
      // the OS key events can be ignored. Hiding first reliably returns focus.
      log('?? Hiding HUD before paste to restore focus...');
      setShow(false);
      await invoke('stop_dictation');
      // Small settle delay so the OS can restore focus to the previous window
      await new Promise((r) => setTimeout(r, 250));

      // Now insert text
      log('?? Inserting text into focused field...');
      const pasted: boolean = await invoke('insert_text', { text: refined });
      log('Insert result: ' + (pasted ? '? pasted successfully' : '? paste failed, copied to clipboard'));

      // CRITICAL: Set state back to INACTIVE after everything is done
      await invoke('set_recording_active', { newState: 'inactive' });
      log('? Backend state set to INACTIVE');

      // HUD already hidden above
      log('??? HUD stop() COMPLETED SUCCESSFULLY ???');
    } catch (e: any) {
      log('??? HUD stop() ERROR: ' + String(e));

      // CRITICAL: Always reset state to INACTIVE on error
      await invoke('set_recording_active', { newState: 'inactive' }).catch(() => {});

      // Hide immediately
      setShow(false);
      await invoke('stop_dictation');
    }
  }
  return (
    <div className="pointer-events-none">
      <AnimatePresence>
        {show && (
          <div className="fixed bottom-5 left-1/2 -translate-x-1/2 flex items-center gap-4 z-50 shadow-none outline-none ring-0">
            {/* Timer - no background, fade from left */}
            <motion.div
              initial={{ x: -10, opacity: 0 }}
              animate={{ x: 0, opacity: 0.9 }}
              exit={{ x: -10, opacity: 0 }}
              transition={{ duration: 0.2, ease: 'easeOut' }}
              className="text-[#f2f1ea] font-mono text-sm tracking-wider select-none pointer-events-none"
            >
              {mm}:{ss}
            </motion.div>

            {/* Waveform Pill - ONLY visible background, scale + slide up */}
            <motion.div
              initial={{ scale: 0.9, y: 10, opacity: 0 }}
              animate={{ scale: 1, y: 0, opacity: 1 }}
              exit={{ scale: 0.9, y: 10, opacity: 0 }}
              transition={{ duration: 0.2, ease: 'easeOut' }}
              className="w-[90px] h-[40px] bg-[#0f0f0f]/95 border border-white/5 rounded-full shadow-lg flex items-center justify-center pointer-events-none"
            >
              {isConnecting ? (
                <div className="w-3 h-3 border-2 border-white/30 border-t-white/80 rounded-full animate-spin" aria-hidden="true" />
              ) : (
                analyser && <Waveform analyser={analyser} />
              )}
            </motion.div>

            {/* Stop Button - must remain clickable, fade from right */}
            <motion.button
              initial={{ x: 10, opacity: 0 }}
              animate={{ x: 0, opacity: 1 }}
              exit={{ x: 10, opacity: 0 }}
              transition={{ duration: 0.2, ease: 'easeOut' }}
              onClick={stop}
              className="w-[32px] h-[32px] rounded-full bg-[#1c1c1c] border border-white/5 flex items-center justify-center hover:scale-105 hover:brightness-110 active:scale-95 transition-all pointer-events-auto focus:outline-none focus:ring-0 focus-visible:outline-none focus-visible:ring-0"
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



