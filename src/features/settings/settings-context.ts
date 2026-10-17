import * as React from 'react';
import type { Settings, SettingsPatch } from '@/lib/api';

export type Ctx = {
  settings: Settings | null;
  update: (patch: SettingsPatch) => Promise<void>;
  reload: () => Promise<void>;
};

export const SettingsCtx = React.createContext<Ctx | null>(null);

export function useSettings() {
  const ctx = React.useContext(SettingsCtx);
  if (!ctx) throw new Error('useSettings outside SettingsProvider');
  return ctx;
}
