import { useEffect, useMemo, useState, useCallback, useRef } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { getVersion } from '@tauri-apps/api/app';
import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow';
import { check, Update, type DownloadEvent } from '@tauri-apps/plugin-updater';
import { relaunch } from '@tauri-apps/plugin-process';
import { initGlobalHotkey } from '../lib/hotkey';
import { motion, AnimatePresence } from 'framer-motion';
import { Check, X, Loader2, Minus, X as XIcon, Download, RefreshCw } from 'lucide-react';
// open external link using the browser; avoids requiring shell plugin here
import { Switch } from '../components/Switch';
import { ChevronRight, ChevronDown, KeyRound, Rocket, Save, Settings as SettingsIcon } from 'lucide-react';
import { log, error as logError, warn as logWarn } from '../lib/log';
import { KeyRecorder } from '../components/KeyRecorder';

type KeysPresent = { openrouter: boolean; deepgram: boolean; megallm: boolean; elevenlabs: boolean };

type UpdateStatus = 'idle' | 'checking' | 'available' | 'downloading' | 'ready' | 'error' | 'uptodate';

// Update state shared between TitleBar and Settings
type UpdateState = {
  status: UpdateStatus;
  progress: number;
  pendingUpdate: Update | null;
  error: string | null;
  newVersion: string | null;
};

// Global update state and callbacks
let globalUpdateState: UpdateState = {
  status: 'idle',
  progress: 0,
  pendingUpdate: null,
  error: null,
  newVersion: null,
};
let updateListeners: Set<(state: UpdateState) => void> = new Set();

function notifyUpdateListeners() {
  updateListeners.forEach(listener => listener({ ...globalUpdateState }));
}

async function checkForUpdatesGlobal(): Promise<void> {
  try {
    globalUpdateState.status = 'checking';
    globalUpdateState.error = null;
    notifyUpdateListeners();
    
    log('🔄 Checking for updates...');
    console.log('🔄 Checking for updates from GitHub...');
    
    const update = await check();
    
    if (update) {
      log(`✅ Update available: ${update.version}`);
      console.log('✅ Update available:', update.version);
      globalUpdateState.pendingUpdate = update;
      globalUpdateState.newVersion = update.version;
      globalUpdateState.status = 'available';
    } else {
      log('✅ App is up to date');
      console.log('✅ App is up to date');
      globalUpdateState.status = 'uptodate';
      // Show "up to date" for 3 seconds then hide
      setTimeout(() => {
        globalUpdateState.status = 'idle';
        notifyUpdateListeners();
      }, 3000);
    }
    notifyUpdateListeners();
  } catch (e) {
    const errorMsg = e instanceof Error ? e.message : String(e);
    logError('❌ Update check failed: ' + errorMsg);
    console.error('❌ Update check failed:', errorMsg);
    globalUpdateState.error = errorMsg;
    globalUpdateState.status = 'error';
    notifyUpdateListeners();
  }
}

async function downloadAndInstallGlobal(): Promise<void> {
  if (!globalUpdateState.pendingUpdate) return;
  
  try {
    globalUpdateState.status = 'downloading';
    globalUpdateState.progress = 0;
    notifyUpdateListeners();
    
    log('📥 Downloading update...');
    console.log('📥 Downloading update...');
    
    let downloaded = 0;
    let contentLength = 0;
    
    await globalUpdateState.pendingUpdate.downloadAndInstall((event: DownloadEvent) => {
      switch (event.event) {
        case 'Started':
          contentLength = event.data.contentLength || 0;
          log(`Download started, size: ${contentLength}`);
          console.log(`Download started, size: ${contentLength} bytes`);
          break;
        case 'Progress':
          downloaded += event.data.chunkLength;
          const progress = contentLength > 0 ? Math.round((downloaded / contentLength) * 100) : 0;
          globalUpdateState.progress = progress;
          notifyUpdateListeners();
          break;
        case 'Finished':
          log('✅ Download finished');
          console.log('✅ Download finished');
          break;
      }
    });
    
    globalUpdateState.status = 'ready';
    notifyUpdateListeners();
    
    log('✅ Update ready, restarting...');
    console.log('✅ Update ready, restarting app...');
    
    // Small delay to show the "ready" state
    setTimeout(async () => {
      await relaunch();
    }, 1000);
  } catch (e) {
    const errorMsg = e instanceof Error ? e.message : String(e);
    logError('❌ Update download failed: ' + errorMsg);
    console.error('❌ Update download failed:', errorMsg);
    globalUpdateState.error = errorMsg;
    globalUpdateState.status = 'error';
    notifyUpdateListeners();
  }
}

function useUpdateState() {
  const [state, setState] = useState<UpdateState>({ ...globalUpdateState });
  
  useEffect(() => {
    const listener = (newState: UpdateState) => setState(newState);
    updateListeners.add(listener);
    return () => { updateListeners.delete(listener); };
  }, []);
  
  return state;
}

