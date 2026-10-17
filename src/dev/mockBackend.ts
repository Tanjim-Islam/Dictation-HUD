// Development-only mock of backend commands, used when the UI runs in a plain browser.
// Lets layout and interaction be inspected without Tauri. Never bundled in production paths.
import { setInvokeImpl, type PoolView, type ProvidersOverview, type Settings } from '@/lib/api';

const nova3 = ['multi', 'en', 'en-US', 'en-GB', 'bn', 'hi', 'es', 'fr', 'de', 'ja', 'zh-CN', 'ar', 'pt', 'ru', 'it'];
const settings: Settings = {
  version: 2,
  auto_paste: true,
  autostart: false,
  stt_provider: 'elevenlabs',
  deepgram_model: 'nova-3',
  elevenlabs_model: 'scribe_v2_realtime',
  language: 'en',
  input_device: null,
  echo_cancellation: true,
  noise_suppression: true,
  ai_refine: true,
  ai_provider: 'openrouter',
  openrouter_model: 'dots-studio/dots-3-note-preview:free',
  megallm_model: 'gpt-4o-mini',
};

let pool: PoolView = {
  multi: true,
  keys: [
    { id: 'a', label: 'Key 1', status: 'usable', remaining: 4200, limit: 10000, last_checked_ms: Date.now(), last_error: null, active: true },
    { id: 'b', label: 'Key 2', status: 'usable', remaining: 6000, limit: 10000, last_checked_ms: Date.now(), last_error: null, active: true },
    { id: 'c', label: 'Key 3', status: 'low_credit', remaining: 310, limit: 10000, last_checked_ms: Date.now(), last_error: 'Out of credits', active: true },
  ],
  total_remaining: 10510,
  total_limit: 30000,
  configured: true,
  using_dev_key: false,
  last_used_id: 'a',
};

function recompute() {
  const active = pool.keys.filter((k, i) => pool.multi || i === 0).map((k) => ({ ...k }));
  pool.keys = pool.keys.map((k, i) => ({ ...k, label: `Key ${i + 1}`, active: pool.multi || i === 0 }));
  pool.total_remaining = active.reduce((s, k) => s + (k.remaining ?? 0), 0);
  pool.total_limit = active.reduce((s, k) => s + (k.limit ?? 0), 0);
  pool.configured = pool.keys.length > 0;
}

const overview = (): ProvidersOverview => ({
  stt: [
    { id: 'deepgram', name: 'Deepgram', models: [{ id: 'nova-3', name: 'Nova-3', languages: nova3, keyterms: true }, { id: 'nova-2', name: 'Nova-2', languages: nova3.filter((l) => l !== 'bn'), keyterms: false }], default_model: 'nova-3', multi_key: false, configured: true, key_source: 'saved' },
    { id: 'elevenlabs', name: 'ElevenLabs', models: [{ id: 'scribe_v2_realtime', name: 'Scribe v2 Realtime', languages: ['multi', 'en', 'bn', 'hi', 'es', 'fr', 'de'], keyterms: true }], default_model: 'scribe_v2_realtime', multi_key: true, configured: pool.configured, key_source: pool.configured ? 'saved' : 'none' },
  ],
  refine: [
    { id: 'openrouter', name: 'OpenRouter', configured: true, key_source: 'saved' },
    { id: 'megallm', name: 'MegaLLM', configured: false, key_source: 'none' },
  ],
});

const delay = <T>(v: T, ms = 120) => new Promise<T>((r) => setTimeout(() => r(structuredClone(v)), ms));

export function installMockBackend() {
  setInvokeImpl(async <T>(cmd: string, args: Record<string, unknown> = {}): Promise<T> => {
    switch (cmd) {
      case 'get_settings':
        return delay(settings) as Promise<T>;
      case 'update_settings':
        Object.assign(settings, args.patch);
        return delay(settings) as Promise<T>;
      case 'providers_overview':
        return delay(overview()) as Promise<T>;
      case 'elevenlabs_pool_view':
      case 'elevenlabs_pool_refresh':
        return delay(pool, 300) as Promise<T>;
      case 'elevenlabs_pool_add': {
        const n = pool.keys.length + 1;
        pool.keys.push({ id: String(Date.now()), label: `Key ${n}`, status: 'usable', remaining: 10000, limit: 10000, last_checked_ms: Date.now(), last_error: null, active: true });
        recompute();
        return delay(pool, 600) as Promise<T>;
      }
      case 'elevenlabs_pool_remove':
        pool.keys = pool.keys.filter((k) => k.id !== args.id);
        recompute();
        return delay(pool) as Promise<T>;
      case 'elevenlabs_pool_clear':
        pool = { ...pool, keys: [] };
        recompute();
        return delay(pool) as Promise<T>;
      case 'elevenlabs_pool_set_multi':
        pool.multi = Boolean(args.multi);
        recompute();
        return delay(pool) as Promise<T>;
      case 'audio_devices':
        return delay([
          { id: 'w:1', name: 'Microphone (2- BOYA mini 2)', is_default: true },
          { id: 'w:2', name: 'Microphone (Realtek(R) Audio)', is_default: false },
          { id: 'w:3', name: 'Headset (AWEI Y669)', is_default: false },
        ]) as Promise<T>;
      case 'get_hotkey':
        return delay('Ctrl+Shift+Alt+H') as Promise<T>;
      case 'list_refine_models':
        return delay(['dots-studio/dots-3-note-preview:free', 'google/gemini-2.5-flash-lite', 'openai/gpt-4o-mini', 'anthropic/claude-haiku-4.5'], 400) as Promise<T>;
      case 'test_provider':
        return delay({ ok: true, message: 'Key works' }, 500) as Promise<T>;
      case 'session_snapshot':
        return delay({ session_id: null, phase: 'idle', error: null }) as Promise<T>;
      default:
        return delay(undefined as T);
    }
  });
}
