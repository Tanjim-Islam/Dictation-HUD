// Development-only mock of backend commands, used when the UI runs in a plain browser.
// Lets layout and interaction be inspected without Tauri. Never bundled in production paths.
import { setInvokeImpl, type HistoryItem, type HistoryQuery, type PoolView, type ProvidersOverview, type Settings, type Snippet, type Term } from '@/lib/api';

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
  audio_retention_days: 14,
  flowbar_edge: 'bottom',
  smart_formatting: true,
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

const HOUR = 3_600_000;
const base = (i: number, over: Partial<HistoryItem>): HistoryItem => ({
  id: `00000000-0000-4000-8000-${String(i).padStart(12, '0')}`,
  created_at: Date.now() - i * 2.7 * HOUR,
  ended_at: null,
  trigger: 'hotkey',
  status: 'completed',
  failure: null,
  provider: 'deepgram',
  model: 'nova-3',
  language: 'en',
  raw_text: null,
  formatted_text: null,
  final_text: null,
  refine_status: 'ok',
  delivery_status: 'pasted',
  delivery_reason: null,
  target_app: 'Notepad',
  duration_ms: 6_400,
  word_count: 12,
  retry_of: null,
  audio_state: 'encoded',
  audio_ms: 6_400,
  audio_bytes: 19_800,
  recovered_from: null,
  attempts: 1,
  ...over,
});
const SAMPLES = [
  'Please send the quarterly report to the whole team before Friday, and include the updated revenue chart.',
  'Remind me to call the dentist tomorrow morning.',
  'The new onboarding flow should ask for the workspace name first, then invite teammates, then pick a plan.',
  'Let us move the standup to ten thirty.',
  'Okay so the bug happens only when the window is minimized during processing.',
];
let history: HistoryItem[] = [
  base(0, { final_text: SAMPLES[0], raw_text: 'please send the quarterly report to the whole team before friday and include the updated revenue chart', target_app: 'Slack' }),
  base(1, { final_text: SAMPLES[1], raw_text: 'remind me to call the dentist tomorrow morning', delivery_status: 'copied', delivery_reason: 'switched_app' }),
  base(2, { status: 'interrupted', failure: 'app_closed', raw_text: 'the new onboarding flow should ask for the workspace name first', refine_status: null, delivery_status: null }),
  base(3, { status: 'failed', failure: 'network', refine_status: null, delivery_status: null, word_count: 0 }),
  base(9, { final_text: SAMPLES[2], raw_text: SAMPLES[2], target_app: 'VS Code', audio_state: 'deleted', audio_ms: null, audio_bytes: 0 }),
  base(10, { final_text: SAMPLES[3], raw_text: SAMPLES[3], recovered_from: 'interrupted', delivery_status: 'none', attempts: 2 }),
  base(30, { final_text: SAMPLES[4], raw_text: SAMPLES[4], target_app: 'Chrome' }),
  base(60, { status: 'no_speech', audio_state: null, refine_status: null, delivery_status: null }),
];

function listHistory(q: HistoryQuery) {
  const term = q.search?.toLowerCase() ?? '';
  return history
    .filter((h) => h.created_at < (q.before ?? Infinity))
    .filter((h) => (q.filter === 'needs_attention' ? h.status === 'interrupted' || h.status === 'failed' : q.filter === 'with_audio' ? h.audio_state === 'encoded' : true))
    .filter((h) => !term || [h.final_text, h.raw_text, h.target_app].some((t) => t?.toLowerCase().includes(term)))
    .slice(0, q.limit ?? 60);
}