function TitleBar({ version }: { version: string }) {
  const win = getCurrentWebviewWindow();
  const updateState = useUpdateState();

  // Check for updates on mount
  useEffect(() => {
    // Small delay to let the window fully initialize
    const timer = setTimeout(() => {
      checkForUpdatesGlobal();
    }, 500);
    return () => clearTimeout(timer);
  }, []);

  const handleMinimize = async () => {
    log('🔽🔽🔽 Minimize button clicked 🔽🔽🔽');
    try {
      await win.minimize();
      log('✅ Window minimized successfully');
    } catch (e) {
      logError('❌ Minimize failed: ' + String(e));
    }
  };

  const handleClose = async () => {
    log('❌❌❌ Close button clicked ❌❌❌');
    try {
      await win.hide();
      log('✅ Window hidden successfully');
    } catch (e) {
      logError('❌ Hide failed: ' + String(e));
    }
  };

  const renderUpdateButton = () => {
    switch (updateState.status) {
      case 'checking':
        return (
          <motion.button
            key="checking"
            initial={{ opacity: 0, scale: 0.9 }}
            animate={{ opacity: 1, scale: 1 }}
            exit={{ opacity: 0, scale: 0.9 }}
            className="flex items-center gap-1.5 px-2.5 py-1 rounded-full bg-neutral-800 border border-neutral-700 text-xs text-muted cursor-default"
          >
            <RefreshCw size={12} className="animate-spin" />
            <span>Checking...</span>
          </motion.button>
        );
      case 'uptodate':
        return (
          <motion.button
            key="uptodate"
            initial={{ opacity: 0, scale: 0.9 }}
            animate={{ opacity: 1, scale: 1 }}
            exit={{ opacity: 0, scale: 0.9 }}
            className="flex items-center gap-1.5 px-2.5 py-1 rounded-full bg-green-500/10 border border-green-500/30 text-xs text-green-400 cursor-default"
          >
            <Check size={12} />
            <span>Up to date</span>
          </motion.button>
        );
      case 'available':
        return (
          <motion.button
            key="available"
            initial={{ opacity: 0, scale: 0.9 }}
            animate={{ opacity: 1, scale: 1 }}
            exit={{ opacity: 0, scale: 0.9 }}
            whileHover={{ scale: 1.05, backgroundColor: 'rgba(34, 197, 94, 0.2)' }}
            whileTap={{ scale: 0.95 }}
            onClick={downloadAndInstallGlobal}
            className="flex items-center gap-1.5 px-2.5 py-1 rounded-full bg-green-500/10 border border-green-500/30 text-xs text-green-400 hover:border-green-500/50 transition-colors cursor-pointer"
          >
            <Download size={12} />
            <span>Update to v{updateState.newVersion}</span>
          </motion.button>
        );
      case 'downloading':
        return (
          <motion.button
            key="downloading"
            initial={{ opacity: 0, scale: 0.9 }}
            animate={{ opacity: 1, scale: 1 }}
            exit={{ opacity: 0, scale: 0.9 }}
            className="flex items-center gap-1.5 px-2.5 py-1 rounded-full bg-blue-500/10 border border-blue-500/30 text-xs text-blue-400 cursor-default"
          >
            <Loader2 size={12} className="animate-spin" />
            <span>Updating {updateState.progress}%</span>
          </motion.button>
        );
      case 'ready':
        return (
          <motion.button
            key="ready"
            initial={{ opacity: 0, scale: 0.9 }}
            animate={{ opacity: 1, scale: 1 }}
            exit={{ opacity: 0, scale: 0.9 }}
            className="flex items-center gap-1.5 px-2.5 py-1 rounded-full bg-green-500/20 border border-green-500/40 text-xs text-green-400 cursor-default"
          >
            <Check size={12} />
            <span>Restarting...</span>
          </motion.button>
        );
      case 'error':
        return (
          <motion.button
            key="error"
            initial={{ opacity: 0, scale: 0.9 }}
            animate={{ opacity: 1, scale: 1 }}
            exit={{ opacity: 0, scale: 0.9 }}
            whileHover={{ scale: 1.05 }}
            whileTap={{ scale: 0.95 }}
            onClick={checkForUpdatesGlobal}
            className="flex items-center gap-1.5 px-2.5 py-1 rounded-full bg-red-500/10 border border-red-500/30 text-xs text-red-400 hover:border-red-500/50 transition-colors cursor-pointer"
            title={updateState.error || 'Update check failed'}
          >
            <X size={12} />
            <span>Retry</span>
          </motion.button>
        );
      default:
        return null;
    }
  };

  return (
    <div 
      className="fixed top-0 left-0 right-0 z-10 h-10 flex items-center px-3 border-b border-neutral-800 bg-[var(--bg)]"
      data-tauri-drag-region
    >
      <div className="flex items-center gap-2 text-sm text-muted pointer-events-none">
        <SettingsIcon className="h-4 w-4" />
        <span>Settings</span>
        <span className="text-neutral-700">|</span>
        <span>Dictation HUD</span>
      </div>
      <div className="ml-auto flex items-center gap-3 pointer-events-auto">
        <AnimatePresence mode="wait">
          {renderUpdateButton()}
        </AnimatePresence>
        <span className="text-xs text-muted select-none">v{version}</span>
        <div className="flex items-center gap-1">
          <button 
            className="p-1.5 rounded hover:bg-neutral-800 active:bg-neutral-700 transition" 
            aria-label="Minimize" 
            title="Minimize" 
            onClick={handleMinimize}
          >
            <Minus size={14}/>
          </button>
          <button 
            className="p-1.5 rounded hover:bg-neutral-800 active:bg-neutral-700 transition" 
            aria-label="Close" 
            title="Close" 
            onClick={handleClose}
          >
            <XIcon size={14}/>
          </button>
        </div>
      </div>
    </div>
  );
}

function UpdateSection({ onToast }: { onToast: (text: string, kind: 'ok' | 'err') => void }) {
  const updateState = useUpdateState();
  const [version, setVersion] = useState('');

  useEffect(() => {
    getVersion().then(setVersion);
  }, []);

  // Show toast notifications for update status changes
  const lastToastRef = useRef<string>('');
  
  useEffect(() => {
    let toastKey = '';
    let toastText = '';
    let toastKind: 'ok' | 'err' = 'ok';
    
    switch (updateState.status) {
      case 'uptodate':
        toastKey = 'uptodate';
        toastText = 'App is up to date ✓';
        break;
      case 'available':
        toastKey = 'available';
        toastText = `Update v${updateState.newVersion} available!`;
        break;
      case 'ready':
        toastKey = 'ready';
        toastText = 'Update installed! Restarting...';
        break;
      case 'error':
        toastKey = 'error';
        toastText = `Update failed: ${updateState.error || 'Unknown error'}`;
        toastKind = 'err';
        break;
    }
    
    // Only show toast if it's different from the last one
    if (toastKey && toastKey !== lastToastRef.current) {
      lastToastRef.current = toastKey;
      onToast(toastText, toastKind);
    }
  }, [updateState.status, onToast]);

  const getStatusText = () => {
    switch (updateState.status) {
      case 'checking': return 'Checking for updates...';
      case 'uptodate': return 'You have the latest version';
      case 'available': return `Version ${updateState.newVersion} is available`;
      case 'downloading': return `Downloading... ${updateState.progress}%`;
      case 'ready': return 'Restarting to apply update...';
      case 'error': return updateState.error || 'Update check failed';
      default: return 'Click to check for updates';
    }
  };

  const isLoading = updateState.status === 'checking' || updateState.status === 'downloading';

  return (
    <section className="bg-card rounded-xl p-5 border border-neutral-800 h-fit mt-4">
      <h2 className="text-sm uppercase tracking-wider text-muted mb-3">About</h2>
      <div className="space-y-3">
        <div className="flex items-center justify-between">
          <div>
            <div className="text-sm font-medium">Dictation HUD</div>
            <div className="text-xs text-muted">Version {version}</div>
          </div>
        </div>
        
        <div className="text-xs text-muted">{getStatusText()}</div>
        
        {updateState.status === 'downloading' && (
          <div className="w-full h-1.5 bg-neutral-800 rounded-full overflow-hidden">
            <motion.div 
              className="h-full bg-blue-500"
              initial={{ width: 0 }}
              animate={{ width: `${updateState.progress}%` }}
              transition={{ duration: 0.3 }}
            />
          </div>
        )}
        
        <div className="flex gap-2">
          {updateState.status === 'available' ? (
            <motion.button
              onClick={downloadAndInstallGlobal}
              whileHover={{ scale: 1.02 }}
              whileTap={{ scale: 0.98 }}
              className="flex-1 px-3 py-2 bg-green-600 text-white rounded hover:bg-green-500 transition flex items-center justify-center gap-2"
            >
              <Download size={14} />
              Download & Install
            </motion.button>
          ) : (
            <motion.button
              onClick={checkForUpdatesGlobal}
              disabled={isLoading}
              whileHover={{ scale: isLoading ? 1 : 1.02 }}
              whileTap={{ scale: isLoading ? 1 : 0.98 }}
              className="flex-1 px-3 py-2 bg-neutral-800 rounded border border-neutral-700 hover:bg-neutral-700 transition flex items-center justify-center gap-2 disabled:opacity-50 disabled:cursor-not-allowed"
            >
              {isLoading ? (
                <Loader2 size={14} className="animate-spin" />
              ) : (
                <RefreshCw size={14} />
              )}
              {isLoading ? 'Checking...' : 'Check for Updates'}
            </motion.button>
          )}
        </div>
      </div>
    </section>
  );
}

