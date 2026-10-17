/* ElevenLabs realtime client (temporary WebView path; moves to Rust in Phase 2). */

import { log } from './log';
import type { Stream } from './deepgram';

type Handlers = {
  onTranscript: (text: string, isFinal: boolean) => void;
  onError: (kind: string) => void;
  onOpen: () => void;
};

function toBase64(buffer: ArrayBuffer): string {
  let binary = '';
  const bytes = new Uint8Array(buffer);
  for (let i = 0; i < bytes.byteLength; i++) binary += String.fromCharCode(bytes[i]);
  return btoa(binary);
}

export async function startElevenLabsStream(token: string, stream: MediaStream, handlers: Handlers): Promise<Stream> {
  const params = new URLSearchParams({
    model_id: 'scribe_v2_realtime',
    commit_strategy: 'vad',
    audio_format: 'pcm_16000',
    token,
  });
  const ws = new WebSocket(`wss://api.elevenlabs.io/v1/speech-to-text/realtime?${params.toString()}`);
  let stopped = false;
  let finishing = false;
  let resolveCommit: () => void = () => {};
  let commitWaiter = new Promise<void>((r) => (resolveCommit = r));

  ws.onopen = () => {
    if (stopped) return ws.close();
    handlers.onOpen();
  };
  ws.onerror = () => {
    if (!finishing && !stopped) handlers.onError('network');
  };
  ws.onclose = (e) => {
    log(`[ElevenLabs] socket closed code=${e.code}`);
    if (!finishing && !stopped) handlers.onError('network');
    resolveCommit();
  };
  ws.onmessage = (ev) => {
    if (stopped) return;
    try {
      const msg = JSON.parse(String(ev.data));
      const type: string = msg?.message_type ?? '';
      const text: string = msg?.text ?? '';
      if (type === 'partial_transcript') handlers.onTranscript(text, false);
      else if (type === 'committed_transcript') {
        handlers.onTranscript(text, true);
        resolveCommit();
        commitWaiter = new Promise<void>((r) => (resolveCommit = r));
      } else if (type.includes('error') || type === 'quota_exceeded' || type === 'rate_limited') {
        log(`[ElevenLabs] error type=${type}`);
        handlers.onError(type === 'auth_error' ? 'auth' : type);
      }
    } catch {
      log('[ElevenLabs] unparseable message');
    }
  };

  const audioContext = new AudioContext({ sampleRate: 16000 });
  const source = audioContext.createMediaStreamSource(stream);
  const processor = audioContext.createScriptProcessor(4096, 1, 1);
  processor.onaudioprocess = (e) => {
    if (finishing || stopped || ws.readyState !== WebSocket.OPEN) return;
    const input = e.inputBuffer.getChannelData(0);
    const pcm = new Int16Array(input.length);
    for (let i = 0; i < input.length; i++) {
      const s = Math.max(-1, Math.min(1, input[i]));
      pcm[i] = s < 0 ? s * 0x8000 : s * 0x7fff;
    }
    ws.send(JSON.stringify({ message_type: 'input_audio_chunk', audio_base_64: toBase64(pcm.buffer), sample_rate: 16000, commit: false }));
  };
  source.connect(processor);
  processor.connect(audioContext.destination);

  const teardownAudio = () => {
    try {
      processor.disconnect();
      source.disconnect();
      audioContext.close().catch(() => {});
    } catch {
      log('[ElevenLabs] audio teardown failed');
    }
  };

  return {
    finish: async () => {
      finishing = true;
      teardownAudio();
      if (ws.readyState === WebSocket.OPEN) {
        // Commit whatever is buffered, wait for the committed transcript, then close.
        ws.send(JSON.stringify({ message_type: 'input_audio_chunk', audio_base_64: '', sample_rate: 16000, commit: true }));
        await commitWaiter;
      }
      ws.close();
    },
    cancel: () => {
      stopped = true;
      teardownAudio();
      ws.close();
    },
  };
}
