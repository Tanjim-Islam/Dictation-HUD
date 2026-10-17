import * as React from 'react';
import { toast } from 'sonner';
import { listen } from '@tauri-apps/api/event';
import { api, errorMessage, type Settings, type SettingsPatch } from '@/lib/api';
import { SettingsCtx } from './settings-context';

/** Single owner of settings in the UI. Rust is the source of truth; updates are optimistic and
 * replaced by the value Rust returns (or rolled back on error). */
export function SettingsProvider({ children }: { children: React.ReactNode }) {
  const [settings, setSettings] = React.useState<Settings | null>(null);

  const reload = React.useCallback(async () => {
    setSettings(await api.getSettings());
  }, []);

  React.useEffect(() => {
    reload().catch((e) => toast.error('Could not load settings', { description: errorMessage(e) }));
    // Settings can change elsewhere (tray, another window); Rust broadcasts the new value.
    if (!('__TAURI_INTERNALS__' in window)) return;
    const un = listen<Settings>('settings://changed', (e) => setSettings(e.payload));
    return () => {
      un.then((f) => f());
    };
  }, [reload]);

  const update = React.useCallback(async (patch: SettingsPatch) => {
    let previous: Settings | null = null;
    setSettings((s) => {
      previous = s;
      return s ? ({ ...s, ...patch, input_device: patch.input_device === undefined ? s.input_device : patch.input_device || null } as Settings) : s;
    });
    try {
      setSettings(await api.updateSettings(patch));
    } catch (e) {
      setSettings(previous);
      toast.error('Setting not saved', { description: errorMessage(e) });
    }
  }, []);

  const value = React.useMemo(() => ({ settings, update, reload }), [settings, update, reload]);
  return <SettingsCtx.Provider value={value}>{children}</SettingsCtx.Provider>;
}