function Toast({ text, kind }: { text: string; kind: 'ok'|'err' }) {
  return (
    <motion.div initial={{ y: 8, opacity: 0 }} animate={{ y: 0, opacity: 1 }} exit={{ y: 8, opacity: 0 }} className={`fixed bottom-4 right-4 px-3 py-2 rounded-md shadow border ${kind==='ok'?'bg-neutral-800 border-neutral-700':'bg-[var(--badge-bg)] border-[var(--badge-border)] text-[var(--badge-text)]'}`}>
      <span className="text-xs">{text}</span>
    </motion.div>
  );
}

export function Settings() {
  const [version, setVersion] = useState('');
  const [autoPaste, setAutoPaste] = useState(true);
  const [streamInsert, setStreamInsert] = useState(false);
  const [autostart, setAutostart] = useState(false);
  const [aiRefine, setAiRefine] = useState(true);
  const [aiProvider, setAiProvider] = useState<'openrouter' | 'megallm'>('openrouter');
  const [sttProvider, setSttProvider] = useState<'deepgram' | 'elevenlabs'>('deepgram');
  const [echoCancellation, setEchoCancellation] = useState(true);
  const [noiseSuppression, setNoiseSuppression] = useState(true);
  const [hotkey, setHotkey] = useState('');
  const [keysPresent, setKeysPresent] = useState<KeysPresent>({ openrouter: false, deepgram: false, megallm: false, elevenlabs: false });
  const [openrouterKey, setOpenrouterKey] = useState('');
  const [deepgramKey, setDeepgramKey] = useState('');
  const [megallmKey, setMegallmKey] = useState('');
  const [elevenlabsKey, setElevenlabsKey] = useState('');
  const [model, setModel] = useState('openai/gpt-oss-20b:free');
  const [megallmModel, setMegallmModel] = useState('gpt-4');
  const [lang, setLang] = useState('en-US');
  const [testing, setTesting] = useState<'dg'|'or'|'el'|null>(null);
  const [testingMega, setTestingMega] = useState<boolean>(false);
  const [toast, setToast] = useState<{text:string, kind:'ok'|'err'}|null>(null);
  const [valid, setValid] = useState<{or:boolean, dg:boolean, mg:boolean, el:boolean}>({or:false, dg:false, mg:false, el:false});
  const [modelList, setModelList] = useState<string[]>([]);
  const [modelMenuOpen, setModelMenuOpen] = useState(false);
  const [modelLoading, setModelLoading] = useState(false);
  const [modelError, setModelError] = useState<string | null>(null);
  const [aiMenuOpen, setAiMenuOpen] = useState(false);
  const [sttMenuOpen, setSttMenuOpen] = useState(false);

  useEffect(() => {
    log('⚙️⚙️⚙️ Settings component mounted ⚙️⚙️⚙️');
    console.log('⚙️ Settings component mounted');
    getVersion().then(v => {
      log('App version:', v);
      console.log('App version:', v);
      setVersion(v);
    });
  }, []);

  useEffect(() => {
    console.log('🔄 Loading settings from backend...');

    invoke<[boolean, boolean, boolean, boolean]>('get_keys_secure')
      .then(([okOr, okDg, okMg, okEl]) => {
        console.log('Keys present - OpenRouter:', okOr, 'Deepgram:', okDg, 'MegaLLM:', okMg, 'ElevenLabs:', okEl);
        setKeysPresent({ openrouter: okOr, deepgram: okDg, megallm: okMg, elevenlabs: okEl });
      })
      .catch((e) => console.error('Failed to get keys:', e));

    invoke<string>('get_hotkey')
      .then((combo) => {
        log('📌 Retrieved hotkey from backend:', combo);
        console.log('📌 Retrieved hotkey from backend:', combo);
        setHotkey(combo);
        // Register the hotkey when Settings loads
        if (combo) {
          log('🎯 Registering hotkey on Settings mount:', combo);
          console.log('🎯 Registering hotkey on Settings mount:', combo);
          initGlobalHotkey(combo).then(success => {
            log('Hotkey registration result:', success ? '✅ success' : '❌ failed');
            console.log('Hotkey registration result:', success ? '✅ success' : '❌ failed');
          });
        } else {
          log('⚠️ No hotkey combo retrieved, not registering');
          console.warn('⚠️ No hotkey combo retrieved, not registering');
        }
      })
      .catch((e) => {
        logError('Failed to get hotkey:', e);
        console.error('Failed to get hotkey:', e);
      });

    invoke<any>('get_behavior')
      .then((b: any) => {
        log('✅ get_behavior returned:', b);
        setAutoPaste(!!b?.auto_paste);
        setStreamInsert(!!b?.stream_insert);
        setAutostart(!!b?.autostart);
        setAiRefine(b?.ai_refine !== false); // Default to true if not set
        if (b?.ai_provider === 'megallm' || b?.ai_provider === 'openrouter') {
          setAiProvider(b.ai_provider);
        }
        if (typeof b?.stt_provider === 'string') {
          setSttProvider(b.stt_provider);
        }
        if (typeof b?.echo_cancellation === 'boolean') setEchoCancellation(b.echo_cancellation);
        if (typeof b?.noise_suppression === 'boolean') setNoiseSuppression(b.noise_suppression);
      })
      .catch((e) => logError('Failed to get behavior:', e));

    invoke<string>('get_model')
      .then(m => {
        console.log('Model:', m);
        setModel(m);
      })
      .catch((e) => console.error('Failed to get model:', e));

    invoke<string>('get_megallm_model')
      .then(m => {
        console.log('MegaLLM model:', m);
        setMegallmModel(m);
      })
      .catch((e) => console.error('Failed to get megallm model:', e));

    invoke<string>('get_language')
      .then(l => {
        console.log('Language:', l);
        setLang(l);
      })
      .catch((e) => console.error('Failed to get language:', e));
  }, []);

  // Load echo cancellation / noise suppression flags so Behavior toggles reflect stored prefs
  useEffect(() => {
    try {
      const raw = window.localStorage.getItem('dictation-audio-flags');
      if (raw) {
        const parsed = JSON.parse(raw);
        if (typeof parsed.echoCancellation === 'boolean') {
          setEchoCancellation(parsed.echoCancellation);
        }
        if (typeof parsed.noiseSuppression === 'boolean') {
          setNoiseSuppression(parsed.noiseSuppression);
        }
      }
    } catch (e) {
      logError('Failed to load audio flags from localStorage:', e);
    }
  }, []);

  const sttOk = sttProvider === 'elevenlabs' ? keysPresent.elevenlabs : keysPresent.deepgram;
  const keysOk = sttOk && (aiProvider === 'megallm' ? keysPresent.megallm : keysPresent.openrouter);

  function validateKeys(or: string, dg: string, mg: string, el: string) {
    const orOk = !or || /^sk-or-v1-[a-zA-Z0-9]{32,}$/.test(or);
    const dgOk = !dg || /^[A-Za-z0-9-_]{30,60}$/.test(dg);
    const mgOk = !mg || /^sk-[A-Za-z0-9-_]{20,80}$/.test(mg);
    const elOk = !el || /^[A-Za-z0-9_-]{24,80}$/.test(el);
    setValid({ or: orOk, dg: dgOk, mg: mgOk, el: elOk });
    return { orOk, dgOk, mgOk, elOk };
  }
  useEffect(()=>{ validateKeys(openrouterKey, deepgramKey, megallmKey, elevenlabsKey); }, [openrouterKey, deepgramKey, megallmKey, elevenlabsKey]);
  useEffect(() => {
    try {
      const payload = {
        echoCancellation,
        noiseSuppression,
      };
      window.localStorage.setItem('dictation-audio-flags', JSON.stringify(payload));
    } catch (e) {
      logError('Failed to persist audio flags to localStorage:', e);
    }
  }, [echoCancellation, noiseSuppression]);
  useEffect(() => { setModelMenuOpen(false); }, [aiProvider]);

  // Debug: log every behavior toggle change (user or programmatic)
  useEffect(() => { log('🔁 [BEHAVIOR] autoPaste ->', autoPaste); }, [autoPaste]);
  useEffect(() => { log('🔁 [BEHAVIOR] aiRefine ->', aiRefine); }, [aiRefine]);
  useEffect(() => { log('🔁 [BEHAVIOR] streamInsert ->', streamInsert); }, [streamInsert]);
  useEffect(() => { log('🔁 [BEHAVIOR] autostart ->', autostart); }, [autostart]);

    async function switchProvider(next: 'openrouter' | 'megallm') {
    setAiProvider(next);
    try {
      await invoke('set_behavior', { args: { ai_provider: next, aiProvider: next, ai_refine: aiRefine } });
    } catch (e) {
      logWarn('Failed to persist provider switch', e);
    }
  }

  async function switchSttProvider(next: 'deepgram' | 'elevenlabs') {
    setSttProvider(next);
    try {
      await invoke('set_behavior', { args: { stt_provider: next, sttProvider: next } });
    } catch (e) {
      logWarn('Failed to persist STT provider switch', e);
    }
  }

async function saveKeys() {
    const { orOk, dgOk, mgOk, elOk } = validateKeys(openrouterKey, deepgramKey, megallmKey, elevenlabsKey);
    if (!(orOk && dgOk && mgOk && elOk)) { setToast({ text: 'Invalid keys', kind: 'err' }); return; }
    await invoke('save_keys_secure', { openrouter: openrouterKey || '', deepgram: deepgramKey || '', megallm: megallmKey || '', elevenlabs: elevenlabsKey || '' });
    const [okOr, okDg, okMg, okEl] = await invoke<[boolean, boolean, boolean, boolean]>('get_keys_secure');
    setKeysPresent({ openrouter: okOr, deepgram: okDg, megallm: okMg, elevenlabs: okEl });
    setOpenrouterKey(''); setDeepgramKey(''); setMegallmKey(''); setElevenlabsKey('');
    setToast({ text: 'Saved successfully', kind: 'ok' }); setTimeout(()=> setToast(null), 2000);
  }

  async function testDeepgram() {
    try {
      setTesting('dg');
      const [_, dg] = await invoke<[string|null,string|null,string|null,string|null]>('runtime_keys');
      const keyToTest = deepgramKey || (dg as any) || '';
      if (!keyToTest) {
        setToast({ text: 'No Deepgram key to test', kind: 'err' });
        setTesting(null);
        setTimeout(() => setToast(null), 2000);
        return;
      }
      // Browser test via WS; Deepgram accepts token in subprotocol
      await new Promise<void>((resolve, reject) => {
        const ws = new WebSocket('wss://api.deepgram.com/v1/listen', ['token', keyToTest]);
        const to = setTimeout(()=> { try { ws.close(); } catch {} reject(new Error('timeout')); }, 2500);
        ws.onopen = () => { clearTimeout(to); try { ws.close(); } catch {}; resolve(); };
        ws.onerror = () => { clearTimeout(to); reject(new Error('error')); };
      });
      setToast({ text: 'Deepgram key valid ✅', kind: 'ok' });
    } catch { setToast({ text: 'Deepgram key invalid ❌', kind: 'err' }); }
    finally { setTesting(null); setTimeout(()=> setToast(null), 2000); }
  }
  async function testOpenRouter() {
    try {
      setTesting('or');
      const [or] = await invoke<[string|null,string|null,string|null,string|null]>('runtime_keys');
      const keyToTest = openrouterKey || (or as any) || '';
      if (!keyToTest) {
        setToast({ text: 'No OpenRouter key to test', kind: 'err' });
        setTesting(null);
        setTimeout(() => setToast(null), 2000);
        return;
      }
      const ctl = new AbortController(); const t = setTimeout(()=> ctl.abort(), 3000);
      const res = await fetch('https://openrouter.ai/api/v1/chat/completions', { method:'POST', headers:{'content-type':'application/json','authorization':`Bearer ${keyToTest}`}, body: JSON.stringify({ model: model, messages:[{role:'system', content:'Return plain text.'},{role:'user', content:'ping'}] }), signal: ctl.signal });
      clearTimeout(t);
      if (!res.ok) throw new Error(String(res.status));
      setToast({ text: 'OpenRouter key valid ✅', kind: 'ok' });
    } catch { setToast({ text: 'OpenRouter key invalid ❌', kind: 'err' }); }
    finally { setTesting(null); setTimeout(()=> setToast(null), 2000); }
  }

  async function testMegaLLM() {
    try {
      setTestingMega(true);
      const [_, __, mg] = await invoke<[string|null,string|null,string|null,string|null]>('runtime_keys');
      const keyToTest = megallmKey || (mg as any) || '';
      if (!keyToTest) {
        setToast({ text: 'No MegaLLM key to test', kind: 'err' });
        setTestingMega(false);
        setTimeout(() => setToast(null), 2000);
        return;
      }
      await invoke('test_megallm', { apiKey: keyToTest });
      setToast({ text: 'MegaLLM key valid ✅', kind: 'ok' });
    } catch (e) {
      console.error(e);
      const msg = e instanceof Error ? e.message : String(e);
      setToast({ text: `MegaLLM test failed: ${msg}`, kind: 'err' });
    } finally {
      setTestingMega(false);
      setTimeout(()=> setToast(null), 2000);
    }
  }

  async function fetchMegaModels() {
    setModelError(null);
    try {
      setModelLoading(true);
      const [_, __, mg] = await invoke<[string|null,string|null,string|null,string|null]>('runtime_keys');
      const key = megallmKey || (mg as any) || '';
      if (!key) {
        setToast({ text: 'Add MegaLLM key first', kind: 'err' });
        setModelError('Add API key first');
        setModelLoading(false);
        setTimeout(() => setToast(null), 2000);
        return;
      }
      const models = await invoke<string[]>('list_megallm_models', { apiKey: key });
      setModelList(models);
      setModelMenuOpen(true);
    } catch (e) {
      console.error(e);
      setModelError('Failed to load models');
      setToast({ text: 'Model fetch failed', kind: 'err' });
      setTimeout(() => setToast(null), 2000);
    } finally {
      setModelLoading(false);
    }
  }

  async function testElevenLabs() {
    try {
      setTesting('el');
      const [_, __, ___, el] = await invoke<[string|null,string|null,string|null,string|null]>('runtime_keys');
      const keyToTest = elevenlabsKey || (el as any) || '';
      if (!keyToTest) {
        setToast({ text: 'No ElevenLabs key to test', kind: 'err' });
        setTesting(null);
        setTimeout(() => setToast(null), 2000);
        return;
      }
      await invoke('test_elevenlabs', { api_key: keyToTest });
      setToast({ text: 'ElevenLabs key valid ✅', kind: 'ok' });
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e);
      setToast({ text: `ElevenLabs test failed: ${msg}`, kind: 'err' });
    } finally {
      setTesting(null);
      setTimeout(()=> setToast(null), 2000);
    }
  }

