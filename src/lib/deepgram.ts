/* Deepgram live WebSocket client (temporary WebView path; moves to Rust in Phase 2). */

import { log } from './log';

type Handlers = {
  onTranscript?: (text: string, isFinal: boolean) => void;
  onError?: (err: unknown) => void;
  onOpen?: () => void;
  onClose?: () => void;
};

export async function startDeepgramStream(apiKey: string, stream: MediaStream, handlers: Handlers = {}) {
  const params = new URLSearchParams({
    model: 'nova-2',
    language: 'en',
    smart_format: 'true',
    interim_results: 'true',
    punctuate: 'true',
    encoding: 'linear16',
    sample_rate: '16000',
    channels: '1',
  });

  const ws = new WebSocket(`wss://api.deepgram.com/v1/listen?${params.toString()}`, ['token', apiKey.trim()]);
  ws.binaryType = 'arraybuffer';

  let keepAliveInterval: number | null = null;
  let canceled = false;

  ws.onopen = () => {
    if (canceled) {
      ws.close();
      return;
    }
    log('[Deepgram] socket open');
    handlers.onOpen?.();
    keepAliveInterval = window.setInterval(() => {
      if (ws.readyState === WebSocket.OPEN) ws.send(JSON.stringify({ type: 'KeepAlive' }));
    }, 5000);
  };

  ws.onerror = () => {
    log('[Deepgram] socket error');
    handlers.onError?.(new Error('deepgram socket error'));
  };

  ws.onclose = (e) => {
    log(`[Deepgram] socket closed code=${e.code}`);
    if (keepAliveInterval) {
      clearInterval(keepAliveInterval);
      keepAliveInterval = null;
    }
    handlers.onClose?.();
  };

  ws.onmessage = (ev) => {
    if (canceled) return;
    try {
      const msg = JSON.parse(typeof ev.data === 'string' ? ev.data : new TextDecoder().decode(ev.data));
      if (msg.type !== 'Results') return;
      const transcript: string | undefined = msg?.channel?.alternatives?.[0]?.transcript;
      if (transcript && transcript.trim().length > 0) {
        handlers.onTranscript?.(transcript, !!msg?.speech_final);
      }
    } catch {
      log('[Deepgram] unparseable message');
    }
  };

  const audioContext = new AudioContext({ sampleRate: 16000 });
  const source = audioContext.createMediaStreamSource(stream);
  const processor = audioContext.createScriptProcessor(4096, 1, 1);

  processor.onaudioprocess = (e) => {
    if (canceled || ws.readyState !== WebSocket.OPEN) return;
    const input = e.inputBuffer.getChannelData(0);
    const pcm = new Int16Array(input.length);
    for (let i = 0; i < input.length; i++) {
      const s = Math.max(-1, Math.min(1, input[i]));
      pcm[i] = s < 0 ? s * 0x8000 : s * 0x7fff;
    }
    ws.send(pcm.buffer);
  };

  source.connect(processor);
  processor.connect(audioContext.destination);

  return {
    stop: () => {
      canceled = true;
      try {
        processor.disconnect();
        source.disconnect();
        audioContext.close().catch(() => {});
      } catch {
        log('[Deepgram] audio graph teardown failed');
      }
      if (keepAliveInterval) {
        clearInterval(keepAliveInterval);
        keepAliveInterval = null;
      }
      try {
        if (ws.readyState === WebSocket.OPEN) {
          ws.send(JSON.stringify({ type: 'CloseStream' }));
          setTimeout(() => ws.close(), 50);
        } else {
          ws.close();
        }
      } catch {
        log('[Deepgram] socket close failed');
      }
    },
  };
}
