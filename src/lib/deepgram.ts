/* Deepgram live WebSocket client for browser-based real-time transcription.
 * Uses MediaRecorder to capture audio and streams to Deepgram API.
 * Based on official Deepgram documentation: https://developers.deepgram.com/docs/getting-started-with-live-streaming-audio
 */

import { invoke } from '@tauri-apps/api/core';

type Handlers = {
  onTranscript?: (text: string, isFinal: boolean) => void;
  onError?: (err: any) => void;
  onOpen?: () => void;
  onClose?: () => void;
};

// Helper to log to both console and terminal
function log(msg: string) {
  console.log(msg);
  invoke('log_to_terminal', { message: msg }).catch(() => {});
}

export async function startDeepgramStream(apiKey: string, stream: MediaStream, handlers: Handlers = {}) {
  // CRITICAL: Use Web Audio API to send RAW PCM audio, not WebM containers
  // Deepgram's WebSocket API expects raw linear16 PCM audio
  // MediaRecorder sends WebM which Deepgram WebSocket doesn't parse correctly

  const params = new URLSearchParams({
    model: 'nova-2',
    language: 'en',
    smart_format: 'true',
    interim_results: 'true',
    punctuate: 'true',
    encoding: 'linear16',  // Tell Deepgram we're sending raw PCM
    sample_rate: '16000',  // 16kHz sample rate
    channels: '1'          // Mono audio
  });

  const key = (apiKey || '').trim();
  const url = `wss://api.deepgram.com/v1/listen?${params.toString()}`;
  const ws = new WebSocket(url, ['token', key]);
  ws.binaryType = 'arraybuffer';

  let keepAliveInterval: number | null = null;
  let canceled = false;

  ws.onopen = () => {
    if (canceled) {
      // Session was cancelled before the socket opened; close immediately.
      try { ws.close(); } catch {}
      return;
    }
    log('[Deepgram] WebSocket OPENED');
    handlers.onOpen?.();

    // Send KeepAlive every 5 seconds to prevent connection timeout
    // Deepgram will close the connection if no data is sent for ~10 seconds
    keepAliveInterval = window.setInterval(() => {
      if (ws.readyState === WebSocket.OPEN) {
        log('[Deepgram] Sending KeepAlive');
        ws.send(JSON.stringify({ type: 'KeepAlive' }));
      }
    }, 5000);
  };

  ws.onerror = (e) => {
    log('[Deepgram] WebSocket ERROR: ' + String(e));
    log('[Deepgram] Error type: ' + (e instanceof Event ? e.type : typeof e));
    try { log(`[Deepgram] readyState: ${ws.readyState}, url: ${ws.url}`); } catch {}
    try { log('[Deepgram] Error details: ' + JSON.stringify(e)); } catch { log('[Deepgram] Error details: [unserializable]'); }
    handlers.onError?.(e);
  };

  ws.onclose = (e) => {
    log(`[Deepgram] WebSocket CLOSED: code=${e.code}, reason=${e.reason}`);
    if (e.code !== 1000) {
      log(`[Deepgram] ⚠️ ABNORMAL CLOSE - Code ${e.code} usually means:`);
      if (e.code === 1002) log('[Deepgram]    1002 = Protocol error (bad audio format or invalid params)');
      if (e.code === 1003) log('[Deepgram]    1003 = Unsupported data type');
      if (e.code === 1006) log('[Deepgram]    1006 = Abnormal closure (auth failed or network issue)');
      if (e.code === 1008) log('[Deepgram]    1008 = Policy violation (invalid API key)');
      if (e.code === 1011) log('[Deepgram]    1011 = Server error');
    }
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
      log('[Deepgram] Received message: ' + JSON.stringify(msg));

      // Handle different message types
      if (msg.type === 'Metadata') {
        log('[Deepgram] Metadata: ' + JSON.stringify(msg));
        return;
      }

      if (msg.type === 'Results') {
        // Extract transcript from the response
        // Response structure: msg.channel.alternatives[0].transcript
        const transcript = msg?.channel?.alternatives?.[0]?.transcript;

        if (transcript && transcript.trim().length > 0) {
          // speech_final: true means this segment naturally ended (pause detected)
          // is_final: true means Deepgram won't send more updates for this time span
          const isFinal = !!msg?.speech_final;
          log(`[Deepgram] Transcript (final=${isFinal}): "${transcript}"`);
          handlers.onTranscript?.(transcript, isFinal);
        }
      }
    } catch (e) {
      log('[Deepgram] Failed to parse message: ' + String(e));
    }
  };

  // CRITICAL: Use Web Audio API to extract RAW PCM samples
  // MediaRecorder outputs WebM containers which WebSocket API rejects
  // We need raw Int16 PCM samples at 16kHz mono

  const audioContext = new AudioContext({ sampleRate: 16000 });
  log(`[Deepgram] AudioContext created with sample rate: ${audioContext.sampleRate}Hz`);

  const source = audioContext.createMediaStreamSource(stream);
  log('[Deepgram] MediaStreamSource created from mic stream');

  // ScriptProcessorNode is deprecated but widely supported
  // Buffer size: 4096 samples = ~256ms at 16kHz (recommended by Deepgram)
  const processor = audioContext.createScriptProcessor(4096, 1, 1);
  log('[Deepgram] ScriptProcessor created with 4096 buffer size');

  let audioChunkCount = 0;
  processor.onaudioprocess = (e) => {
    if (canceled || ws.readyState !== WebSocket.OPEN) {
      return;
    }

    try {
      // Get raw PCM samples (Float32Array, range -1.0 to 1.0)
      const inputData = e.inputBuffer.getChannelData(0);

      // Convert Float32 to Int16 (linear16 format)
      // Deepgram expects 16-bit signed integers (-32768 to 32767)
      const int16Array = new Int16Array(inputData.length);
      for (let i = 0; i < inputData.length; i++) {
        // Clamp to [-1, 1] and scale to Int16 range
        const s = Math.max(-1, Math.min(1, inputData[i]));
        int16Array[i] = s < 0 ? s * 0x8000 : s * 0x7FFF;
      }

      // Send raw PCM bytes to Deepgram
      ws.send(int16Array.buffer);

      // Log only every 20th chunk to reduce noise
      audioChunkCount++;
      if (audioChunkCount % 20 === 0) {
        log(`[Deepgram] Sent ${audioChunkCount} audio chunks (${int16Array.buffer.byteLength} bytes each)`);
      }
    } catch (err) {
      log(`[Deepgram] ❌ Error processing audio: ${String(err)}`);
    }
  };

  // Connect audio graph: microphone -> processor -> destination (speakers, muted)
  source.connect(processor);
  processor.connect(audioContext.destination);
  log('[Deepgram] Audio pipeline connected: mic -> processor -> speakers');

  return {
    stop: () => {
      log('[Deepgram] Stopping recording and closing WebSocket');
      canceled = true;
      try {
        // Disconnect audio graph to stop processing
        processor.disconnect();
        source.disconnect();
        log('[Deepgram] Audio graph disconnected');

        // Close AudioContext to release microphone
        audioContext.close().then(() => {
          log('[Deepgram] AudioContext closed');
        }).catch((e) => {
          log('[Deepgram] Error closing AudioContext: ' + String(e));
        });
      } catch (e) {
        log('[Deepgram] Error stopping audio processing: ' + String(e));
      }

      if (keepAliveInterval) {
        clearInterval(keepAliveInterval);
        keepAliveInterval = null;
      }

      try {
        if (ws.readyState === WebSocket.OPEN) {
          // Send CloseStream message to tell Deepgram to process remaining audio
          ws.send(JSON.stringify({ type: 'CloseStream' }));
          setTimeout(() => { try { ws.close(); } catch {} }, 50);
        } else {
          // If still CONNECTING, attempt to close to prevent late onopen
          try { ws.close(); } catch {}
        }
      } catch (e) {
        log('[Deepgram] Error closing WebSocket: ' + String(e));
      }
    },
  };
}
