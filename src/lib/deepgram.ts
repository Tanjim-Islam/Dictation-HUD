/* Deepgram live WebSocket client (temporary WebView path; moves to Rust in Phase 2). */

import { log } from './log';

type Handlers = {
  onTranscript: (text: string, isFinal: boolean) => void;
  onError: (kind: string) => void;
  onOpen: () => void;
};

export type Stream = {
  /** Flushes buffered audio and resolves once the provider sent its last result. */
  finish: () => Promise<void>;
  cancel: () => void;
};

export async function startDeepgramStream(apiKey: string, stream: MediaStream, handlers: Handlers): Promise<Stream> {
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

  let keepAlive: number | null = null;
  let stopped = false;
  let finishing = false;
  let resolveClosed: () => void = () => {};
  const closed = new Promise<void>((r) => (resolveClosed = r));

  ws.onopen = () => {
    if (stopped) return ws.close();
    handlers.onOpen();
    keepAlive = window.setInterval(() => {
      if (ws.readyState === WebSocket.OPEN) ws.send(JSON.stringify({ type: 'KeepAlive' }));
    }, 5000);
  };
  ws.onerror = () => {
    if (!finishing && !stopped) handlers.onError('network');
  };
  ws.onclose = (e) => {
    log(`[Deepgram] socket closed code=${e.code}`);
    if (keepAlive) clearInterval(keepAlive);
    // 1008 is an auth/policy failure on upgrade.
    if (!finishing && !stopped) handlers.onError(e.code === 1008 ? 'auth' : 'network');
    resolveClosed();
  };
  ws.onmessage = (ev) => {
    if (stopped) return;
    try {
      const msg = JSON.parse(typeof ev.data === 'string' ? ev.data : new TextDecoder().decode(ev.data));
      if (msg.type !== 'Results') return;
      const transcript: string = msg?.channel?.alternatives?.[0]?.transcript ?? '';
      // is_final means this span will not change; speech_final only marks a pause.
      handlers.onTranscript(transcript, !!msg?.is_final);
    } catch {
      log('[Deepgram] unparseable message');
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
    ws.send(pcm.buffer);
  };
  source.connect(processor);
  processor.connect(audioContext.destination);

  const teardownAudio = () => {
    try {
      processor.disconnect();
      source.disconnect();
      audioContext.close().catch(() => {});
    } catch {
      log('[Deepgram] audio teardown failed');
    }
  };

  return {
    finish: async () => {
      finishing = true;
      teardownAudio();
      if (ws.readyState === WebSocket.OPEN) {
        // The server flushes, sends the final results and metadata, then closes.
        ws.send(JSON.stringify({ type: 'CloseStream' }));
        await closed;
      } else {
        ws.close();
      }
    },
    cancel: () => {
      stopped = true;
      teardownAudio();
      if (keepAlive) clearInterval(keepAlive);
      ws.close();
    },
  };
}