async function persistBehavior() {
    log('🖫 Save Behavior clicked with:', { autoPaste, streamInsert, aiRefine, autostart });
    try {
      // Build payload with both snake_case and camelCase to be robust
      const payload: any = {
        auto_paste: autoPaste,
        autoPaste,
        silence_secs: 2,
        silenceSecs: 2,
        stream_insert: streamInsert,
        streamInsert,
        ai_refine: aiRefine,
        aiRefine,
        ai_provider: aiProvider,
        aiProvider,
        stt_provider: sttProvider,
        sttProvider,
        echo_cancellation: echoCancellation,
        echoCancellation,
        noise_suppression: noiseSuppression,
        noiseSuppression,
      };
      log('➡️ set_behavior payload:', payload);
      // Persist behavior and get the saved struct back
      // Backend expects a single parameter named `args` (serde_json::Value)
      const saved = await invoke<any>('set_behavior', { args: payload });
      log('✅ set_behavior returned:', saved);
      // Reflect saved values from backend immediately
      setAutoPaste(!!saved?.auto_paste);
      setStreamInsert(!!saved?.stream_insert);
      setAiRefine(saved?.ai_refine !== false);
      if (saved?.ai_provider) setAiProvider(saved.ai_provider);
      if (saved?.stt_provider) setSttProvider(saved.stt_provider);
      if (typeof saved?.echo_cancellation === 'boolean') setEchoCancellation(saved.echo_cancellation);
      if (typeof saved?.noise_suppression === 'boolean') setNoiseSuppression(saved.noise_suppression);

      // Autostart is persisted via separate command and also controlled by OS
      let autostartOk = true;
      try {
        log('⚙️ Calling set_autostart with enabled=', autostart);
        await invoke('set_autostart', { enabled: autostart });
        log('✅ set_autostart completed');
      } catch (e) {
        logWarn('Autostart update failed:', e);
        autostartOk = false;
      }
      try {
        const osAuto = await invoke<boolean>('get_autostart');
        log('🔎 get_autostart returned:', osAuto);
        if (typeof osAuto === 'boolean') setAutostart(osAuto);
      } catch (e) {
        logWarn('get_autostart failed:', e);
      }

      setToast({ text: autostartOk ? 'Behavior Saved!' : 'Behavior saved. Autostart change failed.', kind: autostartOk ? 'ok' : 'err' });
    } catch (e) {
      logError('Failed to save behavior prefs', e);
      setToast({ text: 'Failed to save behavior', kind: 'err' });
    } finally {
      setTimeout(() => setToast(null), 2200);
    }
  }

  async function persistHotkey() {
    if (!hotkey || hotkey.trim() === '') {
      setToast({ text: 'Invalid hotkey', kind: 'err' });
      setTimeout(() => setToast(null), 2000);
      return;
    }
    await invoke('set_hotkey', { combo: hotkey });
    const success = await initGlobalHotkey(hotkey);
    if (success) {
      console.log('Hotkey saved and registered:', hotkey);
      setToast({ text: 'Hotkey saved', kind: 'ok' });
    } else {
      setToast({ text: 'Hotkey registration failed', kind: 'err' });
    }
    setTimeout(() => setToast(null), 1500);
  }

  const [savingProviderPrefs, setSavingProviderPrefs] = useState(false);

  async function persistProviderPrefs() {
    setSavingProviderPrefs(true);
    try {
      await invoke('set_model', { name: model });
      await invoke('set_megallm_model', { name: megallmModel });
      await invoke('set_language', { code: lang });
      setToast({ text: 'Provider prefs saved', kind: 'ok' });
      setTimeout(() => setToast(null), 2000);
    } catch (e) {
      console.error('Failed to save provider prefs', e);
      setToast({ text: 'Failed to save', kind: 'err' });
      setTimeout(() => setToast(null), 2000);
    } finally {
      setSavingProviderPrefs(false);
    }
  }

  return (
    <div className="h-screen flex flex-col bg-[var(--bg)] text-[var(--fg)] font-geistmono">
      <TitleBar version={version}/>
      <div className="flex-1 overflow-y-auto mt-10">
        <div className="w-full max-w-6xl mx-auto">
          <main className="p-6">
            <div className="flex flex-col md:flex-row items-start gap-4">
              <div className="flex-1 flex flex-col gap-4 min-w-0">
              <section className="bg-card rounded-xl p-5 border border-neutral-800 h-fit">
                <h2 className="text-sm uppercase tracking-wider text-muted mb-3">Keys</h2>
                <div className="space-y-3">
            <div>
              <label htmlFor="hotkey" className="block text-xs text-muted mb-2">Global hotkey</label>
              <KeyRecorder
                value={hotkey}
                onChange={setHotkey}
                onSave={persistHotkey}
              />
            </div>
            <div className="flex items-center justify-between">
              <div>
                <div className="text-sm">Autostart</div>
                <div className="text-xs text-muted">Launch on login</div>
              </div>
              <Switch ariaLabel="Autostart" checked={autostart} onCheckedChange={(v)=>{ setAutostart(v); setToast({text: v?'Autostart on':'Autostart off', kind:'ok'}); setTimeout(()=> setToast(null),1200); }} />
            </div>
          </div>
              </section>

              <section className="bg-card rounded-xl p-5 border border-neutral-800 h-fit">
                <h2 className="text-sm uppercase tracking-wider text-muted mb-3">Behavior</h2>
          <div className="space-y-4">
            <div className="flex items-center justify-between">
              <div>
                <div className="text-sm">Auto-paste into focused field</div>
                <div className="text-xs text-muted">Falls back to manual paste if blocked</div>
              </div>
              <Switch checked={autoPaste} onCheckedChange={(v)=>{ log('🟢 Toggle autoPaste ->', v); setAutoPaste(v); }} />
            </div>
            <div className="flex items-center justify-between">
              <div>
                <div className="text-sm">AI refinement</div>
                <div className="text-xs text-muted">Use the selected AI provider to refine dictation text</div>
              </div>
              <Switch checked={aiRefine} onCheckedChange={(v)=>{ log('🟢 Toggle aiRefine ->', v); setAiRefine(v); }} />
            </div>
            <motion.button
              onClick={persistBehavior}
              whileHover={{ scale: 1.02 }}
              whileTap={{ scale: 0.96 }}
              className="px-3 py-2 bg-accent text-black rounded hover:brightness-110 active:brightness-90 transition-all duration-150 shadow-sm hover:shadow-md"
            >
              Save Behavior
            </motion.button>
          </div>
              </section>

              <section className="bg-card rounded-xl p-5 border border-neutral-800 h-fit mt-4">
                <h2 className="text-sm uppercase tracking-wider text-muted mb-3">Audio behavior</h2>
          <div className="space-y-4">
            <div className="flex items-center justify-between">
              <div>
                <div className="text-sm">Echo cancellation</div>
                <div className="text-xs text-muted">Enable echo control when available</div>
              </div>
              <Switch checked={echoCancellation} onCheckedChange={(v)=>{ log('🔊 Toggle echoCancellation ->', v); setEchoCancellation(v); }} />
            </div>
            <div className="flex items-center justify-between">
              <div>
                <div className="text-sm">Noise suppression</div>
                <div className="text-xs text-muted">Enable noise filtering when available</div>
              </div>
              <Switch checked={noiseSuppression} onCheckedChange={(v)=>{ log('🔊 Toggle noiseSuppression ->', v); setNoiseSuppression(v); }} />
            </div>
          </div>
              </section>

              <UpdateSection onToast={(text: string, kind: 'ok' | 'err') => { setToast({ text, kind }); setTimeout(() => setToast(null), 3000); }} />
              </div>

              <div className="flex-1 min-w-0">
                                                                      <section className="bg-card rounded-xl p-5 border border-neutral-800 h-fit">
                <h2 className="text-sm uppercase tracking-wider text-muted mb-3">Providers</h2>
          <div className="space-y-4">
            <div className="grid md:grid-cols-2 gap-3">
              <div>
                <label className="block text-xs text-muted">AI provider</label>
                <div className="relative">
                  <button
                    type="button"
                    onClick={()=> setAiMenuOpen(o=>!o)}
                    className="w-full px-3 py-2 bg-neutral-900 border border-neutral-700 rounded text-left flex items-center justify-between"
                  >
                    <span className="text-sm">{aiProvider === 'openrouter' ? 'OpenRouter' : 'MegaLLM'}</span>
                    <ChevronDown size={14} className="text-muted" />
                  </button>
                  <AnimatePresence>
                    {aiMenuOpen && (
                      <motion.div
                        initial={{ opacity: 0, y: 4 }}
                        animate={{ opacity: 1, y: 0 }}
                        exit={{ opacity: 0, y: 4 }}
                        transition={{ duration: 0.12 }}
                        className="absolute left-0 right-0 mt-1 rounded-lg border border-neutral-800 bg-neutral-900 shadow-xl z-30 overflow-hidden"
                      >
                        {['openrouter','megallm'].map(opt => (
                          <button
                            key={opt}
                            type="button"
                            className={`w-full text-left px-3 py-2 text-sm hover:bg-neutral-800 ${aiProvider===opt?'text-accent':'text-[var(--fg)]'}`}
                            onMouseDown={() => { switchProvider(opt as any); setAiMenuOpen(false); }}
                          >
                            {opt === 'openrouter' ? 'OpenRouter' : 'MegaLLM'}
                          </button>
                        ))}
                      </motion.div>
                    )}
                  </AnimatePresence>
                </div>
              </div>
              <div>
                <label className="block text-xs text-muted">STT provider</label>
                <div className="relative">
                <button
                  type="button"
                  onClick={()=> setSttMenuOpen(o=>!o)}
                  className="w-full px-3 py-2 bg-neutral-900 border border-neutral-700 rounded text-left flex items-center justify-between"
                >
                    <span className="text-sm">{sttProvider === 'elevenlabs' ? 'ElevenLabs' : 'Deepgram'}</span>
                    <ChevronDown size={14} className="text-muted" />
                  </button>
                  <AnimatePresence>
                    {sttMenuOpen && (
                      <motion.div
                        initial={{ opacity: 0, y: 4 }}
                        animate={{ opacity: 1, y: 0 }}
                        exit={{ opacity: 0, y: 4 }}
                        transition={{ duration: 0.12 }}
                        className="absolute left-0 right-0 mt-1 rounded-lg border border-neutral-800 bg-neutral-900 shadow-xl z-30 overflow-hidden"
                      >
                        {['deepgram','elevenlabs'].map(opt => (
                          <button
                            key={opt}
                            type="button"
                            className={`w-full text-left px-3 py-2 text-sm hover:bg-neutral-800 ${sttProvider===opt?'text-accent':'text-[var(--fg)]'}`}
                            onMouseDown={() => { switchSttProvider(opt as any); setSttMenuOpen(false); }}
                          >
                            {opt === 'deepgram' ? 'Deepgram' : 'ElevenLabs'}
                          </button>
                        ))}
                      </motion.div>
                    )}
                  </AnimatePresence>
                </div>
              </div>
            </div>

            {aiProvider === 'openrouter' ? (
              <div className="space-y-1">
                <label htmlFor="openrouter" className="block text-xs text-muted">OpenRouter API key</label>
                <div className="flex gap-2 items-center">
                  <input
                    id="openrouter"
                    type="password"
                    value={openrouterKey}
                    onChange={e=>setOpenrouterKey(e.target.value)}
                    className={`flex-1 px-3 py-2 bg-neutral-900 rounded border ${valid.or || openrouterKey===''? 'border-neutral-700' : 'border-red-500'}`}
                    placeholder="sk-or-v1-********************************"
                    aria-invalid={!valid.or && openrouterKey!==''}
                  />
                  <button aria-label="Save OpenRouter key" title="Save OpenRouter key" onClick={saveKeys} className="px-3 py-2 bg-accent text-black rounded hover:brightness-95 active:brightness-90 transition disabled:opacity-50" disabled={!valid.or && openrouterKey!==''}>{testing==='or'?<Loader2 className="animate-spin" size={16}/> : <Save size={16}/>}</button>
                  <button aria-label="Test OpenRouter key" title="Test OpenRouter key" onClick={testOpenRouter} className="px-3 py-2 bg-neutral-800 rounded border border-neutral-700 hover:bg-neutral-700 transition disabled:opacity-50" disabled={testing==='or' || (!valid.or && openrouterKey!=='')}>{testing==='or'?'Testing…':'Test'}</button>
                </div>
                {!valid.or && openrouterKey!=='' && <div className="text-xs text-red-400 mt-1">Key format invalid</div>}
              </div>
            ) : (
              <div className="space-y-1">
                <label htmlFor="megallm" className="block text-xs text-muted">MegaLLM API key</label>
                <div className="flex gap-2 items-center">
                  <input
                    id="megallm"
                    type="password"
                    value={megallmKey}
                    onChange={e=>setMegallmKey(e.target.value)}
                    className={`flex-1 px-3 py-2 bg-neutral-900 rounded border ${valid.mg || megallmKey===''? 'border-neutral-700' : 'border-red-500'}`}
                    placeholder="sk-********************************"
                    aria-invalid={!valid.mg && megallmKey!==''}
                  />
                  <button aria-label="Save MegaLLM key" title="Save MegaLLM key" onClick={saveKeys} className="px-3 py-2 bg-accent text-black rounded hover:brightness-95 active:brightness-90 transition disabled:opacity-50" disabled={!valid.mg && megallmKey!==''}>{testingMega?<Loader2 className="animate-spin" size={16}/> : <Save size={16}/>}</button>
                  <button aria-label="Test MegaLLM key" title="Test MegaLLM key" onClick={testMegaLLM} className="px-3 py-2 bg-neutral-800 rounded border border-neutral-700 hover:bg-neutral-700 transition disabled:opacity-50" disabled={testingMega || (!valid.mg && megallmKey!=='')}>{testingMega?'Testing…':'Test'}</button>
                </div>
                {!valid.mg && megallmKey!=='' && <div className="text-xs text-red-400 mt-1">Key format invalid</div>}
              </div>
            )}


            {sttProvider === 'deepgram' ? (
              <div>
                <label htmlFor="deepgram" className="block text-xs text-muted">Deepgram API key</label>
                <div className="flex gap-2 mt-1 items-center">
                  <input id="deepgram" type="password" value={deepgramKey} onChange={e=>setDeepgramKey(e.target.value)} className={`flex-1 px-3 py-2 bg-neutral-900 rounded border ${valid.dg || deepgramKey===''? 'border-neutral-700' : 'border-red-500'}`} placeholder="dg-********************************" aria-invalid={!valid.dg && deepgramKey!==''} />
                  <button aria-label="Save Deepgram key" title="Save Deepgram key" onClick={saveKeys} className="px-3 py-2 bg-accent text-black rounded hover:brightness-95 active:brightness-90 transition disabled:opacity-50" disabled={!valid.dg && deepgramKey!==''}><KeyRound size={16}/></button>
                  <button aria-label="Test Deepgram key" title="Test Deepgram key" onClick={testDeepgram} className="px-3 py-2 bg-neutral-800 rounded border border-neutral-700 hover:bg-neutral-700 transition disabled:opacity-50" disabled={testing==='dg' || (!valid.dg && deepgramKey!=='')}>{testing==='dg'?'Testing...':'Test'}</button>
                </div>
                {!valid.dg && deepgramKey!=='' && <div className="text-xs text-red-400 mt-1">Key format invalid</div>}
              </div>
            ) : (
              <div>
                <label htmlFor="elevenlabs" className="block text-xs text-muted">ElevenLabs API key</label>
                <div className="flex gap-2 mt-1 items-center">
                  <input id="elevenlabs" type="password" value={elevenlabsKey} onChange={e=>setElevenlabsKey(e.target.value)} className={`flex-1 px-3 py-2 bg-neutral-900 rounded border ${valid.el || elevenlabsKey===''? 'border-neutral-700' : 'border-red-500'}`} placeholder="elevenlabs key" aria-invalid={!valid.el && elevenlabsKey!==''} />
                  <button aria-label="Save ElevenLabs key" title="Save ElevenLabs key" onClick={saveKeys} className="px-3 py-2 bg-accent text-black rounded hover:brightness-95 active:brightness-90 transition disabled:opacity-50" disabled={!valid.el && elevenlabsKey!==''}><KeyRound size={16}/></button>
                  <button aria-label="Test ElevenLabs key" title="Test ElevenLabs key" onClick={testElevenLabs} className="px-3 py-2 bg-neutral-800 rounded border border-neutral-700 hover:bg-neutral-700 transition disabled:opacity-50" disabled={testing==='el' || (!valid.el && elevenlabsKey!=='')}>{testing==='el'?'Testing...':'Test'}</button>
                </div>
                {!valid.el && elevenlabsKey!=='' && <div className="text-xs text-red-400 mt-1">Key format invalid</div>}
              </div>
            )}


            <div className="grid grid-cols-2 gap-3">
              <div className="relative">
                <label htmlFor="model" className="block text-xs text-muted">Model ({aiProvider === 'megallm' ? 'MegaLLM' : 'OpenRouter'})</label>
                {aiProvider === 'megallm' ? (
                  <div className="relative">
                    <input
                      id="model-megallm"
                      value={megallmModel}
                      onChange={e=>setMegallmModel(e.target.value)}
                      onFocus={() => fetchMegaModels()}
                      onClick={() => fetchMegaModels()}
                      onBlur={() => setTimeout(()=> setModelMenuOpen(false), 120)}
                      className="w-full px-3 py-2 bg-neutral-900 rounded border border-neutral-700 pr-10"
                      placeholder="Select model"
                    />
                    <button type="button" className="absolute inset-y-0 right-1 px-2 flex items-center text-muted" onMouseDown={e=>e.preventDefault()} onClick={() => { if (modelMenuOpen) { setModelMenuOpen(false); } else { fetchMegaModels(); } }}>
                      {modelLoading ? <Loader2 className="animate-spin" size={16}/> : <ChevronDown size={16}/>}
                    </button>
                    <AnimatePresence>
                      {modelMenuOpen && (
                        <motion.div
                          initial={{ opacity: 0, y: 4 }}
                          animate={{ opacity: 1, y: 0 }}
                          exit={{ opacity: 0, y: 4 }}
                          transition={{ duration: 0.15 }}
                          className="absolute left-0 right-0 mt-1 max-h-52 overflow-auto rounded-lg border border-neutral-800 bg-neutral-900 shadow-xl z-20"
                        >
                          {modelList.length === 0 && !modelLoading && <div className="px-3 py-2 text-xs text-muted">No models returned</div>}
                          {modelList.map(m => (
                            <button
                              key={m}
                              type="button"
                              className={`w-full text-left px-3 py-2 text-sm hover:bg-neutral-800 ${m===megallmModel?'text-accent':'text-[var(--fg)]'}`}
                              onMouseDown={() => { setMegallmModel(m); setModelMenuOpen(false); }}
                            >{m}</button>
                          ))}
                        </motion.div>
                      )}
                    </AnimatePresence>
                    {modelError && <div className="text-xs text-red-400 mt-1">{modelError}</div>}
                  </div>
                ) : (
                  <input id="model" value={model} onChange={e=>setModel(e.target.value)} className="w-full px-3 py-2 bg-neutral-900 rounded border border-neutral-700" placeholder="openai/gpt-oss-20b:free" />
                )}
              </div>
              <div>
                <label htmlFor="language" className="block text-xs text-muted">Language</label>
                <input id="language" value={lang} onChange={e=>setLang(e.target.value)} className="w-full px-3 py-2 bg-neutral-900 rounded border border-neutral-700" placeholder="en-US" />
              </div>
              <motion.button
                onClick={persistProviderPrefs}
                disabled={savingProviderPrefs}
                whileTap={{ scale: 0.98 }}
                className="col-span-2 px-3 py-2 bg-accent text-black rounded hover:brightness-95 active:brightness-90 transition disabled:opacity-50 flex items-center justify-center gap-2"
              >
                {savingProviderPrefs && <Loader2 className="animate-spin" size={16} />}
                Save Provider Prefs
              </motion.button>
            </div>
            <div className="text-xs text-muted">Keys stored securely (Stronghold). We never upload audio.</div>
            {((import.meta as any).env?.DEV) && (
              <button
                type="button"
                onClick={async () => {
                  await invoke('export_test_keys');
                  setToast({ text: 'Keys exported to terminal - check console', kind: 'ok' });
                  setTimeout(() => setToast(null), 3000);
                }}
                className="w-full mt-2 px-3 py-2 bg-neutral-800 rounded border border-neutral-700 hover:bg-neutral-700 transition text-xs"
              >
                🧪 Export Keys for Testing (check terminal)
              </button>
            )}
          </div>
        </section>
      </div>
    </div>
          </main>
        </div>
      </div>
      <AnimatePresence>
        {toast && <Toast text={toast.text} kind={toast.kind}/>}
      </AnimatePresence>
    </div>
  );
}