let terms: Term[] = [
  { id: 1, phrase: 'WACV', heard: 'wack v', source: 'learned', status: 'suggested', uses: 0, created_at: Date.now() },
  { id: 2, phrase: 'Tanjim Riju', heard: null, source: 'manual', status: 'active', uses: 4, created_at: Date.now() - 1000 },
  { id: 3, phrase: 'Citibank', heard: 'city bank', source: 'manual', status: 'active', uses: 2, created_at: Date.now() - 2000 },
  { id: 4, phrase: 'Daraz', heard: null, source: 'learned', status: 'active', uses: 1, created_at: Date.now() - 3000 },
  { id: 5, phrase: 'OpenRouter', heard: null, source: 'import', status: 'active', uses: 0, created_at: Date.now() - 4000 },
];
let snippets: Snippet[] = [
  { id: 1, trigger: 'my email address', expansion: 'tanjim@example.com', uses: 3, created_at: Date.now() },
  { id: 2, trigger: 'my linkedin', expansion: 'https://www.linkedin.com/in/example/', uses: 1, created_at: Date.now() - 1000 },
  { id: 3, trigger: 'organize thoughts prompt', expansion: 'Organize these unstructured thoughts into a clear, polished version without changing the meaning.', uses: 0, created_at: Date.now() - 2000 },
];
let nextId = 100;

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
      case 'history_list':
        return delay(listHistory(args.query as HistoryQuery)) as Promise<T>;
      case 'history_detail': {
        const h = history.find((x) => x.id === args.id)!;
        const attempts = [{ id: 1, kind: 'live', provider: h.provider, model: h.model, language: 'en', started_at: h.created_at, ended_at: h.created_at + 7000, status: h.status === 'failed' ? 'failed' : 'ok', error: h.failure, text: h.raw_text, parent_id: null as number | null }];
        if (h.attempts > 1) attempts.push({ id: 2, kind: 'retry', provider: 'elevenlabs', model: 'scribe_v2_realtime', language: 'en', started_at: h.created_at + HOUR, ended_at: h.created_at + HOUR, status: 'ok', error: null, text: h.raw_text, parent_id: 1 });
        return delay({ ...h, attempts }) as Promise<T>;
      }
      case 'history_retry':
      case 'history_recover':
      case 'history_retry_refinement': {
        history = history.map((h) => (h.id === args.id ? { ...h, final_text: h.final_text ?? 'Recovered transcript text.', status: 'completed', recovered_from: h.status === 'completed' ? h.recovered_from : h.status } : h));
        return delay(history.find((h) => h.id === args.id), 1200) as Promise<T>;
      }
      case 'history_delete':
        history = args.audioOnly ? history.map((h) => (h.id === args.id ? { ...h, audio_state: 'deleted' } : h)) : history.filter((h) => h.id !== args.id);
        return delay(undefined as T);
      case 'history_export_wav':
        return delay('C:\\Users\\me\\Downloads\\Dictation 00000000.wav') as Promise<T>;
      case 'history_audio':
        return Promise.reject(new Error('No audio in browser preview')) as Promise<T>;
      case 'history_storage':
        return delay({ sessions: history.length, audio_files: history.filter((h) => h.audio_state === 'encoded').length, audio_bytes: 98_400, retention_days: settings.audio_retention_days }) as Promise<T>;
      case 'dictionary_list':
        return delay(terms) as Promise<T>;
      case 'dictionary_add': {
        const input = args.input as { phrase: string; heard?: string | null };
        if (terms.some((t) => t.phrase.toLowerCase() === input.phrase.trim().toLowerCase() && (t.heard ?? '') === (input.heard ?? ''))) return Promise.reject(new Error('That entry already exists.'));
        const t: Term = { id: nextId++, phrase: input.phrase.trim(), heard: input.heard ?? null, source: 'manual', status: 'active', uses: 0, created_at: Date.now() };
        terms = [t, ...terms];
        return delay(t) as Promise<T>;
      }
      case 'dictionary_update': {
        const input = args.input as { phrase: string; heard?: string | null };
        terms = terms.map((t) => (t.id === args.id ? { ...t, phrase: input.phrase, heard: input.heard ?? null } : t));
        return delay(terms.find((t) => t.id === args.id)) as Promise<T>;
      }
      case 'dictionary_delete':
        terms = terms.filter((t) => t.id !== args.id);
        return delay(undefined as T);
      case 'dictionary_accept':
        terms = terms.map((t) => (t.id === args.id ? { ...t, status: 'active' } : t));
        return delay(undefined as T);
      case 'dictionary_import': {
        const lines = String(args.text).split('\n').filter((l) => l.trim());
        for (const l of lines) terms = [{ id: nextId++, phrase: l.split('->').pop()!.trim(), heard: l.includes('->') ? l.split('->')[0].trim() : null, source: 'import', status: 'active', uses: 0, created_at: Date.now() }, ...terms];
        return delay({ added: lines.length, skipped: 0 }) as Promise<T>;
      }
      case 'snippets_list':
        return delay(snippets) as Promise<T>;
      case 'snippets_add': {
        const input = args.input as { trigger: string; expansion: string };
        const sn: Snippet = { id: nextId++, trigger: input.trigger.trim(), expansion: input.expansion, uses: 0, created_at: Date.now() };
        snippets = [sn, ...snippets];
        return delay(sn) as Promise<T>;
      }
      case 'snippets_update': {
        const input = args.input as { trigger: string; expansion: string };
        snippets = snippets.map((x) => (x.id === args.id ? { ...x, ...input } : x));
        return delay(snippets.find((x) => x.id === args.id)) as Promise<T>;
      }
      case 'snippets_delete':
        snippets = snippets.filter((x) => x.id !== args.id);
        return delay(undefined as T);
      case 'session_snapshot':
        return delay({ session_id: null, phase: 'idle', error: null }) as Promise<T>;
      default:
        return delay(undefined as T);
    }
  });
}
