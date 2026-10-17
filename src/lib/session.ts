import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';

export type Phase = 'idle' | 'preparing' | 'listening' | 'finalizing' | 'processing' | 'delivering' | 'error';

export type SessionSnapshot = {
  session_id: string | null;
  phase: Phase;
  error: string | null;
};

export type Intent = 'toggle' | 'start' | 'finish' | 'cancel';

export function sendIntent(intent: Intent, source: 'flow_bar' | 'app' = 'app') {
  return invoke('session_intent', { intent, source });
}

export function getSnapshot() {
  return invoke<SessionSnapshot>('session_snapshot');
}

export function onSnapshot(cb: (s: SessionSnapshot) => void): Promise<UnlistenFn> {
  return listen<SessionSnapshot>('session://snapshot', (e) => cb(e.payload));
}

export const CAPTURE_PHASES: Phase[] = ['preparing', 'listening', 'finalizing'];
