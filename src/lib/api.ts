// Typed wrappers for backend commands and events. The backend never returns secrets.
import { invoke as tauriInvoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';

export type KeySource = 'saved' | 'dev_env' | 'none';

export type Settings = {
  version: number;
  auto_paste: boolean;
  autostart: boolean;
  stt_provider: string;
  deepgram_model: string;
  elevenlabs_model: string;
  language: string;
  local_stt_model: string;
  local_model_paths: Record<string, string>;
  local_models_dir: string | null;
  input_device: string | null;
  echo_cancellation: boolean;
  mute_audio: boolean;
  sound_effects: boolean;
  theme: Theme;
  noise_suppression: boolean;
  ai_refine: boolean;
  ai_provider: string;
  openrouter_model: string;
  megallm_model: string;
  ollama_model: string;
  lmstudio_model: string;
  /** Days to keep recordings; 0 keeps them forever. */
  audio_retention_days: number;
  flowbar_edge: 'bottom' | 'top' | 'left' | 'right';
  smart_formatting: boolean;
  context_awareness: boolean;
  correction_learning: 'off' | 'suggest' | 'auto';
  coding_awareness: boolean;
};

export type SettingsPatch = Partial<Omit<Settings, 'version' | 'autostart' | 'input_device'>> & { input_device?: string };

export type ModelInfo = { id: string; name: string; languages: string[]; keyterms: boolean };

export type SttProvider = {
  id: string;
  name: string;
  models: ModelInfo[];
  default_model: string;
  multi_key: boolean;
  configured: boolean;
  key_source: KeySource;
};

export type RefineProvider = { id: string; name: string; configured: boolean; key_source: KeySource; local: boolean; installed: boolean };

export type LocalModel = { id: string; name: string; size_bytes: number; usable: boolean; thinking: boolean };
export type LocalPick = { id: string; name: string; size_bytes: number };
export type LocalLlmState = 'running' | 'stopped' | 'starting' | 'failed' | 'not_local' | 'not_installed';
export type LocalLlmStatus = { installed: boolean; state: Exclude<LocalLlmState, 'starting'>; models: LocalModel[]; picks: LocalPick[] };
export type LlmPullEvent = { provider: string; model: string; pct: number; state: 'downloading' | 'done' | 'failed'; error: string | null };

export type LocalSttState = 'installed' | 'linked' | 'missing' | 'downloading' | 'not_installed';
/** Where downloaded speech models go. `custom` is false for the app's own folder. */
export type ModelsFolder = { path: string; custom: boolean };
export type LocalSttModel = { id: string; name: string; size_bytes: number; languages: string[]; state: LocalSttState; pct: number | null; path: string | null };
export type SttDownloadEvent = { id: string; pct: number; state: 'downloading' | 'done' | 'failed' | 'cancelled'; error: string | null };

export type ProvidersOverview = { stt: SttProvider[]; refine: RefineProvider[] };

export type Theme = 'light' | 'dark' | 'system';

export type KeyStatus = 'unknown' | 'usable' | 'low_credit' | 'invalid' | 'cooldown' | 'error' | 'no_credit_access' | 'refused';

export type PoolKey = {
  id: string;
  label: string;
  status: KeyStatus;
  remaining: number | null;
  limit: number | null;
  last_checked_ms: number | null;
  last_error: string | null;
  active: boolean;
};

export type PoolView = {
  multi: boolean;
  keys: PoolKey[];
  total_remaining: number;
  total_limit: number;
  configured: boolean;
  using_dev_key: boolean;
  last_used_id: string | null;
};

export type SessionStatus = 'in_progress' | 'completed' | 'cancelled' | 'failed' | 'no_speech' | 'interrupted';

export type HistoryItem = {
  id: string;
  created_at: number;
  ended_at: number | null;
  trigger: string;
  status: SessionStatus;
  failure: string | null;
  provider: string;
  model: string;
  language: string;
  raw_text: string | null;
  formatted_text: string | null;
  final_text: string | null;
  refine_status: string | null;
  delivery_status: string | null;
  delivery_reason: string | null;
  target_app: string | null;
  duration_ms: number | null;
  word_count: number | null;
  retry_of: string | null;
  audio_state: 'staging' | 'encoded' | 'failed' | 'deleted' | null;
  audio_ms: number | null;
  audio_bytes: number | null;
  recovered_from: string | null;
  attempts: number;
};

export type HistoryFilter = 'all' | 'needs_attention' | 'with_audio';
export type HistoryQuery = { search?: string; filter?: HistoryFilter; before?: number; limit?: number };

export type AttemptRow = {
  id: number;
  kind: string;
  provider: string;
  model: string;
  language: string;
  started_at: number;
  ended_at: number | null;
  status: string;
  error: string | null;
  text: string | null;
  parent_id: number | null;
};

export type HistoryDetail = HistoryItem & { attempts: AttemptRow[] };
export type StorageView = { sessions: number; audio_files: number; audio_bytes: number; retention_days: number };

export type Term = {
  id: number;
  phrase: string;
  heard: string | null;
  source: 'manual' | 'learned' | 'import';
  status: 'active' | 'suggested';
  uses: number;
  created_at: number;
};
export type TermInput = { phrase: string; heard?: string | null };
export type Snippet = { id: number; trigger: string; expansion: string; uses: number; created_at: number };
export type SnippetInput = { trigger: string; expansion: string };
export type ImportResult = { added: number; skipped: number };

export type Insights = {
  total_words: number;
  sessions: number;
  words_per_minute: number | null;
  words_this_month: number;
  words_last_month: number;
  minutes_saved: number;
  refined_sessions: number;
  dictionary_fixes: number;
  snippets_used: number;
  apps: { app: string; words: number; sessions: number }[];
  current_streak: number;
  longest_streak: number;
  today: number;
  days: [number, number][];
};

export type ImportReport = {
  found: number;
  added: number;
  duplicates: number;
  rejected: number;
  unverified: number;
  failed: number;
  multi: boolean;
};

export type TestResult = { ok: boolean; message: string };
export type AudioDevice = { id: string; name: string; is_default: boolean };

// Lets the UI run in a plain browser with a mocked backend during development.
type InvokeFn = <T>(cmd: string, args?: Record<string, unknown>) => Promise<T>;
let invokeImpl: InvokeFn = tauriInvoke;
export function setInvokeImpl(fn: InvokeFn) {
  invokeImpl = fn;
}
const invoke = <T>(cmd: string, args?: Record<string, unknown>) => invokeImpl<T>(cmd, args);

export const api = {
  getSettings: () => invoke<Settings>('get_settings'),
  updateSettings: (patch: SettingsPatch) => invoke<Settings>('update_settings', { patch }),
  providersOverview: () => invoke<ProvidersOverview>('providers_overview'),
  setProviderKey: (provider: string, secret: string) => invoke<void>('set_provider_key', { provider, secret }),
  clearProviderKey: (provider: string) => invoke<void>('clear_provider_key', { provider }),
  testProvider: (provider: string) => invoke<TestResult>('test_provider', { provider }),
  listRefineModels: (provider: string) => invoke<string[]>('list_refine_models', { provider }),
  localLlm: {
    status: (provider: string) => invoke<LocalLlmStatus>('local_llm_status', { provider }),
    start: (provider: string) => invoke<LocalLlmStatus>('local_llm_start', { provider }),
    pull: (provider: string, model: string) => invoke<void>('local_llm_pull', { provider, model }),
    cancelPull: (provider: string) => invoke<void>('local_llm_cancel_pull', { provider }),
  },
  localStt: {
    models: () => invoke<LocalSttModel[]>('local_stt_models'),
    download: (id: string) => invoke<void>('local_stt_download', { id }),
    cancel: (id: string) => invoke<void>('local_stt_cancel', { id }),
    remove: (id: string) => invoke<void>('local_stt_remove', { id }),
    link: () => invoke<{ id: string; name: string } | null>('local_stt_link'),
    folder: () => invoke<ModelsFolder>('local_stt_folder'),
    chooseFolder: () => invoke<ModelsFolder | null>('local_stt_choose_folder'),
    resetFolder: () => invoke<ModelsFolder>('local_stt_reset_folder'),
  },
  pool: {
    view: () => invoke<PoolView>('elevenlabs_pool_view'),
    add: (secret: string) => invoke<PoolView>('elevenlabs_pool_add', { secret }),
    importText: (text: string) => invoke<{ report: ImportReport; view: PoolView }>('elevenlabs_pool_import', { text }),
    remove: (id: string) => invoke<PoolView>('elevenlabs_pool_remove', { id }),
    clear: () => invoke<PoolView>('elevenlabs_pool_clear'),
    setMulti: (multi: boolean) => invoke<PoolView>('elevenlabs_pool_set_multi', { multi }),
    refresh: () => invoke<PoolView>('elevenlabs_pool_refresh'),
    watch: (active: boolean) => invoke<void>('elevenlabs_pool_watch', { active }),
  },
  history: {
    list: (query: HistoryQuery) => invoke<HistoryItem[]>('history_list', { query }),
    detail: (id: string) => invoke<HistoryDetail>('history_detail', { id }),
    audio: (id: string) => invoke<ArrayBuffer>('history_audio', { id }),
    copy: (id: string) => invoke<void>('history_copy', { id }),
    retry: (id: string) => invoke<HistoryItem>('history_retry', { id }),
    recover: (id: string) => invoke<HistoryItem>('history_recover', { id }),
    retryRefinement: (id: string) => invoke<HistoryItem>('history_retry_refinement', { id }),
    exportWav: (id: string) => invoke<string>('history_export_wav', { id }),
    showExport: (path: string) => invoke<void>('history_show_export', { path }),
    remove: (id: string, audioOnly: boolean) => invoke<void>('history_delete', { id, audioOnly }),
    storage: () => invoke<StorageView>('history_storage'),
  },
  insights: (tzOffsetMinutes: number) => invoke<Insights>('insights', { tzOffsetMinutes }),
  dictionary: {
    list: () => invoke<Term[]>('dictionary_list'),
    add: (input: TermInput) => invoke<Term>('dictionary_add', { input }),
    update: (id: number, input: TermInput) => invoke<Term>('dictionary_update', { id, input }),
    remove: (id: number) => invoke<void>('dictionary_delete', { id }),
    accept: (id: number) => invoke<void>('dictionary_accept', { id }),
    importText: (text: string) => invoke<ImportResult>('dictionary_import', { text }),
  },
  snippets: {
    list: () => invoke<Snippet[]>('snippets_list'),
    add: (input: SnippetInput) => invoke<Snippet>('snippets_add', { input }),
    update: (id: number, input: SnippetInput) => invoke<Snippet>('snippets_update', { id, input }),
    remove: (id: number) => invoke<void>('snippets_delete', { id }),
  },
  audioDevices: () => invoke<AudioDevice[]>('audio_devices'),
  getHotkey: () => invoke<string>('get_hotkey'),
  setHotkey: (combo: string) => invoke<void>('set_hotkey', { combo }),
  setAutostart: (enabled: boolean) => invoke<void>('set_autostart', { enabled }),
};

export function onPoolChange(cb: (v: PoolView) => void): Promise<UnlistenFn> {
  if (!('__TAURI_INTERNALS__' in window)) return Promise.resolve(() => {});
  return listen<PoolView>('elevenlabs://pool', (e) => cb(e.payload));
}

export function onHistoryChange(cb: () => void): Promise<UnlistenFn> {
  if (!('__TAURI_INTERNALS__' in window)) return Promise.resolve(() => {});
  return listen('history://changed', () => cb());
}

export function onSttDownload(cb: (e: SttDownloadEvent) => void): Promise<UnlistenFn> {
  if (!('__TAURI_INTERNALS__' in window)) return Promise.resolve(() => {});
  return listen<SttDownloadEvent>('local://download', (e) => cb(e.payload));
}

export function onLlmPull(cb: (e: LlmPullEvent) => void): Promise<UnlistenFn> {
  if (!('__TAURI_INTERNALS__' in window)) return Promise.resolve(() => {});
  return listen<LlmPullEvent>('local://llm-pull', (e) => cb(e.payload));
}

export function onLexiconChange(cb: () => void): Promise<UnlistenFn> {
  if (!('__TAURI_INTERNALS__' in window)) return Promise.resolve(() => {});
  return listen('lexicon://changed', () => cb());
}

export function errorMessage(e: unknown) {
  return e instanceof Error ? e.message : String(e);
}
