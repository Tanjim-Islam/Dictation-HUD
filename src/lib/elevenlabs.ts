/* ElevenLabs realtime client (temporary WebView path; moves to Rust in Phase 2). */

import { log } from './log';

type Handlers = {
  onTranscript?: (text: string, isFinal: boolean) => void;
  onError?: (err: unknown) => void;
  onOpen?: () => void;
  onClose?: () => void;
};

function toBase64(buffer: ArrayBuffer): string {
  let binary = '';
  const bytes = new Uint8Array(buffer);
  for (let i = 0; i < bytes.byteLength; i++) binary += String.fromCharCode(bytes[i]);
  return btoa(binary);
}

export async function startElevenLabsStream(token: string, stream: MediaStream, handlers: Handlers = {}) {
  const params = new URLSearchParams({
    model_id: 'scribe_v2_realtime',
    commit_strategy: 'vad',
    audio_format: 'pcm_16000',
    token,
  });
  const ws = new WebSocket(`wss://api.elevenlabs.io/v1/speech-to-text/realtime?${params.toString()}`);
  ws.binaryType = 'arraybuffer';

  let stopSending = false;
  let commitResolve: (() => void) | null = null;
  const commitPromise = new Promise<void>((resolve) => {
    commitResolve = resolve;
  });
  const resolveCommitOnce = () => {
    commitResolve?.();
    commitResolve = null;
  };

  ws.onopen = () => {
    log('[ElevenLabs] socket open');
    handlers.onOpen?.();
  };
  ws.onerror = () => {
    log('[ElevenLabs] socket error');
    handlers.onError?.(new Error('elevenlabs socket error'));
  };
  ws.onclose = (e) => {
    log(`[ElevenLabs] socket closed code=${e.code}`);
    handlers.onClose?.();
  };
  ws.onmessage = (ev) => {
    try {
      const msg = JSON.parse(typeof ev.data === 'string' ? ev.data : new TextDecoder().decode(ev.data));
      const type = msg?.message_type;
      const text: string = msg?.text || msg?.transcript || '';
      if (type === 'partial_transcript') {
        if (text) handlers.onTranscript?.(text, false);
      } else if (type === 'committed_transcript') {
        if (text) handlers.onTranscript?.(text, true);
        resolveCommitOnce();
      } else if (typeof type === 'string' && type.includes('error')) {
        log(`[ElevenLabs] error message type=${type}`);
        handlers.onError?.(new Error(type));
      }
    } catch {
      log('[ElevenLabs] unparseable message');
    }
  };

  const audioContext = new AudioContext({ sampleRate: 16000 });
  const source = audioContext.createMediaStreamSource(stream);
  const processor = audioContext.createScriptProcessor(4096, 1, 1);

  processor.onaudioprocess = (e) => {
    if (stopSending || ws.readyState !== WebSocket.OPEN) return;
    const input = e.inputBuffer.getChannelData(0);
    const pcm = new Int16Array(input.length);
    for (let i = 0; i < input.length; i++) {
      const s = Math.max(-1, Math.min(1, input[i]));
      pcm[i] = s < 0 ? s * 0x8000 : s * 0x7fff;
    }
    ws.send(
      JSON.stringify({ message_type: 'input_audio_chunk', audio_base_64: toBase64(pcm.buffer), sample_rate: 16000, commit: false }),
    );
  };

  source.connect(processor);
  processor.connect(audioContext.destination);

  return {
    stop: async () => {
      stopSending = true;
      try {
        processor.disconnect();
        source.disconnect();
        audioContext.close().catch(() => {});
      } catch {
        log('[ElevenLabs] audio graph teardown failed');
      }
      try {
        if (ws.readyState === WebSocket.OPEN) {
          ws.send(JSON.stringify({ message_type: 'input_audio_chunk', audio_base_64: '', sample_rate: 16000, commit: true }));
          await Promise.race([commitPromise, new Promise((resolve) => setTimeout(resolve, 5000))]);
          resolveCommitOnce();
        }
        ws.close();
      } catch {
        log('[ElevenLabs] socket close failed');
      }
    },
  };
}
