import { invoke } from '@tauri-apps/api/core';

type Handlers = {
  onTranscript?: (text: string, isFinal: boolean) => void;
  onError?: (err: any) => void;
  onOpen?: () => void;
  onClose?: () => void;
};

function log(msg: string) {
  console.log(msg);
  invoke('log_to_terminal', { message: msg }).catch(() => {});
}

function toBase64(buffer: ArrayBuffer): string {
  let binary = '';
  const bytes = new Uint8Array(buffer);
  const len = bytes.byteLength;
  for (let i = 0; i < len; i++) {
    binary += String.fromCharCode(bytes[i]);
  }
  return btoa(binary);
}

export async function startElevenLabsStream(token: string, stream: MediaStream, handlers: Handlers = {}) {
  const params = new URLSearchParams({
    model_id: 'scribe_v2_realtime',
    // Use VAD-based committing by default so segments are finalized automatically.
    // We still send a manual commit on stop as an extra guarantee.
    commit_strategy: 'vad',
    audio_format: 'pcm_16000',
    token: token,
  });
  const wsUrl = `wss://api.elevenlabs.io/v1/speech-to-text/realtime?${params.toString()}`;

  const ws = new WebSocket(wsUrl);
  ws.binaryType = 'arraybuffer';

  let stopSending = false;
  let audioContext: AudioContext | null = null;
  let source: MediaStreamAudioSourceNode | null = null;
  let processor: ScriptProcessorNode | null = null;
  let commitResolve: (() => void) | null = null;
  const commitPromise = new Promise<void>((resolve) => { commitResolve = resolve; });
  const resolveCommitOnce = () => {
    if (commitResolve) {
      commitResolve();
      commitResolve = null;
    }
  };

  ws.onopen = () => {
    log('[ElevenLabs] WebSocket OPENED');
    handlers.onOpen?.();
  };

  ws.onerror = (e) => {
    log('[ElevenLabs] WebSocket ERROR: ' + String(e));
    handlers.onError?.(e);
  };

  ws.onclose = (e) => {
    log(`[ElevenLabs] WebSocket CLOSED: code=${e.code} reason=${e.reason}`);
    handlers.onClose?.();
  };

  ws.onmessage = (ev) => {
    try {
      const msg = JSON.parse(typeof ev.data === 'string' ? ev.data : new TextDecoder().decode(ev.data));
      log('[ElevenLabs] Message: ' + JSON.stringify(msg));
      const type = msg?.message_type;
      if (type === 'partial_transcript') {
        const text = msg.text || msg.transcript || '';
        if (text) handlers.onTranscript?.(text, false);
      } else if (type === 'committed_transcript') {
        const text = msg.text || msg.transcript || '';
        if (text) handlers.onTranscript?.(text, true);
        resolveCommitOnce();
      } else if (type === 'error') {
        handlers.onError?.(msg);
      }
    } catch (err) {
      log('[ElevenLabs] Failed to parse message: ' + String(err));
    }
  };

  // Audio pipeline: mic -> processor -> destination (silent)
  audioContext = new AudioContext({ sampleRate: 16000 });
  source = audioContext.createMediaStreamSource(stream);
  processor = audioContext.createScriptProcessor(4096, 1, 1);

  processor.onaudioprocess = (e) => {
    if (stopSending || ws.readyState !== WebSocket.OPEN) return;
    const input = e.inputBuffer.getChannelData(0);
    const int16 = new Int16Array(input.length);
    for (let i = 0; i < input.length; i++) {
      const s = Math.max(-1, Math.min(1, input[i]));
      int16[i] = s < 0 ? s * 0x8000 : s * 0x7fff;
    }
    const payload = {
      message_type: 'input_audio_chunk',
      audio_base_64: toBase64(int16.buffer),
      sample_rate: 16000,
      commit: false,
    };
    ws.send(JSON.stringify(payload));
  };

  source.connect(processor);
  processor.connect(audioContext.destination);

  return {
    stop: async () => {
      stopSending = true;
      try {
        if (processor && source) {
          processor.disconnect();
          source.disconnect();
        }
        if (audioContext) {
          audioContext.close().catch(() => {});
        }
      } catch (err) {
        log('[ElevenLabs] Error stopping audio graph: ' + String(err));
      }
      try {
        if (ws.readyState === WebSocket.OPEN) {
          // Request a manual commit and send an explicit commit chunk to force finalization
          ws.send(JSON.stringify({ message_type: 'commit' }));
          ws.send(JSON.stringify({
            message_type: 'input_audio_chunk',
            audio_base_64: '',
            sample_rate: 16000,
            commit: true,
          }));
          await Promise.race([commitPromise, new Promise((resolve) => setTimeout(resolve, 5000))]);
          resolveCommitOnce();
          try { ws.close(); } catch {}
        } else {
          try { ws.close(); } catch {}
        }
      } catch (err) {
        log('[ElevenLabs] Error closing WebSocket: ' + String(err));
      }
    },
  };
}
